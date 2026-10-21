//! Track A Task 4 (DECISIONS Note CM): with `BatchRequest::derive`, the vanilla plan replaces each PBR set's jobs
//! with derived jobs. `cs_then` plans `cs`, builds the `DeriveSet`s from that plan with `also::derive_sets` (Task 7)
//! and plans `vanilla` with `derive`.
use engine::analyse::analyse;
use engine::api::*;
use engine::codes::{Code, Severity};
use engine::config::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::derive::plan::{finish_derived, DeriveCandidate};
use engine::derive::recipe::LegacyParams;
use engine::derive::{
    vanilla_game_path, vanilla_rel, DeriveJob, DeriveKind, DeriveMode, DeriveRequest, DeriveSet,
    DeriveSource,
};
use engine::discover::{InputFile, InputKind};
use engine::events::*;
use engine::group::TextureSet;
use engine::overrides::{parse_set, NO_FILE_SCOPES};
use engine::pack::{ChannelSource, RecipeKind};
use engine::policy::*;
use engine::presets::default_preset_for;
use engine::session::AnalysisCache;
use engine::shader::{MemberMap, Shader, ShaderOrigin};
use image::{ImageBuffer, Rgba};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn png(path: &Path, w: u32, h: u32, px: [u8; 4]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ImageBuffer::from_pixel(w, h, Rgba(px)).save(path).unwrap();
    path.to_path_buf()
}

fn fake_dds(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"DDS not a real texture").unwrap();
}

/// `<dir>/<rel>/steel{,_n,_rmaos}.png` (16², opaque), plus `_p` when `p`. `rmaos` = R rough, G metal, B AO.
fn steel(dir: &Path, rel: &str, rmaos: Option<[u8; 4]>, p: bool) {
    let d = dir.join(rel);
    png(&d.join("steel.png"), 16, 16, [200, 200, 200, 255]);
    png(&d.join("steel_n.png"), 16, 16, [128, 128, 255, 255]);
    if let Some(px) = rmaos {
        png(&d.join("steel_rmaos.png"), 16, 16, px);
    }
    if p {
        png(&d.join("steel_p.png"), 16, 16, [100, 100, 100, 255]);
    }
}

fn request(inputs: &Path, out: &Path, target: Target) -> BatchRequest {
    let mut r = BatchRequest::new(vec![inputs.to_path_buf()], out.to_path_buf(), target);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r
}

fn plan(r: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let p = Engine::new()
        .unwrap()
        .plan(
            r,
            &Reporter::new(sink.clone(), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    (p, sink.take())
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The `DeriveSet`s of a `cs` plan by the slot rule (Task 4 Step 3), as the Also export builds them.
fn derive_sets(cs: &Plan) -> Vec<DeriveSet> {
    engine::also::derive_sets(cs, &BTreeSet::new()).0
}

/// Plans `cs` on `inputs`, then `vanilla` with a derive request built from it; `tweak` edits both.
fn cs_then(
    mode: DeriveMode,
    inputs: &Path,
    out: &Path,
    tweak: impl Fn(&mut BatchRequest, Option<&mut DeriveRequest>),
) -> (Plan, Vec<EngineEvent>) {
    let mut r = request(inputs, &out.join("pbr"), Target::Cs);
    tweak(&mut r, None);
    let (cs, _) = plan(r);
    let mut d = DeriveRequest {
        mode,
        params: LegacyParams::default(),
        sets: derive_sets(&cs),
        drop: vec![],
        scanned: false,
    };
    let mut r = request(inputs, &out.join("vanilla"), Target::Vanilla);
    tweak(&mut r, Some(&mut d));
    r.derive = Some(d);
    plan(r)
}

fn no_tweak(_: &mut BatchRequest, _: Option<&mut DeriveRequest>) {}

/// The job writing `<output root>/<tail>`.
fn out_job<'a>(p: &'a Plan, tail: &str) -> Option<&'a ResolvedJob> {
    let want = slash(&p.request.output_root.join(tail));
    p.jobs.iter().find(|j| slash(&j.output) == want)
}

fn has(j: &ResolvedJob, c: Code) -> bool {
    j.warnings.iter().any(|w| w.code == c)
}

fn event_codes(ev: &[EngineEvent]) -> Vec<Code> {
    let mut out = vec![];
    for e in ev {
        match &e.kind {
            EventKind::Warning(w) => out.push(w.code),
            EventKind::SetResolved { warnings, .. } => out.extend(warnings.iter().map(|w| w.code)),
            _ => {}
        }
    }
    out
}

#[test]
fn vanilla_paths_fold_the_pbr_folder_below_textures() {
    let gp = "textures/pbr/arm/steel.png";
    assert_eq!(
        vanilla_rel(Path::new("textures/PBR/arm/steel.png"), gp),
        PathBuf::from("textures/arm/steel.png")
    );
    assert_eq!(
        vanilla_rel(Path::new("pbr/arm/steel.png"), gp),
        PathBuf::from("arm/steel.png")
    );
    // Not directly below `textures/`, or no `textures/`: nothing folds.
    for (rel, gp) in [
        ("textures/arm/pbr/steel.png", "textures/arm/pbr/steel.png"),
        ("pbr/steel.png", "pbr/steel.png"),
        ("textures/pbr.png", "textures/pbr.png"),
    ] {
        assert_eq!(vanilla_rel(Path::new(rel), gp), PathBuf::from(rel));
    }
    assert_eq!(
        vanilla_game_path("textures/pbr/arm/steel"),
        "textures/arm/steel"
    );
    assert_eq!(
        vanilla_game_path("textures/arm/pbr/steel"),
        "textures/arm/pbr/steel"
    );
}

#[test]
fn a_pbr_set_becomes_diffuse_and_n_under_vanilla_and_its_pbr_jobs_are_gone() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), true);
    let (p, ev) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let diffuse = out_job(&p, "textures/arm/steel.dds").expect("the derived diffuse");
    let dj = diffuse.derive.as_ref().unwrap();
    assert_eq!(dj.kind, DeriveKind::VanillaDiffuse);
    assert_eq!(diffuse.profile.subtype, Subtype::Diffuse);
    assert_eq!(
        diffuse.provenance["subtype"].to_string(),
        "recipe:vanilla_diffuse"
    );
    assert!(matches!(diffuse.status, JobStatus::Ready), "{diffuse:#?}");
    let n = out_job(&p, "textures/arm/steel_n.dds").expect("the derived _n");
    assert_eq!(n.derive.as_ref().unwrap().kind, DeriveKind::LegacyN);
    assert_eq!(n.profile.subtype, Subtype::Normal);
    // No PBR output and no fallback job of a member is left.
    assert_eq!(p.jobs.len(), 2, "{:#?}", p.jobs);
    for j in &p.jobs {
        assert!(j.derive.is_some());
        assert!(!slash(&j.output).contains("/pbr/"), "{}", slash(&j.output));
        assert!(j.provenance.contains_key("target"));
        let pick = j.shader.as_ref().unwrap();
        assert_eq!(
            (pick.shader, &pick.provenance),
            (Shader::Default, &ShaderOrigin::Derived)
        );
        assert!(
            j.game_path.starts_with("textures/arm/steel"),
            "{}",
            j.game_path
        );
    }
    let codes = event_codes(&ev);
    assert!(!codes.contains(&Code::ShaderUnavailable), "{codes:?}");
    assert!(!codes.contains(&Code::ShaderMissingMap), "{codes:?}");
    assert!(!codes.contains(&Code::InDuplicate), "{codes:?}");
}

