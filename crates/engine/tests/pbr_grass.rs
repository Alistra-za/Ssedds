//! PBR grass (NIF team design notes, PBR grass, 2026-10-14): the auto-pick from
//! GRAS evidence and the PBR grass warnings, at plan level.
use engine::api::*;
use engine::codes::Code;
use engine::config::{Subtype, Target};
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::shader::{AutoRule, Shader, ShaderOrigin};
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

/// The set key (and GRAS key) of the fixture set.
const SET: &str = "textures/plants/g";
const BROWN: [u8; 3] = [120, 90, 60];
/// Decodes to (0, 0, 1): CS's complex-grass detector would accept this texel.
const UNIT_BLUE: [u8; 3] = [128, 128, 255];

fn put(dir: &Path, rel: &str, px: [u8; 3]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb(px))
        .save(p)
        .unwrap();
}

/// The PBR set `textures/plants/g`: an albedo of colour `albedo`, a flat `_n`, an `_rmaos`.
fn pbr_set(dir: &Path, albedo: [u8; 3]) {
    put(dir, "textures/plants/g.png", albedo);
    put(dir, "textures/plants/g_n.png", [128, 128, 255]);
    put(dir, "textures/plants/g_rmaos.png", [128, 0, 255]);
}

/// Plans `in/` under `t` with `gras` as `gras_all_diffuses`, `sets` as `--set` and `gui` as the GUI's set shaders.
fn plan(
    dir: &Path,
    t: Target,
    gras: &[&str],
    sets: &[&str],
    gui: &[(&str, Shader)],
) -> (Plan, Vec<EngineEvent>) {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), t);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.gras_all_diffuses = gras.iter().map(|s| s.to_string()).collect();
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.set_shaders = gui.iter().map(|(k, s)| (k.to_string(), *s)).collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pbr_grass");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

/// The one set's pick.
fn picked(p: &Plan) -> (Shader, ShaderOrigin, Option<Shader>) {
    let s = p.shaders.values().next().expect("one set");
    (s.shader, s.provenance.clone(), s.wanted)
}

#[test]
fn a_pbr_set_a_gras_model_uses_picks_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert_eq!(
        picked(&p),
        (
            Shader::PbrGrass,
            ShaderOrigin::Auto(AutoRule::PbrGrass),
            None
        )
    );
    let (p, _) = plan(d.path(), Target::Cs, &[], &[], &[]);
    assert_eq!(picked(&p).0, Shader::Pbr, "no GRAS evidence, plain pbr");
}

#[test]
fn an_explicit_pbr_wins_over_gras_evidence() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[(SET, Shader::Pbr)]);
    assert_eq!(picked(&p), (Shader::Pbr, ShaderOrigin::Set, None));
}

#[test]
fn the_cli_can_choose_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(
        d.path(),
        Target::Cs,
        &[],
        &["set:textures/plants/g.shader=pbr_grass"],
        &[],
    );
    assert_eq!(picked(&p), (Shader::PbrGrass, ShaderOrigin::Set, None));
}

#[test]
fn vanilla_falls_back_from_pbr_grass() {
    let t = Target::Vanilla;
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(d.path(), t, &[SET], &[], &[(SET, Shader::PbrGrass)]);
    assert_eq!(
        picked(&p),
        (
            Shader::Default,
            ShaderOrigin::Fallback,
            Some(Shader::PbrGrass)
        ),
        "{t:?}"
    );
}

#[test]
fn gras_evidence_on_a_set_that_is_not_pbr_is_not_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/plants/g.png", BROWN);
    put(d.path(), "textures/plants/g_n.png", [128, 128, 255]);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert_ne!(picked(&p).0, Shader::PbrGrass);
}

/// The warning codes of the albedo job.
fn albedo_codes(p: &Plan) -> Vec<Code> {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == "g.png"))
        .expect("the albedo job")
        .warnings
        .iter()
        .map(|w| w.code)
        .collect()
}

#[test]
fn an_albedo_that_reads_as_complex_grass_warns_only_on_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), UNIT_BLUE);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert!(
        albedo_codes(&p).contains(&Code::PbrGrassDetect),
        "{:?}",
        albedo_codes(&p)
    );
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[(SET, Shader::Pbr)]);
    assert!(!albedo_codes(&p).contains(&Code::PbrGrassDetect));
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert!(!albedo_codes(&p).contains(&Code::PbrGrassDetect));
}

/// Writes a `w` × `h` RGB image whose texel `(x, y)` is `px(x, y)`.
fn put_with(dir: &Path, rel: &str, w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 3]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, h, |x, y| Rgb(px(x, y)))
        .save(p)
        .unwrap();
}

