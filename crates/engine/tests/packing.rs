//! M2.5 exit (plan §M2.5): golden cases for channel packing through `Engine::plan` + `Engine::run`. Fixtures are
//! generated; nothing is committed but this file.
use engine::api::*;
use engine::codes::{Code, Warning};
use engine::config::Target;
use engine::dds::{read_dds, DdsFile, DdsPixelFormat, HeaderKind};
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::manifest::{sha256_file, Manifest};
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::validate::ValidationSummary;
use image::{ImageBuffer, Luma, Rgba};
use std::path::Path;
use std::sync::Arc;

const W: u32 = 64;

struct Out {
    summary: BatchSummary,
    events: Vec<EngineEvent>,
}

fn convert_with(dir: &Path, target: Target, sets: &[&str], on_existing: OnExisting) -> Out {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.on_existing = on_existing;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "packing");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let summary = e.run(plan, &rep, &CancelToken::default()).unwrap();
    Out {
        summary,
        events: sink.take(),
    }
}

fn convert(dir: &Path, target: Target, sets: &[&str]) -> Out {
    convert_with(dir, target, sets, OnExisting::Overwrite)
}

/// An 8-bit grey source at `in/textures/a/<name>`.
fn put(dir: &Path, name: &str, f: impl Fn(u32, u32) -> u8) {
    put_sized(dir, name, W, f)
}

fn put_sized(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)]))
        .save(p)
        .unwrap();
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

fn warnings_of(o: &Out, suffix: &str) -> Vec<Warning> {
    o.events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if is_of(e, suffix) => Some(w.clone()),
            _ => None,
        })
        .collect()
}

