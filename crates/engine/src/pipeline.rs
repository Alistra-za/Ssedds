//! One `ResolvedJob` end to end (spec §5, carry order of Task 31):
//! load → preprocess → mips → coverage → quantise → encode → DDS bytes → validate the bytes → atomic write →
//! sidecar → manifest record.
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
    bc1_model_id, dispatched_isa, engine_commit, now_rfc3339, output_key, sha256_file, sha256_json,
    EncoderVersions, Manifest, ManifestRecord, PresetRef, ENGINE_VERSION, MANIFEST_SCHEMA,
};
use crate::mips::build_chain;
use crate::ops::preprocess;
use crate::policy::ResolvedJob;
use crate::quantize::{to_encoder_input, EncoderInput};
use crate::validate::{validate, ValidationInput, ValidationReport};
use std::cell::Cell;
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

/// What the caller needs when `process_file` fails or panics: the last stage the file entered (the stage a panic
/// is reported at) and whether its output has been written (so the caller removes it; see `process_file`).
#[derive(Debug)]
pub struct FileTracker {
    pub stage: Cell<Stage>,
    pub written: Cell<bool>,
}

impl Default for FileTracker {
    fn default() -> Self {
        FileTracker {
            stage: Cell::new(Stage::Load),
            written: Cell::new(false),
        }
    }
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

/// Test hook (Task 44): when `SDC_TEST_FAIL_VALIDATION` names the input's file stem, add one `VAL-HEADER` error to
/// its validation report, so a written output fails validation (exit 4). No setting reaches an error-severity
/// `VAL-*` code (`bc1_transparent_black` is `CFG-INVALID` with BC1, DECISIONS Note F), so the exit-4 path has no
/// other deterministic trigger. Compiled only with the `test-hooks` cargo feature (off in release builds).
#[cfg(feature = "test-hooks")]
fn force_validation_failure_for_test(job: &ResolvedJob, report: &mut ValidationReport) {
    if std::env::var_os("SDC_TEST_FAIL_VALIDATION")
        .is_some_and(|v| job.input.file_stem() == Some(v.as_os_str()))
    {
        report.warnings.push(Warning::new(
            Code::ValHeader,
            "test hook: SDC_TEST_FAIL_VALIDATION",
        ));
        report.summary.passed = false;
    }
}

/// Convert one file and append its manifest record. Cancellation is checked between stages and between encode
/// bands, last just before the write, so a cancelled file never leaves an output behind. Everything that can fail
/// before the output exists (validation of the in-memory DDS bytes, the profile hash, the record and sidecar text)
/// runs before the write; only the sidecar write and the manifest append come after it. When one of those fails,
/// or anything panics, `tracker.written` tells the caller to remove the output, so an output on disk has its
/// record, except after a second Ctrl-C: that aborts the process (exit 3) and can leave `.sdc-*.tmp` files, or an
/// output without a record (`OUT-UNKNOWN-EXISTING` on the next run). A failed validation (`passed = false`) is not a failure here: the output is written and recorded.
pub fn process_file(
    job: &ResolvedJob,
    req: &BatchRequest,
    preset: &PresetRef,
    cancel: &CancelToken,
    rep: &Reporter,
    manifest: &Manifest,
    tracker: &FileTracker,
) -> Result<FileOutcome, FileError> {
    let enter = |st: Stage| tracker.stage.set(st);
    enter(Stage::Load);
    // Test hook (Task 44, `test-hooks` feature only): panic on the named input file to prove per-file panic isolation.
    #[cfg(feature = "test-hooks")]
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

    enter(Stage::Preprocess);
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

    enter(Stage::Mipmaps);
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

    enter(Stage::Compress);
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

    enter(Stage::Write);
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

    // Validate the exact bytes about to be written: `validate` parses them with the engine's DDS reader (spec §5.8).
    enter(Stage::Validate);
    #[cfg_attr(not(feature = "test-hooks"), allow(unused_mut))]
    // only the test hook mutates the report
    let mut report = validate(&ValidationInput {
        bytes: &bytes,
        profile: p,
        encoder_input: Some(&inputs),
        base_coverage,
        source_peak,
        bc1_model: req.bc1_model,
        expected_levels: Some(total),
    });
    drop(inputs);
    #[cfg(feature = "test-hooks")]
    force_validation_failure_for_test(job, &mut report);
    enter(Stage::Write);
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
        bc1_model: Some(bc1_model_id(req.bc1_model).into()),
    };
    let sidecar = req
        .sidecar
        .then(|| serde_json::to_string_pretty(&record))
        .transpose()
        .map_err(|e| fail(Stage::Write)(e.to_string()))?;
    stop()?;

    write_atomic(&job.output, &bytes).map_err(|e| fail(Stage::Write)(e.to_string()))?;
    tracker.written.set(true);
    drop(bytes);
    if let Some(text) = sidecar {
        write_atomic(&job.output.with_extension("dds.json"), text.as_bytes())
            .map_err(|e| fail(Stage::Write)(format!("sidecar: {e}")))?;
    }
    manifest
        .append(&record)
        .map_err(|e| fail(Stage::Write)(format!("manifest {}: {e}", manifest.path().display())))?;
    Ok(FileOutcome {
        output: job.output.clone(),
        report,
        record,
    })
}
