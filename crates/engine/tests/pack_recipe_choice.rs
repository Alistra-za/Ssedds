//! Packing-fills Task 1 (DECISIONS Note R-fills-1): `packing.recipe` picks `_rfaos`/`_rmaos` for a set on cs,
//! per packed output, through `[files."<packed key>"]` of the project config.
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::pack::ChannelSource;
use engine::policy::ResolvedJob;
use image::{ImageBuffer, Luma};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn gray(dir: &Path, rel: &str) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
        .save(p)
        .unwrap();
}

/// `in/` holding the given grey maps and `in/sdc.toml`.
fn fixture(files: &[&str], toml: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    for f in files {
        gray(d.path(), f);
    }
    std::fs::write(d.path().join("in/sdc.toml"), toml).unwrap();
    d
}

fn plan(d: &TempDir) -> Plan {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap()
}

fn packed<'a>(p: &'a Plan, name: &str) -> Option<&'a ResolvedJob> {
    p.jobs.iter().find(|j| {
        j.pack.is_some()
            && j.output
                .file_name()
                .is_some_and(|n| n.to_string_lossy() == name)
    })
}

fn g(j: &ResolvedJob) -> &ChannelSource {
    &j.pack.as_ref().unwrap().channels[1]
}

/// The `PACK-UNUSED-INPUT` message of the raw map `file`, wherever the plan keeps it.
fn unused_reason(p: &Plan, file: &str) -> String {
    let has = |path: &Path| {
        path.file_name()
            .is_some_and(|n| n.to_string_lossy() == file)
    };
    for j in p.jobs.iter().filter(|j| has(&j.input)) {
        if let Some(w) = j.warnings.iter().find(|w| w.code == Code::PackUnusedInput) {
            return w.message.clone();
        }
    }
    for s in p.skipped.iter().filter(|s| has(&s.path)) {
        if s.code == Some(Code::PackUnusedInput) {
            return s.reason.clone();
        }
    }
    panic!("{file} is not reported unused");
}

const RFAOS: &str = "[files.\"iron_rfaos\".packing]\nrecipe = \"rfaos\"\n";

#[test]
fn rfaos_without_fuzz_defaults_g_to_zero() {
    let d = fixture(&["iron_roughness.png", "iron_ao.png"], RFAOS);
    let p = plan(&d);
    let j = packed(&p, "iron_rfaos.dds").expect("an _rfaos output");
    assert!(packed(&p, "iron_rmaos.dds").is_none());
    assert_eq!(*g(j), ChannelSource::Default { value: 0.0 });
    let defaulted: Vec<_> = j
        .warnings
        .iter()
        .filter(|w| w.code == Code::PackDefaulted)
        .map(|w| w.data["channel"].as_str().unwrap().to_string())
        .collect();
    assert!(defaulted.contains(&"G".to_string()), "{defaulted:?}");
}

#[test]
fn rfaos_without_fuzz_takes_the_fuzz_fill() {
    let d = fixture(
        &["iron_roughness.png", "iron_ao.png"],
        "[files.\"iron_rfaos\".packing]\nrecipe = \"rfaos\"\nfill = { fuzz = 128 }\n",
    );
    let p = plan(&d);
    let j = packed(&p, "iron_rfaos.dds").expect("an _rfaos output");
    assert_eq!(
        *g(j),
        ChannelSource::Fill {
            role: engine::config::RawMap::Fuzz,
            code: 128
        }
    );
    assert!(!j
        .warnings
        .iter()
        .any(|w| w.code == Code::PackDefaulted && w.data["channel"] == "G"));
}

#[test]
fn rmaos_overrides_a_fuzz_source() {
    let d = fixture(
        &["iron_roughness.png", "iron_fuzz.png", "iron_ao.png"],
        "[files.\"iron_rmaos\".packing]\nrecipe = \"rmaos\"\n",
    );
    let p = plan(&d);
    assert!(packed(&p, "iron_rmaos.dds").is_some());
    assert!(packed(&p, "iron_rfaos.dds").is_none());
    let why = unused_reason(&p, "iron_fuzz.png");
    assert!(
        why.contains("the set is set to _rmaos (packing.recipe)"),
        "{why}"
    );
}