fn defaulted(o: &Out, suffix: &str) -> Vec<String> {
    warnings_of(o, suffix)
        .iter()
        .filter(|w| w.code == Code::PackDefaulted)
        .map(|w| w.data["channel"].as_str().unwrap().to_string())
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

fn validation_of(o: &Out, suffix: &str) -> ValidationSummary {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileDone { validation, .. } if is_of(e, suffix) => Some(validation.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_done for {suffix}"))
}

fn skip_code(o: &Out, suffix: &str) -> Option<Code> {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileSkipped { code, .. } if is_of(e, suffix) => Some(*code),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_skipped for {suffix}"))
}

fn out_path(dir: &Path, name: &str) -> std::path::PathBuf {
    dir.join("out/textures/a").join(name)
}

fn dds(dir: &Path, name: &str) -> DdsFile {
    read_dds(&std::fs::read(out_path(dir, name)).unwrap()).unwrap()
}

fn levels(f: &DdsFile) -> Vec<Vec<[u8; 4]>> {
    f.levels
        .iter()
        .map(
            |(w, h, r)| match decode_level(f.format, *w, *h, &f.data[r.clone()], None).unwrap() {
                Decoded::Ldr(px) => px,
                Decoded::Hdr(_) => panic!("expected an LDR format"),
            },
        )
        .collect()
}

fn assert_dds(f: &DdsFile, format: DdsPixelFormat, header: HeaderKind, mips: u32) {
    assert_eq!(
        (f.format, f.header_kind, f.mip_count),
        (format, header, mips)
    );
}

/// Every texel of `px` within `tol` of `want(x, y)` on channel `c`.
fn assert_channel(px: &[[u8; 4]], c: usize, tol: i32, want: impl Fn(u32, u32) -> u8) {
    for (i, p) in px.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        let d = (i32::from(p[c]) - i32::from(want(x, y))).abs();
        assert!(
            d <= tol,
            "channel {c} at {x},{y}: {} vs {}",
            p[c],
            want(x, y)
        );
    }
}

fn r(x: u32, _: u32) -> u8 {
    (x * 4) as u8
}
fn m(x: u32, _: u32) -> u8 {
    (255 - x * 4) as u8
}
fn ao(x: u32, _: u32) -> u8 {
    (128 + x) as u8
}
fn spec(x: u32, _: u32) -> u8 {
    (40 + x * 2) as u8
}

fn full_set(dir: &Path) {
    put(dir, "iron_roughness.png", r);
    put(dir, "iron_metallic.png", m);
    put(dir, "iron_ao.png", ao);
    put(dir, "iron_specular.png", spec);
}

/// Exit: cs roughness + metallic + AO + specular → `_rmaos` equal to a reference packing; the manifest lists
/// every source.
#[test]
fn cs_full_set_packs_rmaos_like_a_reference_packing() {
    let d = tempfile::tempdir().unwrap();
    full_set(d.path());
    let o = convert(d.path(), Target::Cs, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(defaulted(&o, "iron_roughness.png").is_empty());
    let f = dds(d.path(), "iron_rmaos.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    let px = &levels(&f)[0];
    assert_channel(px, 0, 4, r);
    assert_channel(px, 1, 4, m);
    assert_channel(px, 2, 4, ao);
    assert_channel(px, 3, 4, spec);
    assert!(validation_of(&o, "iron_roughness.png").passed);
    let man = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = &man["textures/a/iron_rmaos.dds"];
    let names: Vec<&str> = e
        .sources
        .iter()
        .map(|s| s.path.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "iron_roughness.png",
            "iron_metallic.png",
            "iron_ao.png",
            "iron_specular.png"
        ]
    );
    for s in &e.sources {
        let p = d
            .path()
            .join("in/textures/a")
            .join(s.path.rsplit('/').next().unwrap());
        assert_eq!(s.sha256, sha256_file(&p).unwrap());
    }
}

/// Exit: AO-only set → roughness 0.5, metallic 0, specular 1, one PACK-DEFAULTED per channel, `default:<v>`.
#[test]
fn ao_only_set_defaults_three_channels() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_ao.png", ao);
    let o = convert(d.path(), Target::Cs, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(defaulted(&o, "iron_ao.png"), ["R", "G", "A"]);
    let job = planned(&o, "iron_ao.png");
    assert_eq!(job["provenance"]["pack.r"], "default:0.5");
    assert_eq!(job["provenance"]["pack.g"], "default:0");
    assert_eq!(job["provenance"]["pack.a"], "default:1");
    let px = &levels(&dds(d.path(), "iron_rmaos.dds"))[0];
    assert_channel(px, 0, 2, |_, _| 128);
    assert_channel(px, 1, 2, |_, _| 0);
    assert_channel(px, 2, 4, ao);
    assert_channel(px, 3, 2, |_, _| 255);
}

/// Exit: the same set with `--fill metallic=255` (here its `--set` form) → metallic 255 with `fill:255`, the
/// other channels unchanged.
#[test]
fn a_metallic_fill_replaces_the_default() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_ao.png", ao);
    let o = convert(d.path(), Target::Cs, &["packing.fill.metallic=255"]);
    assert_eq!(defaulted(&o, "iron_ao.png"), ["R", "A"]);
    assert_eq!(
        planned(&o, "iron_ao.png")["provenance"]["pack.g"],
        "fill:255"
    );
    let px = &levels(&dds(d.path(), "iron_rmaos.dds"))[0];
    assert_channel(px, 1, 2, |_, _| 255);
    assert_channel(px, 0, 2, |_, _| 128);
}

/// Exit: fuzz set → `_rfaos`; metallic next to fuzz is unused.
#[test]
fn a_fuzz_set_packs_rfaos() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_fuzz.png", ao);
    put(d.path(), "iron_metallic.png", m);
    let o = convert(d.path(), Target::Cs, &[]);
    assert!(
        out_path(d.path(), "iron_rfaos.dds").exists(),
        "{:?}",
        warnings(&o)
    );
    assert!(!out_path(d.path(), "iron_rmaos.dds").exists());
    assert_eq!(
        skip_code(&o, "iron_metallic.png"),
        Some(Code::PackUnusedInput)
    );
    assert_channel(&levels(&dds(d.path(), "iron_rfaos.dds"))[0], 1, 4, ao);
}

/// Exit: height → `_p` BC4; a 16-bit source reports its bit-depth loss like any file.
#[test]
fn height_packs_p_as_bc4() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/iron_height.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(W, W, |x, _| Luma([(x * 1024) as u16]))
        .save(&p)
        .unwrap();
    let o = convert(d.path(), Target::Cs, &[]);
    let f = dds(d.path(), "iron_p.dds");
    assert_dds(&f, DdsPixelFormat::Bc4, HeaderKind::Dx10, 7);
    assert_channel(&levels(&f)[0], 0, 3, |x, _| {
        ((x * 1024) as f32 / 65535.0 * 255.0).round() as u8
    });
    assert!(warnings_of(&o, "iron_height.png")
        .iter()
        .any(|w| w.code == Code::LossBitdepth));
}

/// Exit: gloss-only cs set → roughness = 1 − gloss.
#[test]
fn gloss_only_cs_set_inverts_into_roughness() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_gloss.png", r);
    let o = convert(d.path(), Target::Cs, &[]);
    assert_eq!(
        planned(&o, "iron_gloss.png")["provenance"]["pack.r"],
        "source:1-iron_gloss.png:R"
    );
    assert_channel(
        &levels(&dds(d.path(), "iron_rmaos.dds"))[0],
        0,
        4,
        |x, y| 255 - r(x, y),
    );
}

