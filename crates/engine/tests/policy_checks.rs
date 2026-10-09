use engine::analyse::analyse;
use engine::codes::Code;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

const RGBA8: SourceInfo = SourceInfo {
    bit_depth: 8,
    channels: 4,
    has_alpha: true,
    is_float: false,
};

fn run(
    target: Target,
    game: &str,
    suffix: Option<&str>,
    sets: &[&str],
    img: Rgba32F,
    sets_cli: &[&str],
    allow: &[Code],
) -> ResolvedJob {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow: BTreeSet<Code> = allow.iter().copied().collect();
    let ctx = ResolveCtx {
        target,
        registry: &reg,
        preset: default_preset_for(target),
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
    };
    let name = game.rsplit('/').next().unwrap();
    let f = InputFile {
        path: name.into(),
        rel: name.into(),
        game_path: game.into(),
        output: PathBuf::from(name).with_extension("dds"),
        kind: InputKind::Image,
    };
    let set = TextureSet {
        key: "k".into(),
        members: vec![0],
        suffixes: sets.iter().map(|s| s.to_string()).collect(),
    };
    let a = analyse(&img, &RGBA8);
    resolve_file(
        &ctx,
        &FileFacts {
            input: &f,
            suffix,
            alias: None,
            set: &set,
            analysis: &a,
            info: &RGBA8,
        },
    )
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn opaque() -> Rgba32F {
    Rgba32F::new(64, 64, [0.5, 0.5, 1.0, 1.0])
}

#[test]
fn ready_normal_under_cs() {
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &[],
        &[],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.subtype, Subtype::Normal);
}

#[test]
fn bc5_on_normal_blocks_unless_allowed() {
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &[
            "subtype:normal.format=bc5",
            "subtype:normal.encoder.backend=rgbcx",
        ],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::FmtBc5Normal]
        }
    );
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &[
            "subtype:normal.format=bc5",
            "subtype:normal.encoder.backend=rgbcx",
        ],
        &[Code::FmtBc5Normal],
    );
    assert_eq!(j.status, JobStatus::Ready);
    assert!(codes(&j).contains(&Code::FmtBc5Normal));
}

#[test]
fn pbr_albedo_unavailable_under_vanilla_and_srgb_is_an_error_there() {
    let j = run(
        Target::Vanilla,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=pbr_albedo"],
        &[],
    );
    assert!(matches!(
        j.status,
        JobStatus::Error { .. } | JobStatus::Blocked { .. }
    ));
    assert!(codes(&j).contains(&Code::CfgUnavailable));
    let j = run(
        Target::Vanilla,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse", "color.tag=srgb"],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::TagSrgbVanilla]
        }
    );
}

#[test]
fn diffuse_with_varying_alpha_is_unresolved_until_policy_is_set() {
    let mut img = opaque();
    img.px[0][3] = 0.0;
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        img.clone(),
        &["subtype=diffuse"],
        &[],
    );
    assert!(
        matches!(j.status, JobStatus::Unresolved { .. }),
        "{:?}",
        j.status
    );
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        img,
        &["subtype=diffuse", "alpha.policy=cutout"],
        &[],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(j.profile.coverage_enabled());
}

#[test]
fn threshold_range_and_npot() {
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["alpha.threshold=250"],
        &[],
    );
    assert!(codes(&j).contains(&Code::CfgInvalid));
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        Rgba32F::new(60, 64, [0.5, 0.5, 1.0, 1.0]),
        &[],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::DimNpot]
        }
    );
}

#[test]
fn availability_table() {
    assert_eq!(
        availability(Target::Cpm, Subtype::PbrNormal),
        Availability::Unavailable {
            fallback: Some(Subtype::Normal)
        }
    );
    assert_eq!(
        availability(Target::Vanilla, Subtype::ComplexMaterial),
        Availability::Unavailable {
            fallback: Some(Subtype::Envmask)
        }
    );
    assert_eq!(
        availability(Target::Cpm, Subtype::ComplexMaterial),
        Availability::Available
    );
    assert_eq!(
        availability(Target::Vanilla, Subtype::Flow),
        Availability::Unavailable { fallback: None }
    );
}

