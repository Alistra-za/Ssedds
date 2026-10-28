//! Helper textures Task 1: hand-set helper roles (`helper.role`) and the four `helper.*` per-file selectors.
use engine::api::*;
use engine::config::*;
use engine::events::*;
use engine::helper::{FillTarget, HelperRole};
use engine::overrides::parse_set;
use engine::pack::ChannelSource;
use engine::policy::*;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;

fn gray(dir: &Path, name: &str) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
        .save(p)
        .unwrap();
}

fn color(dir: &Path, name: &str) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 3) as u8, (y * 3) as u8, 90]))
        .save(p)
        .unwrap();
}

/// `in/` holding `files` (`x.png` and `*_n.png` in colour, the rest grey) and `in/sdc.toml`.
fn fixture(files: &[&str], toml: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for f in files {
        if *f == "x.png" || f.ends_with("_n.png") {
            color(d.path(), f);
        } else {
            gray(d.path(), f);
        }
    }
    std::fs::write(d.path().join("in/sdc.toml"), toml).unwrap();
    d
}

fn plan(d: &Path, target: Target, sets: &[&str]) -> Plan {
    let mut r = BatchRequest::new(vec![d.join("in")], d.join("out"), target);
    r.recursive = true;
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    let rep = Reporter::new(Arc::new(VecSink::default()), "helper-roles");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap()
}

/// The job of the input `name` that is not a packed output.
fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.pack.is_none() && j.input.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

fn packed<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.pack.is_some() && j.output.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no packed {name}"))
}

/// Packing off, so a raw job stays in the plan with its role (a consumed raw job leaves it).
const NO_PACK: &[&str] = &["packing.enabled=false"];

fn files(key: &str, body: &str) -> String {
    format!("[files.\"{key}\"]\n{body}\n")
}

#[test]
fn hand_role_overrides_name_guess() {
    let d = fixture(
        &["x.png", "x_gloss.png"],
        &files("x_gloss", "helper.role = \"roughness\""),
    );
    let p = plan(d.path(), Target::Cs, NO_PACK);
    assert_eq!(
        job(&p, "x_gloss.png").status,
        JobStatus::PackingInput {
            role: RawMap::Roughness
        }
    );
    // With packing on, the `_rmaos` R reads the file as roughness: not inverted as a glossiness would be.
    let p = plan(d.path(), Target::Cs, &[]);
    match &packed(&p, "x_rmaos.dds").pack.as_ref().unwrap().channels[0] {
        ChannelSource::Source {
            path, role, invert, ..
        } => {
            assert!(path.ends_with("x_gloss.png"), "{path:?}");
            assert_eq!((*role, *invert), (RawMap::Roughness, false));
        }
        other => panic!("R is {other:?}"),
    }
}

#[test]
fn hand_role_none_converts_alone() {
    let d = fixture(
        &["x.png", "x_opacity.png"],
        &files("x_opacity", "helper.role = \"none\""),
    );
    let p = plan(d.path(), Target::Cs, &[]);
    let j = job(&p, "x_opacity.png");
    assert!(
        !matches!(j.status, JobStatus::PackingInput { .. }),
        "{:?}",
        j.status
    );
    assert!(j.output.ends_with("x_opacity.dds"), "{:?}", j.output);
}

#[test]
fn hand_role_on_unnamed_file_makes_helper() {
    let d = fixture(
        &["x_mask.png"],
        &files("x_mask", "helper.role = \"opacity\""),
    );
    let p = plan(d.path(), Target::Cs, NO_PACK);
    assert_eq!(
        job(&p, "x_mask.png").status,
        JobStatus::PackingInput {
            role: RawMap::Opacity
        }
    );
}

