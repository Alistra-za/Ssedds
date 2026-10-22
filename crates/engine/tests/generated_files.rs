//! v2 Generate from normal map, Task 1: `_generated` after a generated role suffix (DECISIONS Note CO).
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::discover::canonicalise_outputs;
use engine::events::*;
use engine::group::{is_generated, split_suffix, vocab_of, VocabRole};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::{PresetError, PresetRegistry, Provenance};
use image::{ImageBuffer, Luma};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

fn put(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)]))
        .save(p)
        .unwrap();
}

fn request(dir: &Path, target: Target, sets: &[&str]) -> BatchRequest {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req
}

fn plan(dir: &Path, target: Target, sets: &[&str]) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "generated-files");
    let p = Engine::new()
        .unwrap()
        .plan(request(dir, target, sets), &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

fn all_codes(p: &Plan) -> Vec<Code> {
    p.jobs
        .iter()
        .flat_map(|j| j.warnings.iter().map(|w| w.code))
        .chain(p.skipped.iter().filter_map(|s| s.code))
        .collect()
}

#[test]
fn generated_after_a_role_keeps_the_role() {
    assert_eq!(
        split_suffix("rock_p_generated"),
        ("rock".into(), Some("p".into()))
    );
    assert_eq!(
        split_suffix("Rock_P_Generated"),
        ("Rock".into(), Some("p".into()))
    );
    assert_eq!(
        vocab_of("rock_ao_generated").unwrap().role,
        VocabRole::PackingInput(RawMap::Ao)
    );
    assert_eq!(
        vocab_of("rock_height_generated").unwrap().role,
        VocabRole::PackingInput(RawMap::Height)
    );
    // P10: Track A's `curves` role is a generated role from Task 1.
    assert_eq!(
        vocab_of("rock_curves_generated").unwrap().role,
        VocabRole::PackingInput(RawMap::Curves)
    );
    assert!(is_generated("rock_p_generated") && is_generated("rock_ao_generated"));
    assert!(is_generated("rock_curves_generated"));
    assert!(!is_generated("rock_generated") && !is_generated("rock_p"));
    assert_eq!(
        split_suffix("rock_generated"),
        ("rock_generated".into(), None)
    );
    // only the generated roles carry the marker (code-S4, 08e-8)
    assert!(
        !is_generated("rock_n_generated")
            && !is_generated("rock_normal_generated")
            && !is_generated("rock_d_generated")
    );
    assert!(!is_generated("rock_curvature_generated") && !is_generated("rock_cavity_generated"));
    assert_eq!(
        split_suffix("rock_n_generated"),
        ("rock_n_generated".into(), None)
    );
    assert!(vocab_of("rock_normal_generated").is_none());
    // a stem that is only a role and the marker is left whole, as with suffixes
    assert!(!is_generated("_p_generated") && !is_generated("p_generated"));
    assert_eq!(split_suffix("_p_generated"), ("_p_generated".into(), None));
}

#[test]
fn other_generated_names_plan_as_on_main() {
    let d = tempfile::tempdir().unwrap();
    for n in [
        "rock.png",
        "rock_n.png",
        "wall_n.png",
        "wall_n_generated.png",
        "wall_d_generated.png",
    ] {
        put(d.path(), n, 16, |x, y| (x * 8 + y) as u8);
    }
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    for n in ["wall_n_generated", "wall_d_generated"] {
        let j = job(&p, &format!("{n}.png"));
        assert!(
            j.output.ends_with(format!("textures/a/{n}.dds")),
            "{}",
            j.output.display()
        );
        assert!(j.set_key.ends_with(&format!("/{n}")), "{}", j.set_key);
        assert_eq!(j.profile.subtype, Subtype::Diffuse, "{n}");
    }
    assert!(job(&p, "wall_n.png")
        .output
        .ends_with("textures/a/wall_n.dds"));
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    assert!(!all_codes(&p).contains(&Code::InDuplicate));
}

#[test]
fn a_generated_p_writes_the_plain_p_output_and_raw_inputs_keep_their_name() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    let names = [
        ("rock_p_generated.png", "rock_p.dds"),
        ("Rock_P_Generated.tga", "Rock_p.dds"),
        ("rock_ao_generated.png", "rock_ao_generated.dds"),
        ("rock_height_generated.png", "rock_height_generated.dds"),
        ("rock_generated.png", "rock_generated.dds"),
        ("rock_n_generated.png", "rock_n_generated.dds"),
    ];
    for (n, _) in names {
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join(n), b"").unwrap();
    }
    let mut files = engine::discover::discover(&[src], &d.path().join("out"), false).unwrap();
    canonicalise_outputs(&mut files, &BTreeMap::new());
    for (n, want) in names {
        let f = files.iter().find(|f| f.rel == Path::new(n)).unwrap();
        assert_eq!(f.output.file_name().unwrap(), want, "{n}");
    }
}

