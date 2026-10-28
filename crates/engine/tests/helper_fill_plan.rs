//! Note DX (Task 3): which alpha channels the helpers of a set fill (`helper::plan::plan_fills`, Rulings 4-8, 11, 12).
use engine::codes::Code;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::helper::plan::{plan_fills, AlphaFacts, FillPlan};
use engine::helper::{AlphaTarget, FillTarget};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::PresetRegistry;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

enum Kind {
    // A plain file job of this sub-type.
    Plain(Subtype),
    // A raw map waiting for packing.
    Raw(RawMap),
}

struct Fx {
    files: Vec<InputFile>,
    jobs: Vec<ResolvedJob>,
    facts: BTreeMap<PathBuf, AlphaFacts>,
    set: TextureSet,
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(format!("in/textures/a/{name}"))
}

/// Set `x` of `members`, each 64x64 and empty-alpha unless changed.
fn fx(target: Target, members: &[(&str, Kind)]) -> Fx {
    let reg = PresetRegistry::builtin().unwrap();
    let mut out = Fx {
        files: vec![],
        jobs: vec![],
        facts: BTreeMap::new(),
        set: TextureSet {
            key: "textures/a/x".into(),
            members: (0..members.len()).collect(),
            suffixes: Default::default(),
        },
    };
    for (name, kind) in members {
        let stem = Path::new(name).file_stem().unwrap().to_string_lossy();
        let f = InputFile {
            path: path(name),
            rel: name.into(),
            game_path: format!("textures/a/{name}"),
            output: PathBuf::from(format!("out/textures/a/{stem}.dds")),
            kind: InputKind::Image,
            real_rel: None,
        };
        let (subtype, status) = match kind {
            Kind::Plain(s) => (*s, JobStatus::Ready),
            Kind::Raw(r) => (Subtype::GenericData, JobStatus::PackingInput { role: *r }),
        };
        out.jobs.push(ResolvedJob {
            input: f.path.clone(),
            output: f.output.clone(),
            game_path: f.game_path.clone(),
            set_key: out.set.key.clone(),
            profile: reg.base_profile(target, subtype),
            provenance: BTreeMap::new(),
            suggestions: vec![],
            warnings: vec![],
            status,
            suffix_alias: None,
            source: None,
            pack: None,
            bake: None,
            shader: None,
            rnm: None,
            derive: None,
            cube: None,
            fill: None,
        });
        out.facts.insert(
            f.path.clone(),
            AlphaFacts {
                empty: true,
                size: (64, 64),
            },
        );
        out.files.push(f);
    }
    out
}

fn run(target: Target, sets: &[&str], fx: &mut Fx) -> FillPlan {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides: Vec<_> = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow = Default::default();
    let ctx = ResolveCtx {
        target,
        registry: &reg,
        preset: if target == Target::Cs {
            "cs-quality"
        } else {
            "vanilla-quality"
        },
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
    plan_fills(
        &ctx,
        std::slice::from_ref(&fx.set),
        &fx.files,
        &mut fx.jobs,
        &fx.facts,
    )
}

/// The candidates as (helper file, target file, assigned).
fn landed(p: &FillPlan) -> Vec<(String, String, bool)> {
    let n = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    p.candidates
        .iter()
        .map(|c| (n(&c.fill.source.path), n(&c.target), c.fill.assigned))
        .collect()
}

fn own(h: &str, t: &str, assigned: bool) -> Vec<(String, String, bool)> {
    vec![(h.into(), t.into(), assigned)]
}

fn reason<'a>(p: &'a FillPlan, name: &str) -> &'a str {
    p.not_used
        .get(&path(name))
        .unwrap_or_else(|| panic!("{name} is used: {:?}", p.not_used))
}

fn set_facts(fx: &mut Fx, name: &str, empty: bool, size: u32) {
    fx.facts.insert(
        path(name),
        AlphaFacts {
            empty,
            size: (size, size),
        },
    );
}

fn opacity_pair() -> Fx {
    fx(
        Target::Cs,
        &[
            ("x.png", Kind::Plain(Subtype::Diffuse)),
            ("x_opacity.png", Kind::Raw(RawMap::Opacity)),
        ],
    )
}

#[test]
fn suggested_opacity_targets_empty_diffuse() {
    let mut f = opacity_pair();
    let p = run(Target::Cs, &[], &mut f);
    assert_eq!(landed(&p), own("x_opacity.png", "x.png", false));
    assert!(p.used.contains(&path("x_opacity.png")), "{:?}", p.used);
    assert!(p.not_used.is_empty(), "{:?}", p.not_used);
    let c = &p.candidates[0];
    assert_eq!(c.set_key, "textures/a/x");
    assert_eq!(c.fill.identity(), "a:r");
}

#[test]
fn suggested_opacity_skips_varying_alpha() {
    let mut f = opacity_pair();
    set_facts(&mut f, "x.png", false, 64);
    let p = run(Target::Cs, &[], &mut f);
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    assert!(
        reason(&p, "x_opacity.png").contains("already has alpha"),
        "{:?}",
        p.not_used
    );
}