// ---- Controller carries (task-26-carries.md) ----

fn warning(j: &ResolvedJob, c: Code) -> &engine::codes::Warning {
    j.warnings
        .iter()
        .find(|w| w.code == c)
        .unwrap_or_else(|| panic!("{c:?} missing: {:?}", j.warnings))
}

/// Resolve one file with a custom registry, project config and input (relative path with folders).
fn run_with(
    reg: &PresetRegistry,
    preset: &str,
    project: Option<&ConfigFile>,
    rel: &str,
    suffix: Option<&str>,
    sets_cli: &[&str],
) -> ResolvedJob {
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow = BTreeSet::new();
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: reg,
        preset,
        user_config: None,
        project_config: project,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
    };
    let f = InputFile {
        path: rel.into(),
        rel: rel.into(),
        game_path: format!("textures/{rel}"),
        output: PathBuf::from(rel).with_extension("dds"),
        kind: InputKind::Image,
    };
    let set = TextureSet {
        key: "k".into(),
        members: vec![0],
        suffixes: suffix.iter().map(|s| s.to_string()).collect(),
    };
    let a = analyse(&opaque(), &RGBA8);
    resolve_file(
        &ctx,
        &FileFacts {
            input: &f,
            suffix,
            alias: None,
            set: &set,
            analysis: &a,
            info: &RGBA8,
        },
    )
}

/// Carry 1 (I5): BC6H and tag `n/a` go together.
#[test]
fn bc6h_and_tag_na_must_agree() {
    let g = &["subtype=pbr_emissive", "subtype:pbr_emissive.format=bc7"];
    let j = run(
        Target::Cs,
        "textures/pbr/x_g.png",
        Some("g"),
        &["g"],
        opaque(),
        g,
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CfgInvalid]
        },
        "{:?}",
        j.warnings
    );
    assert!(warning(&j, Code::CfgInvalid)
        .message
        .contains("set color.tag=srgb for a non-BC6H _g"));
    let j = run(
        Target::Cs,
        "textures/pbr/x_g.png",
        Some("g"),
        &["g"],
        opaque(),
        &[g[0], g[1], "subtype:pbr_emissive.color.tag=srgb"],
        &[],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    // hdr_sky carries linear content: the fix is UNORM
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=hdr_sky", "format=bc7"],
        &[],
    );
    assert!(
        warning(&j, Code::CfgInvalid)
            .message
            .contains("set color.tag=unorm"),
        "{:?}",
        j.warnings
    );
    // the inverse: BC6H with a UNORM tag
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["format=bc6h_uf16"],
        &[],
    );
    assert_eq!(j.profile.encoder.backend, Backend::IspcBc6h);
    assert!(
        warning(&j, Code::CfgInvalid)
            .message
            .contains("set color.tag=n/a"),
        "{:?}",
        j.warnings
    );
}