#[test]
fn a_generated_p_plans_as_the_sets_p() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put(d.path(), "rock_p_generated.png", 16, |x, y| {
        (x * 8 + y) as u8
    });
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let j = job(&p, "rock_p_generated.png");
    assert!(
        j.output.ends_with("textures/a/rock_p.dds"),
        "{}",
        j.output.display()
    );
    assert_eq!(j.set_key, job(&p, "rock.png").set_key);
    assert_eq!(j.profile.subtype, Subtype::Height);
}

#[test]
fn a_generated_ao_packs_into_the_base_rmaos() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put(d.path(), "rock_roughness.png", 16, |x, _| x as u8 * 4);
    put(d.path(), "rock_ao_generated.png", 16, |_, y| {
        100 + y as u8 * 8
    });
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let packed: Vec<_> = p.jobs.iter().filter(|j| j.pack.is_some()).collect();
    assert_eq!(
        packed.len(),
        1,
        "{:?}",
        p.jobs.iter().map(|j| &j.output).collect::<Vec<_>>()
    );
    let j = packed[0];
    assert!(
        j.output.ends_with("textures/a/rock_rmaos.dds"),
        "{}",
        j.output.display()
    );
    assert_eq!(
        j.provenance["pack.b"],
        Provenance::Source("rock_ao_generated.png:R".into())
    );
    let defaulted: Vec<&str> = j
        .warnings
        .iter()
        .filter(|w| w.code == Code::PackDefaulted)
        .map(|w| w.data["channel"].as_str().unwrap())
        .collect();
    assert!(!defaulted.contains(&"B"), "{defaulted:?}");
}

#[test]
fn a_generated_height_feeds_the_complex_m_alpha_on_vanilla() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put(d.path(), "rock_n.png", 16, |_, _| 128);
    put(d.path(), "rock_height_generated.png", 16, |x, _| {
        20 + x as u8 * 14
    });
    let (p, _) = plan(
        d.path(),
        Target::Vanilla,
        &["set:rock.shader=complex_material"],
    );
    let j = p
        .jobs
        .iter()
        .find(|j| j.output.ends_with("textures/a/rock_m.dds"))
        .unwrap_or_else(|| panic!("{:?}", p.jobs.iter().map(|j| &j.output).collect::<Vec<_>>()));
    assert!(j.pack.is_some());
    assert_eq!(
        j.provenance["pack.a"],
        Provenance::Source("rock_height_generated.png:R".into())
    );
}

#[test]
fn a_preset_alias_named_generated_is_refused() {
    let mut r = PresetRegistry::builtin().unwrap();
    let e = r.add_toml(
        "schema_version = 1\nid = \"gen\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[suffix_aliases]\ngenerated = \"p\"\n",
    );
    match e {
        Err(PresetError::Alias(m)) => assert!(m.contains("generated"), "{m}"),
        other => panic!("{other:?}"),
    }
    // an aliased role is not a generated role: `rock_pp_generated` stays whole
    let aliases = BTreeMap::from([("pp".to_string(), "p".to_string())]);
    let s = engine::group::split_suffix_with("rock_pp_generated", &aliases);
    assert_eq!(
        (s.base.as_str(), s.suffix, s.alias),
        ("rock_pp_generated", None, None)
    );
}

// ---- Task 2: a real map replaces a generated one (GEN-REPLACED, Note CO) ----

fn put_rgb(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> [u8; 3]) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| image::Rgb(f(x, y)))
        .save(p)
        .unwrap();
}

fn put_dds(dir: &Path, name: &str) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"DDS not a real texture").unwrap();
}

fn plan_req(req: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "generated-files");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn replaced<'a>(p: &'a Plan, name: &str) -> Option<&'a SkippedFile> {
    p.skipped.iter().find(|s| {
        s.code == Some(Code::GenReplaced) && s.path.file_name().is_some_and(|n| n == name)
    })
}

fn warning_codes(ev: &[EngineEvent]) -> Vec<Code> {
    ev.iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) => Some(w.code),
            _ => None,
        })
        .collect()
}

