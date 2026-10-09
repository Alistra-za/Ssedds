//! One `ResolvedJob` end to end (spec §5, carry order of Task 31):
//! load → preprocess → mips → coverage → quantise → encode → DDS bytes → atomic write → re-read and validate.
//!
//! The caller (`api::Engine::run`) has initialised the encoders with the batch BC1 model, admitted the file
//! against the memory budget and runs this on its rayon pool under `catch_unwind`; the encode stage's band
//! parallelism runs on the same pool. Nothing here depends on the thread count: every encoded block is a pure
//! function of its own texels, so the written bytes are identical for any `--jobs`.
use crate::api::{BatchRequest, CancelToken};
use crate::codes::{Code, Warning};
use crate::config::{Format, Profile};
use crate::coverage::{coverage_fraction, preserve_coverage};
use crate::dds::{dds_bytes, resolve_header, write_atomic, DdsDesc};
use crate::encode::{encode_level, EncodeCtx};
use crate::events::{EventKind, Reporter, Stage};
use crate::image::Rgba32F;
use crate::load::load_image;
use crate::manifest::{
    dispatched_isa, engine_commit, now_rfc3339, output_key, sha256_file, sha256_json,
    EncoderVersions, ManifestRecord, PresetRef, ENGINE_VERSION, MANIFEST_SCHEMA,
};
use crate::mips::build_chain;
use crate::ops::preprocess;
use crate::policy::ResolvedJob;
use crate::quantize::{to_encoder_input, EncoderInput};
use crate::validate::{validate, ValidationInput, ValidationReport};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum FileError {
    /// Stopped by the cancel token before anything was written.
    Cancelled,
    Failed(Stage, String),
}

#[derive(Debug)]
pub struct FileOutcome {
    pub output: PathBuf,
    pub report: ValidationReport,
    pub record: ManifestRecord,
}

fn fail(stage: Stage) -> impl Fn(String) -> FileError {
    move |e| FileError::Failed(stage, e)
}

/// Coverage of `level` under the profile's alpha test, when coverage preservation applies (spec §5.5): a stored
/// alpha channel and `mips.coverage` on (by default for `cutout` and `cutout_blend`). The same predicate gates the
/// coverage check in `validate`, so a `remove`d channel (decoding to all-255) is never compared.
fn base_coverage(level: &Rgba32F, p: &Profile, alpha_present: bool) -> Option<f64> {
    (alpha_present && p.coverage_enabled()).then(|| {
        coverage_fraction(
            level.px.iter().map(|q| q[3]),
            p.alpha.threshold,
            p.alpha.compare,
        )
    })
}

/// Peak Rec. 709 luminance of the HDR base level as the encoder sees it (channels clamped to `[0, 65504]`,
/// matching the clamp in `quantize`), for the BC6H peak check. `None` for other formats.
fn source_peak(level: &Rgba32F, p: &Profile) -> Option<f32> {
    (p.format == Format::Bc6hUf16).then(|| {
        let c = |v: f32| if v > 0.0 { v.min(65504.0) } else { 0.0 };
        level
            .px
            .iter()
            .map(|q| 0.2126 * c(q[0]) + 0.7152 * c(q[1]) + 0.0722 * c(q[2]))
            .fold(0.0f32, f32::max)
    })
}

