//! DECISIONS Note AH1/AH2 (owner, 2026-10-14): a complex-material `_m` writes alpha 0 (complex material on,
//! parallax off) when its source has no alpha or `alpha.parallax` is off; parallax on keeps the source alpha as height.
use engine::api::*;
use engine::codes::Code;
use engine::config::{Format, Subtype, Target};
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::presets::AlphaContent;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

const W: u32 = 64;
const SUB: &str = "file:**/iron_m.png.subtype=complex_material";

struct Out {
    summary: BatchSummary,
    events: Vec<EngineEvent>,
}

fn convert(dir: &Path, target: Target, sets: &[&str]) -> Out {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.on_existing = OnExisting::Overwrite;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "complex-m");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let summary = e.run(plan, &rep, &CancelToken::default()).unwrap();
    Out {
        summary,
        events: sink.take(),
    }
}

fn src(dir: &Path, name: &str) -> std::path::PathBuf {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    p
}

/// Noisy, non-grey RGB so the encoder uses several block modes.
fn rgb(x: u32, y: u32) -> [u8; 3] {
    let n = x.wrapping_mul(73) ^ y.wrapping_mul(151);
    [(n % 200) as u8 + 20, (x * 3) as u8 + 10, (y * 2) as u8 + 40]
}

fn put_rgb(dir: &Path) {
    ImageBuffer::from_fn(W, W, |x, y| Rgb(rgb(x, y)))
        .save(src(dir, "iron_m.png"))
        .unwrap();
}

fn put_rgba(dir: &Path, a: impl Fn(u32, u32) -> u8) {
    ImageBuffer::from_fn(W, W, |x, y| {
        let [r, g, b] = rgb(x, y);
        Rgba([r, g, b, a(x, y)])
    })
    .save(src(dir, "iron_m.png"))
    .unwrap();
}

fn height(x: u32, y: u32) -> u8 {
    (40 + (x + y) * 2) as u8
}

fn is_of(e: &EngineEvent, suffix: &str) -> bool {
    e.path.as_deref().is_some_and(|p| p.ends_with(suffix))
}

fn warnings(o: &Out) -> Vec<Code> {
    o.events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) => Some(w.code),
            _ => None,
        })
        .collect()
}

fn planned(o: &Out, suffix: &str) -> serde_json::Value {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FilePlanned { job } if is_of(e, suffix) => {
                Some(serde_json::to_value(&**job).unwrap())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_planned for {suffix}"))
}

/// The decoded alpha of every mip of `out/textures/a/iron_m.dds`.
fn alphas(dir: &Path) -> Vec<Vec<u8>> {
    let f = read_dds(&std::fs::read(dir.join("out/textures/a/iron_m.dds")).unwrap()).unwrap();
    f.levels
        .iter()
        .map(
            |(w, h, r)| match decode_level(f.format, *w, *h, &f.data[r.clone()], None).unwrap() {
                Decoded::Ldr(px) => px.iter().map(|p| p[3]).collect(),
                Decoded::Hdr(_) => panic!("HDR output for an _m"),
            },
        )
        .collect()
}

fn assert_all_zero(dir: &Path) {
    let a = alphas(dir);
    assert!(a.len() > 1, "expected a mip chain");
    for (l, lv) in a.iter().enumerate() {
        assert!(lv.iter().all(|&v| v == 0), "mip {l}: {lv:?}");
    }
}

fn assert_quiet(o: &Out) {
    let w = warnings(o);
    for c in [
        Code::AlphaExactOffM,
        Code::ValMComplexMargin,
        Code::ValMHeightClip,
        Code::ValAlphaGuarantee,
        Code::ChanConstantAlphaData,
        Code::SugContradiction,
    ] {
        assert!(!w.contains(&c), "{c:?} in {w:?}");
    }
}

/// Every format complex_material may be written as that stores alpha (BC1 is opaque-only in v1).
fn alpha_formats() -> Vec<Format> {
    let mut v: Vec<Format> =
        engine::policy::allowed_encodings(Subtype::ComplexMaterial, Target::Cs)
            .into_iter()
            .map(|(f, _)| f)
            .filter(|f| f.has_alpha() && *f != Format::Bc1 && !f.is_hdr())
            .collect();
    v.dedup();
    assert!(v.contains(&Format::Bc7), "{v:?}");
    v
}

#[test]
fn a_no_alpha_m_as_complex_material_writes_alpha_0_at_every_mip() {
    for f in alpha_formats() {
        let d = tempfile::tempdir().unwrap();
        put_rgb(d.path());
        let fmt = format!(
            "file:**/iron_m.png.format={}",
            serde_json::to_value(f).unwrap().as_str().unwrap()
        );
        let o = convert(d.path(), Target::Cs, &[SUB, &fmt]);
        assert_eq!(o.summary.converted, 1, "{f:?}: {:?}", warnings(&o));
        assert_all_zero(d.path());
        assert_quiet(&o);
        let j = planned(&o, "iron_m.png");
        assert_eq!(j["profile"]["alpha"]["policy"], "data", "{f:?}");
        assert_eq!(j["profile"]["alpha"]["parallax"], false, "{f:?}");
        assert_eq!(j["provenance"]["alpha.parallax"], "analysis:no-alpha");
    }
}

#[test]
fn exact_opaque_off_does_not_nag_on_a_zero_filled_m() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path());
    let o = convert(
        d.path(),
        Target::Cs,
        &[SUB, "file:**/iron_m.png.alpha.exact_opaque=false"],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_all_zero(d.path());
    assert_quiet(&o);
}