#[test]
fn assigned_fill_replaces_varying_alpha() {
    let mut f = opacity_pair();
    set_facts(&mut f, "x.png", false, 64);
    let p = run(
        Target::Cs,
        &[
            "file:**/x_opacity.png.helper.fills=[\"diffuse.a\"]",
            "file:**/x_opacity.png.helper.read=luma",
            "file:**/x_opacity.png.helper.invert=true",
        ],
        &mut f,
    );
    assert_eq!(landed(&p), own("x_opacity.png", "x.png", true));
    assert_eq!(p.candidates[0].fill.identity(), "a:luma:inv");
}

#[test]
fn specular_fills_n_alpha_in_non_pbr_set() {
    for (n, want) in [(Subtype::Normal, true), (Subtype::PbrNormal, false)] {
        let mut f = fx(
            Target::Cs,
            &[
                ("x_n.png", Kind::Plain(n)),
                ("x_spec.png", Kind::Raw(RawMap::Specular)),
            ],
        );
        let p = run(Target::Cs, &[], &mut f);
        if want {
            assert_eq!(landed(&p), own("x_spec.png", "x_n.png", false));
        } else {
            assert!(p.candidates.is_empty(), "{:?}", landed(&p));
            assert!(
                reason(&p, "x_spec.png").contains("no non-PBR _n"),
                "{:?}",
                p.not_used
            );
        }
    }
}

fn coat_set() -> Fx {
    fx(
        Target::Cs,
        &[
            ("x_cnr.png", Kind::Plain(Subtype::CoatNormal)),
            ("x_roughness.png", Kind::Raw(RawMap::Roughness)),
        ],
    )
}

#[test]
fn roughness_does_not_suggest_cnr_alpha() {
    let mut f = coat_set();
    let p = run(Target::Cs, &[], &mut f);
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    // No alpha target at all: packing alone decides about it.
    assert!(p.not_used.is_empty() && p.used.is_empty(), "{p:?}");
}

#[test]
fn assigned_roughness_fills_cnr_alpha() {
    let mut f = coat_set();
    let p = run(
        Target::Cs,
        &["file:**/x_roughness.png.helper.fills=[\"rmaos.r\",\"cnr.a\"]"],
        &mut f,
    );
    assert_eq!(landed(&p), own("x_roughness.png", "x_cnr.png", true));
}

#[test]
fn vanilla_offers_only_planned_targets() {
    let mut f = fx(
        Target::Vanilla,
        &[
            ("x.png", Kind::Plain(Subtype::Diffuse)),
            ("x_n.png", Kind::Plain(Subtype::Normal)),
            ("x_spec.png", Kind::Raw(RawMap::Specular)),
            ("x_w.png", Kind::Raw(RawMap::FuzzWeight)),
        ],
    );
    let p = run(
        Target::Vanilla,
        &["file:**/x_w.png.helper.role=fuzzweight"],
        &mut f,
    );
    assert_eq!(landed(&p), own("x_spec.png", "x_n.png", false));
    assert_eq!(reason(&p, "x_w.png"), "no _f planned under vanilla");
}

#[test]
fn suggested_fill_of_other_size_is_skipped() {
    let mut f = opacity_pair();
    set_facts(&mut f, "x_opacity.png", true, 32);
    let before = f.jobs.clone();
    let p = run(Target::Cs, &[], &mut f);
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    assert_eq!(f.jobs, before);
    let r = reason(&p, "x_opacity.png");
    assert!(r.contains("32x32") && r.contains("64x64"), "{r}");
}

#[test]
fn assigned_fill_of_other_size_fails_target() {
    let mut f = opacity_pair();
    set_facts(&mut f, "x_opacity.png", true, 32);
    let p = run(
        Target::Cs,
        &["file:**/x_opacity.png.helper.fills=[\"diffuse.a\"]"],
        &mut f,
    );
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    let x = f.jobs.iter().find(|j| j.input == path("x.png")).unwrap();
    assert_eq!(
        x.status,
        JobStatus::Error {
            codes: vec![Code::PackSizeMismatch]
        }
    );
    let w = &x.warnings[0];
    assert_eq!(w.code, Code::PackSizeMismatch);
    assert!(
        w.message.contains("32x32") && w.message.contains("64x64"),
        "{}",
        w.message
    );
    // Review 4 I1: the error job keeps the helper, so the run can end it.
    assert_eq!(
        x.fill.as_ref().map(|f| f.source.path.clone()),
        Some(path("x_opacity.png"))
    );
}