/// The bottom-left texel `(0, h - 1)` is the one read: a unit vector there warns, unit vectors on the other three
/// corners of a brown albedo do not.
#[test]
fn only_the_bottom_left_texel_of_the_albedo_counts() {
    let corner = |at_bl: bool| {
        move |x: u32, y: u32| {
            let bl = x == 0 && y == 23;
            let other = (x == 0 || x == 15) && (y == 0 || y == 23) && !bl;
            if (at_bl && bl) || (!at_bl && other) {
                UNIT_BLUE
            } else {
                BROWN
            }
        }
    };
    for (at_bl, want) in [(true, true), (false, false)] {
        let d = tempfile::tempdir().unwrap();
        pbr_set(d.path(), BROWN);
        put_with(d.path(), "textures/plants/g.png", 16, 24, corner(at_bl));
        let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
        assert_eq!(
            albedo_codes(&p).contains(&Code::PbrGrassDetect),
            want,
            "{at_bl}"
        );
    }
}

/// Review fix 1: a lone GRAS 1:2 diffuse whose bottom half is unit vectors guesses complex_grass in pass 1; an
/// explicit pbr_grass re-resolves it to a diffuse, and the note follows that final sub-type, once.
#[test]
fn a_gras_diffuse_re_resolved_by_an_explicit_pbr_grass_is_noted_once() {
    let d = tempfile::tempdir().unwrap();
    put_with(d.path(), "textures/plants/g.png", 16, 32, |_, y| {
        if y < 16 {
            BROWN
        } else {
            UNIT_BLUE
        }
    });
    let plan_with = |shader: Option<Shader>| {
        let mut req =
            BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
        req.recursive = true;
        req.accept = AcceptSuggestions::High;
        // A non-PBR GRAS diffuse is on both lists (`gras_diffuses` drives the complex-grass guess).
        req.gras_diffuses = [SET.to_string()].into();
        req.gras_all_diffuses = req.gras_diffuses.clone();
        req.set_shaders = shader.map(|s| (SET.to_string(), s)).into_iter().collect();
        let rep = Reporter::new(Arc::new(VecSink::default()), "pbr_grass");
        Engine::new()
            .unwrap()
            .plan(req, &rep, &CancelToken::default())
            .unwrap()
    };
    let job = |p: &Plan| p.jobs[0].profile.subtype;
    assert_eq!(
        job(&plan_with(None)),
        Subtype::ComplexGrass,
        "pass 1 guesses complex grass"
    );
    let p = plan_with(Some(Shader::PbrGrass));
    assert_eq!(job(&p), Subtype::Diffuse);
    let codes = albedo_codes(&p);
    assert_eq!(
        codes.iter().filter(|c| **c == Code::PbrGrassDetect).count(),
        1,
        "{codes:?}"
    );
}

/// Audit 08e-1, owner ruling (b) (Note BV5): PBR grass under `textures/landscape/` is typed as PBR, not landscape:
/// the albedo is `pbr_albedo` (sRGB) and the `_n` `pbr_normal`, so the PBR grass map check finds them.
#[test]
fn pbr_grass_under_landscape_is_typed_pbr() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/landscape/g.png", BROWN);
    put(d.path(), "textures/landscape/g_n.png", [128, 128, 255]);
    put(d.path(), "textures/landscape/g_rmaos.png", [128, 0, 255]);
    let (p, ev) = plan(d.path(), Target::Cs, &["textures/landscape/g"], &[], &[]);
    assert_eq!(picked(&p).0, Shader::PbrGrass);
    let st = |name: &str| {
        p.jobs
            .iter()
            .find(|j| j.input.file_name().is_some_and(|n| n == name))
            .unwrap_or_else(|| panic!("no job for {name}"))
            .profile
            .subtype
    };
    assert_eq!(st("g.png"), Subtype::PbrAlbedo);
    assert_eq!(st("g_n.png"), Subtype::PbrNormal);
    let map_codes: Vec<_> = ev
        .iter()
        .flat_map(|e| match &e.kind {
            EventKind::SetResolved { warnings, .. } => warnings.iter().map(|w| w.code).collect(),
            _ => vec![],
        })
        .filter(|c| matches!(c, Code::ShaderMissingMap | Code::ShaderExtraMap))
        .collect();
    assert!(map_codes.is_empty(), "{map_codes:?}");
}

/// Audit 08b-3: GRAS evidence is matched by the output name the NIF uses (`g.dds`), so a vocabulary-named albedo
/// (`g_basecolor.png`) still makes the set PBR grass.
#[test]
fn a_vocabulary_named_albedo_matches_gras_evidence() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/plants/g_basecolor.png", BROWN);
    put(d.path(), "textures/plants/g_n.png", [128, 128, 255]);
    put(d.path(), "textures/plants/g_rmaos.png", [128, 0, 255]);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert_eq!(picked(&p).0, Shader::PbrGrass);
}
