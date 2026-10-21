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
