//! M6 Track A (DECISIONS Note S): shader choice, fallback and set warnings at plan level.
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::overrides::{parse_set, Scope};
use engine::policy::AcceptSuggestions;
use engine::shader::{Shader, ShaderOrigin};
use image::{ImageBuffer, Rgb};
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
