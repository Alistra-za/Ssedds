//! M2.5 Task 5: a packed job converts from all its sources, lists them in the manifest, is re-encoded when any
//! of them changes and fails when one changed between planning and conversion.
use engine::api::*;
use engine::config::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::manifest::{sha256_file, Manifest};
use engine::pack::*;
use engine::policy::*;
use engine::presets::PresetRegistry;
use image::{ImageBuffer, Luma};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn gray(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) -> PathBuf {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)]))
        .save(&p)
        .unwrap();
    p
}

/// One `_rmaos` job packed from `rough` (R) and `ao` (B), as `Engine::plan` builds it (stamps taken now).
fn plan(dir: &Path, rough: &Path, ao: &Path) -> Plan {
    let registry = PresetRegistry::builtin().unwrap();
    let source = |path: &Path, role| PackSource {
        path: path.to_path_buf(),
        role,
        stamp: SourceStamp::of(path),
    };
    let mut request = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    request.on_existing = OnExisting::IfChanged;
    let job = ResolvedJob {
        input: rough.to_path_buf(),
        output: dir.join("out/iron_rmaos.dds"),
        game_path: "iron_rmaos.png".into(),
        set_key: "out/iron".into(),
        profile: registry
            .profile("cs-quality", Target::Cs, Subtype::Rmaos)
            .unwrap(),
        provenance: Default::default(),
        suggestions: vec![],
        warnings: vec![],
        status: JobStatus::Ready,
        suffix_alias: None,
        source: None,
        pack: Some(PackJob {
            kind: RecipeKind::Rmaos,
            channels: [
                ChannelSource::Source {
                    path: rough.to_path_buf(),
                    role: RawMap::Roughness,
                    invert: false,
                    channel: None,
                },
                ChannelSource::Default { value: 0.0 },
                ChannelSource::Source {
                    path: ao.to_path_buf(),
                    role: RawMap::Ao,
                    invert: false,
                    channel: None,
                },
                ChannelSource::Default { value: 1.0 },
            ],
            gray: GraySource::R,
            sources: vec![source(rough, RawMap::Roughness), source(ao, RawMap::Ao)],
        }),
        bake: None,
        rnm: None,
        derive: None,
        shader: None,
    };
    Plan {
        request,
        registry,
        preset_id: "cs-quality".into(),
        target: Target::Cs,
        jobs: vec![job],
        skipped: vec![],
        failed: vec![],
        peak_admitted_bytes: 0,
        cancelled: vec![],
        shaders: Default::default(),
        nvtt: None,
    }
}

fn run(plan: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pack-run");
    let s = Engine::new()
        .unwrap()
        .run(plan, &rep, &CancelToken::default())
        .unwrap();
    (s, sink.take())
}

#[test]
fn converts_from_every_source_and_records_them() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, y| 255 - y as u8 * 8);
    let (s, ev) = run(plan(d.path(), &rough, &ao));
    assert_eq!(s.converted, 1, "{ev:#?}");
    let f = read_dds(&std::fs::read(d.path().join("out/iron_rmaos.dds")).unwrap()).unwrap();
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    for (i, p) in px.iter().enumerate() {
        let (x, y) = (i as i32 % 32, i as i32 / 32);
        assert!((i32::from(p[0]) - x * 8).abs() <= 6, "R at {x},{y}: {p:?}");
        assert!(i32::from(p[1]) <= 3, "G at {x},{y}: {p:?}");
        assert!(
            (i32::from(p[2]) - (255 - y * 8)).abs() <= 6,
            "B at {x},{y}: {p:?}"
        );
    }
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = m.values().next().unwrap();
    assert_eq!(e.sources.len(), 2);
    assert!(
        e.sources[0].path.ends_with("iron_rough.png") && e.sources[1].path.ends_with("iron_ao.png")
    );
    assert_eq!(e.sources[1].sha256, sha256_file(&ao).unwrap());
    assert_eq!(e.source_sha256, sha256_file(&rough).unwrap());
}

/// Spec §6.4 "Provenance": `if-changed` re-encodes when any source changes.
#[test]
fn up_to_date_until_any_source_changes() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, _| 200);
    assert_eq!(run(plan(d.path(), &rough, &ao)).0.converted, 1);
    let (s, _) = run(plan(d.path(), &rough, &ao));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    gray(d.path(), "iron_ao.png", 32, |_, _| 100);
    let (s, _) = run(plan(d.path(), &rough, &ao));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
}

/// `if-changed` compares the sources' bytes, in order, like an ordinary file (hash only): re-running the batch
/// with the same files spelled another way (`in/../in/x.png`, absolute, relative) re-encodes nothing.
#[test]
fn a_different_spelling_of_the_same_sources_is_up_to_date() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, _| 200);
    assert_eq!(run(plan(d.path(), &rough, &ao)).0.converted, 1);
    let respell = |p: &Path| {
        d.path()
            .join("in")
            .join("..")
            .join("in")
            .join(p.file_name().unwrap())
    };
    let (s, _) = run(plan(d.path(), &respell(&rough), &respell(&ao)));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
}

/// Review Focus 3.
#[test]
fn a_source_changed_after_planning_fails_the_packed_job() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, _| 200);
    let p = plan(d.path(), &rough, &ao);
    gray(d.path(), "iron_ao.png", 64, |_, _| 100);
    let (s, ev) = run(p);
    assert_eq!((s.converted, s.failed), (0, 1));
    let err = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileFailed {
                stage: Stage::Load,
                error,
            } => Some(error.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        err.contains("iron_ao.png") && err.contains("changed since planning"),
        "{err}"
    );
    assert!(!d.path().join("out/iron_rmaos.dds").exists());
}

/// DECISIONS N7: a schema-1 record has no source roles, so its packed output re-encodes once, then is up to date.
#[test]
fn a_schema_1_record_re_encodes_its_packed_output_once() {
    let d = tempfile::tempdir().unwrap();
    gray(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 3);
    gray(d.path(), "iron_ao.png", 64, |_, y| 255 - y as u8);
    let run = || {
        let req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
        let rep = Reporter::new(Arc::new(NullSink), "t");
        let e = Engine::new().unwrap();
        let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
        e.run(plan, &rep, &CancelToken::default()).unwrap()
    };
    assert_eq!(run().converted, 1);
    let path = Manifest::at(&d.path().join("out")).path().to_path_buf();
    let text = std::fs::read_to_string(&path).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["sources"][0]["role"], "roughness");
    assert_eq!(last["sources"][1]["role"], "ao");
    // Rewrite every record as schema 1 without roles.
    let mut out = String::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
        v["schema"] = 1.into();
        for s in v["sources"].as_array_mut().unwrap() {
            let s = s.as_object_mut().unwrap();
            s.remove("role");
            s.remove("invert");
        }
        out.push_str(&serde_json::to_string(&v).unwrap());
        out.push('\n');
    }
    std::fs::write(&path, out).unwrap();
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (0, 1));
}
