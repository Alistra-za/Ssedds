//! Packing-fills Task 1b (DECISIONS Note R-skin-1): the Advanced skin sub-types `skin_albedo`, `skin_rfaos`,
//! `skin_sk` and `skin_wet`.
use engine::api::*;
use engine::codes::Code;
use engine::config::{Format, MipKeyword, MipLevels, Subtype, Target};
use engine::dds::{read_dds, DdsFile, DdsPixelFormat};
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::policy::{availability, AcceptSuggestions, Availability, ResolvedJob};
use engine::suggest::Confidence;
use image::{DynamicImage, ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn put(dir: &Path, rel: &str, img: DynamicImage) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save(p).unwrap();
}

fn colour(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, y| {
        Rgb([(x * 3) as u8, (y * 2) as u8, 90])
    }))
}

fn grey(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, _| Rgb([(x * 3) as u8; 3])))
}

/// A tangent-space-looking normal with a binary mask in alpha.
fn wet_normal(w: u32) -> DynamicImage {
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(w, w, |x, y| {
        Rgba([
            128 + (x % 8) as u8,
            128 + (y % 8) as u8,
            250,
            if x < w / 2 { 255 } else { 0 },
        ])
    }))
}

fn req(d: &TempDir, target: Target, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), target);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.on_existing = OnExisting::Overwrite;
    r.overrides = sets
        .iter()
        .map(|s| engine::overrides::parse_set(s).unwrap())
        .collect();
    r
}

fn plan(d: &TempDir, target: Target, sets: &[&str]) -> Plan {
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    Engine::new()
        .unwrap()
        .plan(req(d, target, sets), &rep, &CancelToken::default())
        .unwrap()
}

fn run(d: &TempDir) -> (Vec<EngineEvent>, BatchSummary) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let e = Engine::new().unwrap();
    let p = e
        .plan(req(d, Target::Cs, &[]), &rep, &CancelToken::default())
        .unwrap();
    let s = e.run(p, &rep, &CancelToken::default()).unwrap();
    (sink.take(), s)
}

fn job<'a>(p: &'a Plan, input: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == input))
        .unwrap_or_else(|| panic!("no job for {input}"))
}

fn top(p: &Plan, input: &str) -> (Subtype, Confidence) {
    let s = &job(p, input).suggestions[0];
    (s.subtype, s.confidence)
}

fn out(d: &TempDir, name: &str) -> DdsFile {
    read_dds(&std::fs::read(d.path().join("out").join(name)).unwrap()).unwrap()
}

fn texels(f: &DdsFile) -> Vec<[u8; 4]> {
    let (w, h, r) = f.levels[0].clone();
    match decode_level(f.format, w, h, &f.data[r], None).unwrap() {
        Decoded::Ldr(px) => px,
        Decoded::Hdr(_) => panic!("expected an LDR format"),
    }
}

fn set_files(d: &TempDir, with_rfaos: bool) {
    put(d.path(), "x.png", colour(64));
    put(d.path(), "x_sk.png", grey(64));
    put(d.path(), "x_wet.png", wet_normal(64));
    if with_rfaos {
        put(d.path(), "x_rfaos.png", colour(64));
    }
}

/// (a) An `_rfaos` member makes the set Advanced skin.
#[test]
fn an_rfaos_set_suggests_the_skin_sub_types() {
    let d = tempfile::tempdir().unwrap();
    set_files(&d, true);
    let p = plan(&d, Target::Cs, &[]);
    assert_eq!(top(&p, "x.png"), (Subtype::SkinAlbedo, Confidence::High));
    assert_eq!(
        top(&p, "x_rfaos.png"),
        (Subtype::SkinRfaos, Confidence::High)
    );
    assert_eq!(top(&p, "x_sk.png"), (Subtype::SkinSk, Confidence::High));
    assert_eq!(top(&p, "x_wet.png"), (Subtype::SkinWet, Confidence::High));
}

