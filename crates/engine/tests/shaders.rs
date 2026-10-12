//! M6 Track A (DECISIONS Note S): shader choice, fallback and set warnings at plan level.
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::overrides::{parse_set, Scope};
use engine::policy::AcceptSuggestions;
use engine::shader::{Shader, ShaderOrigin};
use image::{ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

fn put_rgb(dir: &Path, rel: &str, px: [u8; 3]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb(px))
        .save(p)
        .unwrap();
}

fn plan(dir: &Path, t: Target, sets: &[&str], gui: &[(&str, Shader)]) -> (Plan, Vec<EngineEvent>) {
    plan_with(dir, t, sets, gui, &[])
}

/// `plan` with mesh votes (S-shader-10): `shader::mesh_key` → the shaders the meshes report.
fn plan_with(
    dir: &Path,
    t: Target,
    sets: &[&str],
    gui: &[(&str, Shader)],
    mesh: &[(&str, Vec<Shader>)],
) -> (Plan, Vec<EngineEvent>) {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), t);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.set_shaders = gui.iter().map(|(k, s)| (k.to_string(), *s)).collect();
    req.mesh_shaders = mesh
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "shaders");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn wall(dir: &Path) {
    put_rgb(dir, "textures/a/wall.png", [120, 90, 60]);
    put_rgb(dir, "textures/a/wall_n.png", [128, 128, 255]);
}

fn set_event<'a>(ev: &'a [EngineEvent], tail: &str) -> &'a EventKind {
    &ev.iter()
        .find(|e| {
            matches!(e.kind, EventKind::SetResolved { .. })
                && e.set.as_deref().is_some_and(|s| s.ends_with(tail))
        })
        .unwrap()
        .kind
}

#[test]
fn set_scope_parses_and_rejects_misuse() {
    let o = parse_set("set:textures/a/wall.v2.shader=hair").unwrap();
    assert_eq!(o.scope, Scope::Set("textures/a/wall.v2".into()));
    assert_eq!(o.field, "shader");
    assert!(parse_set("set:wall.shader=nope")
        .unwrap_err()
        .contains("unknown shader"));
    assert!(parse_set("set:wall.shader=auto").is_err());
    assert!(parse_set("set:wall.format=bc7")
        .unwrap_err()
        .contains("only `shader`"));
    assert!(parse_set("shader=hair").is_err());
    assert!(parse_set("file:*.png.shader=hair").is_err());
}

#[test]
fn auto_pick_is_reported_on_the_set() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, ev) = plan(d.path(), Target::Vanilla, &[], &[]);
    let pick = p.shaders.values().next().unwrap();
    assert_eq!(
        (pick.shader, pick.provenance.to_string().as_str()),
        (Shader::Default, "auto:default")
    );
    let EventKind::SetResolved {
        shader, warnings, ..
    } = set_event(&ev, "textures/a/wall")
    else {
        unreachable!()
    };
    assert_eq!(shader.as_ref(), Some(pick));
    assert_eq!(
        warnings.iter().map(|w| w.code).collect::<Vec<_>>(),
        vec![Code::ShaderAuto]
    );
    assert!(p.jobs.iter().all(|j| j.shader.as_ref() == Some(pick)));
}

#[test]
fn explicit_via_cli_and_gui_and_cli_wins() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, _) = plan(
        d.path(),
        Target::Vanilla,
        &[],
        &[("textures/a/wall", Shader::Hair)],
    );
    assert_eq!(p.shaders.values().next().unwrap().shader, Shader::Hair);
    assert_eq!(
        p.shaders.values().next().unwrap().provenance,
        ShaderOrigin::Set
    );
    let (p, _) = plan(
        d.path(),
        Target::Vanilla,
        &["set:wall.shader=glow"],
        &[("textures/a/wall", Shader::Hair)],
    );
    assert_eq!(
        p.shaders.values().next().unwrap().shader,
        Shader::Glow,
        "the command line wins; a bare base matches"
    );
}