#[test]
fn parallax_on_without_source_alpha_is_reported_and_writes_0() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path());
    let o = convert(
        d.path(),
        Target::Cs,
        &[SUB, "file:**/iron_m.png.alpha.parallax=true"],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(warnings(&o).contains(&Code::AlphaParallaxNoAlpha));
    // Parallax on skips the exact-0 BC7 fix (it keeps data alpha untouched), so bc7e's mode-6 p-bit may give 1: still
    // at or below CS's 4/255 parallax-off bound.
    for (l, lv) in alphas(d.path()).iter().enumerate() {
        assert!(lv.iter().all(|&v| v <= 4), "mip {l}: {lv:?}");
    }
}

#[test]
fn a_varying_alpha_m_defaults_to_parallax_on_and_keeps_its_alpha() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), height);
    let o = convert(d.path(), Target::Cs, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let j = planned(&o, "iron_m.png");
    assert_eq!(j["profile"]["subtype"], "complex_material");
    assert_eq!(j["profile"]["alpha"]["parallax"], true);
    assert_eq!(j["provenance"]["alpha.parallax"], "analysis:varying-alpha");
    let a = alphas(d.path());
    for (i, &v) in a[0].iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        assert!(
            (i32::from(v) - i32::from(height(x, y))).abs() <= 8,
            "{x},{y}: {v}"
        );
    }
}

#[test]
fn parallax_off_on_a_varying_alpha_m_writes_0() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), height);
    let o = convert(
        d.path(),
        Target::Cs,
        &["file:**/iron_m.png.alpha.parallax=false"],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_all_zero(d.path());
    assert_quiet(&o);
}

#[test]
fn a_solid_255_alpha_m_defaults_to_parallax_off() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), |_, _| 255);
    let o = convert(d.path(), Target::Cs, &[SUB]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let j = planned(&o, "iron_m.png");
    assert_eq!(j["profile"]["alpha"]["parallax"], false);
    assert_eq!(j["provenance"]["alpha.parallax"], "analysis:constant-alpha");
    assert_all_zero(d.path());
    assert_quiet(&o);
}

#[test]
fn a_solid_255_alpha_m_with_parallax_on_keeps_255_and_warns() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), |_, _| 255);
    let o = convert(
        d.path(),
        Target::Cs,
        &[SUB, "file:**/iron_m.png.alpha.parallax=true"],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(alphas(d.path())[0].iter().all(|&v| v == 255));
    let w = warnings(&o);
    assert!(w.contains(&Code::ValMComplexMargin), "{w:?}");
    assert!(w.contains(&Code::SugContradiction), "{w:?}");
}

fn put_gray(dir: &Path, name: &str, f: impl Fn(u32, u32) -> u8) {
    ImageBuffer::from_fn(W, W, |x, y| Luma([f(x, y)]))
        .save(src(dir, name))
        .unwrap();
}

