//! M6 Track E (DECISIONS Note U): a colour member's alpha policy from the meshes that use it (`BatchRequest::mesh_alpha`,
//! provenance `mesh:alpha`).
use engine::api::*;
use engine::codes::Code;
use engine::config::{AlphaPolicy, Format, Target};
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, JobStatus, Need, Provenance, ResolvedJob};
use engine::shader::MeshAlpha;
use image::{ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

const WALL: &str = "textures/a/wall";
const BLEND: MeshAlpha = MeshAlpha {
    blend: true,
    test: false,
    threshold: 0,
};
const NO_ALPHA: MeshAlpha = MeshAlpha {
    blend: false,
    test: false,
    threshold: 0,
};
const fn tested(threshold: u8) -> MeshAlpha {
    MeshAlpha {
        blend: false,
        test: true,
        threshold,
    }
}

fn put_rgba(dir: &Path, rel: &str, f: impl Fn(u32, u32) -> [u8; 4]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgba(f(x, y)))
        .save(p)
        .unwrap();
}

fn put_rgb(dir: &Path, rel: &str) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([(x * 16) as u8, (y * 16) as u8, 90]))
        .save(p)
        .unwrap();
}

/// A colour map whose alpha varies (a ramp).
fn varying(dir: &Path, rel: &str) {
    put_rgba(dir, rel, |x, _| [90, 60, 30, (x * 16) as u8]);
}

fn plan(dir: &Path, preset: Option<&str>, sets: &[&str], mesh: &[(&str, Vec<MeshAlpha>)]) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.preset = preset.map(str::to_string);
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.mesh_alpha = mesh
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    let rep = Reporter::new(Arc::new(NullSink), "mesh-alpha");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}

fn job<'a>(p: &'a Plan, tail: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| {
            j.output
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(tail)
        })
        .unwrap_or_else(|| panic!("no job {tail}"))
}

fn prov(j: &ResolvedJob, f: &str) -> String {
    j.provenance[f].to_string()
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn asks_alpha(j: &ResolvedJob) -> bool {
    matches!(&j.status, JobStatus::Unresolved { needs, .. } if needs.contains(&Need::AlphaMeaning))
}

#[test]
fn mesh_alpha_provenance_round_trips_and_sits_above_the_shader() {
    assert_eq!(Provenance::MeshAlpha.to_string(), "mesh:alpha");
    assert_eq!(
        "mesh:alpha".parse::<Provenance>().unwrap(),
        Provenance::MeshAlpha
    );
    assert!(Provenance::Shader(String::new()) < Provenance::MeshAlpha);
    assert!(Provenance::MeshAlpha < Provenance::Suffix(String::new()));
}

/// Spec §4.3, §6: evidence decides a varying-alpha diffuse (provenance `mesh:alpha`, the threshold too) and the alpha
/// question is not asked; without evidence, or with evidence for another texture, it is asked as before.
#[test]
fn mesh_evidence_decides_the_alpha_and_no_evidence_asks_as_before() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let p = plan(
        d.path(),
        None,
        &[],
        &[(WALL, vec![tested(100), tested(140), NO_ALPHA])],
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, j.profile.alpha.threshold),
        (AlphaPolicy::Cutout, 140)
    );
    assert_eq!(prov(j, "alpha.policy"), "mesh:alpha");
    assert_eq!(prov(j, "alpha.threshold"), "mesh:alpha");
    assert!(!asks_alpha(j), "{:?}", j.status);
    assert!(
        !codes(j).contains(&Code::SugLowConfidence),
        "{:?}",
        j.warnings
    );
    assert!(asks_alpha(job(&plan(d.path(), None, &[], &[]), "wall.dds")));
    let other = plan(d.path(), None, &[], &[("textures/a/other", vec![BLEND])]);
    assert!(asks_alpha(job(&other, "wall.dds")));
}