#[test]
fn unavailable_falls_back_with_a_warning() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, ev) = plan(
        d.path(),
        Target::Vanilla,
        &["set:textures/a/wall.shader=pbr"],
        &[],
    );
    let pick = p.shaders.values().next().unwrap();
    assert_eq!(
        (pick.shader, &pick.provenance, pick.wanted),
        (Shader::Default, &ShaderOrigin::Fallback, Some(Shader::Pbr))
    );
    let EventKind::SetResolved { warnings, .. } = set_event(&ev, "textures/a/wall") else {
        unreachable!()
    };
    assert_eq!(
        warnings.iter().map(|w| w.code).collect::<Vec<_>>(),
        vec![Code::ShaderUnavailable]
    );
}

#[test]
fn extra_and_missing_maps_warn_and_everything_still_converts() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_g.png", [200, 200, 0]);
    let (p, ev) = plan(
        d.path(),
        Target::Vanilla,
        &["set:wall.shader=parallax"],
        &[],
    );
    let EventKind::SetResolved { warnings, .. } = set_event(&ev, "textures/a/wall") else {
        unreachable!()
    };
    let codes: Vec<Code> = warnings.iter().map(|w| w.code).collect();
    assert_eq!(codes, vec![Code::ShaderExtraMap, Code::ShaderMissingMap]);
    assert_eq!(warnings[0].data["member"], "wall_g.dds");
    assert_eq!(warnings[1].data["map"], "_p");
    assert!(
        p.jobs.iter().any(|j| j.output.ends_with("wall_g.dds")),
        "an extra map still converts"
    );
    assert!(
        !p.jobs.iter().any(|j| j.output.ends_with("wall_p.dds")),
        "nothing is generated"
    );
}

#[test]
fn an_unmatched_set_id_is_stale_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (_, ev) = plan(
        d.path(),
        Target::Vanilla,
        &["set:nothing/here.shader=hair"],
        &[],
    );
    let stale: Vec<_> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == Code::SetEditStale => Some(w.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].data["set"], "nothing/here");
}

#[test]
fn switching_mode_re_resolves() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_rmaos.png", [128, 0, 255]);
    let (cs, _) = plan(d.path(), Target::Cs, &[], &[]);
    assert_eq!(cs.shaders.values().next().unwrap().shader, Shader::Pbr);
    let (va, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(
        va.shaders.values().next().unwrap().provenance,
        ShaderOrigin::Fallback
    );
}

// Controller override (task-4-override.md): `_p` beats a mesh majority, so this uses `_g`.
#[test]
fn meshes_pick_the_shader_unless_a_patchable_map_a_pbr_member_or_a_choice_says_otherwise() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_g.png", [128, 128, 128]);
    let hair = [("textures/a/wall", vec![Shader::Hair, Shader::Hair])];
    let (p, ev) = plan_with(d.path(), Target::Vanilla, &[], &[], &hair);
    let pick = p.shaders.values().next().unwrap();
    assert_eq!(
        (pick.shader, pick.provenance.to_string().as_str()),
        (Shader::Hair, "auto:mesh")
    );
    let EventKind::SetResolved { warnings, .. } = set_event(&ev, "textures/a/wall") else {
        unreachable!()
    };
    assert_eq!(warnings[0].code, Code::ShaderAuto);
    assert_eq!(warnings[0].data["rule"], "mesh");
    // A key for another texture is no evidence: `_g` decides.
    let (p, _) = plan_with(
        d.path(),
        Target::Vanilla,
        &[],
        &[],
        &[("textures/a/other", vec![Shader::Hair])],
    );
    assert_eq!(p.shaders.values().next().unwrap().shader, Shader::Glow);
    // The explicit choice still wins.
    let (p, _) = plan_with(
        d.path(),
        Target::Vanilla,
        &["set:wall.shader=parallax"],
        &[],
        &hair,
    );
    assert_eq!(p.shaders.values().next().unwrap().shader, Shader::Parallax);
    // A `_p` member beats the mesh: PGPatcher patches the mesh to parallax.
    put_rgb(d.path(), "textures/a/wall_p.png", [128, 128, 128]);
    let (p, _) = plan_with(d.path(), Target::Vanilla, &[], &[], &hair);
    assert_eq!(
        p.shaders.values().next().unwrap().provenance.to_string(),
        "auto:p"
    );
    // A PBR member wins over a Default mesh: the shipped mesh is not patched by PGPatcher yet.
    put_rgb(d.path(), "textures/a/wall_rmaos.png", [128, 0, 255]);
    let (p, _) = plan_with(
        d.path(),
        Target::Cs,
        &[],
        &[],
        &[("textures/a/wall", vec![Shader::Default])],
    );
    assert_eq!(
        p.shaders.values().next().unwrap().provenance.to_string(),
        "auto:pbr"
    );
}