/// Carry 2: `_SRGB` and BC6H only under cs; PBR sub-types inherited by cpm/vanilla are caught.
#[test]
fn srgb_and_bc6h_are_gated_by_target() {
    let j = run(
        Target::Cpm,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse", "color.tag=srgb"],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::TagSrgbVanilla]
        }
    );
    // pbr_emissive under cpm: unavailable sub-type (blocking) and inherited BC6H outside cs (error), one code
    // in the status
    let j = run(
        Target::Cpm,
        "textures/a/x_g.png",
        Some("g"),
        &["g"],
        opaque(),
        &["subtype=pbr_emissive"],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    assert_eq!(
        codes(&j)
            .iter()
            .filter(|c| **c == Code::CfgUnavailable)
            .count(),
        2
    );
    // BC6H on an available sub-type outside cs
    let j = run(
        Target::Vanilla,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=glow", "format=bc6h_uf16", "color.tag=n/a"],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    // the inherited srgb tag of pbr_subsurface under vanilla is an error, not a silent write
    let j = run(
        Target::Vanilla,
        "textures/a/x_s.png",
        Some("s"),
        &["s"],
        opaque(),
        &["subtype=pbr_subsurface"],
        &[],
    );
    assert!(
        matches!(&j.status, JobStatus::Error { codes } if codes.contains(&Code::TagSrgbVanilla)),
        "{:?}",
        j.status
    );
    assert!(codes(&j).contains(&Code::CfgUnavailable));
}

/// Carry 3: BC1 is opaque only, whatever the alpha policy; `has_alpha()` is not relied on.
#[test]
fn bc1_blocks_every_alpha_policy_but_remove() {
    for policy in ["cutout", "blend", "cutout_blend", "data"] {
        let p = format!("alpha.policy={policy}");
        let j = run(
            Target::Cs,
            "textures/a/x.png",
            None,
            &[],
            opaque(),
            &["subtype=diffuse", "format=bc1", &p],
            &[],
        );
        assert!(
            matches!(&j.status, JobStatus::Blocked { codes } if codes.contains(&Code::FmtBc1Alpha)),
            "{policy}: {:?}",
            j.status
        );
    }
    // varying alpha on BC1 is also lost data
    let mut img = opaque();
    img.px[0][3] = 0.0;
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        img,
        &["subtype=diffuse", "format=bc1", "alpha.policy=data"],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::FmtBc1Alpha, Code::LossAlphaData]
        }
    );
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse", "format=bc1"],
        &[],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// Carry 6 (+ fix round 1): the backend follows the format unless the format's own patch set it, or it was
/// pinned (sub-type- or file-scoped --set, rule) at least as specifically as the format.
#[test]
fn backend_follows_format() {
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["subtype:normal.format=bc5"],
        &[],
    );
    assert_eq!(j.profile.encoder.backend, Backend::Rgbcx);
    assert_eq!(j.provenance["encoder.backend"], Provenance::User);
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::FmtBc5Normal]
        },
        "{:?}",
        j.warnings
    );
    // an unscoped --set backend applies only where it fits
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["encoder.backend=rgbcx"],
        &[],
    );
    assert_eq!(j.profile.encoder.backend, Backend::Bc7e);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    // a sub-type-scoped, mismatched backend is pinned, kept and reported
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["subtype:normal.encoder.backend=rgbcx"],
        &[],
    );
    assert_eq!(j.profile.encoder.backend, Backend::Rgbcx);
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CfgInvalid]
        },
        "{:?}",
        j.warnings
    );
    // a preset that changes only `format`
    let mut reg = PresetRegistry::builtin().unwrap();
    reg.add_toml("schema_version = 1\nid = \"compact\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[subtypes.rmaos]\nformat = \"bc1\"\n").unwrap();
    let j = run_with(
        &reg,
        "compact",
        None,
        "a/x_rmaos.png",
        Some("rmaos"),
        &["alpha.policy=remove"],
    );
    assert_eq!(
        (j.profile.format, j.profile.encoder.backend),
        (Format::Bc1, Backend::Rgbcx)
    );
    assert_eq!(
        j.provenance["encoder.backend"],
        Provenance::Preset("compact".into())
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// Carry 7: content that contradicts the decided sub-type is a warning, never a block.
#[test]
fn contradictions_are_warnings() {
    // a grayscale "normal"
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]),
        &[],
        &[],
    );
    assert!(warning(&j, Code::SugContradiction)
        .message
        .contains("grayscale"));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    // complex_material without varying alpha
    let j = run(
        Target::Cs,
        "textures/a/x_m.png",
        Some("m"),
        &["m"],
        Rgba32F::new(64, 64, [0.2, 0.6, 0.1, 1.0]),
        &["subtype=complex_material"],
        &[],
    );
    assert!(warning(&j, Code::SugContradiction)
        .message
        .contains("complex_material"));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    // a colour sub-type that looks like a tangent-space normal map
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse"],
        &[],
    );
    assert!(warning(&j, Code::SugContradiction)
        .message
        .contains("normal map"));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    // ordinary colour raises nothing
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        Rgba32F::new(64, 64, [0.6, 0.4, 0.2, 1.0]),
        &["subtype=diffuse"],
        &[],
    );
    assert!(
        !codes(&j).contains(&Code::SugContradiction),
        "{:?}",
        j.warnings
    );
}