#[test]
fn a_project_opened_at_textures_folds_pbr_too() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("MyMod/textures");
    steel(&inputs, "pbr/arm", Some([128, 255, 255, 255]), false);
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), |r, d| {
        r.mesh_shaders = BTreeMap::from([("textures/arm/steel".to_string(), vec![Shader::Envmap])]);
        if let Some(d) = d {
            d.scanned = true;
        }
    });
    assert!(out_job(&p, "arm/steel.dds").is_some(), "{:#?}", p.jobs);
    assert!(out_job(&p, "arm/steel_n.dds").is_some());
    // The vote was read at textures/arm/steel, not textures/pbr/arm/steel.
    let m = out_job(&p, "arm/steel_m.dds").expect("the env _m");
    assert_eq!(m.derive.as_ref().unwrap().kind, DeriveKind::EnvM);
}

#[test]
fn env_m_needs_an_envmap_vote_and_metal() {
    let run = |votes: Option<Vec<Shader>>, metal: bool| {
        let d = tempfile::tempdir().unwrap();
        let inputs = d.path().join("in");
        steel(
            &inputs,
            "textures/pbr/arm",
            metal.then_some([128, 255, 255, 255]),
            false,
        );
        let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), |r, d| {
            if let Some(v) = &votes {
                r.mesh_shaders = BTreeMap::from([("textures/arm/steel".to_string(), v.clone())]);
            }
            if let Some(d) = d {
                d.scanned = votes.is_some();
            }
        });
        (d, p)
    };
    let (_d, p) = run(Some(vec![Shader::Envmap]), true);
    let m = out_job(&p, "textures/arm/steel_m.dds").expect("the env _m");
    assert_eq!(m.derive.as_ref().unwrap().kind, DeriveKind::EnvM);
    assert_eq!(m.profile.subtype, Subtype::Envmask);
    assert_eq!(m.shader.as_ref().unwrap().shader, Shader::Envmap);

    let (_d, p) = run(Some(vec![Shader::Default]), true);
    assert!(out_job(&p, "textures/arm/steel_m.dds").is_none());
    let diffuse = out_job(&p, "textures/arm/steel.dds").unwrap();
    assert!(has(diffuse, Code::DeriveNoEnvmap), "{:?}", diffuse.warnings);
    assert_eq!(diffuse.shader.as_ref().unwrap().shader, Shader::Default);

    let (_d, p) = run(None, true);
    assert!(out_job(&p, "textures/arm/steel_m.dds").is_none());
    assert!(has(
        out_job(&p, "textures/arm/steel.dds").unwrap(),
        Code::DeriveNoScan
    ));

    let (_d, p) = run(None, false);
    assert!(out_job(&p, "textures/arm/steel_m.dds").is_none());
    let diffuse = out_job(&p, "textures/arm/steel.dds").unwrap();
    assert!(!has(diffuse, Code::DeriveNoScan) && !has(diffuse, Code::DeriveNoEnvmap));
}

#[test]
fn cm_mode_writes_only_the_m_unless_keep_metal_brightness() {
    let run = |metal: bool, keep: bool, strength: f64| {
        let d = tempfile::tempdir().unwrap();
        let inputs = d.path().join("in");
        steel(
            &inputs,
            "textures/pbr/arm",
            metal.then_some([128, 255, 255, 255]),
            false,
        );
        let (p, _) = cs_then(DeriveMode::Cm, &inputs, d.path(), |_, d| {
            if let Some(d) = d {
                d.params.keep_metal_brightness = keep;
                d.params.keep_metal_strength = strength;
            }
        });
        let kinds: Vec<DeriveKind> = p
            .jobs
            .iter()
            .map(|j| j.derive.as_ref().unwrap().kind)
            .collect();
        (d, p, kinds)
    };
    let (_d, p, kinds) = run(true, false, 1.0);
    assert_eq!(kinds, [DeriveKind::CmM]);
    let m = out_job(&p, "textures/arm/steel_m.dds").unwrap();
    assert_eq!(m.profile.subtype, Subtype::ComplexMaterial);
    assert_eq!(m.shader.as_ref().unwrap().shader, Shader::ComplexMaterial);
    let (_d, p, kinds) = run(true, true, 1.0);
    assert_eq!(kinds, [DeriveKind::CmM, DeriveKind::CmDiffuse]);
    assert!(out_job(&p, "textures/arm/steel.dds").is_some());
    // TA8: the metal slot is a default (no _rmaos), or the strength is 0.
    assert_eq!(run(false, true, 1.0).2, [DeriveKind::CmM]);
    assert_eq!(run(true, true, 0.0).2, [DeriveKind::CmM]);
}

#[test]
fn a_dds_only_pbr_set_is_skipped_with_derive_dds_source() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    for f in ["steel.dds", "steel_n.dds", "steel_rmaos.dds"] {
        fake_dds(&inputs.join("textures/pbr/arm").join(f));
    }
    png(
        &inputs.join("textures/misc/cup.png"),
        16,
        16,
        [90, 90, 90, 255],
    );
    let (cs, _) = plan(request(&inputs, &d.path().join("pbr"), Target::Cs));
    let mut sets = derive_sets(&cs);
    assert_eq!(sets.len(), 1, "the DDS set is a PBR set");
    assert_eq!(sets[0].dds.len(), 3);
    let mut r = request(&inputs, &d.path().join("vanilla"), Target::Vanilla);
    r.derive = Some(DeriveRequest {
        mode: DeriveMode::Vanilla,
        params: LegacyParams::default(),
        sets: std::mem::take(&mut sets),
        drop: vec![],
        scanned: false,
    });
    let (p, _) = plan(r);
    let skip = p
        .skipped
        .iter()
        .find(|s| s.code == Some(Code::DeriveDdsSource))
        .expect("DERIVE-DDS-SOURCE");
    assert!(slash(&skip.path).contains("/pbr/arm/steel"), "{skip:?}");
    assert_eq!(skip.kind(), SkipKind::DdsInput);
    assert!(p.jobs.iter().all(|j| j.derive.is_none()));
    assert!(out_job(&p, "textures/misc/cup.dds").is_some());
    assert!(!p
        .shaders
        .values()
        .any(|s| s.provenance == ShaderOrigin::Derived));
}

#[test]
fn a_set_without_rmaos_uses_the_slot_defaults() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    // Albedo and `_n` only: the `textures/pbr/` path keeps the set PBR.
    steel(&inputs, "textures/pbr/arm", None, false);
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let diffuse = out_job(&p, "textures/arm/steel.dds").expect("diffuse");
    let dj = diffuse.derive.as_ref().unwrap();
    assert_eq!(dj.metal, ChannelSource::Default { value: 0.0 });
    assert_eq!(dj.ao, ChannelSource::Default { value: 1.0 });
    assert_eq!(dj.sources.len(), 1);
    let n = out_job(&p, "textures/arm/steel_n.dds").expect("_n");
    let r = diffuse.profile.packing.roughness_default as f32;
    assert_eq!(
        n.derive.as_ref().unwrap().rough,
        ChannelSource::Default { value: r }
    );
    // The images: diffuse = 0.9 · albedo; `_n` alpha = 0.66 · (1 − r_default).
    let a = engine::derive::assemble::assemble_derived(dj).unwrap();
    assert!((a.image.px[0][0] - 0.9 * 200.0 / 255.0).abs() < 1e-5);
    let a = engine::derive::assemble::assemble_derived(n.derive.as_ref().unwrap()).unwrap();
    assert!((a.image.px[0][3] - 0.66 * (1.0 - r)).abs() < 1e-5);
}