/// (a) A raw-map set whose recipe resolves to `_rfaos` (a fuzz source, or `packing.recipe = rfaos`) is Advanced skin.
#[test]
fn a_set_that_builds_rfaos_is_advanced_skin() {
    for toml in ["", "[files.\"x_rmaos\".packing]\nrecipe = \"rfaos\"\n"] {
        let d = tempfile::tempdir().unwrap();
        set_files(&d, false);
        put(d.path(), "x_roughness.png", grey(64));
        put(d.path(), "x_fuzz.png", grey(64));
        put(d.path(), "x_ao.png", grey(64));
        std::fs::write(d.path().join("in/sdc.toml"), toml).unwrap();
        let p = plan(&d, Target::Cs, &[]);
        assert_eq!(
            top(&p, "x_sk.png"),
            (Subtype::SkinSk, Confidence::High),
            "{toml}"
        );
        assert_eq!(top(&p, "x.png").0, Subtype::SkinAlbedo, "{toml}");
    }
    // Without a fuzz source and no override the recipe is `_rmaos`: not skin.
    let d = tempfile::tempdir().unwrap();
    set_files(&d, false);
    put(d.path(), "x_roughness.png", grey(64));
    put(d.path(), "x_ao.png", grey(64));
    let p = plan(&d, Target::Cs, &[]);
    assert_eq!(top(&p, "x_sk.png").0, Subtype::BacklightTint);
}

/// (b) Without `_rfaos`: the vanilla `_sk`, and `x_wet` is its own colour map (final review I3: `_wet` is a suffix
/// only in an Advanced skin set).
#[test]
fn outside_advanced_skin_sk_is_the_vanilla_guess_and_wet_is_a_base_name() {
    let d = tempfile::tempdir().unwrap();
    set_files(&d, false);
    let p = plan(&d, Target::Cs, &[]);
    assert_eq!(
        top(&p, "x_sk.png"),
        (Subtype::BacklightTint, Confidence::Low)
    );
    assert_eq!(top(&p, "x_wet.png"), (Subtype::Diffuse, Confidence::Medium));
    assert_eq!(top(&p, "x.png").0, Subtype::Diffuse);
}

/// (c) A grey `_wet` is a BC4 height map and a colour one BC7 with its alpha, and `SKIN-WET-FORM` says which.
#[test]
fn wet_form_follows_the_content() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "g/x.png", colour(64));
    put(d.path(), "g/x_rfaos.png", colour(64));
    put(d.path(), "g/x_wet.png", grey(64));
    put(d.path(), "c/y.png", colour(64));
    put(d.path(), "c/y_rfaos.png", colour(64));
    put(d.path(), "c/y_wet.png", wet_normal(64));
    let (ev, s) = run(&d);
    assert_eq!(s.exit_code(), 0, "{s:?}");
    let form = |suffix: &str| {
        ev.iter()
            .find_map(|e| match &e.kind {
                EventKind::Warning(w)
                    if w.code == Code::SkinWetForm
                        && e.path.as_deref().is_some_and(|p| p.ends_with(suffix)) =>
                {
                    Some(w.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no SKIN-WET-FORM for {suffix}"))
    };
    let g = form("g/x_wet.png");
    assert_eq!(g.data["form"], "height");
    assert!(
        g.message.contains("height map") && g.message.contains("bc4"),
        "{}",
        g.message
    );
    let c = form("c/y_wet.png");
    assert_eq!(c.data["form"], "normal_mask");
    assert!(
        c.message.contains("mask in alpha") && c.message.contains("bc7"),
        "{}",
        c.message
    );
    assert_eq!(out(&d, "g/x_wet.dds").format, DdsPixelFormat::Bc4);
    let cw = out(&d, "c/y_wet.dds");
    assert_eq!(cw.format, DdsPixelFormat::Bc7 { srgb: false });
    // Alpha kept: the mask's two halves decode apart.
    let px = texels(&cw);
    assert!(px[0][3] > 200 && px[63][3] < 50, "{:?} {:?}", px[0], px[63]);
}

/// (c) An explicit `format` overrides the analysis.
#[test]
fn wet_format_override_wins() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x.png", colour(64));
    put(d.path(), "x_rfaos.png", colour(64));
    put(d.path(), "x_wet.png", grey(64));
    let p = plan(&d, Target::Cs, &["subtype:skin_wet.format=bc7"]);
    let j = job(&p, "x_wet.png");
    assert_eq!(j.profile.format, Format::Bc7);
    let w = j
        .warnings
        .iter()
        .find(|w| w.code == Code::SkinWetForm)
        .unwrap();
    assert!(w.message.contains("bc7"), "{}", w.message);
}

/// (d) `skin_albedo` is UNORM, never sRGB.
#[test]
fn skin_albedo_writes_bc7_unorm() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x.png", colour(64));
    put(d.path(), "x_rfaos.png", colour(64));
    let (_, s) = run(&d);
    assert_eq!(s.exit_code(), 0, "{s:?}");
    assert_eq!(out(&d, "x.dds").format, DdsPixelFormat::Bc7 { srgb: false });
    let p = plan(&d, Target::Cs, &[]);
    assert_eq!(job(&p, "x.png").profile.subtype, Subtype::SkinAlbedo);
    assert!(!Subtype::SkinAlbedo.is_pbr_color());
}

/// `skin_sk` keeps the red channel only, as BC4.
#[test]
fn skin_sk_is_bc4_from_red() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x.png", colour(64));
    put(d.path(), "x_rfaos.png", colour(64));
    put(d.path(), "x_sk.png", colour(64));
    let (_, s) = run(&d);
    assert_eq!(s.exit_code(), 0, "{s:?}");
    let f = out(&d, "x_sk.dds");
    assert_eq!(f.format, DdsPixelFormat::Bc4);
    let px = texels(&f);
    // R ramps with x (x * 3); G (y * 2) must not leak in.
    assert!(
        px[0][0] < 12 && (i32::from(px[63][0]) - 189).abs() < 12,
        "{:?} {:?}",
        px[0],
        px[63]
    );
}