/// Exit: cpm → complex `_m` whose smallest-mip alpha stays below CS's complex gate (Spike S8).
#[test]
fn cpm_packs_a_complex_m_below_the_gate() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_metallic.png", |_, _| 40);
    put(d.path(), "iron_height.png", |_, y| (y * 2) as u8);
    let o = convert(d.path(), Target::Cpm, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "iron_m.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    let lv = levels(&f);
    assert!(lv.last().unwrap()[0][3] < 251, "{:?}", lv.last());
    let w = warnings(&o);
    assert!(
        !w.contains(&Code::ValMComplexMargin) && !w.contains(&Code::ValMGrayscale),
        "{w:?}"
    );
    assert_eq!(
        planned(&o, "iron_metallic.png")["profile"]["subtype"],
        "complex_material"
    );
}

/// Spike S8: a packed cpm `_m` whose RGB is grey at the smallest mip is not complex in CS; VAL-M-GRAYSCALE says so.
#[test]
fn a_grey_packed_cpm_m_is_flagged() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_metallic.png", |_, _| 128);
    put(d.path(), "iron_glossiness.png", |_, _| 128);
    let o = convert(d.path(), Target::Cpm, &[]);
    assert_eq!(o.summary.converted, 1);
    assert!(
        warnings(&o).contains(&Code::ValMGrayscale),
        "{:?}",
        warnings(&o)
    );
}

/// Exit: vanilla metallic → `_m` env mask (BC4); a height-only vanilla set → PACK-UNUSED-INPUT, nothing written.
#[test]
fn vanilla_packs_an_env_mask_only_from_metallic_or_specular() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_metallic.png", m);
    put(d.path(), "steel_height.png", r);
    let o = convert(d.path(), Target::Vanilla, &[]);
    let f = dds(d.path(), "iron_m.dds");
    assert_dds(&f, DdsPixelFormat::Bc4, HeaderKind::Dx10, 7);
    assert_channel(&levels(&f)[0], 0, 3, m);
    assert_eq!(
        skip_code(&o, "steel_height.png"),
        Some(Code::PackUnusedInput)
    );
    assert!(!out_path(d.path(), "steel_m.dds").exists());
    assert_eq!(o.summary.exit_code(), 0);
}

/// Exit: size mismatch → PACK-SIZE-MISMATCH naming both files.
#[test]
fn sources_of_different_sizes_fail_with_both_named() {
    let d = tempfile::tempdir().unwrap();
    put_sized(d.path(), "iron_roughness.png", 64, |_, _| 100);
    put_sized(d.path(), "iron_ao.png", 32, |_, _| 200);
    let o = convert(d.path(), Target::Cs, &[]);
    let w = warnings_of(&o, "iron_roughness.png");
    let mm = w.iter().find(|w| w.code == Code::PackSizeMismatch).unwrap();
    assert!(
        mm.message.contains("iron_roughness.png 64x64") && mm.message.contains("iron_ao.png 32x32")
    );
    assert!(!out_path(d.path(), "iron_rmaos.dds").exists());
    assert_eq!((o.summary.failed, o.summary.exit_code()), (1, 1));
}

/// Exit: pre-packed conflict → PACK-CONFLICT and the pre-packed output.
#[test]
fn a_prepacked_file_is_kept_with_a_conflict_warning() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/iron_rmaos.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(W, W, |x, _| Rgba([(x * 4) as u8, 0, 255, 255]))
        .save(&p)
        .unwrap();
    put(d.path(), "iron_roughness.png", m);
    let o = convert(d.path(), Target::Cs, &[]);
    assert!(warnings_of(&o, "iron_rmaos.png")
        .iter()
        .any(|w| w.code == Code::PackConflict));
    assert_eq!(
        skip_code(&o, "iron_roughness.png"),
        Some(Code::PackUnusedInput)
    );
    let man = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = &man["textures/a/iron_rmaos.dds"];
    assert!(e.sources.is_empty());
    assert_eq!(e.source_sha256, sha256_file(&p).unwrap());
    // The pre-packed R ramp, not the raw (inverted-looking) roughness.
    assert_channel(&levels(&dds(d.path(), "iron_rmaos.dds"))[0], 0, 4, r);
}

/// Spec §6.4 "Provenance": `if-changed` keeps a current packed output and re-encodes when any source changes.
#[test]
fn if_changed_follows_every_source() {
    let d = tempfile::tempdir().unwrap();
    full_set(d.path());
    let run = || convert_with(d.path(), Target::Cs, &[], OnExisting::IfChanged).summary;
    assert_eq!(run().converted, 1);
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    put(d.path(), "iron_specular.png", |x, _| (200 - x) as u8);
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (1, 0));
}