#[test]
fn sources_of_different_sizes_fail_only_that_set() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    let dir = inputs.join("textures/pbr/arm");
    png(&dir.join("big.png"), 64, 64, [200, 200, 200, 255]);
    png(&dir.join("big_n.png"), 64, 64, [128, 128, 255, 255]);
    png(&dir.join("big_rmaos.png"), 32, 32, [128, 0, 255, 255]);
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), false);
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let big = out_job(&p, "textures/arm/big.dds").expect("an error job");
    assert_eq!(
        big.status,
        JobStatus::Error {
            codes: vec![Code::PackSizeMismatch]
        }
    );
    let w = big
        .warnings
        .iter()
        .find(|w| w.code == Code::PackSizeMismatch)
        .unwrap();
    let named: Vec<String> = w.data["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["path"].as_str().unwrap().to_string())
        .collect();
    assert!(named.iter().any(|p| p.ends_with("big.png")), "{named:?}");
    assert!(
        named.iter().any(|p| p.ends_with("big_rmaos.png")),
        "{named:?}"
    );
    assert!(big.provenance.contains_key("target"));
    assert!(matches!(
        out_job(&p, "textures/arm/steel.dds").unwrap().status,
        JobStatus::Ready
    ));
}

#[test]
fn a_derived_output_on_another_jobs_path_is_a_derive_path_conflict() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    let dir = inputs.join("textures/pbr");
    png(&dir.join("a.png"), 16, 16, [200, 200, 200, 255]);
    png(&dir.join("a_n.png"), 16, 16, [128, 128, 255, 255]);
    png(&dir.join("a_rmaos.png"), 16, 16, [128, 0, 255, 255]);
    let plain = png(&inputs.join("textures/a.png"), 16, 16, [50, 60, 70, 255]);
    let (p, ev) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let on_path: Vec<&ResolvedJob> = p
        .jobs
        .iter()
        .filter(|j| slash(&j.output).ends_with("/textures/a.dds"))
        .collect();
    assert_eq!(on_path.len(), 1, "{on_path:#?}");
    let kept = on_path[0];
    assert_eq!(kept.input, plain);
    assert!(matches!(kept.status, JobStatus::Ready));
    // TA12 (final review): the refused job is keyed by its member's own output, apart from the plain job.
    let refused = out_job(&p, "textures/pbr/a.dds").expect("the refused job, keyed apart");
    assert_eq!(
        refused.status,
        JobStatus::Error {
            codes: vec![Code::DerivePathConflict]
        }
    );
    assert!(out_job(&p, "textures/a_n.dds").unwrap().derive.is_some());
    assert!(!event_codes(&ev).contains(&Code::InDuplicate));
    assert!(p.skipped.iter().all(|s| s.code != Some(Code::InDuplicate)));
}

#[test]
fn a_skipped_set_leaves_no_fallback_job() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), true);
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), |_, d| {
        if let Some(d) = d {
            let s = d.sets.remove(0);
            d.drop = s.members;
        }
    });
    assert!(
        p.jobs.iter().all(|j| !slash(&j.input).contains("/steel")),
        "{:#?}",
        p.jobs
    );
}

#[test]
fn a_member_with_no_recipe_role_is_reported_not_written() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), false);
    let dir = inputs.join("textures/pbr/arm");
    png(&dir.join("steel_g.png"), 16, 16, [255, 0, 0, 255]);
    png(&dir.join("steel_detail.png"), 16, 16, [128, 128, 255, 255]);
    let (cs, _) = plan(request(&inputs, &d.path().join("pbr"), Target::Cs));
    let mut sets = derive_sets(&cs);
    // An RNM layer has no job role under `cs` either; the PBR set lists it as a member (Task 7 reads it so).
    sets[0].members.push(dir.join("steel_detail.png"));
    let mut r = request(&inputs, &d.path().join("vanilla"), Target::Vanilla);
    r.derive = Some(DeriveRequest {
        mode: DeriveMode::Vanilla,
        params: LegacyParams::default(),
        sets,
        drop: vec![],
        scanned: false,
    });
    let (p, _) = plan(r);
    let diffuse = out_job(&p, "textures/arm/steel.dds").unwrap();
    let notes: Vec<(&str, &str)> = diffuse
        .warnings
        .iter()
        .filter(|w| w.code == Code::DeriveNotConverted)
        .map(|w| {
            (
                w.data["member"].as_str().unwrap(),
                w.data["reason"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(
        notes
            .iter()
            .any(|(m, r)| m.ends_with("steel_g.png") && *r != "detail layer not in the vanilla _n"),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|(m, r)| m.ends_with("steel_detail.png")
                && *r == "detail layer not in the vanilla _n"),
        "{notes:?}"
    );
    assert!(p.jobs.iter().all(
        |j| !slash(&j.output).contains("steel_g") && !slash(&j.output).contains("steel_detail")
    ));
}

#[test]
fn nif_suggestion_rides_on_the_set_with_glossiness_from_mean_roughness() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(
        &inputs,
        "textures/pbr/arm",
        Some([128, 255, 255, 255]),
        false,
    );
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let n = out_job(&p, "textures/arm/steel_n.dds").unwrap();
    let w = n
        .warnings
        .iter()
        .find(|w| w.code == Code::DeriveNifSuggest)
        .expect("DERIVE-NIF-SUGGEST on the _n");
    assert_eq!(w.severity, Severity::Info);
    let g = w.data["glossiness"].as_f64().unwrap();
    assert!((g - 29.5).abs() < 0.1, "{g}");
    assert_eq!(w.data["specular_strength"].as_f64(), Some(1.0));
    assert_eq!(w.data["env_map"].as_bool(), Some(true));
    // Once per set.
    let all: usize = p
        .jobs
        .iter()
        .map(|j| {
            j.warnings
                .iter()
                .filter(|w| w.code == Code::DeriveNifSuggest)
                .count()
        })
        .sum();
    assert_eq!(all, 1);
}

#[test]
fn without_derive_the_vanilla_plan_is_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), true);
    let r = || request(&inputs, &d.path().join("vanilla"), Target::Vanilla);
    let (p, ev) = plan(r());
    // The members' own fallback jobs, under the PBR path; no derived job, origin or code (C6).
    assert!(
        out_job(&p, "textures/pbr/arm/steel.dds").is_some(),
        "{:#?}",
        p.jobs
    );
    assert!(p.jobs.iter().all(|j| j.derive.is_none()));
    assert!(!p
        .shaders
        .values()
        .any(|s| s.provenance == ShaderOrigin::Derived));
    assert!(!event_codes(&ev)
        .iter()
        .any(|c| c.as_str().starts_with("DERIVE-")));
    // A derive request with no set changes nothing either.
    let mut empty = r();
    empty.derive = Some(DeriveRequest {
        mode: DeriveMode::Vanilla,
        params: LegacyParams::default(),
        sets: vec![],
        drop: vec![],
        scanned: false,
    });
    let (q, _) = plan(empty);
    assert_eq!(
        serde_json::to_string(&p.jobs).unwrap(),
        serde_json::to_string(&q.jobs).unwrap()
    );
}

