//! The `inspect` and `validate` verbs (spec §8): read-only analysis of inputs and of existing DDS files.
//!
//! Both are result verbs (DECISIONS Note J): their report goes to stdout. With `--json`, stdout carries only
//! JSON (`inspect`: the plan's events and one `dds_info` object per DDS; `validate`: one `dds_validation`
//! object per file) and `validate`'s human report moves to stderr.
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
fn dds_info(path: &Path, model: Bc1Model) -> Value {
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
                match decode_level(file.format, *w, *h, &file.data[r.clone()], Some(model)) {
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

fn print_planned(job: &Value) {
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
        "  sub-type {}  format {}  tag {}  alpha {}  output {}",
        s(&job["profile"]["subtype"]),
        s(&job["profile"]["format"]),
        s(&job["profile"]["color"]["tag"]),
        s(&job["profile"]["alpha"]["policy"]),
        s(&job["output"])
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
fn print_event(ev: &EngineEvent) {
    let path = ev.path.as_deref().unwrap_or("");
    match &ev.kind {
        EventKind::FilePlanned { job } => print_planned(job),
        EventKind::FileSkipped { code, reason } => println!(
            "{path}\n  skipped {}{reason}",
            code.map(|c| format!("{} ", c.as_str())).unwrap_or_default()
        ),
        EventKind::FileFailed { error, .. } => println!("{path}\n  cannot be read: {error}"),
        _ => {}
    }
}

/// Strips the mapped root off the front of `s`, so a path inside it reads relative to it. `root` is
/// '/'-separated and lower-case; `set` keys are lower-case already, `output` paths keep their case.
fn strip_root(s: &str, root: &str) -> Option<String> {
    let norm = s.replace('\\', "/");
    let n = root.chars().count();
    let head: String = norm.chars().take(n).collect::<String>().to_lowercase();
    let rest = if let Some(rest) = norm.strip_prefix(root) {
        rest
    } else if head == root {
        &norm[norm.char_indices().nth(n).map_or(norm.len(), |(i, _)| i)..]
    } else {
        return None;
    };
    // Only whole components: `sdc-inspect-abc` is not under `sdc-inspect-ab`.
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    Some(rest.trim_start_matches('/').to_string())
}

fn relativise(v: &mut Value, root: &str) {
    match v {
        Value::String(s) => {
            if let Some(r) = strip_root(s, root) {
                *s = r;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| relativise(x, root)),
        Value::Object(m) => m.values_mut().for_each(|x| relativise(x, root)),
        _ => {}
    }
}

/// Passes events on with every path under the throw-away output root rewritten relative to it, so the
/// output of `inspect` does not depend on the temp folder it happened to use.
struct RelativeRoot {
    root: String,
    inner: Arc<dyn EventSink>,
}

impl EventSink for RelativeRoot {
    fn emit(&self, ev: &EngineEvent) {
        let rewritten = serde_json::to_value(ev)
            .ok()
            .map(|mut v| {
                relativise(&mut v, &self.root);
                v
            })
            .and_then(|v| serde_json::from_value::<EngineEvent>(v).ok());
        self.inner.emit(rewritten.as_ref().unwrap_or(ev));
    }
}

/// `sdc inspect`: DDS headers and mip alpha for `.dds` inputs; a dry-run plan for images. Exit 0 unless the
/// command line or a config is invalid (2), or an input could not be read (1, spec §8).
pub fn inspect(a: InspectArgs) -> Result<i32, EngineError> {
    let json = a.batch.json;
    let model: Bc1Model = a.bc1_decoder.into();
    if let Some(p) = a.batch.inputs.iter().find(|p| !p.exists()) {
        return Err(EngineError::Usage(format!(
            "input not found: {}",
            p.display()
        )));
    }
    // The BC1 model is global rgbcx state; set it before any decode.
    encoders_sys::init(model).map_err(|e| EngineError::Fatal(e.to_string()))?;
    // The plan maps outputs under a root nothing is written to; a fresh temp folder keeps it unique and is
    // removed when `root` drops.
    let root = tempfile::Builder::new()
        .prefix("sdc-inspect-")
        .tempdir()
        .map_err(|e| EngineError::Fatal(format!("temp folder: {e}")))?;
    let found = discover(&a.batch.inputs, root.path(), a.batch.recursive)
        .map_err(|e| EngineError::Usage(format!("discover: {e}")))?;
    let mut unreadable = 0u32;
    for f in found.iter().filter(|f| f.kind == InputKind::Dds) {
        let v = dds_info(&f.path, model);
        unreadable += u32::from(v["error"].is_string());
        if json {
            println!("{v}");
        } else {
            print_dds_info(&v);
        }
    }
    if !found.iter().any(|f| f.kind == InputKind::Image) {
        return Ok(i32::from(unreadable > 0));
    }

    let mut req = build_request(&a.batch, root.path().to_path_buf()).map_err(EngineError::Usage)?;
    req.dry_run = true;
    req.bc1_model = model;
    let events = Arc::new(VecSink::default());
    let mut sinks: Vec<Arc<dyn EventSink>> = vec![events.clone()];
    if json {
        sinks.push(Arc::new(JsonLinesSink::new(std::io::stdout())));
    }
    let sink = RelativeRoot {
        root: root
            .path()
            .to_string_lossy()
            .replace('\\', "/")
            .to_lowercase(),
        inner: Arc::new(TeeSink(sinks)),
    };
    let rep = Reporter::new(Arc::new(sink), new_batch_id());
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
            print_event(&ev);
        }
    }
    Ok(i32::from(unreadable > 0 || !plan.failed.is_empty()))
}

/// Mip-0 decoded alpha below this is "varying" for sub-type inference: under the 251/255 complex gate of
/// spec §5.8 (BC7's 254 is not).
const VARYING_ALPHA_BELOW: u8 = 250;

/// Mip-0 decoded alpha at or above this is flat opaque: the file carries no alpha worth keeping, so it is
/// held to the `remove` guarantee (spec §5.8: decoded alpha >= 254 for BC7, 255 otherwise). It is the
/// guarantee's own bound, so a file that passes it is never judged as `data` by mistake, and one that stores
/// even a single lower texel is treated as storing alpha.
const FLAT_ALPHA_MIN: u8 = 254;

/// The smallest decoded alpha of mip 0, for formats that carry alpha (`None` for others and on decode errors).
fn mip0_alpha_min(file: &DdsFile, model: Bc1Model) -> Option<u8> {
    if !carries_alpha(file.format) {
        return None;
    }
    let (w, h, r) = file.levels.first()?;
    match decode_level(file.format, *w, *h, &file.data[r.clone()], Some(model)) {
        Ok(Decoded::Ldr(px)) => Some(alpha_stats(&px).min),
        _ => None,
    }
}

/// The sub-type a DDS is meant to be when `--subtype` is not given: the top suggestion for its name, path and
/// (for `_m`) stored alpha, else `generic_data`.
fn infer_subtype(path: &Path, file: &DdsFile, target: Target, alpha_min: Option<u8>) -> Subtype {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // No user presets are loaded, so there are no suffix aliases to resolve.
    let suffix = split_suffix_with(&stem, &BTreeMap::new()).suffix;
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let rel = PathBuf::from(path.file_name().unwrap_or_default());
    let alpha = if !carries_alpha(file.format) {
        AlphaContent::None
    } else if alpha_min.is_some_and(|m| m < VARYING_ALPHA_BELOW) {
        AlphaContent::Varying
    } else {
        AlphaContent::Constant255
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

/// An inferred sub-type that the target does not offer is judged as its fallback (as Resolve's
/// `--allow CFG-UNAVAILABLE` would), or as `generic_data` when it has none; the second value says so.
fn available_or_fallback(st: Subtype, target: Target) -> (Subtype, Option<String>) {
    match availability(target, st) {
        Availability::Available => (st, None),
        Availability::Unavailable { fallback } => {
            let (to, how) = match fallback {
                Some(fb) => (fb, format!("its fallback {}", fb.id())),
                None => (Subtype::GenericData, "generic_data (no fallback)".into()),
            };
            let note = format!(
                "inferred {} is unavailable under {target:?}; judged as {how}",
                st.id()
            );
            (to, Some(note))
        }
    }
}

/// The alpha policy to judge a file by when `--alpha-policy` is not given. A file whose mip-0 alpha is flat
/// opaque (>= `FLAT_ALPHA_MIN`) is `remove`; one that stores alpha keeps the sub-type's own policy when that
/// keeps alpha, else `data`; a file without alpha keeps the sub-type's policy.
fn infer_alpha_policy(alpha_min: Option<u8>, subtype_default: AlphaPolicy) -> AlphaPolicy {
    match alpha_min {
        Some(m) if m >= FLAT_ALPHA_MIN => AlphaPolicy::Remove,
        Some(_) if subtype_default == AlphaPolicy::Remove => AlphaPolicy::Data,
        _ => subtype_default,
    }
}

/// The report for a file in a format sdc never writes (BC2, signed BC6H, uncompressed): there is no profile
/// format to judge it against, so only its header is checked.
fn header_only_report(file: &DdsFile) -> ValidationReport {
    let warnings: Vec<Warning> = check_header(file)
        .into_iter()
        .map(|m| Warning::new(Code::ValHeader, m))
        .collect();
    ValidationReport {
        summary: ValidationSummary {
            passed: warnings.is_empty(),
            levels: file.mip_count,
            ..Default::default()
        },
        warnings,
    }
}

/// Result-verb output: stdout, or stderr when stdout is reserved for JSON.
fn say(json: bool, line: String) {
    if json {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

/// `sdc validate`: the spec §5.8 checks on existing DDS files. The profile comes from the sub-type and the
/// target, but its format, `_SRGB` tag and alpha policy from the file, so third-party files are judged on the
/// header rules and the alpha rules rather than on sdc's preferred format. A format sdc never writes only has
/// its header checked. Exit 1 if a file cannot be read, else 4 if one fails validation, else 0.
pub fn validate_files(a: ValidateArgs) -> Result<i32, EngineError> {
    let target = a.target.map_or(Target::Cs, Target::from);
    let model: Bc1Model = a.bc1_decoder.into();
    let json = a.json;
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
    let explicit_policy: Option<AlphaPolicy> = a
        .alpha_policy
        .as_ref()
        .map(|id| {
            serde_json::from_value(Value::String(id.clone()))
                .map_err(|_| EngineError::Usage(format!("--alpha-policy {id}: unknown policy")))
        })
        .transpose()?;
    let (mut unreadable, mut failed, mut not_judged) = (0u32, 0u32, 0u32);
    let total = a.dds.len();
    for path in &a.dds {
        let path_s = path.to_string_lossy().replace('\\', "/");
        let (bytes, file) = match read_file(path) {
            Ok(r) => r,
            Err(e) => {
                say(json, format!("FAIL  {path_s}: cannot be read: {e}"));
                if json {
                    println!(
                        "{}",
                        json!({ "kind": "dds_validation", "path": path_s, "readable": false, "passed": false, "error": e })
                    );
                }
                unreadable += 1;
                continue;
            }
        };
        let alpha_min = mip0_alpha_min(&file, model);
        let (st, mut note) = match explicit {
            Some(st) => (st, None),
            None => available_or_fallback(infer_subtype(path, &file, target, alpha_min), target),
        };
        let mut policy = None;
        let mut judged = true;
        let report = match file.format.as_format() {
            None => {
                judged = false;
                let (name, _) = format_name(file.format);
                note = Some(format!("not judged: format not produced by sdc ({name})"));
                header_only_report(&file)
            }
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
                p.alpha.policy = explicit_policy
                    .unwrap_or_else(|| infer_alpha_policy(alpha_min, p.alpha.policy));
                policy = Some(p.alpha.policy);
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
        let passed = report.summary.passed;
        let alpha_part = match policy {
            Some(p) if carries_alpha(file.format) => format!(", alpha {}", policy_id(p)),
            _ => String::new(),
        };
        let word = match (passed, judged) {
            (false, _) => "FAIL ",
            (true, true) => "ok   ",
            (true, false) => "skip ",
        };
        say(json, format!("{word} {path_s} ({}{alpha_part})", st.id()));
        if let Some(n) = &note {
            say(json, format!("  note: {n}"));
        }
        for w in &report.warnings {
            say(
                json,
                format!(
                    "  {} {} {}",
                    format!("{:?}", w.severity).to_lowercase(),
                    w.code.as_str(),
                    w.message
                ),
            );
        }
        if json {
            println!(
                "{}",
                json!({
                    "kind": "dds_validation", "path": path_s, "readable": true, "subtype": st,
                    "alpha_policy": policy, "judged": judged, "note": note,
                    "passed": passed, "summary": report.summary, "warnings": report.warnings,
                })
            );
        }
        failed += u32::from(!passed);
        not_judged += u32::from(passed && !judged);
    }
    let ok = total as u32 - failed - unreadable - not_judged;
    let skipped = if not_judged > 0 {
        format!(", {not_judged} not judged")
    } else {
        String::new()
    };
    say(
        json,
        format!("validated {total}: {ok} ok, {failed} failed validation, {unreadable} unreadable{skipped}"),
    );
    Ok(if unreadable > 0 {
        1
    } else if failed > 0 {
        4
    } else {
        0
    })
}

fn policy_id(p: AlphaPolicy) -> String {
    match serde_json::to_value(p) {
        Ok(Value::String(s)) => s,
        _ => format!("{p:?}"),
    }
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

    #[test]
    fn alpha_policy_is_inferred_from_the_stored_alpha() {
        use AlphaPolicy::*;
        // Flat opaque (BC7's 254 included) is `remove`, whatever the sub-type's default.
        assert_eq!(infer_alpha_policy(Some(255), Data), Remove);
        assert_eq!(infer_alpha_policy(Some(254), Blend), Remove);
        // A single texel below 254 means the file stores alpha: `data` where the default would remove it.
        assert_eq!(infer_alpha_policy(Some(239), Remove), Data);
        assert_eq!(infer_alpha_policy(Some(253), Remove), Data);
        assert_eq!(infer_alpha_policy(Some(0), Cutout), Cutout);
        assert_eq!(infer_alpha_policy(Some(0), CutoutBlend), CutoutBlend);
        // No alpha in the format: the sub-type's own policy.
        assert_eq!(infer_alpha_policy(None, Remove), Remove);
        assert_eq!(infer_alpha_policy(None, Data), Data);
    }

    #[test]
    fn strip_root_handles_case_separators_and_foreign_paths() {
        let root = "c:/users/x/temp/sdc-inspect-ab";
        let s = |v| strip_root(v, root);
        assert_eq!(
            s("C:\\Users\\X\\Temp\\sdc-inspect-AB\\a\\Rock_N.dds").as_deref(),
            Some("a/Rock_N.dds")
        );
        assert_eq!(
            s("c:/users/x/temp/sdc-inspect-ab/a/rock_n").as_deref(),
            Some("a/rock_n")
        );
        assert_eq!(s("in/rock_n.png"), None);
        assert_eq!(s("c:/users/x/temp/sdc-inspect-abc/a"), None);
    }
}