fn put_rgba(dir: &Path, rel: &str, f: impl Fn(u32, u32) -> [u8; 4]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgba(f(x, y)))
        .save(p)
        .unwrap();
}
fn job<'a>(p: &'a Plan, tail: &str) -> &'a engine::policy::ResolvedJob {
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
fn prov(j: &engine::policy::ResolvedJob, f: &str) -> String {
    j.provenance[f].to_string()
}

#[test]
fn hair_diffuse_is_cutout_with_coverage() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), "textures/hair/braid.png", |x, _| {
        [90, 60, 30, if x < 8 { 255 } else { 0 }]
    });
    put_rgb(d.path(), "textures/hair/braid_n.png", [128, 128, 255]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let j = job(&p, "braid.dds");
    assert_eq!(
        serde_json::to_value(j.profile.alpha.policy).unwrap(),
        "cutout"
    );
    assert_eq!(serde_json::to_value(j.profile.mips.coverage).unwrap(), "on");
    assert_eq!(prov(j, "alpha.policy"), "shader:hair");
    // The shader answered the alpha meaning, whatever else the job waits for.
    if let engine::policy::JobStatus::Unresolved { needs, .. } = &j.status {
        assert!(
            !needs.contains(&engine::policy::Need::AlphaMeaning),
            "{needs:?}"
        );
    }
}

/// Task 4's 6c re-resolve is what applies the pick: an explicit `--set set:wall.shader=hair` changes the member's
/// profile, and without it the same files keep the built-in policy.
#[test]
fn the_set_pick_changes_a_members_profile_through_the_re_resolve() {
    let make = || {
        let d = tempfile::tempdir().unwrap();
        put_rgba(d.path(), "textures/a/wall.png", |x, _| {
            [90, 60, 30, (x * 16) as u8]
        });
        put_rgb(d.path(), "textures/a/wall_n.png", [128, 128, 255]);
        d
    };
    let d = make();
    let (p, _) = plan(d.path(), Target::Vanilla, &["set:wall.shader=hair"], &[]);
    let j = job(&p, "wall.dds");
    assert_eq!(prov(j, "alpha.policy"), "shader:hair");
    assert_eq!(
        serde_json::to_value(j.profile.alpha.policy).unwrap(),
        "cutout"
    );
    let d = make();
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    assert_ne!(prov(job(&p, "wall.dds"), "alpha.policy"), "shader:hair");
}

#[test]
fn hair_without_alpha_keeps_remove() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path(), "textures/hair/braid.png", [90, 60, 30]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let j = job(&p, "braid.dds");
    assert_eq!(
        serde_json::to_value(j.profile.alpha.policy).unwrap(),
        "remove"
    );
    assert_eq!(prov(j, "alpha.policy"), "analysis");
}

#[test]
fn eye_n_is_model_space() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path(), "textures/eyes/eye.png", [90, 60, 30]);
    put_rgb(d.path(), "textures/eyes/eye_n.png", [128, 128, 255]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(job(&p, "eye_n.dds").profile.subtype.id(), "msn");
    // Explicit choice: decided with shader provenance.
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, _) = plan(d.path(), Target::Vanilla, &["set:wall.shader=eye"], &[]);
    let n = job(&p, "wall_n.dds");
    assert_eq!(
        (n.profile.subtype.id().as_str(), prov(n, "subtype").as_str()),
        ("msn", "shader:eye")
    );
}

#[test]
fn explicit_landscape_makes_the_diffuse_landscape_diffuse() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, _) = plan(
        d.path(),
        Target::Vanilla,
        &["set:wall.shader=landscape"],
        &[],
    );
    assert_eq!(
        job(&p, "wall.dds").profile.subtype.id(),
        "landscape_diffuse"
    );
}