#[test]
fn a_packed_cpm_m_without_height_is_still_0() {
    let d = tempfile::tempdir().unwrap();
    put_gray(d.path(), "iron_roughness.png", |x, _| (x * 4) as u8);
    put_gray(d.path(), "iron_metallic.png", |_, _| 40);
    let o = convert(d.path(), Target::Cpm, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    // The M1 height default is a constant 0: nothing to enable, so parallax reads off (DECISIONS AH2).
    let j = planned(&o, "iron_metallic.png");
    assert_eq!(j["profile"]["alpha"]["parallax"], false);
    assert_eq!(j["provenance"]["alpha.parallax"], "analysis:constant-alpha");
    assert_all_zero(d.path());
}

/// Any constant alpha is non-varying (DECISIONS AH2): a mid-grey 128 would run parallax on a flat height, so it
/// defaults to off and writes 0.
#[test]
fn a_constant_mid_alpha_m_defaults_to_parallax_off() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), |_, _| 128);
    let o = convert(d.path(), Target::Cs, &[SUB]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let j = planned(&o, "iron_m.png");
    assert_eq!(j["profile"]["alpha"]["parallax"], false);
    assert_eq!(j["provenance"]["alpha.parallax"], "analysis:constant-alpha");
    assert_all_zero(d.path());
    assert_quiet(&o);
}

/// DECISIONS AH2: an explicit `remove` writes 255; parallax has nothing to say and its no-alpha warning stays quiet.
#[test]
fn an_explicit_remove_raises_no_parallax_warning() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path());
    let o = convert(
        d.path(),
        Target::Cs,
        &[
            SUB,
            "file:**/iron_m.png.alpha.policy=remove",
            "file:**/iron_m.png.alpha.parallax=true",
        ],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::AlphaParallaxNoAlpha));
}

#[test]
fn a_packed_cpm_m_with_height_defaults_to_parallax_on() {
    let d = tempfile::tempdir().unwrap();
    put_gray(d.path(), "iron_roughness.png", |x, _| (x * 4) as u8);
    put_gray(d.path(), "iron_metallic.png", |_, _| 40);
    put_gray(d.path(), "iron_height.png", height);
    let o = convert(d.path(), Target::Cpm, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let j = planned(&o, "iron_metallic.png");
    assert_eq!(j["profile"]["alpha"]["parallax"], true);
    assert!(alphas(d.path())[0].iter().any(|&v| v > 4));
}

#[test]
fn suggestion_text_names_alpha_0_not_255() {
    let (p, _, why) =
        engine::suggest::suggest_alpha(Subtype::ComplexMaterial, AlphaContent::None, 0.0).unwrap();
    assert_eq!(p, engine::config::AlphaPolicy::Data);
    assert!(why.contains("alpha 0"), "{why}");
    assert!(!why.contains("255"), "{why}");
}

/// Note AT (D16): under vanilla a varying-alpha `_m` is a complex `_m` too: BC7 with its alpha as height, parallax on.
#[test]
fn vanilla_keeps_a_varying_alpha_m_as_complex_material() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), height);
    let o = convert(d.path(), Target::Vanilla, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let j = planned(&o, "iron_m.png");
    assert_eq!(j["profile"]["subtype"], "complex_material");
    assert_eq!(j["profile"]["format"], "bc7");
    assert_eq!(j["profile"]["alpha"]["policy"], "data");
    assert_eq!(j["profile"]["alpha"]["parallax"], true);
    let w = warnings(&o);
    assert!(
        !w.contains(&Code::LossAlphaData) && !w.contains(&Code::CfgUnavailable),
        "{w:?}"
    );
    let a = alphas(d.path());
    for (i, &v) in a[0].iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        assert!(
            (i32::from(v) - i32::from(height(x, y))).abs() <= 8,
            "{x},{y}: {v}"
        );
    }
}

/// Note AT: a plain `_m` (solid 255 or no alpha) stays a BC4 env mask under vanilla.
#[test]
fn vanilla_m_with_solid_or_no_alpha_stays_an_envmask() {
    let cases: [fn(&Path); 2] = [|d| put_rgba(d, |_, _| 255), put_rgb];
    for put in cases {
        let d = tempfile::tempdir().unwrap();
        put(d.path());
        let o = convert(d.path(), Target::Vanilla, &[]);
        assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
        let j = planned(&o, "iron_m.png");
        assert_eq!(
            (
                j["profile"]["subtype"].as_str(),
                j["profile"]["format"].as_str()
            ),
            (Some("envmask"), Some("bc4"))
        );
    }
}

/// Note AT + AH1: an explicit `complex_material` without source alpha under vanilla writes alpha 0, not a fallback.
#[test]
fn vanilla_explicit_complex_material_without_alpha_writes_0() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path());
    let o = convert(d.path(), Target::Vanilla, &[SUB]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::CfgUnavailable));
    assert_all_zero(d.path());
}