/// P25: a derived job has no cached analysis; the albedo's own stays the albedo's.
#[test]
fn a_derived_job_has_no_cached_analysis() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), false);
    let (cs, _) = plan(request(&inputs, &d.path().join("pbr"), Target::Cs));
    let mut r = request(&inputs, &d.path().join("vanilla"), Target::Vanilla);
    r.derive = Some(DeriveRequest {
        mode: DeriveMode::Vanilla,
        params: LegacyParams::default(),
        sets: derive_sets(&cs),
        drop: vec![],
        scanned: false,
    });
    let c = AnalysisCache::default();
    let p = Engine::new()
        .unwrap()
        .plan_cached(
            r,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
            &c,
        )
        .unwrap();
    let diffuse = out_job(&p, "textures/arm/steel.dds").unwrap();
    assert!(c.analysis(diffuse).is_none());
    let mut plain = diffuse.clone();
    plain.derive = None;
    let a = c.analysis(&plain).expect("the albedo's own analysis");
    assert_eq!((a.width, a.height), (16, 16));
}

/// The derived jobs' inputs get a terminal event naming the derived output, as packed sources do.
#[test]
fn a_run_reports_each_derive_source_as_consumed() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), false);
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let sink = Arc::new(VecSink::default());
    let s = Engine::new()
        .unwrap()
        .run(
            p,
            &Reporter::new(sink.clone(), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    assert_eq!(s.converted, 2);
    let consumed: BTreeSet<(String, String)> = sink
        .take()
        .into_iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::FileSkipped {
                    kind: SkipKind::PackedSource,
                    ..
                }
            )
        })
        .map(|e| {
            let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
            (
                name(Path::new(e.path.as_deref().unwrap())),
                e.job.clone().unwrap_or_default(),
            )
        })
        .collect();
    let names: BTreeSet<&str> = consumed.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        BTreeSet::from(["steel.png", "steel_n.png", "steel_rmaos.png"])
    );
}

