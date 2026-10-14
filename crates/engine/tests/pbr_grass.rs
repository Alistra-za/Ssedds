//! PBR grass (design `F:\ai\dds\nifparseref\plan\pbr-grass\2026-10-14-pbr-grass-design.md`): the auto-pick from
//! GRAS evidence and the PBR grass warnings, at plan level.
use engine::api::*;
use engine::config::Target;
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
fn vanilla_and_cpm_fall_back_from_pbr_grass() {
    for t in [Target::Vanilla, Target::Cpm] {
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
}

#[test]
fn gras_evidence_on_a_set_that_is_not_pbr_is_not_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/plants/g.png", BROWN);
    put(d.path(), "textures/plants/g_n.png", [128, 128, 255]);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert_ne!(picked(&p).0, Shader::PbrGrass);
}