#[test]
fn the_rival_table() {
    use engine::discover::InputKind::{Dds, Image};
    use engine::group::real_rival;
    let (cs, va) = (Target::Cs, Target::Vanilla);
    // generated _p
    assert!(real_rival("rock_p_generated", "rock_p", Image, va));
    assert!(real_rival("rock_p_generated", "rock_p", Dds, va));
    for h in ["rock_height", "rock_displacement", "rock_disp"] {
        assert!(real_rival("rock_p_generated", h, Image, cs), "{h}");
        assert!(
            !real_rival("rock_p_generated", h, Image, va),
            "{h} on vanilla"
        );
        assert!(!real_rival("rock_p_generated", h, Dds, cs), "{h}.dds");
    }
    assert!(!real_rival("rock_p_generated", "rock_m", Image, va));
    assert!(!real_rival("rock_p_generated", "rock_n", Image, cs));
    // generated _height
    for t in [cs, va] {
        assert!(real_rival("rock_height_generated", "rock_p", Image, t));
        assert!(real_rival("rock_height_generated", "rock_p", Dds, t));
        assert!(real_rival("rock_height_generated", "rock_m", Dds, t));
        assert!(real_rival("rock_height_generated", "rock_height", Image, t));
        assert!(!real_rival("rock_height_generated", "rock_height", Dds, t));
    }
    // generated _ao
    for r in [
        "rock_ao",
        "rock_occlusion",
        "rock_ambientocclusion",
        "rock_orm",
    ] {
        assert!(real_rival("rock_ao_generated", r, Image, cs), "{r}");
        assert!(!real_rival("rock_ao_generated", r, Dds, cs), "{r}.dds");
    }
    for r in ["rock_rmaos", "rock_rfaos"] {
        // Task 11: on cs an image's B is read by content in pack planning; a DDS stays a rival.
        assert!(!real_rival("rock_ao_generated", r, Image, cs), "{r}");
        assert!(
            real_rival("rock_ao_generated", r, Image, va),
            "{r} on vanilla"
        );
        assert!(real_rival("rock_ao_generated", r, Dds, cs), "{r}.dds");
        assert!(real_rival("rock_ao_generated", r, Dds, va), "{r}.dds");
    }
    assert!(!real_rival(
        "rock_ao_generated",
        "rock_roughness",
        Image,
        cs
    ));
    // generated _curves (P10): a real _curves / _curvature image; _cavity is no rival
    assert!(real_rival(
        "rock_curves_generated",
        "rock_curves",
        Image,
        cs
    ));
    assert!(real_rival(
        "rock_curves_generated",
        "rock_curvature",
        Image,
        va
    ));
    assert!(!real_rival("rock_curves_generated", "rock_curves", Dds, cs));
    assert!(!real_rival(
        "rock_curves_generated",
        "rock_cavity",
        Image,
        cs
    ));
    // a generated file is never a real rival
    assert!(!real_rival(
        "rock_height_generated",
        "rock_p_generated",
        Image,
        cs
    ));
    assert!(!real_rival(
        "rock_p_generated",
        "rock_height_generated",
        Image,
        cs
    ));
    assert!(!real_rival(
        "rock_ao_generated",
        "rock_ao_generated",
        Image,
        cs
    ));
}

#[test]
fn a_real_p_replaces_the_generated_p_with_an_info_skip() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
    put(d.path(), "rock_p_generated.png", 16, |x, y| {
        (x * 8 + y) as u8
    });
    put(d.path(), "rock_p.png", 16, |x, _| 100 + x as u8);
    let (p, ev) = plan(d.path(), Target::Vanilla, &[]);
    assert!(
        !all_codes(&p).contains(&Code::InDuplicate),
        "{:?}",
        p.skipped
    );
    assert!(!warning_codes(&ev).contains(&Code::InDuplicate));
    let s = replaced(&p, "rock_p_generated.png").expect("GEN-REPLACED skip");
    assert_eq!(s.kind(), SkipKind::GeneratedReplaced);
    assert_eq!(Code::GenReplaced.severity(), engine::codes::Severity::Info);
    assert_eq!(
        s.reason,
        "generated rock_p_generated.png replaced by rock_p.png"
    );
    assert_eq!(s.set.as_deref(), Some(job(&p, "rock.png").set_key.as_str()));
    assert!(job(&p, "rock_p.png")
        .output
        .ends_with("textures/a/rock_p.dds"));
    assert!(!p.jobs.iter().any(|j| j
        .input
        .file_name()
        .is_some_and(|n| n == "rock_p_generated.png")));
}

#[test]
fn a_real_raw_height_beats_a_generated_p_without_pack_conflict() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put(d.path(), "rock_height.png", 16, |x, _| 50 + x as u8 * 4);
    put(d.path(), "rock_p_generated.png", 16, |x, y| {
        (x * 8 + y) as u8
    });
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    let j = p
        .jobs
        .iter()
        .find(|j| j.output.ends_with("textures/a/rock_p.dds"))
        .expect("a rock_p.dds job");
    assert!(j.pack.is_some(), "the Parallax recipe packs rock_p.dds");
    assert_eq!(
        j.provenance["pack.r"],
        Provenance::Source("rock_height.png:R".into())
    );
    assert!(!all_codes(&p).contains(&Code::PackConflict));
    assert!(!warning_codes(&ev).contains(&Code::PackConflict));
    let s = replaced(&p, "rock_p_generated.png").expect("GEN-REPLACED");
    assert!(
        s.set.as_deref().is_some_and(|k| k.ends_with("/rock")),
        "{:?}",
        s.set
    );
    assert_eq!(s.set.as_deref(), Some(j.set_key.as_str()));
}