#[test]
fn hand_layer_role_makes_normal_layer() {
    let d = fixture(
        &["x_n.png", "x_extra.png"],
        &files("x_extra", "helper.role = \"detailnorm\""),
    );
    let p = plan(d.path(), Target::Cs, &[]);
    match job(&p, "x_extra.png").status {
        JobStatus::NormalLayer { term } => {
            assert_eq!(term.layer, LayerId::Detail);
            assert!(!term.mask);
            assert_eq!(term.prefix, None);
        }
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn hand_role_beats_requested_subtype() {
    let d = fixture(
        &["x_ao.png"],
        &files("x_ao", "subtype = \"diffuse\"\nhelper.role = \"ao\""),
    );
    let p = plan(d.path(), Target::Cs, NO_PACK);
    assert_eq!(
        job(&p, "x_ao.png").status,
        JobStatus::PackingInput { role: RawMap::Ao }
    );
}

#[test]
fn skin_f_default_unchanged_and_overridable() {
    let rfaos = "[files.\"x_rfaos\".packing]\nrecipe = \"rfaos\"\n";
    let set = ["x.png", "x_roughness.png", "x_f.png"];
    // Note DR: the `_f` of an Advanced skin set fills the `_rfaos` G as the fuzz mask.
    let d = fixture(&set, rfaos);
    let p = plan(d.path(), Target::Cs, &[]);
    match &packed(&p, "x_rfaos.dds").pack.as_ref().unwrap().channels[1] {
        ChannelSource::Source { path, role, .. } => {
            assert!(path.ends_with("x_f.png"), "{path:?}");
            assert_eq!(*role, RawMap::Fuzz);
        }
        other => panic!("G is {other:?}"),
    }
    let d = fixture(
        &set,
        &format!("{rfaos}{}", files("x_f", "helper.role = \"fuzzweight\"")),
    );
    let p = plan(d.path(), Target::Cs, &[]);
    assert_eq!(
        job(&p, "x_f.png").status,
        JobStatus::PackingInput {
            role: RawMap::FuzzWeight
        }
    );
}

#[test]
fn hand_role_on_dds_is_ignored() {
    use engine::dds::*;
    let desc = DdsDesc {
        width: 4,
        height: 4,
        format: Format::Rgba8,
        srgb: false,
        header: resolve_header(Format::Rgba8, false, HeaderForm::Auto),
    };
    let bytes = dds_bytes(&desc, &[vec![128; level_size(Format::Rgba8, 4, 4)]]).unwrap();
    let status = |toml: &str| {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("in");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x_ao.dds"), &bytes).unwrap();
        std::fs::write(d.path().join("in/sdc.toml"), toml).unwrap();
        let p = plan(d.path(), Target::Cs, &["reencode=true"]);
        let j = job(&p, "x_ao.dds");
        (j.status.clone(), j.profile.subtype)
    };
    let before = status("");
    // The key reaches the file: a sub-type there changes its job.
    assert_ne!(status(&files("x_ao.dds", "subtype = \"height\"")), before);
    assert_eq!(
        status(&files("x_ao.dds", "helper.role = \"opacity\"")),
        before
    );
}

/// The recipe reports of a fresh cs plan of `d/in`: (output file name, status, channel provenances).
fn recipe_reports(d: &Path) -> Vec<(String, RecipeStatus, Vec<String>)> {
    let sink = Arc::new(VecSink::default());
    let mut r = BatchRequest::new(vec![d.join("in")], d.join("out"), Target::Cs);
    r.recursive = true;
    let rep = Reporter::new(sink.clone(), "helper-roles");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap();
    sink.take()
        .into_iter()
        .flat_map(|e| match e.kind {
            EventKind::SetResolved { recipes, .. } => recipes,
            _ => vec![],
        })
        .map(|r| {
            let name = Path::new(&r.output)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (name, r.status, r.channels)
        })
        .collect()
}

/// Review fix 1: a failed image keeps its hand-set role, so its recipe fails instead of defaulting the channel.
#[test]
fn failed_helper_keeps_hand_role() {
    let d = fixture(
        &["x_ao.png"],
        &files("x_opacity", "helper.role = \"roughness\""),
    );
    std::fs::write(d.path().join("in/x_opacity.png"), b"not a png").unwrap();
    let p = plan(d.path(), Target::Cs, &[]);
    assert!(p.failed.iter().any(|f| f.path.ends_with("x_opacity.png")));
    let reports = recipe_reports(d.path());
    let rmaos = reports
        .iter()
        .find(|(n, ..)| n == "x_rmaos.dds")
        .unwrap_or_else(|| panic!("{reports:?}"));
    assert_eq!(rmaos.1, RecipeStatus::Failed, "{reports:?}");
    assert!(
        rmaos.2.iter().any(|s| s.contains("x_opacity.png")),
        "{reports:?}"
    );
    assert!(
        !p.jobs.iter().any(|j| j.pack.is_some()
            && j.output.ends_with("x_rmaos.dds")
            && j.status == JobStatus::Ready),
        "no _rmaos built with a default roughness"
    );
}

/// Review fix 2: a hand-set raw role points to `helper.role = "none"`, which a sub-type does not override.
#[test]
fn hand_role_warning_points_to_none() {
    // Resolved alone: in a plan, packing re-tags the job's warning (consumed, unused or disabled).
    use engine::presets::PresetRegistry;
    let reg = PresetRegistry::builtin().unwrap();
    let overrides = vec![parse_set("file:**/x_mask.png.helper.role=opacity").unwrap()];
    let allow = Default::default();
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
        files: &engine::overrides::NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
    };
    let f = engine::discover::InputFile {
        path: "x_mask.png".into(),
        rel: "x_mask.png".into(),
        game_path: "textures/a/x_mask.png".into(),
        output: "x_mask.dds".into(),
        kind: engine::discover::InputKind::Image,
        real_rel: None,
    };
    let set = engine::group::TextureSet {
        key: "textures/a/x_mask".into(),
        members: vec![0],
        suffixes: Default::default(),
    };
    let img = engine::image::Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]);
    let info = engine::image::SourceInfo {
        bit_depth: 8,
        channels: 1,
        has_alpha: false,
        is_float: false,
    };
    let a = engine::analyse::analyse(&img, &info);
    let j = resolve_file(
        &ctx,
        &FileFacts {
            input: &f,
            suffix: None,
            alias: None,
            set: &set,
            analysis: &a,
            info: &info,
        },
    );
    assert_eq!(
        j.status,
        JobStatus::PackingInput {
            role: RawMap::Opacity
        }
    );
    let w = &j.warnings[0].message;
    assert!(w.contains("helper.role = \"none\""), "{w}");
    assert!(!w.contains("sub-type"), "{w}");
}