#[test]
fn two_assigned_helpers_first_by_path_wins() {
    let mut f = fx(
        Target::Cs,
        &[
            ("x.png", Kind::Plain(Subtype::Diffuse)),
            ("b_o.png", Kind::Raw(RawMap::Opacity)),
            ("a_o.png", Kind::Raw(RawMap::Opacity)),
        ],
    );
    let p = run(
        Target::Cs,
        &[
            "file:**/a_o.png.helper.role=opacity",
            "file:**/a_o.png.helper.fills=[\"diffuse.a\"]",
            "file:**/b_o.png.helper.role=opacity",
            "file:**/b_o.png.helper.fills=[\"diffuse.a\"]",
        ],
        &mut f,
    );
    assert_eq!(landed(&p), own("a_o.png", "x.png", true));
    assert_eq!(reason(&p, "b_o.png"), "diffuse.a is filled by a_o.png");
}

#[test]
fn assigned_beats_suggested() {
    let mut f = fx(
        Target::Cs,
        &[
            ("x.png", Kind::Plain(Subtype::Diffuse)),
            ("x_opacity.png", Kind::Raw(RawMap::Opacity)),
            ("x_zmask.png", Kind::Raw(RawMap::Opacity)),
        ],
    );
    // `x_opacity.png` sorts first by path; the hand-set role still wins.
    let p = run(
        Target::Cs,
        &["file:**/x_zmask.png.helper.role=opacity"],
        &mut f,
    );
    assert_eq!(landed(&p), own("x_zmask.png", "x.png", true));
    assert_eq!(
        reason(&p, "x_opacity.png"),
        "diffuse.a is filled by x_zmask.png"
    );
}

/// Review fix 2: an assigned fill without facts still holds its target; the suggested helper does not take it.
#[test]
fn assigned_without_facts_reserves_its_target() {
    let mut f = fx(
        Target::Cs,
        &[
            ("x.png", Kind::Plain(Subtype::Diffuse)),
            ("x_opacity.png", Kind::Raw(RawMap::Opacity)),
            ("x_zmask.png", Kind::Raw(RawMap::Opacity)),
        ],
    );
    f.facts.remove(&path("x_zmask.png"));
    let p = run(
        Target::Cs,
        &["file:**/x_zmask.png.helper.role=opacity"],
        &mut f,
    );
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    assert_eq!(reason(&p, "x_zmask.png"), "x_zmask.png was not analysed");
    assert_eq!(
        reason(&p, "x_opacity.png"),
        "diffuse.a is filled by x_zmask.png"
    );
}

/// Review fix 1: the fill's stamp is the helper job's `source`, never read from disk (the paths here do not exist).
#[test]
fn fill_stamp_comes_from_the_helper_job() {
    let mut f = opacity_pair();
    let stamp = SourceStamp {
        size: 123,
        mtime_ns: 456,
    };
    f.jobs[1].source = Some(stamp);
    let p = run(Target::Cs, &[], &mut f);
    assert_eq!(p.candidates[0].fill.source.stamp, Some(stamp));
}

#[test]
fn cube_job_is_never_a_target() {
    let mut f = fx(
        Target::Cs,
        &[
            ("x.png", Kind::Plain(Subtype::Cubemap)),
            ("x_opacity.png", Kind::Raw(RawMap::Opacity)),
        ],
    );
    f.jobs[0].cube = Some(engine::cube::CubeJob {
        layout: CubeLayout::Faces,
        sources: vec![engine::cube::CubeSource {
            path: path("x.png"),
            stamp: None,
            face: None,
        }],
    });
    let p = run(Target::Cs, &[], &mut f);
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    f.jobs[0].profile.subtype = Subtype::Diffuse;
    let p = run(Target::Cs, &[], &mut f);
    assert!(
        p.candidates.is_empty(),
        "a diffuse with a cube: {:?}",
        landed(&p)
    );
}

#[test]
fn helper_never_fills_itself() {
    let mut f = fx(Target::Cs, &[("x_d.png", Kind::Plain(Subtype::Diffuse))]);
    let p = run(
        Target::Cs,
        &[
            "file:**/x_d.png.helper.role=opacity",
            "file:**/x_d.png.helper.fills=[\"diffuse.a\"]",
        ],
        &mut f,
    );
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    assert!(p.not_used.contains_key(&path("x_d.png")), "{p:?}");
}

#[test]
fn incompatible_hand_targets_are_reported() {
    let mut f = fx(
        Target::Cs,
        &[
            ("x.png", Kind::Plain(Subtype::Diffuse)),
            ("x_mask.png", Kind::Plain(Subtype::Normal)),
        ],
    );
    let p = run(
        Target::Cs,
        &[
            "file:**/x_mask.png.helper.role=bump",
            "file:**/x_mask.png.helper.fills=[\"diffuse.a\"]",
        ],
        &mut f,
    );
    assert!(p.candidates.is_empty(), "{:?}", landed(&p));
    let s = &p.skipped_targets[&path("x_mask.png")];
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].0, FillTarget::Alpha(AlphaTarget::Diffuse));
    assert!(
        s[0].1.contains("normal-type roles feed RNM layers only"),
        "{s:?}"
    );
}