/// Spec §6.4 "Provenance", final review Minor 1: a source renamed to another role keeps its bytes and its place in
/// R, but R is now stored inverted (roughness -> glossiness), so the stale output is re-encoded.
#[test]
fn if_changed_follows_a_source_renamed_to_another_role() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", |x, _| x as u8 * 4);
    let run = || convert_with(d.path(), Target::Cs, &[], OnExisting::IfChanged).summary;
    assert_eq!(run().converted, 1);
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    let a = d.path().join("in/textures/a");
    std::fs::rename(a.join("iron_roughness.png"), a.join("iron_glossiness.png")).unwrap();
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (0, 1));
}

/// Task 3b (owner 2026-10-10, amends Note M1): a packed cpm `_m` with no height source gets alpha 0, so CS keeps
/// complex material on with parallax off; PACK-DEFAULTED still names the height channel.
#[test]
fn a_cpm_m_without_a_height_source_gets_alpha_zero() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_metallic.png", |_, _| 40);
    let o = convert(d.path(), Target::Cpm, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(defaulted(&o, "iron_metallic.png").contains(&"A".to_string()));
    let px = &levels(&dds(d.path(), "iron_m.dds"))[0];
    assert_channel(px, 3, 1, |_, _| 0);
}

/// An explicit height fill is not the neutral default: it keeps its value.
#[test]
fn an_explicit_height_fill_still_applies() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_metallic.png", |_, _| 40);
    // DECISIONS AH2: a fill is a constant alpha, so it is written only with parallax turned on.
    let o = convert(
        d.path(),
        Target::Cpm,
        &[
            "packing.fill.height=128",
            "subtype:complex_material.alpha.parallax=true",
        ],
    );
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(!defaulted(&o, "iron_metallic.png").contains(&"A".to_string()));
    let px = &levels(&dds(d.path(), "iron_m.dds"))[0];
    assert_channel(px, 3, 1, |_, _| 128);
}

/// DECISIONS AH2: a height fill alone is a constant alpha, so parallax stays off and the alpha is exactly 0.
#[test]
fn a_height_fill_without_parallax_writes_alpha_0() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_metallic.png", |_, _| 40);
    let o = convert(d.path(), Target::Cpm, &["packing.fill.height=128"]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let job = planned(&o, "iron_metallic.png");
    assert_eq!(job["profile"]["alpha"]["parallax"], false);
    assert_eq!(
        job["provenance"]["alpha.parallax"],
        "analysis:constant-alpha"
    );
    for (l, px) in levels(&dds(d.path(), "iron_m.dds")).iter().enumerate() {
        assert!(px.iter().all(|p| p[3] == 0), "mip {l}");
    }
}

/// DECISIONS AH1/AH2: the checks see the final parallax value. A 255 fill with parallax on writes 255, so turning
/// exact opaque off on BC7 is ALPHA-EXACT-OFF-M again.
#[test]
fn a_255_fill_with_parallax_on_and_exact_opaque_off_is_flagged() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_metallic.png", |_, _| 40);
    let o = convert(
        d.path(),
        Target::Cpm,
        &[
            "packing.fill.height=255",
            "subtype:complex_material.alpha.parallax=true",
            "subtype:complex_material.alpha.exact_opaque=false",
        ],
    );
    assert!(
        warnings(&o).contains(&Code::AlphaExactOffM),
        "{:?}",
        warnings(&o)
    );
}

/// Note R-fills-1: a forced `_rfaos` without a fuzz source writes G = 0, or the fuzz fill.
#[test]
fn a_forced_rfaos_writes_the_fuzz_default_or_fill_in_g() {
    for (sets, want) in [
        (vec!["packing.recipe=rfaos"], 0u8),
        (vec!["packing.recipe=rfaos", "packing.fill.fuzz=128"], 128),
    ] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "iron_roughness.png", r);
        put(d.path(), "iron_ao.png", ao);
        let o = convert(d.path(), Target::Cs, &sets);
        assert!(
            out_path(d.path(), "iron_rfaos.dds").exists(),
            "{:?}",
            warnings(&o)
        );
        assert!(!out_path(d.path(), "iron_rmaos.dds").exists());
        let px = &levels(&dds(d.path(), "iron_rfaos.dds"))[0];
        assert_channel(px, 1, 2, |_, _| want);
        assert_channel(px, 0, 4, r);
        assert_channel(px, 2, 4, ao);
    }
}