#[test]
fn auto_is_the_fuzz_rule() {
    for (files, want, other) in [
        (
            &["iron_roughness.png", "iron_fuzz.png"][..],
            "iron_rfaos.dds",
            "iron_rmaos.dds",
        ),
        (
            &["iron_roughness.png", "iron_metallic.png"][..],
            "iron_rmaos.dds",
            "iron_rfaos.dds",
        ),
    ] {
        let d = fixture(files, "");
        let p = plan(&d);
        assert!(packed(&p, want).is_some(), "{want}");
        assert!(packed(&p, other).is_none(), "{other}");
    }
    let d = fixture(
        &["iron_roughness.png", "iron_fuzz.png"],
        "[files.\"iron_rfaos\".packing]\nrecipe = \"auto\"\n",
    );
    assert!(packed(&plan(&d), "iron_rfaos.dds").is_some());
}

#[test]
fn recipe_is_chosen_per_set() {
    let d = fixture(
        &[
            "iron_roughness.png",
            "iron_ao.png",
            "gold_roughness.png",
            "gold_ao.png",
        ],
        RFAOS,
    );
    let p = plan(&d);
    assert!(packed(&p, "iron_rfaos.dds").is_some());
    assert!(packed(&p, "gold_rmaos.dds").is_some());
    assert!(packed(&p, "gold_rfaos.dds").is_none());
    assert!(packed(&p, "iron_rmaos.dds").is_none());
}

fn conflicts(j: &ResolvedJob, code: Code) -> Vec<String> {
    j.warnings
        .iter()
        .filter(|w| w.code == code)
        .map(|w| w.message.clone())
        .collect()
}

#[test]
fn rfaos_on_the_rmaos_key_builds_rfaos() {
    let d = fixture(
        &["iron_roughness.png", "iron_ao.png"],
        "[files.\"iron_rmaos\".packing]\nrecipe = \"rfaos\"\n",
    );
    let p = plan(&d);
    assert!(packed(&p, "iron_rfaos.dds").is_some());
    assert!(packed(&p, "iron_rmaos.dds").is_none());
}

#[test]
fn rmaos_on_the_rfaos_key_builds_rmaos() {
    let d = fixture(
        &["iron_roughness.png", "iron_fuzz.png", "iron_ao.png"],
        "[files.\"iron_rfaos\".packing]\nrecipe = \"rmaos\"\n",
    );
    let p = plan(&d);
    assert!(packed(&p, "iron_rmaos.dds").is_some());
    assert!(packed(&p, "iron_rfaos.dds").is_none());
}

#[test]
fn a_project_wide_recipe_applies_to_every_set() {
    let d = fixture(
        &["iron_roughness.png", "gold_roughness.png"],
        "[defaults.packing]\nrecipe = \"rfaos\"\n",
    );
    let p = plan(&d);
    assert!(packed(&p, "iron_rfaos.dds").is_some());
    assert!(packed(&p, "gold_rfaos.dds").is_some());
}

#[test]
fn conflicting_keys_follow_the_rfaos_key_and_warn() {
    let d = fixture(
        &["iron_roughness.png", "iron_fuzz.png"],
        concat!(
            "[files.\"iron_rfaos\".packing]\nrecipe = \"rfaos\"\n",
            "[files.\"iron_rmaos\".packing]\nrecipe = \"rmaos\"\n"
        ),
    );
    let p = plan(&d);
    let j = packed(&p, "iron_rfaos.dds").expect("the _rfaos key wins");
    assert!(packed(&p, "iron_rmaos.dds").is_none());
    let c = conflicts(j, Code::PackRecipeConflict);
    assert_eq!(c.len(), 1, "{c:?}");
    assert!(
        c[0].contains("iron_rfaos") && c[0].contains("iron_rmaos"),
        "{c:?}"
    );
}

#[test]
fn rfaos_on_a_metallic_only_set_says_why_it_builds_rmaos() {
    let d = fixture(&["iron_metallic.png"], RFAOS);
    let p = plan(&d);
    let j = packed(&p, "iron_rmaos.dds").expect("falls back to _rmaos");
    let c = conflicts(j, Code::PackRecipeUnbuildable);
    assert_eq!(c.len(), 1, "{c:?}");
    assert!(
        c[0].contains("needs a roughness, glossiness or AO source"),
        "{c:?}"
    );
}