#[test]
fn a_raw_height_on_vanilla_does_not_replace_a_generated_p() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
    put(d.path(), "rock_height.png", 16, |x, _| 50 + x as u8 * 4);
    put(d.path(), "rock_p_generated.png", 16, |x, y| {
        (x * 8 + y) as u8
    });
    let (p, ev) = plan(d.path(), Target::Vanilla, &[]);
    assert!(
        !all_codes(&p).contains(&Code::GenReplaced),
        "{:?}",
        p.skipped
    );
    assert!(job(&p, "rock_p_generated.png")
        .output
        .ends_with("textures/a/rock_p.dds"));
    assert!(warning_codes(&ev).contains(&Code::PackUnusedInput));
}

#[test]
fn a_dds_p_is_a_real_rival_but_the_generated_files_own_export_is_not() {
    // (a) output root elsewhere: a hand-placed rock_p.dds beside the generated file is the user's real map.
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put(d.path(), "rock_p_generated.png", 16, |x, y| {
        (x * 8 + y) as u8
    });
    put_dds(d.path(), "rock_p.dds");
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let s = replaced(&p, "rock_p_generated.png").expect("GEN-REPLACED by the DDS");
    assert_eq!(
        s.reason,
        "generated rock_p_generated.png replaced by rock_p.dds (a DDS: not written to the output unless DDS inputs are re-encoded)"
    );
    // (b) output root == input folder: that rock_p.dds is the generated member's own output.
    let mut req = request(d.path(), Target::Cs, &[]);
    req.output_root = d.path().join("in");
    let (p, _) = plan_req(req);
    assert!(
        replaced(&p, "rock_p_generated.png").is_none(),
        "{:?}",
        p.skipped
    );
    assert!(job(&p, "rock_p_generated.png")
        .output
        .ends_with("textures/a/rock_p.dds"));
    // (c) the same folder spelled differently: still the same file (same_file, not string folding).
    let mut req = request(d.path(), Target::Cs, &[]);
    req.output_root = d.path().join("in").join("..").join("in");
    let (p, _) = plan_req(req);
    assert!(
        replaced(&p, "rock_p_generated.png").is_none(),
        "{:?}",
        p.skipped
    );
    job(&p, "rock_p_generated.png");
}

#[test]
fn an_earlier_export_of_the_generated_p_does_not_replace_it() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put(d.path(), "rock_p_generated.png", 16, |x, y| {
        (x * 8 + y) as u8
    });
    let in_place = || {
        let mut req = request(d.path(), Target::Cs, &[]);
        req.output_root = d.path().join("in");
        req.accept = AcceptSuggestions::High;
        req
    };
    let (p, _) = plan_req(in_place());
    job(&p, "rock_p_generated.png");
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink, "generated-files");
    let summary = Engine::new()
        .unwrap()
        .run(p, &rep, &CancelToken::default())
        .unwrap();
    assert!(summary.converted >= 1, "{summary:?}");
    assert!(d.path().join("in/textures/a/rock_p.dds").is_file());
    let (p, _) = plan_req(in_place());
    assert!(
        !all_codes(&p).contains(&Code::GenReplaced),
        "{:?}",
        p.skipped
    );
    job(&p, "rock_p_generated.png");
}

/// Fix round 1 (M1): plan in place, run, re-plan; the packed output a generated member fed (`want`, now a DDS in the
/// input folder) is not a real rival, and a packed job still reads `generated`.
fn packed_export_is_no_rival(target: Target, sets: &[&str], generated: &str, want: &str) {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
    put(d.path(), "rock_roughness.png", 16, |x, _| x as u8 * 4);
    put(d.path(), generated, 16, |x, y| 60 + (x + y) as u8 * 4);
    let in_place = || {
        let mut req = request(d.path(), target, sets);
        req.output_root = d.path().join("in");
        req.accept = AcceptSuggestions::High;
        req
    };
    let reads_generated = |p: &Plan| {
        p.jobs.iter().any(|j| {
            j.output.ends_with(format!("textures/a/{want}"))
                && j.provenance
                    .values()
                    .any(|v| *v == Provenance::Source(format!("{generated}:R")))
        })
    };
    let (p, _) = plan_req(in_place());
    assert!(
        reads_generated(&p),
        "{:?}",
        p.jobs.iter().map(|j| &j.output).collect::<Vec<_>>()
    );
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink, "generated-files");
    Engine::new()
        .unwrap()
        .run(p, &rep, &CancelToken::default())
        .unwrap();
    assert!(d.path().join("in/textures/a").join(want).is_file());
    let (p, _) = plan_req(in_place());
    assert!(replaced(&p, generated).is_none(), "{want}: {:?}", p.skipped);
    assert!(reads_generated(&p), "{want}: still packed from {generated}");
}

