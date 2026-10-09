//! The `inspect` and `validate` verbs (spec §8): read-only analysis of inputs and of existing DDS files.
//!
//! `inspect` prints its report on stdout (or JSON Lines with `--json`, then nothing else). `validate` logs on
//! stderr and, with `--json`, writes one `dds_validation` object per file on stdout.
use crate::args::*;
use crate::human::TeeSink;
use encoders_sys::Bc1Model;
use engine::api::{Engine, EngineError};
use engine::codes::{Code, Warning};
use engine::config::*;
use engine::dds::{check_header, read_dds, DdsFile, DdsPixelFormat};
use engine::decode::{decode_level, Decoded};
use engine::discover::{discover, game_path, InputKind};
use engine::events::{
    new_batch_id, EngineEvent, EventKind, EventSink, JsonLinesSink, Reporter, VecSink,
};
use engine::group::split_suffix_with;
use engine::policy::{availability, Availability};
use engine::presets::{default_preset_for, AlphaContent};
use engine::suggest::{suggest, SuggestContext};
use engine::validate::alpha::{alpha_stats, AlphaStats};
use engine::validate::{validate, ValidationReport, ValidationSummary};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Formats whose decoded RGBA carries a meaningful alpha channel.
fn carries_alpha(f: DdsPixelFormat) -> bool {
    use DdsPixelFormat::*;
    matches!(
        f,
        Bc1 { .. }
            | Bc2 { .. }
            | Bc3 { .. }
            | Bc7 { .. }
            | Rgba8 { .. }
            | Bgra8
            | Rgba16f
            | Rgba32f
    )
}

/// `("Bc7", Some(false))`: the format's name and, where it has one, its `_SRGB` tag.
fn format_name(f: DdsPixelFormat) -> (String, Option<bool>) {
    match serde_json::to_value(f) {
        Ok(Value::String(s)) => (s, None),
        Ok(Value::Object(m)) => m.into_iter().next().map_or_else(
            || (format!("{f:?}"), None),
            |(k, v)| (k, v.get("srgb").and_then(Value::as_bool)),
        ),
        _ => (format!("{f:?}"), None),
    }
}

fn read_file(path: &Path) -> Result<(Vec<u8>, DdsFile), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let file = read_dds(&bytes).map_err(|e| e.to_string())?;
    Ok((bytes, file))
}

/// The `dds_info` object for one file: header facts and per-mip decoded-alpha statistics.
fn dds_info(path: &Path) -> Value {
    let path_s = path.to_string_lossy().replace('\\', "/");
    let file = match read_file(path) {
        Ok((_, f)) => f,
        Err(e) => return json!({ "kind": "dds_info", "path": path_s, "error": e }),
    };
    let mips: Vec<Value> = file
        .levels
        .iter()
        .map(|(w, h, r)| {
            let mut m = json!({ "width": w, "height": h });
            if carries_alpha(file.format) {
                match decode_level(file.format, *w, *h, &file.data[r.clone()], None) {
                    Ok(Decoded::Ldr(px)) => m["alpha"] = json!(alpha_stats(&px)),
                    Ok(Decoded::Hdr(_)) => {}
                    Err(e) => m["decode_error"] = json!(e.to_string()),
                }
            }
            m
        })
        .collect();
    let (format, srgb) = format_name(file.format);
    json!({
        "kind": "dds_info", "path": path_s, "format": format, "srgb": srgb,
        "header": file.header_kind, "width": file.width, "height": file.height,
        "mip_count": file.mip_count, "cubemap": file.is_cubemap,
        "header_issues": check_header(&file), "mips": mips,
    })
}

fn pct(n: u64, of: u64) -> String {
    format!(
        "{:.1}%",
        if of == 0 {
            0.0
        } else {
            n as f64 * 100.0 / of as f64
        }
    )
}