#[test]
fn multilayer_diffuse_keeps_alpha() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), "textures/a/glass.png", |x, _| {
        [90, 90, 90, (x * 16) as u8]
    });
    put_rgb(d.path(), "textures/a/glass_n.png", [128, 128, 255]);
    put_rgb(d.path(), "textures/a/glass_i.png", [40, 40, 90]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let j = job(&p, "glass.dds");
    assert_eq!(
        serde_json::to_value(j.profile.alpha.policy).unwrap(),
        "data"
    );
    assert_eq!(prov(j, "alpha.policy"), "shader:multilayer");
}

#[test]
fn env_m_and_parallax_p_are_bc4_by_shader() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_m.png", [200, 200, 200]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let m = job(&p, "wall_m.dds");
    assert_eq!(
        (
            serde_json::to_value(m.profile.format).unwrap(),
            prov(m, "format")
        ),
        ("bc4".into(), "shader:envmap".to_string())
    );
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_p.png", [128, 128, 128]);
    let (p, _) = plan(d.path(), Target::Cs, &[], &[]);
    let h = job(&p, "wall_p.dds");
    assert_eq!(
        (
            serde_json::to_value(h.profile.format).unwrap(),
            prov(h, "format")
        ),
        ("bc4".into(), "shader:parallax".to_string())
    );
}

#[test]
fn explicit_layers_beat_the_shader_default() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_p.png", [128, 128, 128]);
    // --set format
    let (p, _) = plan(
        d.path(),
        Target::Cs,
        &["file:**/wall_p.png.format=bc7"],
        &[],
    );
    let h = job(&p, "wall_p.dds");
    assert_eq!(
        (
            serde_json::to_value(h.profile.format).unwrap(),
            prov(h, "format")
        ),
        ("bc7".into(), "user".to_string())
    );
    // a project config [subtypes.height] table
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[subtypes.height]\nformat = \"bc7\"\n",
    )
    .unwrap();
    let (p, _) = plan(d.path(), Target::Cs, &[], &[]);
    assert_eq!(prov(job(&p, "wall_p.dds"), "format"), "project_config");
    // a rule that names the sub-type beats the eye sub-type effect
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[[rules]]\nmatch = { glob = \"**/wall_n.png\" }\nsubtype = \"normal\"\n",
    )
    .unwrap();
    let (p, _) = plan(d.path(), Target::Cs, &["set:wall.shader=eye"], &[]);
    assert_eq!(job(&p, "wall_n.dds").profile.subtype.id(), "normal");
}

/// S-mips-0 / S-mips-13 (Track C): the hair `cutout` bars a non-box filter. Unscoped, the filter falls back to box
/// with CFG-FILTER-IGNORED (Info); file-scoped, it stays and is CFG-INVALID.
#[test]
fn hair_cutout_and_a_non_box_filter() {
    let soft = |x: u32, _| [90, 60, 30, (x * 16) as u8];
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), "textures/hair/braid.png", soft);
    let (p, _) = plan(d.path(), Target::Vanilla, &["mips.filter=kaiser"], &[]);
    let j = job(&p, "braid.dds");
    assert_eq!(prov(j, "alpha.policy"), "shader:hair");
    assert_eq!(
        (
            serde_json::to_value(j.profile.mips.filter).unwrap(),
            prov(j, "mips.filter")
        ),
        ("box".into(), "fallback".to_string())
    );
    assert!(
        j.warnings.iter().any(|w| w.code == Code::CfgFilterIgnored),
        "{:?}",
        j.warnings
    );
    let (p, _) = plan(
        d.path(),
        Target::Vanilla,
        &["file:**/braid.png.mips.filter=kaiser"],
        &[],
    );
    let j = job(&p, "braid.dds");
    assert!(
        j.warnings.iter().any(|w| w.code == Code::CfgInvalid),
        "{:?}",
        j.warnings
    );
}

#[test]
fn shader_provenance_round_trips_and_orders() {
    use engine::presets::Provenance;
    assert_eq!(Provenance::Shader("hair".into()).to_string(), "shader:hair");
    assert_eq!(
        "shader:hair".parse::<Provenance>().unwrap(),
        Provenance::Shader("hair".into())
    );
    assert!(Provenance::Shader(String::new()) < Provenance::Suffix(String::new()));
    assert!(Provenance::Shader(String::new()) > Provenance::AnalysisFact(String::new()));
}