#[test]
fn an_earlier_packed_export_of_a_generated_ao_does_not_replace_it() {
    packed_export_is_no_rival(Target::Cs, &[], "rock_ao_generated.png", "rock_rmaos.dds");
}

#[test]
fn an_earlier_packed_export_of_a_generated_height_does_not_replace_it() {
    packed_export_is_no_rival(
        Target::Vanilla,
        &["set:rock.shader=complex_material"],
        "rock_height_generated.png",
        "rock_m.dds",
    );
    packed_export_is_no_rival(Target::Cs, &[], "rock_height_generated.png", "rock_p.dds");
}

#[test]
fn a_packed_output_moved_by_output_paths_onto_an_earlier_export_is_no_rival() {
    // Task 10 fix round 2: the GUI's PBR path (`textures/pbr/…` under the mod folder) moves the packed `_rmaos` onto
    // the set's own folder; the DDS there is that output's earlier export, not a real AO source.
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
    put(d.path(), "rock_roughness.png", 16, |x, _| x as u8 * 4);
    put(d.path(), "rock_ao_generated.png", 16, |x, y| {
        60 + (x + y) as u8 * 4
    });
    put_dds(d.path(), "rock_rmaos.dds");
    let moved = d.path().join("in/textures/a/rock_rmaos.dds");
    let mut req = request(d.path(), Target::Cs, &[]);
    // The default outputs land under `<root>/textures/a`, not beside the inputs in `<root>/in/textures/a`.
    req.output_root = d.path().to_path_buf();
    req.accept = AcceptSuggestions::High;
    req.output_paths = BTreeMap::from([(
        engine::sets::OutputKey::Packed {
            set: "textures/a/rock".into(),
            suffix: "rmaos".into(),
        },
        std::path::PathBuf::from("in/textures/a/rock_rmaos.dds"),
    )]);
    let (p, _) = plan_req(req);
    assert!(
        replaced(&p, "rock_ao_generated.png").is_none(),
        "{:?}",
        p.skipped
    );
    let rm = p
        .jobs
        .iter()
        .find(|j| j.output == moved)
        .unwrap_or_else(|| {
            panic!(
                "the moved _rmaos: {:?}",
                p.jobs.iter().map(|j| &j.output).collect::<Vec<_>>()
            )
        });
    assert!(rm
        .provenance
        .values()
        .any(|v| *v == Provenance::Source("rock_ao_generated.png:R".into())));
}

#[test]
fn a_generated_ao_loses_to_any_real_ao_source() {
    for real in ["rock_ao.png", "rock_orm.png", "rock_rmaos.png"] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
        // no raw roughness: a real _rmaos beside it would be an M9 PACK-CONFLICT of its own
        put(d.path(), "rock_ao_generated.png", 16, |_, y| {
            100 + y as u8 * 8
        });
        // P11: a varying B, so the _rmaos stays a real AO source once Task 11 reads its content
        put_rgb(d.path(), real, 16, |x, y| {
            [x as u8 * 8, 30, 100 + (x + y) as u8 * 4]
        });
        let (p, ev) = plan(d.path(), Target::Cs, &[]);
        assert!(
            replaced(&p, "rock_ao_generated.png").is_some(),
            "{real}: {:?}",
            p.skipped
        );
        assert!(!all_codes(&p).contains(&Code::PackConflict), "{real}");
        assert!(!warning_codes(&ev).contains(&Code::PackConflict), "{real}");
        assert!(!p.jobs.iter().any(|j| j
            .provenance
            .values()
            .any(|v| *v == Provenance::Source("rock_ao_generated.png:R".into()))));
    }
    // the bent layer's AO is the real one
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
    put_rgb(d.path(), "rock_bent.png", 16, |x, y| {
        [(x * 8) as u8, (y * 8) as u8, 255]
    });
    put(d.path(), "rock_ao_generated.png", 16, |_, y| {
        100 + y as u8 * 8
    });
    put(d.path(), "rock_ao.png", 16, |x, _| 120 + x as u8);
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let ao = job(&p, "rock_n.png")
        .rnm
        .as_ref()
        .unwrap()
        .ao
        .clone()
        .unwrap();
    assert!(
        ao.file.path.ends_with("rock_ao.png"),
        "{}",
        ao.file.path.display()
    );
}