fn print_dds_info(v: &Value) {
    println!("{}", v["path"].as_str().unwrap_or(""));
    if let Some(e) = v["error"].as_str() {
        println!("  cannot be read: {e}");
        return;
    }
    let srgb = match v["srgb"].as_bool() {
        Some(true) => " srgb",
        Some(false) => " unorm",
        None => "",
    };
    println!(
        "  format {}{srgb}  {}x{}  {} mips  {} header  cubemap {}",
        v["format"].as_str().unwrap_or(""),
        v["width"],
        v["height"],
        v["mip_count"],
        v["header"].as_str().unwrap_or(""),
        v["cubemap"]
    );
    for issue in v["header_issues"].as_array().into_iter().flatten() {
        println!("  header issue: {}", issue.as_str().unwrap_or(""));
    }
    for (i, m) in v["mips"].as_array().into_iter().flatten().enumerate() {
        let alpha = match serde_json::from_value::<AlphaStats>(m["alpha"].clone()) {
            Ok(a) => format!(
                "  alpha min {} =255 {} =254 {} <254 {}",
                a.min,
                pct(a.eq255, a.pixels),
                pct(a.eq254, a.pixels),
                pct(a.below254, a.pixels)
            ),
            Err(_) => m["decode_error"]
                .as_str()
                .map(|e| format!("  decode failed: {e}"))
                .unwrap_or_default(),
        };
        println!("  mip {i} {}x{}{alpha}", m["width"], m["height"]);
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("-")
}

fn print_planned(job: &Value, root: &Path) {
    let output = job["output"].as_str().map(Path::new).map_or_else(
        || "-".to_string(),
        |o| {
            o.strip_prefix(root)
                .unwrap_or(o)
                .to_string_lossy()
                .replace('\\', "/")
        },
    );
    println!("{}", s(&job["input"]));
    match job["suggestions"].get(0) {
        Some(g) => println!(
            "  suggested {} ({} confidence: {})",
            s(&g["subtype"]),
            s(&g["confidence"]),
            s(&g["reason"])
        ),
        None => println!("  no suggestion"),
    }
    println!(
        "  sub-type {}  format {}  tag {}  alpha {}  output {output}",
        s(&job["profile"]["subtype"]),
        s(&job["profile"]["format"]),
        s(&job["profile"]["color"]["tag"]),
        s(&job["profile"]["alpha"]["policy"])
    );
    let status = &job["status"];
    let detail: Vec<&str> = ["codes", "reasons"]
        .iter()
        .flat_map(|k| status[k].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    println!("  status {} {}", s(&status["state"]), detail.join("; "));
    for w in job["warnings"].as_array().into_iter().flatten() {
        println!(
            "  {} {} {}",
            s(&w["severity"]),
            s(&w["code"]),
            s(&w["message"])
        );
    }
}

/// Plan-time events as human text. Warnings are listed under their `file_planned`; skips and failures are
/// the lines `convert` would report for the same files.
fn print_event(ev: &EngineEvent, root: &Path) {
    let path = ev.path.as_deref().unwrap_or("");
    match &ev.kind {
        EventKind::FilePlanned { job } => print_planned(job, root),
        EventKind::FileSkipped { code, reason } => println!(
            "{path}\n  skipped {}{reason}",
            code.map(|c| format!("{} ", c.as_str())).unwrap_or_default()
        ),
        EventKind::FileFailed { error, .. } => println!("{path}\n  cannot be read: {error}"),
        _ => {}
    }
}

/// `sdc inspect`: DDS headers and mip alpha for `.dds` inputs; a dry-run plan for images. Exit 0 unless the
/// command line or a config is invalid (a DDS or image that cannot be read is reported, not an error).
pub fn inspect(a: InspectArgs) -> Result<i32, EngineError> {
    let json = a.batch.json;
    if let Some(p) = a.batch.inputs.iter().find(|p| !p.exists()) {
        return Err(EngineError::Usage(format!(
            "input not found: {}",
            p.display()
        )));
    }
    // The plan maps outputs under a root nothing is written to; a fresh temp folder keeps it unique and is
    // removed when `root` drops.
    let root = tempfile::Builder::new()
        .prefix("sdc-inspect-")
        .tempdir()
        .map_err(|e| EngineError::Fatal(format!("temp folder: {e}")))?;
    let found = discover(&a.batch.inputs, root.path(), a.batch.recursive)
        .map_err(|e| EngineError::Usage(format!("discover: {e}")))?;
    for f in found.iter().filter(|f| f.kind == InputKind::Dds) {
        let v = dds_info(&f.path);
        if json {
            println!("{v}");
        } else {
            print_dds_info(&v);
        }
    }
    if !found.iter().any(|f| f.kind == InputKind::Image) {
        return Ok(0);
    }

    let mut req = build_request(&a.batch, root.path().to_path_buf()).map_err(EngineError::Usage)?;
    req.dry_run = true;
    let events = Arc::new(VecSink::default());
    let mut sinks: Vec<Arc<dyn EventSink>> = vec![events.clone()];
    if json {
        sinks.push(Arc::new(JsonLinesSink::new(std::io::stdout())));
    }
    let rep = Reporter::new(Arc::new(TeeSink(sinks)), new_batch_id());
    let plan = Engine::new()?.plan(req, &rep)?;
    // What `run` would report for the files `plan` set aside. DDS files are inspected above, not skipped.
    for sk in plan
        .skipped
        .iter()
        .filter(|sk| sk.code != Some(Code::InDdsSkipped))
    {
        rep.emit(
            sk.set.as_deref(),
            Some(&sk.path),
            EventKind::FileSkipped {
                code: sk.code,
                reason: sk.reason.clone(),
            },
        );
    }
    for f in &plan.failed {
        rep.emit(
            None,
            Some(&f.path),
            EventKind::FileFailed {
                stage: f.stage,
                error: f.error.clone(),
            },
        );
    }
    if !json {
        for ev in events.take() {
            print_event(&ev, root.path());
        }
    }
    Ok(0)
}

/// The sub-type a DDS is meant to be when `--subtype` is not given: the top suggestion for its name, path and
/// (for `_m`) stored alpha, else `generic_data`.
fn infer_subtype(path: &Path, file: &DdsFile, target: Target, model: Bc1Model) -> Subtype {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // No user presets are loaded, so there are no suffix aliases to resolve.
    let suffix = split_suffix_with(&stem, &BTreeMap::new()).suffix;
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let rel = PathBuf::from(path.file_name().unwrap_or_default());
    let alpha =
        if !carries_alpha(file.format) {
            AlphaContent::None
        } else {
            match file.levels.first().map(|(w, h, r)| {
                decode_level(file.format, *w, *h, &file.data[r.clone()], Some(model))
            }) {
                // Below the 251/255 complex gate (spec §5.8) counts as varying; BC7's 254 does not.
                Some(Ok(Decoded::Ldr(px))) if alpha_stats(&px).min < 250 => AlphaContent::Varying,
                _ => AlphaContent::Constant255,
            }
        };
    let set = BTreeSet::new();
    let ctx = SuggestContext {
        target,
        suffix: suffix.as_deref(),
        game_path: &game_path(&abs, &rel),
        set_suffixes: &set,
        alpha,
        is_float: matches!(
            file.format,
            DdsPixelFormat::Rgba16f | DdsPixelFormat::Rgba32f | DdsPixelFormat::Bc6hUf16
        ),
    };
    suggest(&ctx)
        .first()
        .map_or(Subtype::GenericData, |g| g.subtype)
}

/// The report for a file in a format sdc never writes (BC2, signed BC6H, uncompressed): there is no profile
/// format to judge it against.
fn unwritable_format(file: &DdsFile) -> ValidationReport {
    let (name, _) = format_name(file.format);
    let w = Warning::new(
        Code::ValHeader,
        format!("format {name} is not one sdc writes (BC1, BC3, BC4, BC5, BC6H UF16, BC7)"),
    );
    ValidationReport {
        summary: ValidationSummary {
            passed: false,
            levels: file.mip_count,
            ..Default::default()
        },
        warnings: vec![w],
    }
}

/// `sdc validate`: the spec §5.8 checks on existing DDS files. The profile comes from the sub-type and the
/// target, but its format and `_SRGB` tag from the file, so third-party files are judged on the header rules and
/// the alpha rules rather than on sdc's preferred format. Exit 1 if a file cannot be read, else 4 if one fails
/// validation, else 0.
pub fn validate_files(a: ValidateArgs) -> Result<i32, EngineError> {
    let target = a.target.map_or(Target::Cs, Target::from);
    let model: Bc1Model = a.bc1_decoder.into();
    // The BC1 model is global rgbcx state; set it before any decode.
    encoders_sys::init(model).map_err(|e| EngineError::Fatal(e.to_string()))?;
    let reg = Engine::new()?.registry().clone();
    let explicit: Option<Subtype> = a
        .subtype
        .as_ref()
        .map(|id| {
            serde_json::from_value(Value::String(id.clone()))
                .map_err(|_| EngineError::Usage(format!("--subtype {id}: unknown sub-type")))
        })
        .transpose()?;
    if let Some(st) =
        explicit.filter(|&st| !matches!(availability(target, st), Availability::Available))
    {
        return Err(EngineError::Usage(format!(
            "--subtype {} is unavailable under {target:?} (CFG-UNAVAILABLE)",
            st.id()
        )));
    }
    let (mut unreadable, mut failed) = (0u32, 0u32);
    let total = a.dds.len();
    for path in &a.dds {
        let path_s = path.to_string_lossy().replace('\\', "/");
        let (bytes, file) = match read_file(path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("FAIL  {path_s}: cannot be read: {e}");
                if a.json {
                    println!(
                        "{}",
                        json!({ "kind": "dds_validation", "path": path_s, "readable": false, "passed": false, "error": e })
                    );
                }
                unreadable += 1;
                continue;
            }
        };
        let st = explicit.unwrap_or_else(|| infer_subtype(path, &file, target, model));
        let report = match file.format.as_format() {
            None => unwritable_format(&file),
            Some((format, srgb)) => {
                let mut p = reg
                    .profile(default_preset_for(target), target, st)
                    .map_err(|e| EngineError::Fatal(e.to_string()))?;
                p.format = format;
                p.color.tag = match (format, srgb) {
                    (Format::Bc6hUf16, _) => ColorTag::NotApplicable,
                    (_, true) => ColorTag::Srgb,
                    (_, false) => ColorTag::Unorm,
                };
                validate(&engine::validate::ValidationInput {
                    bytes: &bytes,
                    profile: &p,
                    encoder_input: None,
                    base_coverage: None,
                    source_peak: None,
                    bc1_model: model,
                    expected_levels: None,
                })
            }
        };
        eprintln!(
            "{} {path_s} ({})",
            if report.summary.passed {
                "ok   "
            } else {
                "FAIL "
            },
            st.id()
        );
        for w in &report.warnings {
            eprintln!(
                "  {} {} {}",
                format!("{:?}", w.severity).to_lowercase(),
                w.code.as_str(),
                w.message
            );
        }
        if a.json {
            println!(
                "{}",
                json!({
                    "kind": "dds_validation", "path": path_s, "readable": true, "subtype": st,
                    "passed": report.summary.passed, "summary": report.summary, "warnings": report.warnings,
                })
            );
        }
        failed += u32::from(!report.summary.passed);
    }
    eprintln!(
        "validated {total}: {} ok, {failed} failed validation, {unreadable} unreadable",
        total as u32 - failed - unreadable
    );
    Ok(if unreadable > 0 {
        1
    } else if failed > 0 {
        4
    } else {
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_name_splits_the_srgb_tag() {
        assert_eq!(
            format_name(DdsPixelFormat::Bc7 { srgb: true }),
            ("Bc7".into(), Some(true))
        );
        assert_eq!(format_name(DdsPixelFormat::Bc5), ("Bc5".into(), None));
        assert_eq!(format_name(DdsPixelFormat::Bgra8), ("Bgra8".into(), None));
    }
}