/// CM14: a derived grass stack is never planned with `remove`, which would drop the `_n` spec alpha in its bottom
/// half, even on an opaque albedo with an explicit `alpha.policy = remove`. (Task 5 plans grass sets; this drives
/// `finish_derived`, the step every derived job resolves through, directly.)
#[test]
fn a_grass_stack_on_an_opaque_albedo_keeps_its_spec_alpha() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/landscape/grass");
    let albedo = png(&dir.join("g.png"), 16, 16, [90, 140, 60, 255]);
    let normal = png(&dir.join("g_n.png"), 16, 16, [128, 128, 255, 255]);
    let src = |p: &Path, role: &str| DeriveSource {
        path: p.to_path_buf(),
        role: role.into(),
        stamp: SourceStamp::of(p),
    };
    let job = DeriveJob {
        kind: DeriveKind::GrassStack,
        landscape: false,
        albedo: Some(albedo.clone()),
        normal: Some(normal.clone()),
        // rough 0.5, metal 1, AO 1: 0.66 · 0.5 · 2 = 0.66 → 168.
        rough: ChannelSource::Default { value: 0.5 },
        metal: ChannelSource::Default { value: 1.0 },
        ao: ChannelSource::Default { value: 1.0 },
        height: None,
        curves: None,
        cavity: None,
        gray: GraySource::R,
        params: LegacyParams::default(),
        sources: vec![src(&albedo, "albedo"), src(&normal, "normal")],
    };
    let out = d.path().join("out");
    let input = InputFile {
        path: albedo.clone(),
        rel: PathBuf::from("textures/landscape/grass/g.png"),
        game_path: "textures/landscape/grass/g.png".into(),
        output: out.join("textures/landscape/grass/g.dds"),
        kind: InputKind::Image,
        real_rel: None,
    };
    let c = DeriveCandidate {
        set_key: "k".into(),
        input,
        job: job.clone(),
        notes: vec![],
        nif: None,
    };
    let set = TextureSet {
        key: "k".into(),
        members: vec![0],
        suffixes: BTreeSet::new(),
    };
    let asm = engine::derive::assemble::assemble_derived(&job).unwrap();
    let analysis = analyse(&asm.image, &asm.info);
    let reg = engine::presets::PresetRegistry::builtin().unwrap();
    let allow = BTreeSet::new();
    let resolve = |overrides: &[engine::overrides::SetOverride]| {
        let ctx = ResolveCtx {
            target: Target::Vanilla,
            registry: &reg,
            preset: default_preset_for(Target::Vanilla),
            user_config: None,
            project_config: None,
            overrides,
            accept: AcceptSuggestions::High,
            allow: &allow,
            files: &NO_FILE_SCOPES,
            shaders: &NO_SHADERS,
            mesh_alpha: &NO_MESH_ALPHA,
            nvtt: None,
        };
        finish_derived(&ctx, &c, &set, &analysis, &asm.info, 0.5)
    };
    let explicit = resolve(&[parse_set("alpha.policy=remove").unwrap()]);
    assert_ne!(explicit.profile.alpha.policy, AlphaPolicy::Remove);
    let job = resolve(&[]);
    assert_ne!(job.profile.alpha.policy, AlphaPolicy::Remove);
    assert_eq!(job.profile.subtype, Subtype::ComplexGrass);

    // Encoded: the bottom half (the `_n`) keeps the spec alpha.
    let mut p = plan(request(&d.path().join("in"), &out, Target::Vanilla)).0;
    p.jobs = vec![ResolvedJob {
        status: JobStatus::Ready,
        ..job
    }];
    let sink = Arc::new(VecSink::default());
    let s = Engine::new()
        .unwrap()
        .run(
            p,
            &Reporter::new(sink.clone(), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    assert_eq!(s.converted, 1, "{:#?}", sink.take());
    let f = read_dds(&std::fs::read(out.join("textures/landscape/grass/g.dds")).unwrap()).unwrap();
    let (w, h, r) = f.levels[0].clone();
    assert_eq!((w, h), (16, 32));
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    for q in &px[(16 * 16)..] {
        assert!((i32::from(q[3]) - 168).abs() <= 2, "bottom alpha {}", q[3]);
    }
    for q in &px[..(16 * 16)] {
        assert_eq!(q[3], 255);
    }
}

/// TA29: a derived pick names no map notes (a kept landscape `_p` has no vanilla role) and round-trips as `derived`.
#[test]
fn a_derived_pick_skips_the_map_notes() {
    let maps = [MemberMap {
        name: "dirt_p.dds".into(),
        suffix: Some("p".into()),
        subtype: Some(Subtype::Height),
    }];
    let pick = |provenance| engine::shader::ShaderPick {
        shader: Shader::Landscape,
        provenance,
        wanted: None,
    };
    assert!(
        !engine::shader::check_maps(&pick(ShaderOrigin::Set), Target::Vanilla, &maps).is_empty()
    );
    assert!(
        engine::shader::check_maps(&pick(ShaderOrigin::Derived), Target::Vanilla, &maps).is_empty()
    );
    assert_eq!(ShaderOrigin::Derived.to_string(), "derived");
    assert_eq!("derived".parse::<ShaderOrigin>(), Ok(ShaderOrigin::Derived));
}

/// Sets an engine test-hook variable for one test and removes it when dropped (as `tests/api.rs` does). The values
/// name output files no other test has, so tests running in parallel never match them.
struct HookVar(&'static str);

impl HookVar {
    fn set(name: &'static str, value: &str) -> HookVar {
        std::env::set_var(name, value);
        HookVar(name)
    }
}

impl Drop for HookVar {
    fn drop(&mut self) {
        std::env::remove_var(self.0);
    }
}

fn plan_and_run(
    mode: DeriveMode,
    inputs: &Path,
    out: &Path,
    tweak: impl Fn(&mut BatchRequest, Option<&mut DeriveRequest>),
) -> (BatchSummary, Vec<EngineEvent>) {
    let mut r = request(inputs, &out.join("pbr"), Target::Cs);
    tweak(&mut r, None);
    let (cs, _) = plan(r);
    let mut d = DeriveRequest {
        mode,
        params: LegacyParams::default(),
        sets: derive_sets(&cs),
        drop: vec![],
        scanned: false,
    };
    let mut r = request(inputs, &out.join("vanilla"), Target::Vanilla);
    tweak(&mut r, Some(&mut d));
    r.derive = Some(d);
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let e = Engine::new().unwrap();
    let p = e.plan(r, &rep, &CancelToken::default()).unwrap();
    let s = e.run(p, &rep, &CancelToken::default()).unwrap();
    (s, sink.take())
}

/// Review I1: every discovered file gets exactly one terminal event, whatever the derive step did with it: read by
/// a derived job, not read (`_g`, an RNM layer), left out (`drop`), in a set with no albedo or a DDS source, or a
/// source of a size-mismatch error job. Task 5: in both modes, with a landscape set whose `_p` is read by the diffuse
/// and kept as its own job (O6), and a grass set whose `_n` the vanilla diffuse does not read.
#[test]
fn every_discovered_file_gets_one_terminal_event() {
    for mode in [DeriveMode::Vanilla, DeriveMode::Cm] {
        one_terminal_event_each(mode);
    }
}

fn one_terminal_event_each(mode: DeriveMode) {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    let dir = inputs.join("textures/pbr/arm");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), true);
    png(&dir.join("steel_g.png"), 16, 16, [255, 0, 0, 255]);
    png(&dir.join("steel_detail.png"), 16, 16, [128, 128, 255, 255]);
    for base in ["box", "nod"] {
        png(&dir.join(format!("{base}.png")), 16, 16, [90, 90, 90, 255]);
        png(
            &dir.join(format!("{base}_n.png")),
            16,
            16,
            [128, 128, 255, 255],
        );
        png(
            &dir.join(format!("{base}_rmaos.png")),
            16,
            16,
            [128, 0, 255, 255],
        );
    }
    png(&dir.join("big.png"), 32, 32, [200, 200, 200, 255]);
    png(&dir.join("big_n.png"), 32, 32, [128, 128, 255, 255]);
    png(&dir.join("big_rmaos.png"), 16, 16, [128, 0, 255, 255]);
    // TA12: `pip`'s Vanilla diffuse lands on the plain `textures/arm/pip.png`'s output (DERIVE-PATH-CONFLICT).
    png(&dir.join("pip.png"), 16, 16, [90, 90, 90, 255]);
    png(&dir.join("pip_n.png"), 16, 16, [128, 128, 255, 255]);
    png(&dir.join("pip_rmaos.png"), 16, 16, [128, 0, 255, 255]);
    png(
        &inputs.join("textures/arm/pip.png"),
        16,
        16,
        [50, 60, 70, 255],
    );
    png(&dir.join("crate.png"), 16, 16, [90, 90, 90, 255]);
    png(&dir.join("crate_n.png"), 16, 16, [128, 128, 255, 255]);
    fake_dds(&dir.join("crate_rmaos.dds"));
    png(
        &inputs.join("textures/misc/cup.png"),
        16,
        16,
        [90, 90, 90, 255],
    );
    dirt(&inputs, true);
    // A landscape set with no albedo: its kept `_p` (O6) is read by nothing.
    landscape(&inputs, "rock", true);
    // A landscape set whose height is a raw `_height`: packed into the kept Vanilla `_p` and read by the diffuse.
    landscape(&inputs, "mud", false);
    png(
        &inputs.join("textures/landscape/mud_height.png"),
        16,
        16,
        [100, 100, 100, 255],
    );
    grass(&inputs, "textures/landscape/grass", 16);
    // An unfolded set (no `textures/pbr/`) whose derived jobs fail at Load: a truncated rough source (review M1).
    let misc = inputs.join("textures/misc");
    png(&misc.join("ore.png"), 16, 16, [90, 90, 90, 255]);
    png(&misc.join("ore_n.png"), 16, 16, [128, 128, 255, 255]);
    png(&misc.join("ore_rmaos.png"), 16, 16, [128, 0, 255, 255]);
    let bad = d.path().join("elsewhere/bad_rough.png");
    png(&bad, 16, 16, [128, 128, 128, 255]);
    let bytes = std::fs::read(&bad).unwrap();
    std::fs::write(&bad, &bytes[..40]).unwrap();
    let (_, ev) = plan_and_run(mode, &inputs, d.path(), |r, d| {
        gras(r, None);
        if let Some(d) = d {
            let ore = d
                .sets
                .iter_mut()
                .find(|s| s.members.iter().any(|m| slash(m).ends_with("/ore_n.png")))
                .unwrap();
            ore.rough = ChannelSource::Source {
                path: bad.clone(),
                role: RawMap::Roughness,
                invert: false,
                channel: None,
            };
            let rock = d
                .sets
                .iter_mut()
                .find(|s| s.members.iter().any(|m| slash(m).ends_with("/rock_n.png")))
                .unwrap();
            rock.albedo = None;
            let is = |s: &DeriveSet, b: &str| {
                s.members
                    .iter()
                    .any(|m| slash(m).ends_with(&format!("/{b}_n.png")))
            };
            let i = d.sets.iter().position(|s| is(s, "box")).unwrap();
            d.drop = d.sets.remove(i).members;
            let nod = d.sets.iter_mut().find(|s| is(s, "nod")).unwrap();
            (nod.albedo, nod.normal) = (None, None);
            let steel = d.sets.iter_mut().find(|s| is(s, "steel")).unwrap();
            steel.members.push(dir.join("steel_detail.png"));
        }
    });
    let discovered: Vec<(String, String)> = ev
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FileDiscovered { .. }))
        .map(|e| (e.path.clone().unwrap(), e.job.clone().unwrap()))
        .collect();
    assert!(discovered.len() >= 36, "{discovered:#?}");
    // The unfolded failure ends `ore.png` with its own `file_failed` (the derived output is its own key).
    assert!(ev
        .iter()
        .any(|e| matches!(e.kind, EventKind::FileFailed { .. })
            && e.path.as_deref().is_some_and(|p| p.ends_with("/ore.png"))));
    for (path, job) in &discovered {
        let n = ev
            .iter()
            .filter(|e| match &e.kind {
                EventKind::FileSkipped {
                    kind: SkipKind::PackedSource,
                    ..
                } => e.path.as_ref() == Some(path),
                EventKind::FileSkipped { .. }
                | EventKind::FileDone { .. }
                | EventKind::FileFailed { .. } => e.job.as_ref() == Some(job),
                _ => false,
            })
            .count();
        assert_eq!(n, 1, "{mode:?}: {path} ({job}) has {n} terminal events");
    }
}

