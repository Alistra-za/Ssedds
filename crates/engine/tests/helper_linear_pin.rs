//! Task 5 (Note FA12): pins what `helper::plan::plan_fills` and `helper::report::reports` return for a set with
//! two jobs on one input, a member with no job and an assigned fill whose size differs, so that indexing the jobs
//! cannot change a first match.
use engine::codes::Code;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::helper::plan::{plan_fills, AlphaFacts, FillCandidate, FillPlan};
use engine::helper::report::{reports, HelperReport};
use engine::helper::{AlphaTarget, FillJob, FillTarget, HelperRole};
use engine::overrides::parse_set;
use engine::pack::{ChannelSource, PackJob, PackSource, RecipeKind};
use engine::policy::*;
use engine::presets::PresetRegistry;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn path(name: &str) -> PathBuf {
    PathBuf::from(format!("in/textures/a/{name}"))
}

fn stamp(n: u64) -> Option<SourceStamp> {
    Some(SourceStamp {
        size: n,
        mtime_ns: n * 10,
    })
}

fn job(f: &InputFile, subtype: Subtype, status: JobStatus, n: u64) -> ResolvedJob {
    let reg = PresetRegistry::builtin().unwrap();
    ResolvedJob {
        input: f.path.clone(),
        output: f.output.clone(),
        game_path: f.game_path.clone(),
        set_key: "textures/a/x".into(),
        profile: reg.base_profile(Target::Cs, subtype),
        provenance: BTreeMap::new(),
        suggestions: vec![],
        warnings: vec![],
        status,
        suffix_alias: None,
        source: stamp(n),
        pack: None,
        bake: None,
        shader: None,
        rnm: None,
        derive: None,
        cube: None,
        fill: None,
    }
}

struct Fx {
    files: Vec<InputFile>,
    jobs: Vec<ResolvedJob>,
    facts: BTreeMap<PathBuf, AlphaFacts>,
    set: TextureSet,
}

/// x.png (diffuse, empty alpha), x_opacity.png (a packing input with a recipe job in front of its own job),
/// x_spec.png (no job at all) and x_n.png (a 128 px `_n`, the target of the assigned spec fill).
fn fx() -> Fx {
    let names = ["x.png", "x_opacity.png", "x_spec.png", "x_n.png"];
    let files: Vec<InputFile> = names
        .iter()
        .map(|name| InputFile {
            path: path(name),
            rel: (*name).into(),
            game_path: format!("textures/a/{name}"),
            output: PathBuf::from(format!(
                "out/textures/a/{}.dds",
                name.trim_end_matches(".png")
            )),
            kind: InputKind::Image,
            real_rel: None,
        })
        .collect();
    // The recipe job comes first and has the helper as its first source (Note M8): same `input`, different stamp.
    let mut recipe = job(&files[1], Subtype::GenericData, JobStatus::Ready, 1);
    recipe.output = PathBuf::from("out/textures/a/x_rmaos.dds");
    recipe.pack = Some(PackJob {
        kind: RecipeKind::Rmaos,
        channels: [
            ChannelSource::Source {
                path: files[1].path.clone(),
                role: RawMap::Opacity,
                invert: false,
                channel: None,
            },
            ChannelSource::Fill {
                role: RawMap::Metallic,
                code: 0,
            },
            ChannelSource::Fill {
                role: RawMap::Ao,
                code: 255,
            },
            ChannelSource::Fill {
                role: RawMap::Specular,
                code: 255,
            },
        ],
        gray: GraySource::R,
        sources: vec![PackSource {
            path: files[1].path.clone(),
            role: RawMap::Opacity,
            stamp: stamp(1),
            read: None,
        }],
        hand_placed: false,
    });
    let jobs = vec![
        job(&files[0], Subtype::Diffuse, JobStatus::Ready, 2),
        recipe,
        job(
            &files[1],
            Subtype::GenericData,
            JobStatus::PackingInput {
                role: RawMap::Opacity,
            },
            3,
        ),
        job(&files[3], Subtype::Normal, JobStatus::Ready, 4),
    ];
    let facts = files
        .iter()
        .map(|f| {
            let s = if f.path == path("x_n.png") { 128 } else { 64 };
            (
                f.path.clone(),
                AlphaFacts {
                    empty: true,
                    size: (s, s),
                },
            )
        })
        .collect();
    Fx {
        set: TextureSet {
            key: "textures/a/x".into(),
            members: (0..files.len()).collect(),
            suffixes: Default::default(),
        },
        files,
        jobs,
        facts,
    }
}