#[test]
fn a_skip_summary_counts_gen_replaced_as_intentional() {
    let run = |generated: bool| {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
        put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
        put(d.path(), "rock_p.png", 16, |x, _| 100 + x as u8);
        if generated {
            put(d.path(), "rock_p_generated.png", 16, |x, y| {
                (x * 8 + y) as u8
            });
        }
        let mut req = request(d.path(), Target::Vanilla, &[]);
        req.dry_run = true;
        let (p, _) = plan_req(req);
        let sink = Arc::new(VecSink::default());
        let rep = Reporter::new(sink, "generated-files");
        Engine::new()
            .unwrap()
            .run(p, &rep, &CancelToken::default())
            .unwrap()
    };
    let (with, without) = (run(true), run(false));
    assert_eq!(
        with.skipped_intentional,
        without.skipped_intentional + 1,
        "{with:?}"
    );
    assert_eq!(with.skipped, without.skipped + 1);
    assert_eq!(with.exit_code(), without.exit_code());
}

#[test]
fn a_generated_ao_fills_b_with_an_info_note_instead_of_defaulted() {
    let packed = |ao: bool| {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
        put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
        put(d.path(), "rock_roughness.png", 16, |x, _| x as u8 * 4);
        if ao {
            put(d.path(), "rock_ao_generated.png", 16, |_, y| {
                100 + y as u8 * 8
            });
        }
        let (p, _) = plan(d.path(), Target::Cs, &[]);
        let j = p
            .jobs
            .iter()
            .find(|j| j.output.ends_with("rock_rmaos.dds"))
            .expect("rock_rmaos job");
        j.warnings.clone()
    };
    let with = packed(true);
    let note = with.iter().find(|w| w.code == Code::GenAo).expect("GEN-AO");
    assert!(
        note.message.contains("rock_ao_generated.png"),
        "{}",
        note.message
    );
    assert_eq!(note.data["channel"], "B");
    assert_eq!(note.data["source"], "rock_ao_generated.png");
    assert!(note.data["output"]
        .as_str()
        .unwrap()
        .ends_with("rock_rmaos.dds"));
    assert!(!with
        .iter()
        .any(|w| w.code == Code::PackDefaulted && w.data["channel"] == "B"));
    let without = packed(false);
    assert!(!without.iter().any(|w| w.code == Code::GenAo));
    assert!(without
        .iter()
        .any(|w| w.code == Code::PackDefaulted && w.data["channel"] == "B"));
}

#[test]
fn a_bent_layer_set_weights_by_the_generated_ao() {
    let ao_file = |real: bool| {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
        put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
        put_rgb(d.path(), "rock_bent.png", 16, |x, y| {
            [(x * 8) as u8, (y * 8) as u8, 255]
        });
        put(d.path(), "rock_ao_generated.png", 16, |_, y| {
            100 + y as u8 * 8
        });
        if real {
            put(d.path(), "rock_ao.png", 16, |x, _| 120 + x as u8);
        }
        let (p, _) = plan(d.path(), Target::Cs, &[]);
        let ao = job(&p, "rock_n.png")
            .rnm
            .as_ref()
            .unwrap()
            .ao
            .clone()
            .unwrap();
        ao.file
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    assert_eq!(ao_file(false), "rock_ao_generated.png");
    assert_eq!(ao_file(true), "rock_ao.png");
}

// ---- Task 11: a generated AO on a pre-packed `_rmaos` image ----

fn put_rgba(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> [u8; 4]) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| image::Rgba(f(x, y)))
        .save(p)
        .unwrap();
}

/// rock.png, rock_rmaos.png (R 120, G 0, A 0; B `b`) and, with `generated`, rock_ao_generated.png.
fn prepacked_rock(dir: &Path, b: impl Fn(u32, u32) -> u8, generated: bool) {
    put(dir, "rock.png", 16, |x, _| x as u8 * 8);
    put_rgba(dir, "rock_rmaos.png", 16, |x, y| [120, 0, b(x, y), 0]);
    if generated {
        put(dir, "rock_ao_generated.png", 16, |x, y| {
            (x * 8 + y * 4) as u8
        });
    }
}

fn rmaos_jobs(p: &Plan) -> Vec<&ResolvedJob> {
    p.jobs
        .iter()
        .filter(|j| {
            j.output
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains("_rmaos"))
        })
        .collect()
}