/// Review M1: DERIVE-NO-ALBEDO is a note on the `_n` under Vanilla, only with a CM diffuse under CM, and a skip
/// for a set with no albedo and no `_n`.
#[test]
fn no_albedo_is_noted_where_a_diffuse_is_lost() {
    let run = |mode: DeriveMode, keep: bool, bare: bool| {
        let d = tempfile::tempdir().unwrap();
        let inputs = d.path().join("in");
        let dir = inputs.join("textures/pbr/arm");
        png(&dir.join("steel_n.png"), 16, 16, [128, 128, 255, 255]);
        png(&dir.join("steel_rmaos.png"), 16, 16, [128, 255, 255, 255]);
        let (p, _) = cs_then(mode, &inputs, d.path(), |_, d| {
            if let Some(d) = d {
                d.params.keep_metal_brightness = keep;
                if bare {
                    d.sets[0].normal = None;
                }
            }
        });
        (d, p)
    };
    let noted = |p: &Plan| p.jobs.iter().any(|j| has(j, Code::DeriveNoAlbedo));
    let (_d, p) = run(DeriveMode::Vanilla, false, false);
    assert!(has(
        out_job(&p, "textures/arm/steel_n.dds").unwrap(),
        Code::DeriveNoAlbedo
    ));
    let (_d, p) = run(DeriveMode::Cm, false, false);
    assert!(out_job(&p, "textures/arm/steel_m.dds").is_some());
    assert!(!noted(&p));
    let (_d, p) = run(DeriveMode::Cm, true, false);
    assert!(noted(&p));
    let (_d, p) = run(DeriveMode::Vanilla, false, true);
    assert!(p.jobs.is_empty(), "{:#?}", p.jobs);
    assert!(p
        .skipped
        .iter()
        .any(|s| s.code == Some(Code::DeriveNoAlbedo)));
}

/// Review M2: a DDS set's first file is skipped once (DERIVE-DDS-SOURCE replaces its IN-DDS-SKIPPED).
#[test]
fn a_dds_set_skips_each_file_once() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    for f in ["steel.dds", "steel_n.dds", "steel_rmaos.dds"] {
        fake_dds(&inputs.join("textures/pbr/arm").join(f));
    }
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let paths: Vec<&PathBuf> = p.skipped.iter().map(|s| &s.path).collect();
    let unique: BTreeSet<&PathBuf> = paths.iter().copied().collect();
    assert_eq!(paths.len(), 3, "{:#?}", p.skipped);
    assert_eq!(unique.len(), 3);
    let sink = Arc::new(VecSink::default());
    let s = Engine::new()
        .unwrap()
        .run(p, &Reporter::new(sink, "t"), &CancelToken::default())
        .unwrap();
    assert_eq!(s.skipped, 3);
}

/// Review M3: a `[files]` key for a derived `_m` (its virtual `<base>_m`, TA15) applies and is not unmatched.
#[test]
fn a_files_key_for_a_derived_m_applies_and_matches() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    steel(
        &inputs,
        "textures/pbr/arm",
        Some([128, 255, 255, 255]),
        false,
    );
    let (p, ev) = cs_then(DeriveMode::Cm, &inputs, d.path(), |r, _| {
        let mut cfg = engine::presets::ConfigFile::default();
        cfg.files.insert(
            "textures/pbr/arm/steel_m".into(),
            serde_json::json!({ "alpha": { "threshold": 150 } }),
        );
        r.project_config_inline = Some(cfg);
    });
    let m = out_job(&p, "textures/arm/steel_m.dds").unwrap();
    assert_eq!(m.profile.alpha.threshold, 150);
    assert!(
        !event_codes(&ev).contains(&Code::CfgFileUnmatched),
        "{:?}",
        event_codes(&ev)
    );
}

/// Review M4: `admit_all`'s failure arms. A derived candidate that panics at Resolve and one whose source fails to
/// decode each fail alone (`FailedFile` at the stage reached); so does a packed recipe that panics at Load.
#[test]
fn derive_and_pack_candidates_fail_alone() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    let dir = inputs.join("textures/pbr/arm");
    steel(&inputs, "textures/pbr/arm", Some([128, 0, 255, 255]), false);
    png(&dir.join("boomd.png"), 16, 16, [90, 90, 90, 255]);
    png(&dir.join("boomd_n.png"), 16, 16, [128, 128, 255, 255]);
    png(&dir.join("boomd_rmaos.png"), 16, 16, [128, 0, 255, 255]);
    {
        let _hook = HookVar::set("SDC_TEST_PANIC_ON_PLAN", "boomd.dds@resolve");
        let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
        assert_eq!(p.failed.len(), 1, "{:#?}", p.failed);
        let f = &p.failed[0];
        assert!(f.path.ends_with("boomd.png"), "{f:?}");
        assert_eq!(f.stage, Stage::Resolve);
        assert!(
            f.error.contains("internal error") && f.error.contains("test hook"),
            "{}",
            f.error
        );
        assert!(out_job(&p, "textures/arm/boomd_n.dds").is_some());
        assert!(out_job(&p, "textures/arm/steel.dds").is_some());
    }
    // A source that passes the header check but does not decode: the derived jobs that read it fail at Load.
    let bad = d.path().join("elsewhere/bad_rough.png");
    png(&bad, 16, 16, [128, 128, 128, 255]);
    let bytes = std::fs::read(&bad).unwrap();
    std::fs::write(&bad, &bytes[..40]).unwrap();
    let (p, ev) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), |_, d| {
        if let Some(d) = d {
            let s = d
                .sets
                .iter_mut()
                .find(|s| slash(s.albedo.as_deref().unwrap()).ends_with("/steel.png"))
                .unwrap();
            s.rough = ChannelSource::Source {
                path: bad.clone(),
                role: RawMap::Roughness,
                invert: false,
                channel: None,
            };
        }
    });
    let steel_fails: Vec<_> = p
        .failed
        .iter()
        .filter(|f| slash(&f.path).contains("/steel"))
        .collect();
    assert_eq!(steel_fails.len(), 2, "{:#?}", p.failed);
    assert!(steel_fails.iter().all(|f| f.stage == Stage::Load));
    assert!(
        event_codes(&ev).contains(&Code::InDecode),
        "{:?}",
        event_codes(&ev)
    );
    assert!(out_job(&p, "textures/arm/boomd.dds").is_some());

    // Packing pass 2: a recipe that panics at Load fails alone.
    let pk = d.path().join("pk");
    png(&pk.join("boomp.png"), 16, 16, [90, 90, 90, 255]);
    png(
        &pk.join("boomp_roughness.png"),
        16,
        16,
        [120, 120, 120, 255],
    );
    png(&pk.join("boomp_ao.png"), 16, 16, [200, 200, 200, 255]);
    let _hook = HookVar::set("SDC_TEST_PANIC_ON_PLAN", "boomp_rmaos.dds@load");
    let (p, _) = plan(request(&pk, &d.path().join("pk_out"), Target::Cs));
    assert_eq!(p.failed.len(), 1, "{:#?}", p.failed);
    assert_eq!(p.failed[0].stage, Stage::Load);
    assert!(p.failed[0].error.contains("test hook"));
    assert!(p.jobs.iter().all(|j| j.pack.is_none()));
}

/// `<dir>/textures/landscape/<base>{,_n,_rmaos}.png` (16², opaque), plus `_p` (R 100) when `p`. `_rmaos`: rough
/// 0.8, metal 1 (landscape ignores it), AO 128.
fn landscape(dir: &Path, base: &str, p: bool) {
    let d = dir.join("textures/landscape");
    png(&d.join(format!("{base}.png")), 16, 16, [200, 200, 200, 255]);
    png(
        &d.join(format!("{base}_n.png")),
        16,
        16,
        [128, 128, 255, 255],
    );
    png(
        &d.join(format!("{base}_rmaos.png")),
        16,
        16,
        [204, 255, 128, 255],
    );
    if p {
        png(
            &d.join(format!("{base}_p.png")),
            16,
            16,
            [100, 100, 100, 255],
        );
    }
}

fn dirt(dir: &Path, p: bool) {
    landscape(dir, "dirt", p);
}