/// Carry 8: an explicit layer (here a user preset) decides diffuse alpha; the built-in default does not.
#[test]
fn preset_alpha_policy_counts_as_explicit() {
    let mut reg = PresetRegistry::builtin().unwrap();
    reg.add_toml("schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nalpha = { policy = \"blend\" }\n").unwrap();
    let mut img = opaque();
    img.px[0][3] = 0.5;
    let overrides = [parse_set("subtype=diffuse").unwrap()];
    let allow = BTreeSet::new();
    let f = InputFile {
        path: "x.png".into(),
        rel: "x.png".into(),
        game_path: "textures/a/x.png".into(),
        output: "x.dds".into(),
        kind: InputKind::Image,
    };
    let set = TextureSet {
        key: "k".into(),
        members: vec![0],
        suffixes: BTreeSet::new(),
    };
    let a = analyse(&img, &RGBA8);
    let facts = FileFacts {
        input: &f,
        suffix: None,
        alias: None,
        set: &set,
        analysis: &a,
        info: &RGBA8,
    };
    let ctx = |preset| ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset,
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::None,
        allow: &allow,
    };
    let j = resolve_file(&ctx("mine"), &facts);
    assert_eq!(
        (j.profile.alpha.policy, j.status.clone()),
        (AlphaPolicy::Blend, JobStatus::Ready),
        "{:?}",
        j.warnings
    );
    let j = resolve_file(&ctx("cs-quality"), &facts);
    match &j.status {
        JobStatus::Unresolved { reasons } => assert!(
            reasons[0].contains("alpha meaning is unconfirmed"),
            "{reasons:?}"
        ),
        s => panic!("{s:?}"),
    }
    assert!(codes(&j).contains(&Code::SugLowConfidence));
}

/// Carry 9: sub-type and category record where they came from; the target's origin is left to Engine::plan.
#[test]
fn identity_fields_have_provenance() {
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse"],
        &[],
    );
    assert_eq!(j.provenance["subtype"], Provenance::User);
    assert_eq!(j.provenance["category"], Provenance::User);
    assert!(!j.provenance.contains_key("target"));
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &[],
        &[],
    );
    assert_eq!(
        (
            j.provenance["subtype"].clone(),
            j.provenance["category"].clone()
        ),
        (Provenance::Suggestion, Provenance::Suggestion)
    );
}