/// Controller ruling 4: a hand-set helper without a recognised suffix joins the set of its stem minus the last
/// `_segment` when that set exists; alone it stays its own set.
#[test]
fn unnamed_helper_joins_its_base_set() {
    let toml = files("x_mask", "helper.role = \"opacity\"");
    let d = fixture(&["x.png", "x_mask.png"], &toml);
    let p = plan(d.path(), Target::Cs, NO_PACK);
    let m = job(&p, "x_mask.png");
    assert_eq!(
        m.status,
        JobStatus::PackingInput {
            role: RawMap::Opacity
        }
    );
    assert_eq!(m.set_key, job(&p, "x.png").set_key);
    assert!(m.set_key.ends_with("/x"), "{}", m.set_key);

    let d = fixture(&["x_mask.png"], &toml);
    let p = plan(d.path(), Target::Cs, NO_PACK);
    assert!(
        job(&p, "x_mask.png").set_key.ends_with("/x_mask"),
        "{}",
        job(&p, "x_mask.png").set_key
    );
}

/// Review fix round 2: a joined helper that is a recipe's first source names the output after its set (`x_rmaos`).
#[test]
fn joined_helper_packs_under_its_set_name() {
    let d = fixture(
        &["x.png", "x_mask.png", "x_ao.png"],
        &files("x_mask", "helper.role = \"roughness\""),
    );
    let p = plan(d.path(), Target::Cs, &[]);
    let outs: Vec<String> = p
        .jobs
        .iter()
        .filter(|j| j.pack.is_some())
        .map(|j| j.output.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(outs, ["x_rmaos.dds"]);
    match &packed(&p, "x_rmaos.dds").pack.as_ref().unwrap().channels[0] {
        ChannelSource::Source { path, role, .. } => {
            assert!(path.ends_with("x_mask.png"), "{path:?}");
            assert_eq!(*role, RawMap::Roughness);
        }
        other => panic!("R is {other:?}"),
    }
}

/// Review fix round 3: a set base that itself ends in a term (`x_ao`) is not cut twice, and the packed output's own
/// `[files]` key reaches it.
#[test]
fn joined_helper_keeps_a_base_ending_in_a_term() {
    let toml = format!(
        "{}{}",
        files("x_ao_mask", "helper.role = \"roughness\""),
        "[files.\"x_ao_rmaos\".packing.fill]\nmetallic = 77\n"
    );
    let d = fixture(&["x_ao_diffuse.png", "x_ao_ao.png", "x_ao_mask.png"], &toml);
    let p = plan(d.path(), Target::Cs, &[]);
    let outs: Vec<String> = p
        .jobs
        .iter()
        .filter(|j| j.pack.is_some())
        .map(|j| j.output.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(outs, ["x_ao_rmaos.dds"]);
    let ch = &packed(&p, "x_ao_rmaos.dds").pack.as_ref().unwrap().channels;
    match &ch[0] {
        ChannelSource::Source { path, role, .. } => {
            assert!(path.ends_with("x_ao_mask.png"), "{path:?}");
            assert_eq!(*role, RawMap::Roughness);
        }
        other => panic!("R is {other:?}"),
    }
    assert_eq!(
        ch[1],
        ChannelSource::Fill {
            role: RawMap::Metallic,
            code: 77
        }
    );
}

#[test]
fn parse_set_helper_fields() {
    for ok in [
        "file:**/a.png.helper.role=ao",
        "file:**/a.png.helper.fills=[\"diffuse.a\",\"rmaos.r\"]",
        "file:**/a.png.helper.read=luma",
        "file:**/a.png.helper.invert=true",
    ] {
        let o = parse_set(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        assert!(o.field.starts_with("helper."), "{ok}: {}", o.field);
    }
    for (bad, field) in [
        ("file:**/a.png.helper.role=bogus", "helper.role"),
        ("file:**/a.png.helper.fills=[\"rmaos.x\"]", "helper.fills"),
        ("file:**/a.png.helper.read=q", "helper.read"),
        ("file:**/a.png.helper.invert=1", "helper.invert"),
        ("subtype:diffuse.helper.role=ao", "helper.role"),
        ("helper.role=ao", "helper.role"),
        ("helper.invert=true", "helper.invert"),
    ] {
        let e = parse_set(bad).expect_err(bad);
        assert!(e.contains(field), "{bad}: {e}");
    }
}

#[test]
fn role_and_target_ids_round_trip() {
    let roles = HelperRole::all();
    assert_eq!(roles.len(), 1 + 13 + 10);
    for r in roles {
        assert_eq!(HelperRole::parse(&r.id()), Some(r), "{}", r.id());
    }
    let targets = FillTarget::all();
    assert_eq!(targets.len(), 5 + 4 + 4 + 4 + 1);
    for t in targets {
        assert_eq!(FillTarget::parse(&t.id()), Some(t), "{}", t.id());
    }
    assert_eq!(HelperRole::parse("rmaos"), None);
    assert_eq!(
        HelperRole::parse("detail"),
        None,
        "rnm.py aliases are no role ids"
    );
    assert_eq!(FillTarget::parse("p.g"), None);
}