/// `<dir>/<rel>/g{,_n,_rmaos}.png`, a `w`² opaque albedo.
fn grass(dir: &Path, rel: &str, w: u32) {
    let d = dir.join(rel);
    png(&d.join("g.png"), w, w, [90, 140, 60, 255]);
    png(&d.join("g_n.png"), w, w, [128, 128, 255, 255]);
    png(&d.join("g_rmaos.png"), w, w, [128, 0, 255, 255]);
}

/// Picks `pbr_grass` for `textures/landscape/grass/g` by GRAS evidence (both requests).
fn gras(r: &mut BatchRequest, _: Option<&mut DeriveRequest>) {
    r.gras_all_diffuses
        .insert("textures/landscape/grass/g".to_string());
}

fn kinds(p: &Plan) -> Vec<DeriveKind> {
    p.jobs
        .iter()
        .filter_map(|j| j.derive.as_ref().map(|d| d.kind))
        .collect()
}

/// TA5, O5, O6: the landscape diffuse alpha is the `_p` R, AO strength 0.3, the `_n` alpha x2.5 with metal ignored;
/// CM writes only an `_m` (R = B = 0, A = `_p` R); the Vanilla folder keeps the `_p`, the CM folder does not.
#[test]
fn landscape_puts_p_in_the_diffuse_alpha_and_uses_ao_0_3() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    dirt(&inputs, true);
    let (p, ev) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let diffuse = out_job(&p, "textures/landscape/dirt.dds").expect("the landscape diffuse");
    assert_eq!(diffuse.profile.subtype, Subtype::LandscapeDiffuse);
    let dj = diffuse.derive.as_ref().unwrap();
    assert_eq!(dj.kind, DeriveKind::VanillaDiffuse);
    assert!(dj.landscape);
    let a = engine::derive::assemble::assemble_derived(dj).unwrap();
    let ao = 128.0 / 255.0;
    let rgb = 200.0 / 255.0 * 0.9 * (1.0 - 0.3 * (1.0 - ao));
    let px = a.image.px[0];
    assert!((px[0] - rgb).abs() < 1e-4, "{px:?} vs {rgb}");
    assert!((px[3] - 100.0 / 255.0).abs() < 1e-6, "alpha {}", px[3]);
    assert!(a.info.has_alpha);
    let n = out_job(&p, "textures/landscape/dirt_n.dds").expect("the landscape _n");
    assert_eq!(n.profile.subtype, Subtype::LandscapeNormal);
    let a = engine::derive::assemble::assemble_derived(n.derive.as_ref().unwrap()).unwrap();
    let spec: f32 = (2.5 * 0.66 * (1.0 - 204.0 / 255.0) * ao).min(1.0);
    assert!(
        (a.image.px[0][3] - spec).abs() < 1e-4,
        "{}",
        a.image.px[0][3]
    );
    // O6: the set's own `_p` job stays in the Vanilla plan, under the derived landscape pick.
    let kept = out_job(&p, "textures/landscape/dirt_p.dds").expect("the kept _p (O6)");
    assert!(kept.derive.is_none());
    assert!(slash(&kept.input).ends_with("dirt_p.png"));
    assert!(matches!(kept.status, JobStatus::Ready), "{kept:#?}");
    for j in &p.jobs {
        let pick = j.shader.as_ref().unwrap();
        assert_eq!(
            (pick.shader, &pick.provenance),
            (Shader::Landscape, &ShaderOrigin::Derived)
        );
    }
    assert_eq!(p.jobs.len(), 3, "{:#?}", p.jobs);
    assert!(out_job(&p, "textures/landscape/dirt_m.dds").is_none());
    let codes = event_codes(&ev);
    for c in [
        Code::ShaderExtraMap,
        Code::ShaderUnavailable,
        Code::DeriveNoScan,
        Code::DeriveNifSuggest,
    ] {
        assert!(!codes.contains(&c), "{codes:?}");
    }

    // CM: only the `_m`, even with Keep metal brightness (no metal on landscape, TA5).
    let (p, _) = cs_then(DeriveMode::Cm, &inputs, &d.path().join("cm"), |_, d| {
        if let Some(d) = d {
            d.params.keep_metal_brightness = true;
        }
    });
    assert_eq!(kinds(&p), [DeriveKind::CmM], "{:#?}", p.jobs);
    assert_eq!(p.jobs.len(), 1, "no _p in the CM folder: {:#?}", p.jobs);
    let m = out_job(&p, "textures/landscape/dirt_m.dds").unwrap();
    assert_eq!(m.shader.as_ref().unwrap().shader, Shader::ComplexMaterial);
    let a = engine::derive::assemble::assemble_derived(m.derive.as_ref().unwrap()).unwrap();
    let px = a.image.px[0];
    assert_eq!((px[0], px[2]), (0.0, 0.0));
    assert!((px[3] - 100.0 / 255.0).abs() < 1e-6, "{px:?}");
}

/// TA5: no `_p`, an opaque diffuse with no alpha, and no `_p` job.
#[test]
fn landscape_without_p_has_an_opaque_diffuse() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    dirt(&inputs, false);
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    assert!(out_job(&p, "textures/landscape/dirt_p.dds").is_none());
    let dj = out_job(&p, "textures/landscape/dirt.dds")
        .unwrap()
        .derive
        .clone()
        .unwrap();
    let a = engine::derive::assemble::assemble_derived(&dj).unwrap();
    assert!(!a.info.has_alpha);
    assert_eq!(
        analyse(&a.image, &a.info).alpha,
        engine::presets::AlphaContent::None
    );
    let out = p.request.output_root.clone();
    let s = Engine::new()
        .unwrap()
        .run(
            p,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    assert_eq!(s.converted, 2);
    let f = read_dds(&std::fs::read(out.join("textures/landscape/dirt.dds")).unwrap()).unwrap();
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    assert!(px.iter().all(|q| q[3] == 255));
}

/// 08e-1 (b): PBR grass under `textures/landscape/` reaches the derive step as a grass set, not a landscape one.
#[test]
fn grass_under_textures_landscape_is_still_grass() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    grass(&inputs, "textures/landscape/grass", 16);
    let mut r = request(&inputs, &d.path().join("pbr"), Target::Cs);
    gras(&mut r, None);
    let (cs, _) = plan(r);
    assert_eq!(derive_sets(&cs)[0].shader, Shader::PbrGrass);
    for mode in [DeriveMode::Vanilla, DeriveMode::Cm] {
        let (p, _) = cs_then(mode, &inputs, d.path(), gras);
        let j = out_job(&p, "textures/landscape/grass/g.dds").expect("the grass job");
        assert!(!j.derive.as_ref().unwrap().landscape);
        assert_ne!(j.profile.subtype, Subtype::LandscapeDiffuse);
    }
}