#[test]
fn fills_and_reports_pin_first_match_order() {
    let mut f = fx();
    let reg = PresetRegistry::builtin().unwrap();
    let overrides = vec![parse_set("file:**/x_spec.png.helper.fills=[\"n.a\"]").unwrap()];
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
    let plan: FillPlan = plan_fills(
        &ctx,
        std::slice::from_ref(&f.set),
        &f.files,
        &mut f.jobs,
        &f.facts,
    );
    let rep: Vec<HelperReport> =
        reports(&ctx, std::slice::from_ref(&f.set), &f.files, &f.jobs, &plan);
    let sp = |i: usize| f.files[i].path.clone();
    let rmaos = |channel| FillTarget::Packed {
        suffix: "rmaos",
        channel,
    };
    // The assigned spec fill stays out of `candidates`: its target job turned into the error, and the opacity
    // fill's stamp is the recipe job's (the first job of that input), not the helper's own job's.
    assert_eq!(
        plan,
        FillPlan {
            candidates: vec![FillCandidate {
                set_key: "textures/a/x".into(),
                target: sp(0),
                fill: FillJob {
                    source: PackSource {
                        path: sp(1),
                        role: RawMap::Opacity,
                        stamp: stamp(1),
                        read: None,
                    },
                    read: None,
                    invert: false,
                    assigned: false,
                },
            }],
            used: [sp(1), sp(2)].into_iter().collect(),
            not_used: BTreeMap::new(),
            skipped_targets: BTreeMap::new(),
        }
    );
    // Both packed `options` lists are the same: the alpha target the plain diffuse job offers, then the recipes.
    let options = vec![
        FillTarget::Alpha(AlphaTarget::Diffuse),
        rmaos(0),
        rmaos(1),
        rmaos(2),
        rmaos(3),
        FillTarget::Packed {
            suffix: "p",
            channel: 0,
        },
    ];
    let helper = |i: usize, role, fills, hand, outputs: &str| HelperReport {
        path: sp(i),
        set_key: "textures/a/x".into(),
        role: HelperRole::Raw(role),
        role_hand_set: false,
        fills,
        fills_hand_set: hand,
        options: options.clone(),
        skipped_targets: vec![],
        not_used: None,
        read: None,
        invert: false,
        rnm_target: None,
        outputs: vec![PathBuf::from(outputs)],
    };
    assert_eq!(
        rep,
        vec![
            // Its own job is the one without a recipe (PackingInput), though the recipe job comes first.
            helper(
                1,
                RawMap::Opacity,
                vec![rmaos(0)],
                false,
                "out/textures/a/x_rmaos.dds"
            ),
            // No job: the name decides the role.
            helper(
                2,
                RawMap::Specular,
                vec![FillTarget::Alpha(AlphaTarget::N)],
                true,
                "out/textures/a/x_n.dds"
            ),
        ]
    );
    let status: Vec<&JobStatus> = f.jobs.iter().map(|j| &j.status).collect();
    assert_eq!(
        status,
        [
            &JobStatus::Ready,
            &JobStatus::Ready,
            &JobStatus::PackingInput {
                role: RawMap::Opacity
            },
            &JobStatus::Error {
                codes: vec![Code::PackSizeMismatch]
            },
        ]
    );
    let bad = &f.jobs[3];
    assert_eq!(bad.warnings.len(), 1);
    assert_eq!(bad.warnings[0].code, Code::PackSizeMismatch);
    // The error job keeps the assigned helper; a helper without a job has no stamp.
    assert_eq!(
        bad.fill,
        Some(FillJob {
            source: PackSource {
                path: sp(2),
                role: RawMap::Specular,
                stamp: None,
                read: None,
            },
            read: None,
            invert: false,
            assigned: true,
        })
    );
    assert!(f
        .jobs
        .iter()
        .take(3)
        .all(|j| j.fill.is_none() && j.warnings.is_empty()));
}