/// Convert one file. Cancellation is checked between stages and between encode bands; once the DDS has been
/// written the file is finished (validated and recorded) rather than reported as cancelled, so a cancelled file
/// never leaves an output behind and a written output always gets its record.
pub fn process_file(
    job: &ResolvedJob,
    req: &BatchRequest,
    preset: &PresetRef,
    cancel: &CancelToken,
    rep: &Reporter,
) -> Result<FileOutcome, FileError> {
    // Test hook (Task 44): panic on the named input file to prove per-file panic isolation.
    if std::env::var_os("SDC_TEST_PANIC_ON")
        .is_some_and(|v| job.input.file_name() == Some(v.as_os_str()))
    {
        panic!("test hook: SDC_TEST_PANIC_ON");
    }
    let p = &job.profile;
    let set = Some(job.set_key.as_str());
    let path = Some(job.input.as_path());
    let stop = || {
        if cancel.is_cancelled() {
            Err(FileError::Cancelled)
        } else {
            Ok(())
        }
    };

    let source_sha256 = sha256_file(&job.input).map_err(|e| fail(Stage::Load)(e.to_string()))?;
    let (img, _) = load_image(&job.input).map_err(|e| fail(Stage::Load)(e.to_string()))?;
    stop()?;

    let work = preprocess(img, p);
    if work.nonfinite_samples > 0 {
        rep.emit(
            set,
            path,
            EventKind::Warning(
                Warning::new(
                    Code::InNonfinite,
                    format!(
                        "{} NaN or infinite source samples were replaced before processing",
                        work.nonfinite_samples
                    ),
                )
                .with_data(serde_json::json!({ "samples": work.nonfinite_samples })),
            ),
        );
    }
    stop()?;

    let mut chain = build_chain(&work, p);
    let alpha_present = work.alpha_present;
    // Spec §5.5 (Task 30): coverage runs after the mips and before quantise; for `cutout_blend` it rewrites the
    // only alpha channel.
    let base_coverage = base_coverage(&chain[0], p, alpha_present);
    if base_coverage.is_some() {
        preserve_coverage(&mut chain, p.alpha.threshold, p.alpha.compare);
    }
    let source_peak = source_peak(&chain[0], p);
    let (width, height) = (chain[0].width, chain[0].height);
    let dims: Vec<(u32, u32)> = chain.iter().map(|l| (l.width, l.height)).collect();
    // Quantise clamps mip 0 (Task 31). These exact levels are encoded and handed to Validate (Task 35).
    let inputs: Vec<EncoderInput> = chain
        .iter()
        .map(|l| to_encoder_input(l, &work, p))
        .collect();
    drop(chain);
    drop(work);
    stop()?;

    let ectx = EncodeCtx {
        cancel: Some(cancel.flag()),
    };
    let total = inputs.len() as u32;
    let mut levels = Vec::with_capacity(inputs.len());
    for (i, (inp, &(w, h))) in inputs.iter().zip(&dims).enumerate() {
        levels.push(encode_level(inp, w, h, p, &ectx).map_err(|_| FileError::Cancelled)?);
        rep.emit(
            set,
            path,
            EventKind::Progress {
                stage: Stage::Compress,
                done: i as u32 + 1,
                total,
            },
        );
    }

    let srgb = p.srgb_tag();
    let desc = DdsDesc {
        width,
        height,
        format: p.format,
        srgb,
        header: resolve_header(p.format, srgb, p.header),
    };
    let bytes = dds_bytes(&desc, &levels).map_err(|e| fail(Stage::Write)(e.to_string()))?;
    drop(levels);
    stop()?;
    write_atomic(&job.output, &bytes).map_err(|e| fail(Stage::Write)(e.to_string()))?;
    drop(bytes);

    let written = std::fs::read(&job.output).map_err(|e| fail(Stage::Validate)(e.to_string()))?;
    let report = validate(&ValidationInput {
        bytes: &written,
        profile: p,
        encoder_input: Some(&inputs),
        base_coverage,
        source_peak,
        bc1_model: req.bc1_model,
        expected_levels: Some(total),
    });
    let record = ManifestRecord {
        schema: MANIFEST_SCHEMA,
        ts: now_rfc3339(),
        engine_version: ENGINE_VERSION.into(),
        engine_commit: engine_commit().into(),
        encoders: EncoderVersions::current(),
        isa: dispatched_isa().into(),
        preset: preset.clone(),
        profile: p.clone(),
        profile_sha256: sha256_json(p).map_err(|e| fail(Stage::Write)(e.to_string()))?,
        provenance: job.provenance.clone(),
        source: job.input.to_string_lossy().replace('\\', "/"),
        source_sha256,
        sources: vec![],
        output: output_key(&req.output_root, &job.output),
        validation: report.summary.clone(),
    };
    if req.sidecar {
        let side = job.output.with_extension("dds.json");
        let text =
            serde_json::to_string_pretty(&record).map_err(|e| fail(Stage::Write)(e.to_string()))?;
        write_atomic(&side, text.as_bytes()).map_err(|e| fail(Stage::Write)(e.to_string()))?;
    }
    Ok(FileOutcome {
        output: job.output.clone(),
        report,
        record,
    })
}