/// O3, CM14: Vanilla gets the plain converted diffuse only; CM gets the stack only (64x128), planned through
/// `finish_derived`, so an opaque albedo does not `remove` the `_n` spec alpha.
#[test]
fn grass_is_a_plain_diffuse_under_vanilla_and_a_stack_under_cm() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    grass(&inputs, "textures/landscape/grass", 64);
    let (p, ev) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), gras);
    assert_eq!(kinds(&p), [DeriveKind::VanillaDiffuse], "{:#?}", p.jobs);
    assert_eq!(p.jobs.len(), 1);
    let j = out_job(&p, "textures/landscape/grass/g.dds").unwrap();
    assert_eq!(j.profile.subtype, Subtype::Diffuse);
    assert_eq!(j.shader.as_ref().unwrap().shader, Shader::Default);
    let codes = event_codes(&ev);
    assert!(!codes.contains(&Code::DeriveNifSuggest), "{codes:?}");
    assert!(!codes.contains(&Code::DeriveNoScan), "{codes:?}");

    // `[files]` forces `remove`; only the CM14 hook in `finish_derived` turns it into `cutout` (review M2).
    let (p, _) = cs_then(DeriveMode::Cm, &inputs, &d.path().join("cm"), |r, d| {
        gras(r, d);
        let mut cfg = engine::presets::ConfigFile::default();
        cfg.files.insert(
            "textures/landscape/grass/g".into(),
            serde_json::json!({ "alpha": { "policy": "remove" } }),
        );
        r.project_config_inline = Some(cfg);
    });
    assert_eq!(kinds(&p), [DeriveKind::GrassStack], "{:#?}", p.jobs);
    assert_eq!(p.jobs.len(), 1);
    let j = out_job(&p, "textures/landscape/grass/g.dds").unwrap();
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
    assert_eq!(j.shader.as_ref().unwrap().shader, Shader::ComplexGrass);
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Cutout);
    assert_eq!(
        j.provenance["alpha.policy"].to_string(),
        "recipe:grass_stack"
    );
    assert!(matches!(j.status, JobStatus::Ready), "{j:#?}");
    let a = engine::derive::assemble::assemble_derived(j.derive.as_ref().unwrap()).unwrap();
    assert_eq!((a.image.width, a.image.height), (64, 128));
}

/// A grass albedo that is not square power-of-two: the stack fails with `GRASS-SHAPE`, as complex grass does.
#[test]
fn a_grass_stack_of_the_wrong_shape_is_grass_shape() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    grass(&inputs, "textures/landscape/grass", 24);
    // 24² is DIM-NPOT, a blocking code: acknowledged, so the PBR export converts the set and it is derived (CM99).
    let (p, _) = cs_then(DeriveMode::Cm, &inputs, d.path(), |r, _| {
        gras(r, None);
        r.allow.insert(Code::DimNpot);
    });
    let j = out_job(&p, "textures/landscape/grass/g.dds").unwrap();
    assert_eq!(j.derive.as_ref().unwrap().kind, DeriveKind::GrassStack);
    assert!(has(j, Code::GrassShape), "{:?}", j.warnings);
    assert!(!matches!(j.status, JobStatus::Ready), "{j:#?}");
}

/// O6 with a raw `_height` (review I1): a Vanilla derive plans the cs `Parallax` recipe for a landscape set's raw
/// height, so the Vanilla folder gets `dirt_p.dds` and the height is not an unused input. A plan without derive has
/// no such recipe (C6), and the CM folder no `_p`.
#[test]
fn a_landscape_height_becomes_the_vanilla_p() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    dirt(&inputs, false);
    png(
        &inputs.join("textures/landscape/dirt_height.png"),
        16,
        16,
        [100, 100, 100, 255],
    );
    let (p, ev) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    let kept = out_job(&p, "textures/landscape/dirt_p.dds").expect("the vanilla _p (O6)");
    assert_eq!(
        kept.pack.as_ref().map(|pk| pk.kind),
        Some(RecipeKind::Parallax)
    );
    assert!(matches!(kept.status, JobStatus::Ready), "{kept:#?}");
    let pick = kept.shader.as_ref().unwrap();
    assert_eq!(
        (pick.shader, &pick.provenance),
        (Shader::Landscape, &ShaderOrigin::Derived)
    );
    let dj = out_job(&p, "textures/landscape/dirt.dds")
        .unwrap()
        .derive
        .clone()
        .unwrap();
    let a = engine::derive::assemble::assemble_derived(&dj).unwrap();
    assert!((a.image.px[0][3] - 100.0 / 255.0).abs() < 1e-6);
    assert!(!event_codes(&ev).contains(&Code::PackUnusedInput));
    assert!(p.jobs.iter().all(|j| !has(j, Code::PackUnusedInput)));
    assert!(p
        .skipped
        .iter()
        .all(|s| s.code != Some(Code::PackUnusedInput)));

    let (p, _) = cs_then(DeriveMode::Cm, &inputs, &d.path().join("cm"), no_tweak);
    assert!(out_job(&p, "textures/landscape/dirt_p.dds").is_none());
    assert!(p.jobs.iter().all(|j| j.pack.is_none()), "{:#?}", p.jobs);

    let (p, _) = plan(request(&inputs, &d.path().join("plain"), Target::Vanilla));
    assert!(p.jobs.iter().all(|j| j
        .pack
        .as_ref()
        .is_none_or(|pk| pk.kind != RecipeKind::Parallax)));
}

/// Final review M4a: a no-albedo PBR landscape set under `textures/pbr/` keeps its `_p` (O6), and that `_p` gets no
/// misleading `DERIVE-NOT-CONVERTED`: its kept job ends it.
#[test]
fn a_kept_p_of_a_no_albedo_set_is_not_called_not_converted() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    let dir = inputs.join("textures/pbr/landscape");
    png(&dir.join("rock.png"), 16, 16, [200, 200, 200, 255]);
    png(&dir.join("rock_n.png"), 16, 16, [128, 128, 255, 255]);
    png(&dir.join("rock_rmaos.png"), 16, 16, [204, 255, 128, 255]);
    png(&dir.join("rock_p.png"), 16, 16, [100, 100, 100, 255]);
    let (s, ev) = plan_and_run(DeriveMode::Vanilla, &inputs, d.path(), |_, d| {
        if let Some(d) = d {
            assert_eq!(d.sets[0].shader, Shader::PbrLandscape);
            d.sets[0].albedo = None;
        }
    });
    let p_events: Vec<&EngineEvent> = ev
        .iter()
        .filter(|e| {
            e.path
                .as_deref()
                .is_some_and(|p| p.ends_with("/rock_p.png"))
        })
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::FileSkipped { .. } | EventKind::FileDone { .. }
            )
        })
        .collect();
    assert_eq!(p_events.len(), 1, "{p_events:#?}");
    assert!(
        matches!(p_events[0].kind, EventKind::FileDone { .. }),
        "{p_events:#?}"
    );
    assert_eq!(s.failed, 0);
}

/// Final review M4b: a landscape set whose every derived job fails at plan time (sources of different sizes) does
/// not keep its `_p`: the `_p` follows its set.
#[test]
fn a_p_follows_its_set_when_the_set_does_not_derive() {
    let d = tempfile::tempdir().unwrap();
    let inputs = d.path().join("in");
    let dir = inputs.join("textures/landscape");
    png(&dir.join("rock.png"), 32, 32, [200, 200, 200, 255]);
    png(&dir.join("rock_n.png"), 32, 32, [128, 128, 255, 255]);
    png(&dir.join("rock_rmaos.png"), 16, 16, [204, 255, 128, 255]);
    png(&dir.join("rock_p.png"), 16, 16, [100, 100, 100, 255]);
    let (p, _) = cs_then(DeriveMode::Vanilla, &inputs, d.path(), no_tweak);
    assert!(p.jobs.iter().all(|j| j.derive.is_none()), "{:#?}", p.jobs);
    assert!(
        out_job(&p, "textures/landscape/rock_p.dds").is_none(),
        "{:#?}",
        p.jobs
    );
    assert!(p
        .skipped
        .iter()
        .any(|s| s.path.ends_with("rock_p.png") && s.code == Some(Code::DeriveNotConverted)));
}