/// Carry 10: a `file:` or rule glob without '/' also matches the file name.
#[test]
fn slashless_globs_match_the_file_name() {
    let reg = PresetRegistry::builtin().unwrap();
    let j = run_with(
        &reg,
        "cs-quality",
        None,
        "sub/x_n.png",
        Some("n"),
        &["file:*.png.format=bc3", "file:X_N.png.mips.levels=1"],
    );
    assert_eq!(
        (j.profile.format, j.profile.mips.levels),
        (Format::Bc3, MipLevels::Count(1))
    );
    assert_eq!(j.profile.encoder.backend, Backend::Rgbcx);
    let j = run_with(
        &reg,
        "cs-quality",
        None,
        "sub/x_n.png",
        Some("n"),
        &["file:other.png.format=bc3", "file:x_*.tga.mips.levels=1"],
    );
    assert_eq!(
        (j.profile.format, j.profile.mips.levels),
        (Format::Bc7, MipLevels::Keyword(MipKeyword::Full))
    );
    // a glob with '/' is still matched against the whole path only
    let j = run_with(
        &reg,
        "cs-quality",
        None,
        "sub/x_n.png",
        Some("n"),
        &["file:x/*.png.format=bc3"],
    );
    assert_eq!(j.profile.format, Format::Bc7);
    // rule globs too
    let project: ConfigFile = toml::from_str(
        "[[rules]]\nmatch = { glob = \"*_n.png\" }\noverrides = { mips = { levels = 2 } }\n",
    )
    .unwrap();
    let j = run_with(
        &reg,
        "cs-quality",
        Some(&project),
        "sub/x_n.png",
        Some("n"),
        &[],
    );
    assert_eq!(
        (j.profile.mips.levels, j.provenance["mips.levels"].clone()),
        (MipLevels::Count(2), Provenance::Rule("project#0".into()))
    );
}

/// The JSON keys Task 42's `inspect` reads.
#[test]
fn resolved_job_json_shape() {
    let mut img = opaque();
    img.px[0][3] = 0.0;
    let j = run(Target::Cs, "textures/a/x.png", None, &[], img, &[], &[]);
    let v = serde_json::to_value(&j).unwrap();
    assert_eq!(v["input"], "x.png");
    assert_eq!(v["profile"]["subtype"], "diffuse");
    assert_eq!(v["profile"]["format"], "bc7");
    assert_eq!(v["profile"]["color"]["tag"], "unorm");
    assert_eq!(v["profile"]["alpha"]["policy"], "remove");
    assert_eq!(v["suggestions"][0]["confidence"], "medium");
    assert!(v["suggestions"][0]["reason"].is_string());
    assert_eq!(v["status"]["state"], "unresolved");
    assert!(v["status"]["reasons"].is_array());
    let w = &v["warnings"][0];
    assert!(
        w["severity"].is_string() && w["code"].is_string() && w["message"].is_string(),
        "{w}"
    );
    let ready = serde_json::to_value(run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &[],
        &[],
    ))
    .unwrap();
    assert_eq!(ready["status"], serde_json::json!({ "state": "ready" }));
    let schema = serde_json::to_string(&schemars::schema_for!(ResolvedJob)).unwrap();
    assert!(schema.contains("\"state\""));
}

// ---- Fix round 1 ----

fn sets(cli: &[&str]) -> Vec<engine::overrides::SetOverride> {
    cli.iter().map(|s| parse_set(s).unwrap()).collect()
}

fn resolve_in(ctx: &ResolveCtx, rel: &str, suffix: Option<&str>, img: &Rgba32F) -> ResolvedJob {
    let f = InputFile {
        path: rel.into(),
        rel: rel.into(),
        game_path: format!("textures/{rel}"),
        output: PathBuf::from(rel).with_extension("dds"),
        kind: InputKind::Image,
    };
    let set = TextureSet {
        key: "k".into(),
        members: vec![0],
        suffixes: suffix.iter().map(|s| s.to_string()).collect(),
    };
    let a = analyse(img, &RGBA8);
    resolve_file(
        ctx,
        &FileFacts {
            input: &f,
            suffix,
            alias: None,
            set: &set,
            analysis: &a,
            info: &RGBA8,
        },
    )
}