/// (e) The skin sub-types exist on `cs` only.
#[test]
fn skin_sub_types_are_unavailable_off_cs() {
    for st in [
        Subtype::SkinAlbedo,
        Subtype::SkinRfaos,
        Subtype::SkinSk,
        Subtype::SkinWet,
    ] {
        assert_eq!(availability(Target::Cs, st), Availability::Available);
        for t in [Target::Cpm, Target::Vanilla] {
            assert_eq!(
                availability(t, st),
                Availability::Unavailable { fallback: None }
            );
        }
    }
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x_sk.png", grey(64));
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    let err = Engine::new()
        .unwrap()
        .plan(
            req(&d, Target::Vanilla, &["subtype=skin_sk"]),
            &rep,
            &CancelToken::default(),
        )
        .err()
        .expect("a batch-wide unavailable sub-type is refused")
        .to_string();
    assert!(
        err.contains("skin_sk") && err.contains("unavailable") && err.contains("CFG-UNAVAILABLE"),
        "{err}"
    );
}

/// The pre-rename id loads as `skin_rfaos` (JSON and a config table) and the new id is the one written.
#[test]
fn the_old_rfaos_id_loads_as_skin_rfaos() {
    assert_eq!(
        serde_json::from_str::<Subtype>("\"rfaos\"").unwrap(),
        Subtype::SkinRfaos
    );
    assert_eq!(Subtype::SkinRfaos.id(), "skin_rfaos");
    assert_eq!(
        serde_json::to_string(&Subtype::SkinRfaos).unwrap(),
        "\"skin_rfaos\""
    );
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x_rfaos.png", colour(64));
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[subtypes.rfaos]\nmips = { levels = \"none\" }\n",
    )
    .unwrap();
    let p = plan(&d, Target::Cs, &[]);
    let j = job(&p, "x_rfaos.png");
    assert_eq!(j.profile.subtype, Subtype::SkinRfaos);
    assert_eq!(j.profile.mips.levels, MipLevels::Keyword(MipKeyword::None));
}

/// `_wet` is not a §6.1 suffix (only `join_skin_wet` splits it, in a cs Advanced skin set) and is no vocabulary term.
#[test]
fn wet_is_not_a_general_suffix() {
    assert_eq!(
        engine::group::split_suffix("skin_wet"),
        ("skin_wet".to_string(), None)
    );
    assert!(!engine::group::is_vocabulary("wet"));
}