#[test]
fn a_generated_ao_fills_the_flat_b_of_a_prepacked_rmaos() {
    let d = tempfile::tempdir().unwrap();
    prepacked_rock(d.path(), |_, _| 255, true);
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    let jobs = rmaos_jobs(&p);
    assert_eq!(jobs.len(), 1, "one _rmaos job: {:?}", p.jobs);
    let j = jobs[0];
    let pk = j.pack.as_ref().expect("a packed _rmaos");
    // R, G and A from the image, byte for byte; B from the generated file.
    let asm = engine::pack::assemble::assemble(pk, pk.gray).unwrap();
    for (i, px) in asm.image.px.iter().enumerate() {
        let (x, y) = (i as u32 % 16, i as u32 / 16);
        let q = px.map(|v| (v * 255.0).round() as u8);
        assert_eq!(q, [120, 0, (x * 8 + y * 4) as u8, 0], "texel ({x}, {y})");
    }
    for (ch, want) in [("r", "rock_rmaos.png:R"), ("a", "rock_rmaos.png:A")] {
        assert_eq!(
            j.provenance[&format!("pack.{ch}")],
            Provenance::Source(want.into())
        );
    }
    assert_eq!(
        j.provenance["pack.b"],
        Provenance::Source("rock_ao_generated.png:R".into())
    );
    let gen_ao = j
        .warnings
        .iter()
        .find(|w| w.code == Code::GenAo)
        .expect("GEN-AO");
    assert!(
        gen_ao.message.contains("rock_ao_generated.png"),
        "{}",
        gen_ao.message
    );
    assert!(
        !all_codes(&p).contains(&Code::PackConflict),
        "{:?}",
        all_codes(&p)
    );
    assert!(!warning_codes(&ev).contains(&Code::PackConflict));
    assert!(replaced(&p, "rock_ao_generated.png").is_none());
    // The image is a channel source, not a job of its own.
    assert!(!p
        .jobs
        .iter()
        .any(|j| j.input.ends_with("rock_rmaos.png") && j.pack.is_none()));
}

#[test]
fn the_packed_output_replaces_the_prepacked_rmaos_name() {
    let d = tempfile::tempdir().unwrap();
    prepacked_rock(d.path(), |_, _| 255, true);
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let jobs = rmaos_jobs(&p);
    assert!(jobs.iter().all(|j| j.pack.is_some()), "the packed job only");
    let outs: Vec<_> = jobs.iter().map(|j| j.output.clone()).collect();
    assert_eq!(outs.len(), 1, "{outs:?}");
    assert!(outs[0].ends_with("textures/a/rock_rmaos.dds"), "{outs:?}");
}

/// Converts the folder on cs; every output DDS (path under `out`, bytes).
fn convert_all(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut req = request(dir, Target::Cs, &[]);
    req.on_existing = OnExisting::Overwrite;
    req.accept = AcceptSuggestions::High;
    let (p, _) = plan_req(req);
    assert!(
        p.jobs
            .iter()
            .any(|j| j.output.ends_with("rock_rmaos.dds") && matches!(j.status, JobStatus::Ready)),
        "{:?}",
        p.jobs
    );
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink, "generated-files");
    Engine::new()
        .unwrap()
        .run(p, &rep, &CancelToken::default())
        .unwrap();
    let out = dir.join("out");
    let mut files = BTreeMap::new();
    let mut stack = vec![out.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "dds") {
                let rel = p.strip_prefix(&out).unwrap().to_string_lossy();
                files.insert(rel.replace('\\', "/"), std::fs::read(&p).unwrap());
            }
        }
    }
    files
}

#[test]
fn a_prepacked_rmaos_with_baked_ao_keeps_its_b() {
    let baked = |x: u32, y: u32| 100 + ((x + y) * 5) as u8; // 100..250
    let with = tempfile::tempdir().unwrap();
    prepacked_rock(with.path(), baked, true);
    let (p, ev) = plan(with.path(), Target::Cs, &[]);
    let s = replaced(&p, "rock_ao_generated.png").expect("GEN-REPLACED");
    assert!(
        s.reason.ends_with("replaced by rock_rmaos.png (B)"),
        "{}",
        s.reason
    );
    assert!(!all_codes(&p).contains(&Code::PackConflict));
    assert!(!warning_codes(&ev).contains(&Code::PackConflict));
    assert!(!p.jobs.iter().any(|j| j.pack.is_some()), "no recipe");
    let without = tempfile::tempdir().unwrap();
    prepacked_rock(without.path(), baked, false);
    let (a, b) = (convert_all(with.path()), convert_all(without.path()));
    assert!(
        a.keys().any(|k| k.ends_with("rock_rmaos.dds")),
        "{:?}",
        a.keys()
    );
    assert_eq!(a, b, "the output equals the run without the generated file");
    // A bent layer weights by the image's B, not by the replaced generated file.
    let d = tempfile::tempdir().unwrap();
    prepacked_rock(d.path(), baked, true);
    put_rgb(d.path(), "rock_n.png", 16, |_, _| [128, 128, 255]);
    put_rgb(d.path(), "rock_bent.png", 16, |x, y| {
        [(x * 8) as u8, (y * 8) as u8, 255]
    });
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let ao = job(&p, "rock_n.png")
        .rnm
        .as_ref()
        .unwrap()
        .ao
        .clone()
        .unwrap();
    assert!(
        ao.file.path.ends_with("rock_rmaos.png"),
        "{}",
        ao.file.path.display()
    );
    assert_eq!(ao.channel, 2);
}