/// Spec §4.3, §6: an explicit layer beats the mesh: `--set`/`[files]` (applied after it) and a project config
/// (applied before it).
#[test]
fn an_explicit_alpha_policy_beats_the_mesh() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let mesh = [(WALL, vec![BLEND])];
    let p = plan(
        d.path(),
        None,
        &["file:**/wall.png.alpha.policy=cutout"],
        &mesh,
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Cutout, "user".to_string())
    );
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[subtypes.diffuse]\nalpha = { policy = \"data\" }\n",
    )
    .unwrap();
    let p = plan(d.path(), None, &[], &mesh);
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Data, "project_config".to_string())
    );
}

/// Spec §4.3 "source without alpha", Review Focus 2 (Note U7): a source with no alpha channel, or alpha constant
/// 255, keeps the analysis `remove` whatever the meshes say, so an opaque RGBA colour map stays BC1 under cs-compact.
#[test]
fn a_source_without_varying_alpha_keeps_the_analysis_remove() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path(), "textures/a/wall.png");
    put_rgba(d.path(), "textures/a/glass.png", |x, y| {
        [(x * 16) as u8, (y * 16) as u8, 90, 255]
    });
    let mesh = [
        (WALL, vec![tested(128)]),
        ("textures/a/glass", vec![tested(128)]),
    ];
    let p = plan(d.path(), Some("cs-compact"), &[], &mesh);
    for tail in ["wall.dds", "glass.dds"] {
        let j = job(&p, tail);
        assert_eq!(
            (j.profile.alpha.policy, prov(j, "alpha.policy")),
            (AlphaPolicy::Remove, "analysis".to_string()),
            "{tail}"
        );
        assert_eq!(j.profile.format, Format::Bc1, "{tail}");
    }
}

/// Spec Review Focus 5 (Note U8) and owner O1: a mesh `remove` on a varying-alpha diffuse drops the alpha with no
/// LOSS-ALPHA-DATA (the meshes never read it), and under cs-compact it is opaque colour, so BC1 like any other.
#[test]
fn a_mesh_remove_on_varying_alpha_is_bc1_under_compact_without_a_loss_warning() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let p = plan(
        d.path(),
        Some("cs-compact"),
        &[],
        &[(WALL, vec![NO_ALPHA, NO_ALPHA])],
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Remove, "mesh:alpha".to_string())
    );
    assert_eq!(j.profile.format, Format::Bc1);
    assert!(!codes(j).contains(&Code::LossAlphaData), "{:?}", j.warnings);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// Owner O1: an explicit policy beats the mesh `remove`, and then the BC1 rules do not fire (no BC1 + blend).
#[test]
fn an_explicit_policy_over_a_mesh_remove_keeps_compact_off_bc1() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let p = plan(
        d.path(),
        Some("cs-compact"),
        &["file:**/wall.png.alpha.policy=blend"],
        &[(WALL, vec![NO_ALPHA])],
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, j.profile.format),
        (AlphaPolicy::Blend, Format::Bc7)
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// Spec Review Focus 4: a colour member renamed by the vocabulary (`wall_albedo.png` → `wall.dds`) is keyed by its
/// output stem, the name the meshes reference.
#[test]
fn a_renamed_colour_member_is_keyed_by_its_output_stem() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall_albedo.png");
    let p = plan(d.path(), None, &[], &[(WALL, vec![BLEND])]);
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Blend, "mesh:alpha".to_string())
    );
}

/// Note U7, Review Focus 1 and 5 of the plan, owner O2: the mesh beats a shader's alpha default (hair: cutout), and a
/// mesh threshold outside 1..=254 is clamped, never CFG-INVALID.
#[test]
fn the_mesh_beats_the_shader_default_and_its_threshold_is_clamped() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let p = plan(
        d.path(),
        None,
        &["set:textures/a/wall.shader=hair"],
        &[(WALL, vec![BLEND])],
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Blend, "mesh:alpha".to_string())
    );
    let p = plan(d.path(), None, &[], &[(WALL, vec![tested(0)])]);
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, j.profile.alpha.threshold),
        (AlphaPolicy::Cutout, 1)
    );
    assert!(!codes(j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
}