/// IMPORTANT 1: `--allow CFG-UNAVAILABLE` accepts a §4.2 fallback; no-fallback sub-types and BC6H outside cs
/// stay out.
#[test]
fn allow_cfg_unavailable_accepts_fallbacks_only() {
    let allow = &[Code::CfgUnavailable];
    // BC6H outside cs: an error even with --allow
    let j = run(
        Target::Vanilla,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=glow", "format=bc6h_uf16", "color.tag=n/a"],
        allow,
    );
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    // pbr_albedo under vanilla resolves as diffuse
    let j = run(
        Target::Vanilla,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=pbr_albedo"],
        allow,
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(
        (j.profile.subtype, j.profile.color.tag),
        (Subtype::Diffuse, ColorTag::Unorm)
    );
    assert_eq!(j.provenance["subtype"], Provenance::Fallback);
    assert_eq!(j.provenance["subtype"].to_string(), "fallback");
    let w = warning(&j, Code::CfgUnavailable);
    assert_eq!(w.severity, engine::codes::Severity::Warning);
    assert!(w.message.contains("fallback diffuse"), "{}", w.message);
    // pbr_emissive under cpm resolves as glow (BC7, not the inherited BC6H)
    let j = run(
        Target::Cpm,
        "textures/a/x_g.png",
        Some("g"),
        &["g"],
        opaque(),
        &["subtype=pbr_emissive"],
        allow,
    );
    assert_eq!(
        (j.profile.subtype, j.profile.format, j.status.clone()),
        (Subtype::Glow, Format::Bc7, JobStatus::Ready),
        "{:?}",
        j.warnings
    );
    // complex_material under vanilla falls back to envmask, and its varying alpha is reported as lost
    let mut m = Rgba32F::new(64, 64, [0.2, 0.6, 0.1, 1.0]);
    m.px[0][3] = 0.5;
    let reg = PresetRegistry::builtin().unwrap();
    let overrides = sets(&["subtype=complex_material"]);
    let allow_set: BTreeSet<Code> = [Code::CfgUnavailable].into();
    let ctx = ResolveCtx {
        target: Target::Vanilla,
        registry: &reg,
        preset: "vanilla-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow_set,
    };
    let j = resolve_in(&ctx, "a/x_m.png", Some("m"), &m);
    assert_eq!(
        (j.profile.subtype, j.status.clone()),
        (
            Subtype::Envmask,
            JobStatus::Blocked {
                codes: vec![Code::LossAlphaData]
            }
        ),
        "{:?}",
        j.warnings
    );
    // no fallback: still blocked
    let j = run(
        Target::Vanilla,
        "textures/a/x_rmaos.png",
        Some("rmaos"),
        &["rmaos"],
        opaque(),
        &[],
        allow,
    );
    assert_eq!(j.profile.subtype, Subtype::Rmaos);
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    // without --allow the fallback is only offered
    let j = run(
        Target::Vanilla,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["subtype=pbr_normal"],
        &[],
    );
    assert_eq!(j.profile.subtype, Subtype::PbrNormal);
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    assert!(warning(&j, Code::CfgUnavailable)
        .message
        .contains("--allow CFG-UNAVAILABLE"));
}

/// IMPORTANT 2: config and preset tables never pin a backend against a more specific format.
#[test]
fn table_backends_follow_more_specific_formats() {
    let reg = PresetRegistry::builtin().unwrap();
    let user: ConfigFile =
        toml::from_str("[defaults]\nencoder = { backend = \"nvtt\" }\n").unwrap();
    let allow = BTreeSet::new();
    let o_env = sets(&["subtype=envmask"]);
    let c = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: Some(&user),
        project_config: None,
        overrides: &o_env,
        accept: AcceptSuggestions::High,
        allow: &allow,
    };
    let j = resolve_in(&c, "a/x_m.png", Some("m"), &opaque());
    assert_eq!(
        (j.profile.format, j.profile.encoder.backend),
        (Format::Bc4, Backend::Rgbcx)
    );
    assert!(!codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
    assert_eq!(j.provenance["encoder.backend"], Provenance::Builtin);
    // the same config on a BC7 diffuse keeps NVTT
    let o_dif = sets(&["subtype=diffuse"]);
    let c = ResolveCtx {
        overrides: &o_dif,
        ..c
    };
    let j = resolve_in(&c, "a/x.png", None, &opaque());
    assert_eq!(
        (j.profile.format, j.profile.encoder.backend),
        (Format::Bc7, Backend::Nvtt)
    );
    assert_eq!(j.provenance["encoder.backend"], Provenance::UserConfig);
    // a rule that sets format=bc1 and backend=bc7e is an explicit conflict
    let project: ConfigFile = toml::from_str("[[rules]]\nmatch = { glob = \"*.png\" }\noverrides = { format = \"bc1\", encoder = { backend = \"bc7e\" } }\n").unwrap();
    let c = ResolveCtx {
        user_config: None,
        project_config: Some(&project),
        ..c
    };
    let j = resolve_in(&c, "a/x.png", None, &opaque());
    assert_eq!(
        (j.profile.format, j.profile.encoder.backend),
        (Format::Bc1, Backend::Bc7e)
    );
    assert!(
        matches!(&j.status, JobStatus::Error { codes } if codes.contains(&Code::CfgInvalid)),
        "{:?}",
        j.status
    );
    // ... while a rule setting only format=bc1 takes rgbcx
    let project: ConfigFile = toml::from_str(
        "[[rules]]\nmatch = { glob = \"*.png\" }\noverrides = { format = \"bc1\" }\n",
    )
    .unwrap();
    let c = ResolveCtx {
        project_config: Some(&project),
        ..c
    };
    let j = resolve_in(&c, "a/x.png", None, &opaque());
    assert_eq!(j.profile.encoder.backend, Backend::Rgbcx);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// M1: a built-in preset's alpha policy is a default, not a choice.
#[test]
fn builtin_presets_never_confirm_mesh_alpha() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut img = opaque();
    img.px[0][3] = 0.5;
    let allow = BTreeSet::new();
    assert!(reg.is_builtin(BASE_ID));
    for target in [Target::Cs, Target::Cpm, Target::Vanilla] {
        let preset = default_preset_for(target);
        assert!(reg.is_builtin(preset));
        for st in ["diffuse", "pbr_albedo", "ui_sky"] {
            let overrides = sets(&[&format!("subtype={st}")]);
            // resolved under cs so pbr_albedo is available and the alpha gate alone decides the status
            let ctx = ResolveCtx {
                target: Target::Cs,
                registry: &reg,
                preset,
                user_config: None,
                project_config: None,
                overrides: &overrides,
                accept: AcceptSuggestions::High,
                allow: &allow,
            };
            let j = resolve_in(&ctx, "a/x.png", None, &img);
            match &j.status {
                JobStatus::Unresolved { reasons } => assert!(
                    reasons
                        .iter()
                        .any(|r| r.contains("alpha meaning is unconfirmed")),
                    "{preset}/{st}: {reasons:?}"
                ),
                s => panic!("{preset}/{st}: {s:?}"),
            }
        }
    }
    // a user preset loaded under a built-in id is not built in
    let mut reg = PresetRegistry::builtin().unwrap();
    reg.add_toml("schema_version = 1\nid = \"cs-quality\"\nversion = \"9.0.0\"\nbase = \"sdc-base\"\n[subtypes.diffuse]\nalpha = { policy = \"blend\" }\n").unwrap();
    assert!(!reg.is_builtin("cs-quality"));
    let overrides = sets(&["subtype=diffuse"]);
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
    };
    let j = resolve_in(&ctx, "a/x.png", None, &img);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// M4: the tag to suggest for a non-BC6H format is srgb only for PBR colour under cs.
#[test]
fn na_tag_message_suggests_srgb_only_for_pbr_colour_under_cs() {
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=glow", "color.tag=n/a"],
        &[],
    );
    let m = &warning(&j, Code::CfgInvalid).message;
    assert!(
        m.contains("set color.tag=unorm") && !m.contains("srgb"),
        "{m}"
    );
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=pbr_albedo", "color.tag=n/a"],
        &[],
    );
    assert!(warning(&j, Code::CfgInvalid)
        .message
        .contains("set color.tag=srgb for a non-BC6H _g"));
}