// ---- Task 11 review fixes (CO106-CO109) ----

#[test]
fn b_has_content_follows_the_stated_thresholds() {
    use engine::analyse::ChannelStats;
    let b = |min: u8, max: u8, mean: f64| ChannelStats {
        min: f32::from(min) / 255.0,
        max: f32::from(max) / 255.0,
        mean: mean / 255.0,
    };
    let has = |s: ChannelStats| engine::pack::b_has_content(&s);
    assert!(has(b(253, 255, 254.0)), "a range of exactly 2/255");
    assert!(!has(b(254, 255, 254.5)), "a range of 1/255, mean above 250");
    assert!(has(b(250, 250, 250.0)), "a flat mean of exactly 250/255");
    assert!(!has(b(251, 251, 251.0)), "a flat 251/255");
    assert!(has(b(0, 0, 0.0)), "an all-zero B");
}

#[test]
fn a_generated_ao_fills_the_flat_b_of_a_prepacked_rfaos() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put_rgba(d.path(), "rock_rfaos.png", 16, |x, _| {
        [120, x as u8 * 10, 255, 0]
    });
    put(d.path(), "rock_ao_generated.png", 16, |x, y| {
        (x * 8 + y * 4) as u8
    });
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let packed: Vec<_> = p.jobs.iter().filter(|j| j.pack.is_some()).collect();
    assert_eq!(packed.len(), 1, "{:?}", p.jobs);
    let j = packed[0];
    let pk = j.pack.as_ref().unwrap();
    assert_eq!(pk.kind, engine::pack::RecipeKind::Rfaos);
    assert!(
        j.output.ends_with("textures/a/rock_rfaos.dds"),
        "{:?}",
        j.output
    );
    assert!(
        matches!(&pk.channels[1], engine::pack::ChannelSource::Source { path, channel: Some(1), .. }
            if path.ends_with("rock_rfaos.png")),
        "{:?}",
        pk.channels[1]
    );
    assert!(j.warnings.iter().any(|w| w.code == Code::GenAo));
    assert!(!p
        .jobs
        .iter()
        .any(|j| j.input.ends_with("rock_rfaos.png") && j.pack.is_none()));
}

#[test]
fn an_aliased_prepacked_image_is_not_packed_under_its_alias() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "rock.png", 16, |x, _| x as u8 * 8);
    put_rgba(d.path(), "rock_skinfaos.png", 16, |x, _| {
        [120, x as u8 * 10, 255, 0]
    });
    put(d.path(), "rock_ao_generated.png", 16, |x, y| {
        (x * 8 + y * 4) as u8
    });
    let alias = d.path().join("skin.toml");
    std::fs::write(
        &alias,
        "schema_version = 1\nid = \"skin\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\
         [suffix_aliases]\nskinfaos = \"rfaos\"\n",
    )
    .unwrap();
    let mut req = request(d.path(), Target::Cs, &[]);
    req.preset_files = vec![alias];
    let (p, _) = plan_req(req);
    let outs: Vec<String> = p
        .jobs
        .iter()
        .map(|j| j.output.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(!outs.iter().any(|o| o.contains("skinfaos_")), "{outs:?}");
    assert_eq!(
        outs.iter().filter(|o| o.contains("faos")).count(),
        1,
        "{outs:?}"
    );
    let s = replaced(&p, "rock_ao_generated.png").expect("GEN-REPLACED");
    assert!(
        s.reason.ends_with("replaced by rock_skinfaos.png"),
        "{}",
        s.reason
    );
}

#[test]
fn a_forced_recipe_on_a_prepacked_image_set_raises_no_unbuildable_notice() {
    let d = tempfile::tempdir().unwrap();
    prepacked_rock(d.path(), |_, _| 255, true);
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[defaults.packing]\nrecipe = \"rfaos\"\n",
    )
    .unwrap();
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let jobs = rmaos_jobs(&p);
    assert_eq!(jobs.len(), 1, "{:?}", p.jobs);
    assert!(jobs[0].pack.is_some());
    let codes = all_codes(&p);
    assert!(!codes.contains(&Code::PackRecipeUnbuildable), "{codes:?}");
    assert!(!codes.contains(&Code::PackRecipeConflict), "{codes:?}");
}