/// Final review I1: a forced `_rmaos` beats a fuzz source. The set builds `_rmaos`, so it is not Advanced skin;
/// only a real `_rfaos` member short-circuits, never the `rfaos` suffix that `auto` predicts from the fuzz source.
#[test]
fn a_forced_rmaos_with_a_fuzz_source_is_not_advanced_skin() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x.png", colour(64));
    put(d.path(), "x_sk.png", grey(64));
    put(d.path(), "x_roughness.png", grey(64));
    put(d.path(), "x_fuzz.png", grey(64));
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[files.\"x_rmaos\".packing]\nrecipe = \"rmaos\"\n",
    )
    .unwrap();
    let p = plan(&d, Target::Cs, &[]);
    assert!(p
        .jobs
        .iter()
        .any(|j| j.output.file_name().is_some_and(|n| n == "x_rmaos.dds")));
    assert_eq!(top(&p, "x.png").0, Subtype::PbrAlbedo);
    assert_eq!(top(&p, "x_sk.png").0, Subtype::BacklightTint);
}

fn set_of<'a>(p: &'a Plan, input: &str) -> &'a str {
    &job(p, input).set_key
}

/// Final review I3: outside a cs Advanced skin set `_wet` is part of the base name, as it was before `_wet` became
/// a skin suffix. A landscape `road_wet` is still a landscape diffuse on every target.
#[test]
fn a_wet_stem_outside_skin_keeps_its_old_reading() {
    for target in [Target::Vanilla, Target::Cs] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "textures/landscape/road_wet.png", colour(64));
        let p = plan(&d, target, &[]);
        assert_eq!(
            top(&p, "road_wet.png"),
            (Subtype::LandscapeDiffuse, Confidence::High),
            "{target:?}"
        );
        assert!(
            set_of(&p, "road_wet.png").ends_with("/road_wet"),
            "{target:?}"
        );
    }
}

/// Final review I3: `cobble_wet.png` and `cobble_wet_n.png` stay one set (`cobble_wet`), next to a `cobble` set.
#[test]
fn a_wet_colour_map_and_its_normal_are_one_set() {
    for target in [Target::Vanilla, Target::Cs] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "cobble.png", colour(64));
        put(d.path(), "cobble_wet.png", colour(64));
        put(d.path(), "cobble_wet_n.png", wet_normal(64));
        let p = plan(&d, target, &[]);
        assert_eq!(
            set_of(&p, "cobble_wet.png"),
            set_of(&p, "cobble_wet_n.png"),
            "{target:?}"
        );
        assert!(set_of(&p, "cobble_wet.png").ends_with("/cobble_wet"));
        assert_ne!(set_of(&p, "cobble.png"), set_of(&p, "cobble_wet.png"));
        assert_ne!(top(&p, "cobble_wet.png").0, Subtype::SkinWet, "{target:?}");
    }
}

/// Final review I3: `_wet` joins its base as a skin member only on cs and only in an Advanced skin set: an `_rfaos`
/// member, or a raw-map set whose recipe resolves to `rfaos`.
#[test]
fn a_wet_member_joins_only_a_cs_advanced_skin_set() {
    let d = tempfile::tempdir().unwrap();
    set_files(&d, true);
    let p = plan(&d, Target::Cs, &[]);
    assert_eq!(set_of(&p, "x_wet.png"), set_of(&p, "x.png"));
    assert_eq!(top(&p, "x_wet.png"), (Subtype::SkinWet, Confidence::High));
    // A raw-map set with `packing.recipe = rfaos` and no fuzz source.
    let d = tempfile::tempdir().unwrap();
    set_files(&d, false);
    put(d.path(), "x_roughness.png", grey(64));
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[files.\"x_rfaos\".packing]\nrecipe = \"rfaos\"\n",
    )
    .unwrap();
    let p = plan(&d, Target::Cs, &[]);
    assert_eq!(set_of(&p, "x_wet.png"), set_of(&p, "x.png"));
    assert_eq!(top(&p, "x_wet.png"), (Subtype::SkinWet, Confidence::High));
    // Not on cpm, even with an `_rfaos` beside it.
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "x.png", colour(64));
    put(d.path(), "x_rfaos.png", colour(64));
    put(d.path(), "x_wet.png", colour(64));
    let p = plan(&d, Target::Cpm, &[]);
    assert_ne!(set_of(&p, "x_wet.png"), set_of(&p, "x.png"));
    assert_eq!(top(&p, "x_wet.png").0, Subtype::Diffuse);
}
