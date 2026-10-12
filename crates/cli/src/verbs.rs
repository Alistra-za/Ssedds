//! The `inspect` and `validate` verbs (spec §8): read-only analysis of inputs and of existing DDS files.
//!
//! Both are result verbs (DECISIONS Note J): their report goes to stdout. With `--json`, stdout carries only
//! JSON (`inspect`: the plan's events and one `dds_info` object per DDS; `validate`: one `dds_validation`
//! object per file) and `validate`'s human report moves to stderr.
use crate::args::*;
use crate::human::TeeSink;
use encoders_sys::Bc1Model;
use engine::api::{CancelToken, Engine, EngineError};
use engine::codes::{Code, Warning};
use engine::config::*;
use engine::dds::{check_header, read_dds, DdsFile, DdsPixelFormat};
use engine::decode::{decode_level, Decoded};
use engine::discover::{discover, game_path, InputKind};
use engine::events::{
    new_batch_id, EngineEvent, EventKind, EventSink, JsonLinesSink, Reporter, SkipKind, VecSink,
};
use engine::group::split_suffix_with;
use engine::manifest::output_key;
use engine::policy::{availability, Availability, JobStatus, ResolvedJob};
use engine::presets::{default_preset_for, AlphaContent};
use engine::reports::{
    DdsHeaderInfo, DdsInfo, DdsInfoKind, DdsMipInfo, DdsUnreadable, DdsValidation,
    DdsValidationKind, DdsValidationUnreadable, DdsVerdict, REPORT_SCHEMA,
};
use engine::schemas::SchemaKind;
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
            | Bgra8 { .. }
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
fn dds_info(path: &Path, model: Bc1Model) -> DdsInfo {
    let path_s = path.to_string_lossy().replace('\\', "/");
    let file = match read_file(path) {
        Ok((_, f)) => f,
        Err(error) => {
            return DdsInfo::Unreadable(DdsUnreadable {
                kind: DdsInfoKind::DdsInfo,
                schema: REPORT_SCHEMA,
                path: path_s,
                error,
            })
        }
    };
    let mips: Vec<DdsMipInfo> = file
        .levels
        .iter()
        .map(|(w, h, r)| {
            let mut m = DdsMipInfo {
                width: *w,
                height: *h,
                alpha: None,
                decode_error: None,
            };
            if carries_alpha(file.format) {
                match decode_level(file.format, *w, *h, &file.data[r.clone()], Some(model)) {
                    Ok(Decoded::Ldr(px)) => m.alpha = Some(alpha_stats(&px)),
                    Ok(Decoded::Hdr(_)) => {}
                    Err(e) => m.decode_error = Some(e.to_string()),
                }
            }
            m
        })
        .collect();
    let (format, srgb) = format_name(file.format);
    DdsInfo::Info(DdsHeaderInfo {
        kind: DdsInfoKind::DdsInfo,
        schema: REPORT_SCHEMA,
        path: path_s,
        format,
        srgb,
        header: file.header_kind,
        width: file.width,
        height: file.height,
        mip_count: file.mip_count,
        cubemap: file.is_cubemap,
        header_issues: check_header(&file),
        mips,
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

/// The serde string form of an enum value (`"bc7"`, `"high"`), as the JSON events carry it.
macro_rules! tag {
    ($v:expr) => {
        match serde_json::to_value($v) {
            Ok(Value::String(t)) => t,
            _ => "-".to_string(),
        }
    };
}

fn print_planned(job: &ResolvedJob) {
    println!("{}", job.input.display());
    match job.suggestions.first() {
        Some(g) => println!(
            "  suggested {} ({} confidence: {})",
            g.subtype.id(),
            tag!(&g.confidence),
            g.reason
        ),
        None => println!("  no suggestion"),
    }
    println!(
        "  sub-type {}  format {}  tag {}  alpha {}  output {}",
        job.profile.subtype.id(),
        tag!(&job.profile.format),
        tag!(&job.profile.color.tag),
        tag!(&job.profile.alpha.policy),
        job.output.display()
    );
    let detail: Vec<String> = match &job.status {
        JobStatus::Unresolved { reasons, .. } => reasons.clone(),
        JobStatus::Blocked { codes } | JobStatus::Error { codes } => {
            codes.iter().map(|c| c.as_str().to_string()).collect()
        }
        JobStatus::Ready | JobStatus::PackingInput { .. } => vec![],
    };
    let state = serde_json::to_value(&job.status)
        .ok()
        .and_then(|v| v["state"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "-".into());
    println!("  status {state} {}", detail.join("; "));
    for w in &job.warnings {
        println!("  {} {} {}", tag!(&w.severity), w.code.as_str(), w.message);
    }
}

/// Plan-time events as human text. Warnings are listed under their `file_planned`; skips and failures are
/// the lines `convert` would report for the same files.
fn print_event(ev: &EngineEvent) {
    let path = ev.path.as_deref().unwrap_or("");
    match &ev.kind {
        EventKind::FilePlanned { job } => print_planned(job),
        EventKind::FileSkipped {
            kind: SkipKind::PackedSource,
            ..
        } => {}
        EventKind::FileSkipped { code, reason, .. } => println!(
            "{path}\n  skipped {}{reason}",
            code.map(|c| format!("{} ", c.as_str())).unwrap_or_default()
        ),
        EventKind::FileFailed { error, .. } => println!("{path}\n  cannot be read: {error}"),
        // A batch-level warning (CFG-PRESET-SKIPPED).
        EventKind::Warning(w) => {
            println!("{} {} {}", tag!(&w.severity), w.code.as_str(), w.message)
        }
        EventKind::SetResolved {
            recipes,
            shader,
            warnings,
            ..
        } => {
            let shown = shader
                .as_ref()
                .map(|p| format!("  shader {} ({})", p.shader.id(), p.provenance))
                .unwrap_or_default();
            println!("set {}{shown}", ev.set.as_deref().unwrap_or(""));
            for w in warnings {
                println!("  {} {} {}", tag!(&w.severity), w.code.as_str(), w.message);
            }
            for r in recipes {
                let status = serde_json::to_value(r.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let reason = r
                    .reason
                    .as_deref()
                    .map(|x| format!(": {x}"))
                    .unwrap_or_default();
                println!("  pack {} ({}) {status}{reason}", r.output, r.subtype.id());
                let channels: Vec<String> = ["R", "G", "B", "A"]
                    .iter()
                    .zip(&r.channels)
                    .filter(|(_, p)| !p.is_empty())
                    .map(|(c, p)| format!("{c} {p}"))
                    .collect();
                println!("    {}", channels.join("  "));
            }
        }
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
    // The plan maps outputs under a root nothing is written to; a fresh temp folder keeps it unique and is
    // removed when `root` drops.
    let root = tempfile::Builder::new()
        .prefix("sdc-inspect-")
        .tempdir()
        .map_err(|e| EngineError::Fatal(format!("temp folder: {e}")))?;
    // Codex-CLI 2: the shared batch options and every config are checked before any output, even when only DDS
    // files (or nothing) are found and no plan runs.
    let mut req = build_request(&a.batch, root.path().to_path_buf()).map_err(EngineError::Usage)?;
    // A DDS is reported once, by its header below, never also as a re-encode job (DECISIONS S6): drop every
    // command-line `reencode`; the unscoped `false` then outranks any `[files]` opt-in.
    req.overrides
        .retain(|o| o.field != engine::overrides::REENCODE_FIELD);
    req.overrides.push(engine::overrides::SetOverride {
        scope: engine::overrides::Scope::All,
        field: engine::overrides::REENCODE_FIELD.into(),
        value: false.into(),
    });
    let engine = Engine::new()?;
    let preset_skips = engine.preflight(&req)?;
    // The BC1 model is global rgbcx state; set it before any decode.
    encoders_sys::init(model).map_err(|e| EngineError::Fatal(e.to_string()))?;
    let found = discover(&a.batch.inputs, root.path(), a.batch.recursive)
        .map_err(|e| EngineError::Usage(format!("discover: {e}")))?;
    let mut unreadable = 0u32;
    for f in found.iter().filter(|f| f.kind == InputKind::Dds) {
        let info = dds_info(&f.path, model);
        unreadable += u32::from(matches!(info, DdsInfo::Unreadable(_)));
        let v = serde_json::to_value(&info).unwrap_or(Value::Null);
        if json {
            println!("{v}");
        } else {
            print_dds_info(&v);
        }
    }
    if !found.iter().any(|f| f.kind == InputKind::Image) {
        // No plan runs here, so report the skipped user presets ourselves (a plan reports them itself).
        for w in preset_skips {
            if json {
                let ev = Reporter::new(
                    Arc::new(JsonLinesSink::new(std::io::stdout())),
                    new_batch_id(),
                );
                ev.emit(None, None, EventKind::Warning(w));
            } else {
                let sink = Arc::new(VecSink::default());
                Reporter::new(sink.clone(), new_batch_id()).emit(None, None, EventKind::Warning(w));
                sink.take().iter().for_each(print_event);
            }
        }
        return Ok(i32::from(unreadable > 0));
    }

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
    let plan = engine.plan(req, &rep, &CancelToken::default())?;
    // What `run` would report for the files `plan` set aside. DDS files are inspected above, not skipped.
    for sk in plan
        .skipped
        .iter()
        .filter(|sk| sk.code != Some(Code::InDdsSkipped))
    {
        rep.emit_job(
            Some(&output_key(&plan.request.output_root, &sk.output)),
            sk.set.as_deref(),
            Some(&sk.path),
            EventKind::FileSkipped {
                kind: sk.kind(),
                code: sk.code,
                reason: sk.reason.clone(),
            },
        );
    }
    for f in &plan.failed {
        rep.emit_job(
            Some(&output_key(&plan.request.output_root, &f.output)),
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

/// Mip-0 decoded alpha below this is "varying" for sub-type inference. DECISIONS Note L: alpha whose minimum
/// is 251 or more is opaque (BC encode noise, Community Shaders' 251 opacity gate), so BC7's 254 is not varying.
const VARYING_ALPHA_BELOW: u8 = 251;

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
        advanced_skin: false,
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

/// One report object as a single JSON line.
fn json_line(v: &DdsValidation) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
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
/// header rules and the alpha rules rather than on sdc's preferred format. `alpha.exact_opaque` is off unless
/// `--exact-opaque` is given. A format sdc never writes only has its header checked. Exit 1 if a file cannot
/// be read, else 4 if one fails validation, else 0.
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
                    let rec = DdsValidation::Unreadable(DdsValidationUnreadable {
                        kind: DdsValidationKind::DdsValidation,
                        schema: REPORT_SCHEMA,
                        path: path_s,
                        readable: false,
                        passed: false,
                        error: e,
                    });
                    println!("{}", json_line(&rec));
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
                    (f, _) if f.is_hdr() => ColorTag::NotApplicable,
                    (_, true) => ColorTag::Srgb,
                    (_, false) => ColorTag::Unorm,
                };
                p.alpha.policy = explicit_policy
                    .unwrap_or_else(|| infer_alpha_policy(alpha_min, p.alpha.policy));
                // Codex-CLI 3: exact-opaque is a promise of sdc's own encoder (on by default for `envmask` and
                // `complex_material`), not of the file, so it is not inherited from the sub-type's preset.
                p.alpha.exact_opaque = a.exact_opaque;
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
            let rec = DdsValidation::Validated(DdsVerdict {
                kind: DdsValidationKind::DdsValidation,
                schema: REPORT_SCHEMA,
                path: path_s,
                readable: true,
                subtype: st,
                alpha_policy: policy,
                judged,
                note,
                passed,
                summary: report.summary,
                warnings: report.warnings,
            });
            println!("{}", json_line(&rec));
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

/// `sdc presets list|show|schema`. Result verb: the report goes to stdout (DECISIONS Note J).
pub fn presets(cmd: PresetsCmd) -> Result<i32, EngineError> {
    let engine = Engine::new()?;
    let reg = engine.registry();
    match cmd {
        PresetsCmd::List { json } => {
            for p in reg.selectable() {
                if json {
                    println!(
                        "{}",
                        json!({
                            "id": p.id, "version": p.version, "target": p.target,
                            "description": p.description, "suffix_aliases": p.suffix_aliases,
                        })
                    );
                } else {
                    let aliases = alias_list(&p.suffix_aliases)
                        .map(|a| format!("  aliases: {a}"))
                        .unwrap_or_default();
                    println!(
                        "{:<18} {:<8} {:<8} {}{aliases}",
                        p.id,
                        p.version,
                        p.target
                            .map(|t| format!("{t:?}").to_lowercase())
                            .unwrap_or_default(),
                        p.description
                    );
                }
            }
            Ok(0)
        }
        PresetsCmd::Show {
            id,
            subtype,
            target,
        } => {
            let usage = |e: engine::presets::PresetError| EngineError::Usage(e.to_string());
            reg.chain(&id).map_err(usage)?;
            let preset = reg
                .get(&id)
                .ok_or_else(|| EngineError::Usage(format!("unknown preset '{id}'")))?;
            let target = target
                .map(Target::from)
                .or(preset.target)
                .unwrap_or(Target::Cs);
            let subtypes: Vec<Subtype> = match subtype {
                Some(s) => vec![serde_json::from_value(Value::String(s.clone()))
                    .map_err(|_| EngineError::Usage(format!("--subtype {s}: unknown sub-type")))?],
                None => Subtype::ALL.to_vec(),
            };
            if let Some(a) = alias_list(&preset.suffix_aliases) {
                println!("suffix aliases: {a}");
            }
            for st in subtypes {
                let profile = reg.profile(&id, target, st).map_err(usage)?;
                let mut prov = BTreeMap::new();
                for (source, patch) in reg.layers(&id, st).map_err(usage)? {
                    let mut paths = vec![];
                    engine::presets::leaf_paths(&patch, "", &mut paths);
                    for p in paths {
                        prov.insert(p, source.to_string());
                    }
                }
                let shown = json!({ "subtype": st, "profile": profile, "provenance": prov });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&shown).unwrap_or_default()
                );
            }
            Ok(0)
        }
        PresetsCmd::Schema { which, out } => {
            match (SchemaKind::from(which), out) {
                (SchemaKind::All, Some(dir)) => {
                    engine::schemas::write_all(&dir)
                        .map_err(|e| EngineError::Fatal(format!("{}: {e}", dir.display())))?;
                    println!(
                        "wrote {} schemas to {}",
                        SchemaKind::FILES.len(),
                        dir.display()
                    );
                }
                (SchemaKind::All, None) => {
                    return Err(EngineError::Usage("schema all needs --out <dir>".into()))
                }
                (kind, Some(dir)) => {
                    std::fs::create_dir_all(&dir)
                        .and_then(|()| {
                            std::fs::write(
                                dir.join(kind.file_name()),
                                engine::schemas::schema_json(kind),
                            )
                        })
                        .map_err(|e| EngineError::Fatal(format!("{}: {e}", dir.display())))?;
                    println!("wrote {} to {}", kind.file_name(), dir.display());
                }
                (kind, None) => print!("{}", engine::schemas::schema_json(kind)),
            }
            Ok(0)
        }
    }
}

/// `alias→canonical, ...`, or `None` when there are no aliases.
fn alias_list(aliases: &BTreeMap<String, String>) -> Option<String> {
    (!aliases.is_empty()).then(|| {
        aliases
            .iter()
            .map(|(a, c)| format!("{a}\u{2192}{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// Where NVTT's `nvcompress` was found (never bundled, spec §8).
#[derive(Debug, PartialEq)]
struct NvttStatus {
    path: Option<PathBuf>,
    /// `--nvtt` or `PATH`; for a missing `--nvtt` file, `--nvtt` with no path.
    source: &'static str,
}

/// `--nvtt <file>` if given (it must exist), else `nvcompress.exe` on `path_var`.
fn find_nvtt(explicit: Option<&Path>, path_var: Option<&std::ffi::OsStr>) -> NvttStatus {
    if let Some(p) = explicit {
        return NvttStatus {
            path: p.is_file().then(|| p.to_path_buf()),
            source: "--nvtt",
        };
    }
    let path = path_var.and_then(|v| {
        std::env::split_paths(v)
            .map(|d| d.join("nvcompress.exe"))
            .find(|c| c.is_file())
    });
    NvttStatus {
        path,
        source: "PATH",
    }
}

/// `sdc encoders`: the vendored encoder versions, the ISA the ISPC kernels dispatch to, the active BC1 model
/// and whether NVTT can be found. Result verb: stdout (DECISIONS Note J); `--json` prints one object.
pub fn encoders(a: EncodersArgs) -> Result<i32, EngineError> {
    let v = engine::manifest::EncoderVersions::current();
    let isa = engine::manifest::dispatched_isa();
    let model = engine::manifest::bc1_model_id(a.bc1_decoder.into());
    let nvtt = find_nvtt(a.nvtt.as_deref(), std::env::var_os("PATH").as_deref());
    let nvtt_path = nvtt
        .path
        .as_ref()
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    if a.json {
        println!(
            "{}",
            json!({
                "kind": "encoders",
                "bc7enc_rdo": v.bc7enc_rdo, "ispc_texture_compressor": v.ispctc, "ispc": v.ispc,
                "bc7e_patch": v.bc7e_patch, "isa": isa, "bc1_model": model,
                "nvtt": { "found": nvtt.path.is_some(), "path": nvtt_path, "source": nvtt.source },
            })
        );
        return Ok(0);
    }
    println!(
        "bc7e (bc7enc_rdo {}) patch {}",
        v.bc7enc_rdo,
        v.bc7e_patch.as_deref().unwrap_or("none")
    );
    println!("rgbcx (bc7enc_rdo {})", v.bc7enc_rdo);
    println!("ISPC Texture Compressor {} (BC6H)", v.ispctc);
    println!("ISPC {} objects; dispatched ISA on this CPU: {isa}", v.ispc);
    println!("BC1 decoder model: {model}");
    match nvtt_path {
        Some(p) => println!("NVTT: found at {p} (backend arrives in milestone M5)"),
        None => println!(
            "NVTT: not found ({}; backend arrives in milestone M5)",
            if nvtt.source == "PATH" {
                "not on PATH, use --nvtt <nvcompress.exe>"
            } else {
                "no such file"
            }
        ),
    }
    Ok(0)
}

/// `sdc patch-header` (DECISIONS S7). Exit 1 when a file is unreadable or refused, else 0; usage errors are 2
/// (including `--out-dir` resolving to an input itself, DECISIONS S6). A write failure is exit 3 and stops the
/// batch; files written before it stay written.
pub fn patch_header(a: PatchHeaderArgs) -> Result<i32, EngineError> {
    use engine::dds_patch::{plan_patch, PatchRequest};
    let target = a.target.map_or(Target::Cs, Target::from);
    let mut req = PatchRequest::default();
    if let Some(id) = &a.subtype {
        let st: Subtype = serde_json::from_value(Value::String(id.clone()))
            .map_err(|_| EngineError::Usage(format!("--subtype {id}: unknown sub-type")))?;
        if !matches!(availability(target, st), Availability::Available) {
            return Err(EngineError::Usage(format!(
                "--subtype {id} is unavailable under {target:?} (CFG-UNAVAILABLE)"
            )));
        }
        let p = Engine::new()?.registry().base_profile(target, st);
        req.srgb = Some(p.srgb_tag());
        req.require_full_mips = p.mips.levels == MipLevels::Keyword(MipKeyword::Full);
    }
    if let Some(t) = a.tag {
        req.srgb = Some(matches!(t, TagArg::Srgb));
    }
    if let Some(f) = &a.format {
        req.expect_format = Some(
            serde_json::from_value(Value::String(f.clone()))
                .map_err(|_| EngineError::Usage(format!("--format {f}: unknown format")))?,
        );
    }
    let mut names = std::collections::BTreeSet::new();
    if let Some(dir) = &a.out_dir {
        for p in &a.dds {
            let out = dir.join(p.file_name().unwrap_or_default());
            // DECISIONS S6, as convert does (api.rs).
            if engine::dds::same_file(p, &out) {
                return Err(EngineError::Usage(format!(
                    "--out-dir would write over the input {}; use --in-place to rewrite it",
                    p.display()
                )));
            }
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            if !names.insert(n.clone()) {
                return Err(EngineError::Usage(format!(
                    "two inputs are named {n}: --out-dir would write one over the other"
                )));
            }
        }
    }
    let mut exit = 0;
    for path in &a.dds {
        let shown = path.display();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                println!("{shown}: unreadable: {e}");
                exit = 1;
                continue;
            }
        };
        match plan_patch(&bytes, &req) {
            Ok(None) => println!("{shown}: header already valid; nothing to patch"),
            Ok(Some(p)) => {
                if !a.dry_run {
                    let out = match &a.out_dir {
                        Some(dir) => dir.join(path.file_name().unwrap_or_default()),
                        None => path.clone(),
                    };
                    engine::dds::write_atomic(&out, &p.bytes)
                        .map_err(|e| EngineError::Fatal(format!("{}: {e}", out.display())))?;
                }
                // After the write, so a failed write never reads as patched.
                let w = p.warning();
                println!("{shown}: {}: {}", w.code.as_str(), w.message);
            }
            Err(u) => {
                let w = u.warning();
                println!("{shown}: {}: {}", w.code.as_str(), w.message);
                exit = 1;
            }
        }
    }
    Ok(exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvtt_is_found_on_path_or_by_flag_only() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let exe = b.join("nvcompress.exe");
        std::fs::write(&exe, b"").unwrap();
        let var = std::env::join_paths([&a, &b]).unwrap();
        let on_path = find_nvtt(None, Some(&var));
        assert_eq!(
            (on_path.path.as_deref(), on_path.source),
            (Some(exe.as_path()), "PATH")
        );
        assert_eq!(find_nvtt(None, None).path, None);
        assert_eq!(find_nvtt(None, Some(a.as_os_str())).path, None);
        // The flag wins over PATH, and a missing file is not found (no fall-back to PATH).
        let missing = a.join("nvcompress.exe");
        let flagged = find_nvtt(Some(&missing), Some(&var));
        assert_eq!((flagged.path, flagged.source), (None, "--nvtt"));
        assert_eq!(
            find_nvtt(Some(&exe), None).path.as_deref(),
            Some(exe.as_path())
        );
    }

    #[test]
    fn format_name_splits_the_srgb_tag() {
        assert_eq!(
            format_name(DdsPixelFormat::Bc7 { srgb: true }),
            ("Bc7".into(), Some(true))
        );
        assert_eq!(format_name(DdsPixelFormat::Bc5), ("Bc5".into(), None));
        assert_eq!(
            format_name(DdsPixelFormat::Bgra8 { srgb: false }),
            ("Bgra8".into(), Some(false))
        );
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
