use engine::analyse::analyse;
use engine::codes::Code;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::image::{Rgba32F, SourceInfo};
use engine::nvtt::{NvttCell, NvttProbe, NvttTool};
use engine::overrides::{parse_set, NO_FILE_SCOPES};
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
    run_nvtt(target, game, suffix, sets, img, sets_cli, allow, None)
}

#[allow(clippy::too_many_arguments)]
fn run_nvtt(
    target: Target,
    game: &str,
    suffix: Option<&str>,
    sets: &[&str],
    img: Rgba32F,
    sets_cli: &[&str],
    allow: &[Code],
    nvtt: Option<&NvttCell>,
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt,
    };
    let name = game.rsplit('/').next().unwrap();
    let f = InputFile {
        path: name.into(),
        rel: name.into(),
        game_path: game.into(),
        output: PathBuf::from(name).with_extension("dds"),
        kind: InputKind::Image,
        real_rel: None,
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
    // Fix batch 39 item 2: the unavailable sub-type is the root cause; the inherited `_SRGB` tag of its profile is
    // a cascade, so TAG-SRGB-VANILLA does not fire and the file is blocked, not failed.
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    assert_eq!(codes(&j), vec![Code::CfgUnavailable]);
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
        &["alpha.threshold=255"],
        &[],
    );
    assert!(codes(&j).contains(&Code::CfgInvalid));
    // Owner O2 (M6 Track E): 1..=254 is valid.
    for t in ["1", "254"] {
        let set = format!("alpha.threshold={t}");
        let j = run(
            Target::Cs,
            "textures/a/x_n.png",
            Some("n"),
            &["n"],
            opaque(),
            &[set.as_str()],
            &[],
        );
        assert!(!codes(&j).contains(&Code::CfgInvalid), "{t}");
    }
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
fn rgbcx_level_above_max_is_cfg_invalid() {
    let job = |level: u32| {
        run(
            Target::Cs,
            "textures/a/x.png",
            None,
            &[],
            opaque(),
            &[
                "subtype=diffuse",
                "format=bc1",
                &format!("encoder.rgbcx_level={level}"),
            ],
            &[],
        )
    };
    let j = job(encoders_sys::RGBCX_MAX_LEVEL + 1);
    assert!(codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
    assert!(j
        .warnings
        .iter()
        .any(|w| w.message.contains("encoder.rgbcx_level")));
    let j = job(encoders_sys::RGBCX_MAX_LEVEL);
    assert!(!codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
}

/// DECISIONS Note BZ: `encoder.effort = "max"` is bc7e patch 0002. It plans cleanly when the vendored objects
/// carry the patch and is refused with CFG-INVALID (naming the level) when they do not.
#[test]
fn effort_max_needs_the_patch_0002_objects() {
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse", "encoder.effort=max"],
        &[],
    );
    assert_eq!(j.profile.encoder.effort, Effort::Max);
    let refused = j
        .warnings
        .iter()
        .any(|w| w.code == Code::CfgInvalid && w.message.contains("encoder.effort max"));
    assert_eq!(refused, !encoders_sys::BC7E_MAX_NATIVE, "{:?}", j.warnings);
    // The level maps to bc7e's level 7 and keeps BC6H on ISPC veryslow, as `slowest` does.
    assert_eq!(
        engine::encode::bc7_level(Effort::Max),
        encoders_sys::Bc7Level::Max
    );
    assert_eq!(
        engine::encode::bc6h_level(Effort::Max),
        engine::encode::bc6h_level(Effort::Slowest)
    );
}

#[test]
fn availability_table() {
    assert_eq!(
        availability(Target::Vanilla, Subtype::PbrNormal),
        Availability::Unavailable {
            fallback: Some(Subtype::Normal)
        }
    );
    assert_eq!(
        availability(Target::Vanilla, Subtype::ComplexMaterial),
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
    };
    let f = InputFile {
        path: rel.into(),
        rel: rel.into(),
        game_path: format!("textures/{rel}"),
        output: PathBuf::from(rel).with_extension("dds"),
        kind: InputKind::Image,
        real_rel: None,
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
    // DECISIONS S3: on pbr_emissive the tag follows the format unless a layer set it.
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.color.tag, ColorTag::Srgb);
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

/// Owner, 2026-10-11: a packed map cannot be BC6H, even with the matching tag.
#[test]
fn packed_maps_reject_bc6h() {
    for (game, suffix, subtype) in [
        ("textures/pbr/x_rmaos.png", "rmaos", "rmaos"),
        ("textures/a/x_m.png", "m", "complex_material"),
        ("textures/a/x_flow.png", "flow", "flow"),
    ] {
        let set = format!("subtype={subtype}");
        let j = run(
            Target::Cs,
            game,
            Some(suffix),
            &[suffix],
            opaque(),
            &[&set, "format=bc6h_uf16", "color.tag=n/a"],
            &[],
        );
        assert!(
            j.warnings
                .iter()
                .any(|w| w.code == Code::CfgInvalid && w.message.contains("packed map")),
            "{subtype}: {:?}",
            j.warnings
        );
    }
}

/// Carry 2: `_SRGB` and BC6H only under cs; PBR sub-types inherited by vanilla are caught.
#[test]
fn srgb_and_bc6h_are_gated_by_target() {
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
    // pbr_emissive under vanilla: the unavailable sub-type is the only report; its inherited BC6H is a cascade
    // (fix batch 39 item 2), so the file is blocked by one CFG-UNAVAILABLE, not failed.
    let j = run(
        Target::Vanilla,
        "textures/a/x_g.png",
        Some("g"),
        &["g"],
        opaque(),
        &["subtype=pbr_emissive"],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    assert_eq!(codes(&j), vec![Code::CfgUnavailable]);
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
    // pbr_subsurface under vanilla is blocked as unavailable, never a silent write; its inherited `_SRGB` tag is a
    // cascade and raises no TAG-SRGB-VANILLA (fix batch 39 item 2)
    let j = run(
        Target::Vanilla,
        "textures/a/x_s.png",
        Some("s"),
        &["s"],
        opaque(),
        &["subtype=pbr_subsurface"],
        &[],
    );
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    assert_eq!(codes(&j), vec![Code::CfgUnavailable]);
}

/// Fix batch 39 item 2: an unavailable sub-type reports CFG-UNAVAILABLE only. The checks of its profile (format,
/// tag, alpha, dimensions, alpha meaning) would be a cascade, so none of them fire and the job is blocked.
#[test]
fn unavailable_subtype_reports_cfg_unavailable_only() {
    // varying alpha, non-power-of-two: LOSS-ALPHA-DATA, DIM-NPOT and the unconfirmed alpha meaning of
    // pbr_albedo would all fire on an available profile
    let mut img = Rgba32F::new(60, 64, [0.5, 0.4, 0.3, 1.0]);
    for (i, p) in img.px.iter_mut().enumerate() {
        p[3] = (i % 7) as f32 / 6.0;
    }
    for (target, st) in [
        (Target::Vanilla, "pbr_albedo"),
        (Target::Vanilla, "pbr_emissive"),
        (Target::Vanilla, "pbr_fuzz"),
        (Target::Vanilla, "rmaos"),
        (Target::Vanilla, "flow"),
        (Target::Vanilla, "hdr_sky"),
    ] {
        let set = format!("subtype={st}");
        let j = run(
            target,
            "textures/a/x.png",
            None,
            &[],
            img.clone(),
            &[set.as_str()],
            &[],
        );
        assert_eq!(
            (codes(&j), j.status.clone()),
            (
                vec![Code::CfgUnavailable],
                JobStatus::Blocked {
                    codes: vec![Code::CfgUnavailable]
                }
            ),
            "{st} under {target:?}: {:?}",
            j.warnings
        );
    }
    // a sub-type without a fallback stays blocked by CFG-UNAVAILABLE alone, even with --allow
    let j = run(
        Target::Vanilla,
        "textures/a/x_rmaos.png",
        Some("rmaos"),
        &["rmaos"],
        img.clone(),
        &[],
        &[Code::CfgUnavailable, Code::LossAlphaData, Code::DimNpot],
    );
    assert_eq!(codes(&j), vec![Code::CfgUnavailable], "{:?}", j.warnings);
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::CfgUnavailable]
        }
    );
    // with --allow the fallback re-resolves and its own checks run normally
    let j = run(
        Target::Vanilla,
        "textures/a/x.png",
        None,
        &[],
        img,
        &["subtype=pbr_albedo"],
        &[Code::CfgUnavailable],
    );
    assert_eq!(j.profile.subtype, Subtype::Diffuse);
    let c = codes(&j);
    assert!(
        c.contains(&Code::DimNpot) && c.contains(&Code::SugLowConfidence),
        "{c:?}"
    );
    assert!(
        matches!(j.status, JobStatus::Unresolved { .. }),
        "{:?}",
        j.status
    );
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
    // complex_material writing a solid 255 alpha (parallax on; off writes 0 and does not contradict, DECISIONS AH2)
    let j = run(
        Target::Cs,
        "textures/a/x_m.png",
        Some("m"),
        &["m"],
        Rgba32F::new(64, 64, [0.2, 0.6, 0.1, 1.0]),
        &["subtype=complex_material", "alpha.parallax=true"],
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
        real_rel: None,
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
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
        JobStatus::Unresolved { reasons, .. } => assert!(
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
    assert_eq!(v["suggestions"][0]["confidence"], "high");
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
        real_rel: None,
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
    // pbr_emissive under vanilla resolves as glow (BC7, not the inherited BC6H)
    let j = run(
        Target::Vanilla,
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
    // complex_material is available under vanilla (Note AT, D16): no fallback, and its varying alpha is kept
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
    };
    let j = resolve_in(&ctx, "a/x_m.png", Some("m"), &m);
    assert_eq!(
        (
            j.profile.subtype,
            j.profile.format,
            j.profile.alpha.policy,
            j.status.clone()
        ),
        (
            Subtype::ComplexMaterial,
            Format::Bc7,
            AlphaPolicy::Data,
            JobStatus::Ready
        ),
        "{:?}",
        j.warnings
    );
    assert!(
        !codes(&j).contains(&Code::CfgUnavailable),
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
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
    for target in [Target::Cs, Target::Vanilla] {
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
                files: &NO_FILE_SCOPES,
                shaders: &NO_SHADERS,
                mesh_alpha: &NO_MESH_ALPHA,
                nvtt: None,
            };
            let j = resolve_in(&ctx, "a/x.png", None, &img);
            match &j.status {
                JobStatus::Unresolved { reasons, .. } => assert!(
                    reasons
                        .iter()
                        .any(|r| r.contains("alpha meaning is unconfirmed")),
                    "{preset}/{st}: {reasons:?}"
                ),
                s => panic!("{preset}/{st}: {s:?}"),
            }
        }
    }
    // A user preset may not reuse a built-in id (M6 Track D, Note T7) …
    let mut reg = PresetRegistry::builtin().unwrap();
    assert!(reg
        .add_toml(
            "schema_version = 1\nid = \"cs-quality\"\nversion = \"9.0.0\"\nbase = \"sdc-base\"\n"
        )
        .is_err());
    assert!(reg.is_builtin("cs-quality"));
    // … and a user preset's alpha policy is a choice.
    reg.add_toml("schema_version = 1\nid = \"mine\"\nversion = \"9.0.0\"\nbase = \"sdc-base\"\n[subtypes.diffuse]\nalpha = { policy = \"blend\" }\n").unwrap();
    assert!(!reg.is_builtin("mine"));
    let overrides = sets(&["subtype=diffuse"]);
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "mine",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
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

/// Codex #2: replacing `sdc-base` is refused, so resolving never reaches an unusable base; a cycle through user
/// presets yields an error status, not a panic.
#[test]
fn reserved_base_and_preset_cycles_give_errors_not_panics() {
    let mut reg = PresetRegistry::builtin().unwrap();
    assert!(reg
        .add_toml(
            "schema_version = 1
id = \"sdc-base\"
version = \"1\"
"
        )
        .is_err());
    let j = run_with(&reg, "cs-quality", None, "a/x_n.png", Some("n"), &[]);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);

    reg.add_toml(
        "schema_version = 1
id = \"cyc-a\"
version = \"1\"
base = \"cyc-b\"
",
    )
    .unwrap();
    reg.add_toml(
        "schema_version = 1
id = \"cyc-b\"
version = \"1\"
base = \"cyc-a\"
",
    )
    .unwrap();
    let j = run_with(&reg, "cyc-a", None, "a/x_n.png", Some("n"), &[]);
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CfgInvalid]
        }
    );
    assert!(warning(&j, Code::CfgInvalid).message.contains("cycle"));

    // A registry with no base at all (never built by the CLI) also errors instead of panicking.
    let empty = PresetRegistry::default();
    let j = run_with(&empty, "cs-quality", None, "a/x_n.png", Some("n"), &[]);
    assert!(
        matches!(j.status, JobStatus::Error { .. }),
        "{:?}",
        j.status
    );
}

/// Codex #5: file overrides match case-insensitively beyond ASCII.
#[test]
fn file_override_matches_non_ascii_case_differences() {
    let reg = PresetRegistry::builtin().unwrap();
    let j = run_with(
        &reg,
        "cs-quality",
        None,
        "Ärmör_n.png",
        Some("n"),
        &["file:ärmör_n.png.channels.flip_green=true"],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(j.profile.channels.flip_green);
    assert_eq!(j.provenance["channels.flip_green"], Provenance::User);
    // the same pattern in upper case, and a rule glob, behave alike
    let j = run_with(
        &reg,
        "cs-quality",
        None,
        "ärmör_n.png",
        Some("n"),
        &["file:ÄRMÖR_N.PNG.channels.flip_green=true"],
    );
    assert!(j.profile.channels.flip_green);
    let project = ConfigFile {
        rules: vec![Rule {
            matcher: RuleMatch {
                glob: Some("**/ÄRMÖR_*.png".into()),
                ..Default::default()
            },
            subtype: None,
            overrides: serde_json::json!({ "channels": { "flip_green": true } }),
        }],
        ..Default::default()
    };
    let j = run_with(
        &reg,
        "cs-quality",
        Some(&project),
        "x/Ärmör_n.png",
        Some("n"),
        &[],
    );
    assert!(j.profile.channels.flip_green);
    assert!(matches!(
        j.provenance["channels.flip_green"],
        Provenance::Rule(_)
    ));
}

/// Codex #6: ALPHA-EXACT-OFF-M follows the output format, not the alpha policy.
#[test]
fn alpha_exact_off_warns_for_bc7_envmask_even_when_alpha_is_removed() {
    let envmask = |extra: &[&str]| {
        let mut sets = vec!["subtype=envmask", "alpha.exact_opaque=false"];
        sets.extend_from_slice(extra);
        run(
            Target::Cs,
            "textures/a/x_m.png",
            Some("m"),
            &["m"],
            Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]),
            &sets,
            &[],
        )
    };
    let j = envmask(&["format=bc7", "encoder.backend=bc7e"]);
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove);
    assert!(
        codes(&j).contains(&Code::AlphaExactOffM),
        "{:?}",
        j.warnings
    );
    // BC3 stores 255 exactly: no 254 risk
    let j = envmask(&["format=bc3", "encoder.backend=rgbcx"]);
    assert_eq!(j.profile.format, Format::Bc3);
    assert!(
        !codes(&j).contains(&Code::AlphaExactOffM),
        "{:?}",
        j.warnings
    );
    // BC4 stores no alpha
    let j = envmask(&[]);
    assert_eq!(j.profile.format, Format::Bc4);
    assert!(
        !codes(&j).contains(&Code::AlphaExactOffM),
        "{:?}",
        j.warnings
    );
}

// ---- Fix batch 33: explicit legacy header, rgbcx_level scope ----

/// The "no legacy FourCC" CFG-INVALID messages for a diffuse resolved with `sets_cli`.
fn legacy_header_errors(sets_cli: &[&str]) -> Vec<String> {
    let mut sets = vec!["subtype=diffuse"];
    sets.extend_from_slice(sets_cli);
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &sets,
        &[],
    );
    j.warnings
        .iter()
        .filter(|w| w.code == Code::CfgInvalid && w.message.contains("no legacy FourCC"))
        .map(|w| w.message.clone())
        .collect()
}

#[test]
fn explicit_legacy_header_without_a_legacy_form_is_cfg_invalid() {
    for (case, sets) in [
        ("bc7", vec!["format=bc7", "color.tag=unorm"]),
        ("bc7 srgb", vec!["format=bc7", "color.tag=srgb"]),
        ("bc6h", vec!["format=bc6h_uf16", "color.tag=n/a"]),
        (
            "bc1 srgb",
            vec!["format=bc1", "color.tag=srgb", "alpha.policy=remove"],
        ),
        ("bc3 srgb", vec!["format=bc3", "color.tag=srgb"]),
    ] {
        let mut s = sets.clone();
        s.push("header=legacy");
        let m = legacy_header_errors(&s);
        assert_eq!(m.len(), 1, "{case}: {m:?}");
        assert!(m[0].contains("use dx10"), "{case}: {m:?}");
    }
    let m = legacy_header_errors(&["format=bc7", "color.tag=srgb", "header=legacy"]);
    assert!(m[0].contains("_SRGB Bc7"), "{m:?}");
    // The job as a whole is an error, not just warned.
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse", "format=bc7", "header=legacy"],
        &[],
    );
    assert!(
        matches!(j.status, JobStatus::Error { ref codes } if codes.contains(&Code::CfgInvalid)),
        "{:?}",
        j.status
    );
}

#[test]
fn legacy_header_is_accepted_where_a_legacy_form_exists_and_auto_never_errors() {
    for fmt in ["bc1", "bc3", "bc4", "bc5"] {
        let m = legacy_header_errors(&[
            &format!("format={fmt}"),
            "color.tag=unorm",
            "alpha.policy=remove",
            "header=legacy",
        ]);
        assert!(m.is_empty(), "{fmt} legacy: {m:?}");
    }
    for (fmt, tag) in [
        ("bc7", "unorm"),
        ("bc7", "srgb"),
        ("bc6h_uf16", "n/a"),
        ("bc1", "srgb"),
    ] {
        for header in ["auto", "dx10"] {
            let m = legacy_header_errors(&[
                &format!("format={fmt}"),
                &format!("color.tag={tag}"),
                "alpha.policy=remove",
                &format!("header={header}"),
            ]);
            assert!(m.is_empty(), "{fmt} {tag} {header}: {m:?}");
        }
    }
}

#[test]
fn rgbcx_level_check_applies_only_to_the_rgbcx_backend() {
    let level_error = |fmt: &str, level: u32| {
        let j = run(
            Target::Cs,
            "textures/a/x.png",
            None,
            &[],
            opaque(),
            &[
                "subtype=diffuse",
                &format!("format={fmt}"),
                "color.tag=unorm",
                "alpha.policy=remove",
                &format!("encoder.rgbcx_level={level}"),
            ],
            &[],
        );
        j.warnings
            .iter()
            .any(|w| w.code == Code::CfgInvalid && w.message.contains("encoder.rgbcx_level"))
    };
    let too_high = encoders_sys::RGBCX_MAX_LEVEL + 1;
    assert!(level_error("bc1", too_high), "BC1 uses rgbcx");
    for fmt in ["bc3", "bc4", "bc5"] {
        assert!(level_error(fmt, too_high), "{fmt} uses rgbcx");
    }
    assert!(!level_error("bc1", encoders_sys::RGBCX_MAX_LEVEL));
    assert!(
        !level_error("bc7", too_high),
        "BC7 does not read rgbcx_level"
    );
    assert!(!level_error("bc6h_uf16", too_high), "BC6H does not read it");
}

/// DECISIONS Note F: BC1 is opaque-only in v1, so `encoder.bc1_transparent_black = true` with a BC1 output is
/// CFG-INVALID (an error, not acknowledgeable); the setting is inert for every other format.
#[test]
fn bc1_transparent_black_is_cfg_invalid_with_bc1_only() {
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &[
            "subtype=diffuse",
            "format=bc1",
            "encoder.backend=rgbcx",
            "encoder.bc1_transparent_black=true",
        ],
        &[Code::CfgInvalid],
    );
    assert!(
        matches!(&j.status, JobStatus::Error { codes } if codes == &vec![Code::CfgInvalid]),
        "{:?}",
        j.status
    );
    let w = j
        .warnings
        .iter()
        .find(|w| w.code == Code::CfgInvalid)
        .unwrap();
    assert!(w.message.contains("bc1_transparent_black"), "{}", w.message);
    assert!(
        !w.message.contains("  "),
        "stray run of spaces: {}",
        w.message
    );
    // Off (the default) is fine, and so is the setting under another format.
    for sets in [
        vec![
            "subtype=diffuse",
            "format=bc1",
            "encoder.bc1_transparent_black=false",
        ],
        vec![
            "subtype=diffuse",
            "format=bc7",
            "encoder.bc1_transparent_black=true",
        ],
    ] {
        let j = run(
            Target::Cs,
            "textures/a/x.png",
            None,
            &[],
            opaque(),
            &sets,
            &[],
        );
        assert!(
            !codes(&j).contains(&Code::CfgInvalid),
            "{sets:?}: {:?}",
            j.warnings
        );
    }
}

/// A 64 px image whose alpha runs through `codes` (8-bit values) across its first texels, 255 elsewhere.
fn alpha_codes(codes: &[u8]) -> Rgba32F {
    let mut img = opaque();
    for (p, &c) in img.px.iter_mut().zip(codes) {
        p[3] = f32::from(c) / 255.0;
    }
    img
}

/// DECISIONS Note L: a PBR normal re-exported from BC7 with alpha 251..=255 removes the alpha silently, with the
/// ALPHA-NEAR-OPAQUE info and no LOSS-ALPHA-DATA.
#[test]
fn near_opaque_alpha_on_pbr_normal_resolves_to_remove_with_an_info() {
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        alpha_codes(&[251, 252, 253, 254]),
        &["subtype=pbr_normal"],
        &[],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove);
    assert!(
        !codes(&j).contains(&Code::LossAlphaData),
        "{:?}",
        j.warnings
    );
    let w = warning(&j, Code::AlphaNearOpaque);
    assert_eq!(w.severity, engine::codes::Severity::Info);
    assert_eq!(
        w.message,
        "source alpha 251–254 present (min 251): treated as opaque (BC encode artefact); alpha removed"
    );
    // One texel below the cutoff: genuinely varying alpha is no longer "near opaque", so the PBR-normal drop info replaces the near-opaque one
    // (owner 2026-10-11, DECISIONS P25: no LOSS-ALPHA-DATA block on a PBR normal).
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        alpha_codes(&[250, 252, 254]),
        &["subtype=pbr_normal"],
        &[],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(!codes(&j).contains(&Code::LossAlphaData));
    assert!(codes(&j).contains(&Code::AlphaPbrNormalDropped));
    assert!(!codes(&j).contains(&Code::AlphaNearOpaque));
    // Constant 255 raises neither.
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["subtype=pbr_normal"],
        &[],
    );
    assert!(
        !codes(&j).contains(&Code::AlphaNearOpaque),
        "{:?}",
        j.warnings
    );
}

/// An explicit `alpha.policy=data` still wins: the alpha is kept and nothing claims it was removed.
#[test]
fn explicit_data_policy_keeps_near_opaque_alpha() {
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        alpha_codes(&[252, 253]),
        &["subtype=pbr_normal", "alpha.policy=data"],
        &[],
    );
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Data);
    assert!(
        !codes(&j).contains(&Code::AlphaNearOpaque) && !codes(&j).contains(&Code::LossAlphaData),
        "{:?}",
        j.warnings
    );
    // Final review m8: the info says what the alpha holds, not "constant 255".
    assert_eq!(
        warning(&j, Code::ChanConstantAlphaData).message,
        "data alpha is near-opaque (min 252, 251–254 is BC encode noise); remove would save bits"
    );
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &["subtype=pbr_normal", "alpha.policy=data"],
        &[],
    );
    assert_eq!(
        warning(&j, Code::ChanConstantAlphaData).message,
        "data alpha is constant 255; remove would save bits"
    );
}

/// A diffuse whose alpha is 252..=255 is not blocked on the alpha meaning under `--accept-suggestions high`;
/// at 250 it still is.
#[test]
fn near_opaque_diffuse_alpha_is_not_an_unconfirmed_meaning() {
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        alpha_codes(&[252, 253, 254]),
        &["subtype=diffuse"],
        &[],
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove);
    assert!(codes(&j).contains(&Code::AlphaNearOpaque));
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        alpha_codes(&[250, 253]),
        &["subtype=diffuse"],
        &[],
    );
    assert!(
        matches!(j.status, JobStatus::Unresolved { .. }),
        "{:?}",
        j.status
    );
}

/// M2.5: `packing.roughness_default` must lie in 0..=1 on the sub-types whose recipe reads it.
#[test]
fn roughness_default_outside_0_1_is_cfg_invalid() {
    let img = Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]);
    let rmaos = |v: &str| {
        let set = format!("packing.roughness_default={v}");
        run(
            Target::Cs,
            "textures/a/x_rmaos.png",
            Some("rmaos"),
            &["rmaos"],
            img.clone(),
            &[set.as_str()],
            &[],
        )
    };
    assert!(codes(&rmaos("1.5")).contains(&Code::CfgInvalid));
    assert!(codes(&rmaos("-0.1")).contains(&Code::CfgInvalid));
    assert!(!codes(&rmaos("1.0")).contains(&Code::CfgInvalid));
    // A diffuse never reads it.
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        img,
        &["packing.roughness_default=1.5", "subtype=diffuse"],
        &[],
    );
    assert!(!codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
}

// Owner 2026-10-11 (DECISIONS P25): LOSS-ALPHA-DATA was too broad. A PBR normal's vanilla specular in alpha is
// dropped with an info, and an alpha the user explicitly removed is not reported as a loss.

/// A varying `pbr_normal` alpha (inferred remove) is an info, not a block.
#[test]
fn pbr_normal_alpha_drop_is_an_info_not_a_block() {
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        alpha_codes(&[10, 120, 250]),
        &["subtype=pbr_normal"],
        &[],
    );
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove);
    assert!(
        !codes(&j).contains(&Code::LossAlphaData),
        "{:?}",
        j.warnings
    );
    let w = warning(&j, Code::AlphaPbrNormalDropped);
    assert_eq!(w.severity, engine::codes::Severity::Info);
    assert_eq!(
        w.message,
        "vanilla specular in alpha dropped (PBR normals don't use alpha)"
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// An explicit `alpha.policy = "remove"` (here a `[files]`-style scope and a `--set`) is the user's decision.
#[test]
fn explicit_remove_raises_no_alpha_loss() {
    for sets in [
        &["subtype=diffuse", "file:x_d.png.alpha.policy=remove"][..],
        &["subtype=diffuse", "alpha.policy=remove"][..],
    ] {
        let j = run(
            Target::Cs,
            "textures/a/x_d.png",
            Some("d"),
            &["d"],
            alpha_codes(&[10, 120, 250]),
            sets,
            &[],
        );
        assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove, "{sets:?}");
        assert!(
            !codes(&j).contains(&Code::LossAlphaData),
            "{sets:?}: {:?}",
            j.warnings
        );
    }
}

/// An inferred remove on a diffuse (or albedo) with varying alpha still blocks, and so does a vanilla normal.
#[test]
fn inferred_remove_on_diffuse_and_vanilla_normal_still_block() {
    // The diffuse default keeps alpha, so force an inferred remove through a format that cannot store it.
    let j = run(
        Target::Cs,
        "textures/a/x_d.png",
        Some("d"),
        &["d"],
        alpha_codes(&[10, 120, 250]),
        &["subtype=diffuse", "format=bc1"],
        &[],
    );
    assert!(codes(&j).contains(&Code::LossAlphaData), "{:?}", j.warnings);
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        alpha_codes(&[10, 120, 250]),
        &["subtype=normal", "alpha.policy=data", "format=bc1"],
        &[],
    );
    assert!(codes(&j).contains(&Code::LossAlphaData), "{:?}", j.warnings);
    assert!(!codes(&j).contains(&Code::AlphaPbrNormalDropped));
}

fn short_normal(len: f32) -> Rgba32F {
    // Decodes to (len, 0, 0): mean decoded length `len`.
    Rgba32F::new(64, 64, [0.5 + len / 2.0, 0.5, 0.5, 1.0])
}

fn run_normal(suffix: &str, img: Rgba32F, sets_cli: &[&str]) -> ResolvedJob {
    let sets: &[&str] = if suffix == "wet" {
        &["wet", "rfaos"] // an Advanced skin set
    } else {
        &[suffix]
    };
    run(
        Target::Cs,
        &format!("textures/a/x_{suffix}.png"),
        Some(suffix),
        sets,
        img,
        sets_cli,
        &[],
    )
}

fn count(j: &ResolvedJob, c: Code) -> usize {
    codes(j).iter().filter(|x| **x == c).count()
}

#[test]
fn chan_normal_short_fires_below_0_9_only() {
    let j = run_normal("n", short_normal(0.89), &[]);
    assert_eq!(count(&j, Code::ChanNormalShort), 1, "{:?}", j.warnings);
    let j = run_normal("n", short_normal(0.91), &[]); // 0.9 itself is not exact in f32
    assert_eq!(count(&j, Code::ChanNormalShort), 0, "{:?}", j.warnings);
}

#[test]
fn chan_normal_short_is_reported_under_both_normal_bases() {
    // The brief emits it under `keep` too, with a message saying mip 0 is stored as authored.
    let msg = |j: &ResolvedJob| {
        j.warnings
            .iter()
            .find(|w| w.code == Code::ChanNormalShort)
            .map(|w| w.message.clone())
            .unwrap_or_else(|| panic!("no CHAN-NORMAL-SHORT: {:?}", j.warnings))
    };
    let renorm = run_normal("n", short_normal(0.5), &[]);
    assert!(msg(&renorm).contains("renormalised"), "{}", msg(&renorm));
    let keep = run_normal("n", short_normal(0.5), &["mips.normal_base=keep"]);
    assert!(msg(&keep).contains("as authored"), "{}", msg(&keep));
}

#[test]
fn chan_normal_short_fires_for_msn_and_coexists_with_the_length_warning() {
    let msn = run_normal("msn", short_normal(0.5), &[]);
    assert_eq!(count(&msn, Code::ChanNormalShort), 1, "{:?}", msn.warnings);
    let n = run_normal("n", short_normal(0.5), &[]);
    assert_eq!(count(&n, Code::ChanNormalShort), 1);
    assert_eq!(count(&n, Code::ChanNormalLength), 1, "{:?}", n.warnings);
}

#[test]
fn skin_wet_height_form_is_stored_domain_and_the_normal_form_is_not() {
    use engine::ops::{domain_for, Domain};
    let grey = Rgba32F::new(64, 64, [0.4, 0.4, 0.4, 1.0]);
    let j = run_normal("wet", grey, &[]);
    assert_eq!(j.profile.subtype, Subtype::SkinWet, "{:?}", j.warnings);
    assert_eq!(j.profile.category, Category::Grayscale);
    assert_eq!(domain_for(&j.profile), Domain::Stored);
    let j = run_normal("wet", opaque(), &[]);
    assert_eq!(j.profile.category, Category::Normal);
    assert_eq!(domain_for(&j.profile), Domain::Normal);
}

/// DECISIONS S4: r8/rg8 have no _SRGB form; on a normal map they block like BC4/BC5.
#[test]
fn r8_and_rg8_are_unorm_only_and_block_on_normals() {
    let j = run(
        Target::Cs,
        "textures/pbr/x.png",
        None,
        &[],
        opaque(),
        &["subtype=pbr_albedo", "format=r8"],
        &[],
    );
    assert!(
        warning(&j, Code::CfgInvalid)
            .message
            .contains("has no _SRGB form"),
        "{:?}",
        j.warnings
    );
    let j = run(
        Target::Cs,
        "textures/a/x_n.png",
        Some("n"),
        &["n"],
        opaque(),
        &[
            "subtype=normal",
            "format=rg8",
            "channels.green_convention=directx",
        ],
        &[],
    );
    assert!(
        j.warnings.iter().any(|w| w.code == Code::FmtBc5Normal),
        "{:?}",
        j.warnings
    );
}

/// Controller ruling (Task 6): every HDR format (BC6H, RGBA16F) pairs with tag n/a, both ways, and the message
/// names the format.
#[test]
fn rgba16f_and_tag_na_must_agree() {
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=hdr_sky", "format=rgba16f", "color.tag=unorm"],
        &[],
    );
    assert!(
        warning(&j, Code::CfgInvalid)
            .message
            .contains("RGBA16F has no _SRGB or UNORM form; set color.tag=n/a"),
        "{:?}",
        j.warnings
    );
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=hdr_sky", "format=rgba16f", "color.tag=n/a"],
        &[],
    );
    assert!(
        !j.warnings.iter().any(|w| w.code == Code::CfgInvalid),
        "{:?}",
        j.warnings
    );
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse", "format=rgba8", "color.tag=n/a"],
        &[],
    );
    assert!(
        warning(&j, Code::CfgInvalid).message.contains("(RGBA8)"),
        "{:?}",
        j.warnings
    );
}

/// Controller ruling (Task 6): RGBA16F is cs-only, an error even with --allow CFG-UNAVAILABLE.
#[test]
fn rgba16f_outside_cs_is_an_error() {
    let j = run(
        Target::Vanilla,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=glow", "format=rgba16f", "color.tag=n/a"],
        &[Code::CfgUnavailable],
    );
    assert_eq!(
        j.status,
        JobStatus::Error {
            codes: vec![Code::CfgUnavailable]
        },
        "{:?}",
        j.warnings
    );
    assert!(warning(&j, Code::CfgUnavailable)
        .message
        .starts_with("RGBA16F is usable only under Community Shaders"));
}

/// Controller ruling (Task 6): a packed map cannot be RGBA16F either (packed channels are 8-bit data).
#[test]
fn packed_maps_reject_rgba16f() {
    let j = run(
        Target::Cs,
        "textures/pbr/x_rmaos.png",
        Some("rmaos"),
        &["rmaos"],
        opaque(),
        &["subtype=rmaos", "format=rgba16f", "color.tag=n/a"],
        &[],
    );
    assert!(
        j.warnings.iter().any(|w| w.code == Code::CfgInvalid
            && w.message == "rmaos is a packed map; RGBA16F cannot store it, use bc7"),
        "{:?}",
        j.warnings
    );
}

/// Resolve `textures/a/x.png` (opaque RGBA8) under cs with the given `--set`s.
fn m6_job(sets_cli: &[&str]) -> ResolvedJob {
    run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        sets_cli,
        &[],
    )
}

fn cfg_invalid(j: &ResolvedJob) -> Vec<&str> {
    j.warnings
        .iter()
        .filter(|w| w.code == Code::CfgInvalid)
        .map(|w| w.message.as_str())
        .collect()
}

/// S-mips-2: tent, Mitchell and Kaiser on sRGB colour without alpha weighting.
#[test]
fn kaiser_on_srgb_diffuse_without_alpha_is_allowed() {
    for alpha in ["remove", "data"] {
        for f in ["tent", "mitchell", "kaiser"] {
            let (a, m) = (format!("alpha.policy={alpha}"), format!("mips.filter={f}"));
            let j = m6_job(&["subtype=diffuse", &a, &m]);
            assert!(cfg_invalid(&j).is_empty(), "{alpha} {f}: {:?}", j.warnings);
        }
    }
}

#[test]
fn non_box_filters_are_cfg_invalid_outside_srgb_colour() {
    for st in [
        "normal",
        "pbr_normal",
        "msn",
        "flow",
        "rmaos",
        "generic_data",
        "height",
        "envmask",
    ] {
        for f in ["tent", "mitchell", "kaiser"] {
            let (s, m) = (
                format!("subtype={st}"),
                format!("file:**/x.png.mips.filter={f}"),
            );
            let j = m6_job(&[&s, &m]);
            let id = format!("mips.filter = {f}: ");
            assert!(
                cfg_invalid(&j)
                    .iter()
                    .any(|m| m.starts_with(&id) && m.contains("sRGB colour only")),
                "{st} {f}: {:?}",
                j.warnings
            );
        }
    }
}

#[test]
fn alpha_weighted_colour_keeps_box() {
    for alpha in ["cutout", "blend", "cutout_blend"] {
        let a = format!("alpha.policy={alpha}");
        let j = m6_job(&["subtype=diffuse", &a, "file:**/x.png.mips.filter=kaiser"]);
        assert!(
            cfg_invalid(&j)
                .iter()
                .any(|m| m.contains("alpha-weighted box")),
            "{alpha}: {:?}",
            j.warnings
        );
    }
}

/// S-mips-2 with F2/M9: an HDR format (`is_hdr()`, set explicitly so the source rule does not pick BC7) on a
/// colour or HDR sub-type without alpha weighting takes tent; every other non-box case is CFG-INVALID.
#[test]
fn hdr_allows_tent_only() {
    for fmt in ["bc6h_uf16", "rgba16f"] {
        for st in ["pbr_emissive", "hdr_sky"] {
            for f in ["tent", "mitchell", "kaiser"] {
                let (s, ff, m) = (
                    format!("subtype={st}"),
                    format!("format={fmt}"),
                    format!("file:**/x.png.mips.filter={f}"),
                );
                let j = m6_job(&[&s, &ff, "color.tag=n/a", "alpha.policy=remove", &m]);
                let bad = cfg_invalid(&j);
                if f == "tent" {
                    assert!(bad.is_empty(), "{fmt} {st} {f}: {:?}", j.warnings);
                } else {
                    assert!(
                        bad.iter().any(|m| m.contains("box or tent")),
                        "{fmt} {st} {f}: {:?}",
                        j.warnings
                    );
                }
            }
        }
    }
    // F2: alpha-weighted HDR colour keeps the alpha-weighted box.
    for alpha in ["cutout", "blend", "cutout_blend"] {
        let a = format!("alpha.policy={alpha}");
        let j = m6_job(&[
            "subtype=pbr_emissive",
            "format=rgba16f",
            "color.tag=n/a",
            &a,
            "file:**/x.png.mips.filter=tent",
        ]);
        assert!(
            cfg_invalid(&j)
                .iter()
                .any(|m| m.contains("alpha-weighted box")),
            "{alpha}: {:?}",
            j.warnings
        );
    }
    // F2: HDR-format normals and grayscale keep box (spec §5.5).
    for st in ["normal", "height", "envmask"] {
        for fmt in ["bc6h_uf16", "rgba16f"] {
            let (s, ff) = (format!("subtype={st}"), format!("format={fmt}"));
            let j = m6_job(&[&s, &ff, "color.tag=n/a", "file:**/x.png.mips.filter=tent"]);
            assert!(
                cfg_invalid(&j)
                    .iter()
                    .any(|m| m.starts_with("mips.filter = tent: ")
                        && m.contains("tent only on colour or HDR maps")),
                "{st} {fmt}: {:?}",
                j.warnings
            );
        }
    }
}

#[test]
fn linear_content_colour_keeps_box() {
    let j = m6_job(&[
        "subtype=diffuse",
        "color.content=linear",
        "file:**/x.png.mips.filter=kaiser",
    ]);
    assert!(
        cfg_invalid(&j)
            .iter()
            .any(|m| m.starts_with("mips.filter = kaiser: ")),
        "{:?}",
        j.warnings
    );
}

/// S-mips-13 (owner, 2026-10-11): an unscoped non-box filter the texture cannot use resolves box, with a
/// CFG-FILTER-IGNORED Info naming the file; set at sub-type or file scope it stays CFG-INVALID.
#[test]
fn unscoped_filter_falls_back_to_box() {
    let j = m6_job(&["subtype=normal", "mips.filter=kaiser"]);
    assert!(cfg_invalid(&j).is_empty(), "{:?}", j.warnings);
    assert_eq!(j.profile.mips.filter, MipFilter::Box);
    assert_eq!(j.provenance["mips.filter"], Provenance::Fallback);
    let w = warning(&j, Code::CfgFilterIgnored);
    assert_eq!(w.severity, engine::codes::Severity::Info);
    assert_eq!(
        w.message,
        "mips.filter = kaiser only affects colour maps; x.png uses box"
    );
    assert!(matches!(j.status, JobStatus::Ready), "{:?}", j.status);
    // A colour map that cannot take it says why.
    let j = m6_job(&[
        "subtype=diffuse",
        "alpha.policy=cutout",
        "mips.filter=kaiser",
    ]);
    assert_eq!(j.profile.mips.filter, MipFilter::Box);
    assert!(
        warning(&j, Code::CfgFilterIgnored).message.ends_with(
            "uses box: alpha-tested and blended colour uses the alpha-weighted box (spec §5.5)"
        ),
        "{:?}",
        j.warnings
    );
    // An allowed filter is kept, with no notice.
    let j = m6_job(&[
        "subtype=diffuse",
        "alpha.policy=remove",
        "mips.filter=kaiser",
    ]);
    assert_eq!(j.profile.mips.filter, MipFilter::Kaiser);
    assert!(
        !codes(&j).contains(&Code::CfgFilterIgnored),
        "{:?}",
        j.warnings
    );
    // Scoped: sub-type `--set`, and a file-scoped one over an unscoped one.
    for sets in [
        &["subtype=normal", "subtype:normal.mips.filter=kaiser"][..],
        &[
            "subtype=normal",
            "mips.filter=tent",
            "file:**/x.png.mips.filter=kaiser",
        ][..],
    ] {
        let j = m6_job(sets);
        assert_eq!(j.profile.mips.filter, MipFilter::Kaiser, "{sets:?}");
        assert!(!cfg_invalid(&j).is_empty(), "{sets:?}: {:?}", j.warnings);
        assert!(!codes(&j).contains(&Code::CfgFilterIgnored), "{sets:?}");
    }
}

/// Scope is per field: an unrelated sub-type-scoped `--set` does not scope the unscoped filter, and an unrelated
/// unscoped `--set` does not unscope a scoped one; a later scoped `box` overrides the unscoped filter silently.
#[test]
fn filter_scope_is_per_field() {
    let j = m6_job(&[
        "subtype=normal",
        "mips.filter=kaiser",
        "subtype:normal.mips.normal_base=keep",
    ]);
    assert!(cfg_invalid(&j).is_empty(), "{:?}", j.warnings);
    assert_eq!(j.profile.mips.filter, MipFilter::Box);
    assert!(
        codes(&j).contains(&Code::CfgFilterIgnored),
        "{:?}",
        j.warnings
    );
    let j = m6_job(&[
        "subtype=normal",
        "subtype:normal.mips.filter=kaiser",
        "mips.normal_base=keep",
    ]);
    assert!(!cfg_invalid(&j).is_empty(), "{:?}", j.warnings);
    let j = m6_job(&[
        "subtype=normal",
        "mips.filter=kaiser",
        "subtype:normal.mips.filter=box",
    ]);
    assert_eq!(j.profile.mips.filter, MipFilter::Box);
    assert!(cfg_invalid(&j).is_empty(), "{:?}", j.warnings);
    assert!(
        !codes(&j).contains(&Code::CfgFilterIgnored),
        "{:?}",
        j.warnings
    );
}

/// S-mips-7: `resize = pot-down` turns DIM-NPOT into DIM-RESIZED (Info) with the sizes in its data.
#[test]
fn pot_down_turns_dim_npot_into_dim_resized() {
    let img = || Rgba32F::new(48, 40, [0.5, 0.5, 0.5, 1.0]);
    let diffuse = |sets: &[&str]| run(Target::Cs, "textures/a/x.png", None, &[], img(), sets, &[]);
    let j = diffuse(&["subtype=diffuse", "resize=pot-down"]);
    assert!(!codes(&j).contains(&Code::DimNpot), "{:?}", j.warnings);
    let w = warning(&j, Code::DimResized);
    assert_eq!(w.severity, engine::codes::Severity::Info);
    assert_eq!(w.data["from"], serde_json::json!([48, 40]));
    assert_eq!(w.data["to"], serde_json::json!([32, 32]));
    let j = diffuse(&["subtype=diffuse"]);
    assert_eq!(
        warning(&j, Code::DimNpot).severity,
        engine::codes::Severity::Blocking
    );
    assert!(!codes(&j).contains(&Code::DimResized), "{:?}", j.warnings);
}

/// S-mips-7: a power-of-two input is not resized, so pot-down reports nothing.
#[test]
fn pot_down_on_a_power_of_two_input_reports_no_resize() {
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        Rgba32F::new(64, 32, [0.5, 0.5, 0.5, 1.0]),
        &["subtype=diffuse", "resize=pot-down"],
        &[],
    );
    assert!(!codes(&j).contains(&Code::DimResized), "{:?}", j.warnings);
    assert!(!codes(&j).contains(&Code::DimNpot), "{:?}", j.warnings);
}

#[test]
fn flow_small_tests_the_resized_size() {
    let j = run(
        Target::Cs,
        "textures/a/x_flow.png",
        Some("flow"),
        &["flow"],
        Rgba32F::new(40, 40, [0.5, 0.5, 0.5, 1.0]),
        &["resize=pot-down"],
        &[],
    );
    assert!(codes(&j).contains(&Code::DimFlowSmall), "{:?}", j.warnings);
    assert!(codes(&j).contains(&Code::DimResized), "{:?}", j.warnings);
}

fn project_rules(rules: &str) -> ConfigFile {
    ConfigFile::parse(rules, "sdc.toml").unwrap()
}

/// Carried fix 3 (Note T10): a rule's sub-type then a `--set` sub-type — the rule's overrides no longer apply.
#[test]
fn a_set_subtype_drops_the_overrides_of_a_rule_that_named_another_subtype() {
    let reg = PresetRegistry::builtin().unwrap();
    let project = project_rules(
        "[[rules]]\nmatch = { glob = \"**/r/*.png\" }\nsubtype = \"height\"\noverrides = { validation = { min_mip_psnr_db = 31.0 } }\n",
    );
    let j = run_with(&reg, "cs-quality", Some(&project), "r/x.png", None, &[]);
    assert_eq!(j.profile.subtype, Subtype::Height);
    assert_eq!(j.profile.validation.min_mip_psnr_db, 31.0);
    let j = run_with(
        &reg,
        "cs-quality",
        Some(&project),
        "r/x.png",
        None,
        &["subtype=diffuse"],
    );
    assert_eq!(j.profile.subtype, Subtype::Diffuse);
    assert_eq!(j.profile.validation.min_mip_psnr_db, 30.0);
    assert!(!matches!(
        j.provenance.get("validation.min_mip_psnr_db"),
        Some(Provenance::Rule(_))
    ));
}

/// Carried fix 3: a `--set` sub-type then a rule for the new sub-type — that rule applies, by `subtype` or by
/// `match.subtypes`; the first rule of a source that names another sub-type is skipped for the next one.
#[test]
fn a_rule_for_the_new_subtype_applies_after_a_set_subtype() {
    let reg = PresetRegistry::builtin().unwrap();
    let project = project_rules(concat!(
        "[[rules]]\nmatch = { glob = \"**/r/*.png\" }\nsubtype = \"height\"\noverrides = { validation = { min_mip_psnr_db = 31.0 } }\n",
        "[[rules]]\nmatch = { glob = \"**/r/*.png\", subtypes = [\"diffuse\"] }\noverrides = { validation = { min_mip_psnr_db = 32.0 } }\n",
    ));
    let j = run_with(
        &reg,
        "cs-quality",
        Some(&project),
        "r/x.png",
        None,
        &["subtype=diffuse"],
    );
    assert_eq!(j.profile.validation.min_mip_psnr_db, 32.0);
    assert_eq!(
        j.provenance["validation.min_mip_psnr_db"].to_string(),
        "rule:project#1"
    );
    // A rule that names sub-types in `match` never decides one.
    let only = project_rules("[[rules]]\nmatch = { glob = \"**/r/*.png\", subtypes = [\"diffuse\"] }\nsubtype = \"height\"\n");
    let j = run_with(&reg, "cs-quality", Some(&only), "r/x.png", None, &[]);
    assert_ne!(j.provenance["subtype"].to_string(), "rule:project#0");
}

/// Carried fix 4 (Note T10): an explicit flip_green that contradicts green_convention is kept and warned.
#[test]
fn an_explicit_flip_green_against_the_convention_warns_and_wins() {
    let n = |sets: &[&str]| {
        run(
            Target::Cs,
            "textures/a/x_n.png",
            Some("n"),
            &["n"],
            opaque(),
            sets,
            &[],
        )
    };
    let j = n(&[
        "channels.green_convention=opengl",
        "channels.flip_green=false",
    ]);
    assert!(!j.profile.channels.flip_green);
    assert!(
        codes(&j).contains(&Code::CfgGreenConflict),
        "{:?}",
        j.warnings
    );
    let w = j
        .warnings
        .iter()
        .find(|w| w.code == Code::CfgGreenConflict)
        .unwrap();
    assert_eq!(
        w.data,
        serde_json::json!({"flip_green": false, "convention": "opengl"})
    );
    let j = n(&[
        "channels.green_convention=directx",
        "channels.flip_green=true",
    ]);
    assert!(j.profile.channels.flip_green);
    assert!(codes(&j).contains(&Code::CfgGreenConflict));
    // Agreeing values and the convention alone raise nothing.
    for sets in [
        &["channels.green_convention=opengl"][..],
        &[
            "channels.green_convention=opengl",
            "channels.flip_green=true",
        ],
        &["channels.green_convention=directx"],
    ] {
        let j = n(sets);
        assert!(!codes(&j).contains(&Code::CfgGreenConflict), "{sets:?}");
    }
    assert!(
        n(&["channels.green_convention=opengl"])
            .profile
            .channels
            .flip_green
    );
}

/// M5 (DECISIONS Note NV3): the NVTT codes come from the batch's probe.
#[test]
fn nvtt_codes_follow_the_probe() {
    let diffuse = |cell: Option<&NvttCell>, allow: &[Code]| {
        run_nvtt(
            Target::Cs,
            "textures/a/x.png",
            None,
            &[],
            opaque(),
            &["subtype=diffuse", "encoder.backend=nvtt"],
            allow,
            cell,
        )
    };
    let found = |v: &str| {
        NvttCell::fixed(NvttProbe::Found(NvttTool {
            path: "C:/nv/nvcompress.exe".into(),
            version: v.into(),
        }))
    };
    let j = diffuse(Some(&found("3.2.5")), &[]);
    assert_eq!(j.profile.encoder.backend, Backend::Nvtt);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);

    let j = diffuse(Some(&found("3.3.0")), &[]);
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::EncNvttVersion]
        }
    );
    let w = j
        .warnings
        .iter()
        .find(|w| w.code == Code::EncNvttVersion)
        .unwrap();
    assert!(
        w.message.contains("NVTT 3.3.0") && w.message.contains("3.2.5 is"),
        "{}",
        w.message
    );
    let j = diffuse(Some(&found("3.3.0")), &[Code::EncNvttVersion]);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);

    let missing = NvttCell::fixed(NvttProbe::Missing("not on PATH and not at X".into()));
    let j = diffuse(Some(&missing), &[]);
    assert_eq!(
        j.status,
        JobStatus::Blocked {
            codes: vec![Code::EncNvttMissing]
        }
    );
    assert!(j
        .warnings
        .iter()
        .any(|w| w.code == Code::EncNvttMissing && w.message.contains("not on PATH and not at X")));
    let j = diffuse(None, &[]);
    assert!(codes(&j).contains(&Code::EncNvttMissing));
}

/// A profile on a built-in backend never asks for the probe (no nvcompress process for such batches).
#[test]
fn builtin_backend_does_not_probe() {
    let cell = NvttCell::new(Some("Z:/no/such/nvcompress.exe".into()));
    let j = run_nvtt(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        opaque(),
        &["subtype=diffuse"],
        &[],
        Some(&cell),
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(cell.into_probed(), None);
}

/// An `_e` cubemap source (cubemap support v1), resolved with `sets_cli`.
fn cube(sets_cli: &[&str]) -> ResolvedJob {
    run(
        Target::Cs,
        "textures/a/x_e.png",
        Some("e"),
        &["e"],
        Rgba32F::new(64, 64, [0.5, 0.4, 0.3, 1.0]),
        sets_cli,
        &[],
    )
}

#[test]
fn non_cube_subtypes_ignore_cubemap_keys() {
    let diffuse = |sets: &[&str]| {
        run(
            Target::Cs,
            "textures/a/x.png",
            None,
            &[],
            Rgba32F::new(64, 64, [0.5, 0.4, 0.3, 1.0]),
            sets,
            &[],
        )
    };
    let plain = diffuse(&[]);
    let set = diffuse(&["cubemap.blur=0.5"]);
    assert_eq!(set.profile.subtype, Subtype::Diffuse);
    assert!(set.profile.cubemap.is_default());
    assert!(!set.provenance.keys().any(|k| k.starts_with("cubemap.")));
    assert_eq!(
        engine::manifest::profile_sha256(&set.profile).unwrap(),
        engine::manifest::profile_sha256(&plain.profile).unwrap()
    );
}

#[test]
fn styles_fill_unset_keys_only() {
    let j = cube(&["cubemap.style=room"]);
    assert_eq!(j.profile.subtype, Subtype::Cubemap);
    assert_eq!(j.profile.cubemap.face_size, Some(128));
    assert_eq!(j.profile.format, Format::Bc1);
    assert_eq!(j.profile.encoder.backend, Backend::Rgbcx);
    assert_eq!(j.profile.cubemap.blur, Some(0.35));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);

    let j = cube(&["cubemap.style=room", "format=bc7"]);
    assert_eq!(j.profile.format, Format::Bc7);
    assert_eq!(j.profile.encoder.backend, Backend::Bc7e);
    assert_eq!(j.profile.cubemap.face_size, Some(128));

    let j = cube(&["cubemap.style=vanilla_metal", "cubemap.face_size=64"]);
    assert_eq!(j.profile.cubemap.face_size, Some(64));
    assert_eq!(j.profile.cubemap.saturation, 0.8);
}

#[test]
fn cubemap_srgb_tag_is_invalid() {
    assert_eq!(cube(&[]).profile.color.tag, ColorTag::Unorm);
    let j = cube(&["color.tag=srgb"]);
    assert!(codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
}

#[test]
fn mips_none_on_a_cube_warns() {
    assert!(!codes(&cube(&[])).contains(&Code::CubeNoMips));
    let j = cube(&["mips.levels=none"]);
    assert!(codes(&j).contains(&Code::CubeNoMips), "{:?}", j.warnings);
}

#[test]
fn out_of_range_cube_keys_are_invalid() {
    assert!(!codes(&cube(&[])).contains(&Code::CfgInvalid));
    for set in [
        "cubemap.face_size=100",
        "cubemap.face_rotate=[45, 0, 0, 0, 0, 0]",
        "cubemap.blur=1.5",
        "cubemap.midtones=0",
        "cubemap.exposure=11",
        "cubemap.saturation=2.5",
        "cubemap.highlight_clamp=0.5",
    ] {
        let j = cube(&[set]);
        assert!(
            codes(&j).contains(&Code::CfgInvalid),
            "{set}: {:?}",
            j.warnings
        );
    }
}

/// An `_e` source of `w` x `h` with the given source facts, resolved as a cubemap; `tweak` edits the analysis.
fn cube_src(
    size: (u32, u32),
    info: SourceInfo,
    tweak: impl FnOnce(&mut engine::analyse::Analysis),
    sets_cli: &[&str],
) -> ResolvedJob {
    cube_src_named("x_e.png", size, info, tweak, sets_cli)
}

fn cube_src_named(
    name: &str,
    (w, h): (u32, u32),
    info: SourceInfo,
    tweak: impl FnOnce(&mut engine::analyse::Analysis),
    sets_cli: &[&str],
) -> ResolvedJob {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow = BTreeSet::new();
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: default_preset_for(Target::Cs),
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
    };
    let f = InputFile {
        path: name.into(),
        rel: name.into(),
        game_path: format!("textures/a/{name}"),
        output: PathBuf::from(name).with_extension("dds"),
        kind: InputKind::Image,
        real_rel: None,
    };
    let set = TextureSet {
        key: "k".into(),
        members: vec![0],
        suffixes: BTreeSet::from(["e".to_string()]),
    };
    let mut a = analyse(&Rgba32F::new(w, h, [0.5, 0.4, 0.3, 1.0]), &info);
    tweak(&mut a);
    resolve_file(
        &ctx,
        &FileFacts {
            input: &f,
            suffix: Some("e"),
            alias: None,
            set: &set,
            analysis: &a,
            info: &info,
        },
    )
}

fn cube_dims(size: (u32, u32), sets_cli: &[&str]) -> ResolvedJob {
    cube_src(size, RGBA8, |_| {}, sets_cli)
}

#[test]
fn tiny_cube_is_dim_small() {
    let j = cube_dims((48, 8), &["cubemap.face_size=2"]);
    assert!(codes(&j).contains(&Code::DimSmall), "{:?}", j.warnings);
    assert!(codes(&j).contains(&Code::CfgInvalid));
    let j = cube_dims((24, 4), &["drop_top_mip=true"]);
    assert!(codes(&j).contains(&Code::DimSmall), "{:?}", j.warnings);
    let j = cube_dims((24, 4), &[]);
    assert!(!codes(&j).contains(&Code::DimSmall), "{:?}", j.warnings);
}

#[test]
fn cube_source_dims_are_not_npot() {
    let j = cube_dims((1024, 768), &[]);
    assert!(!codes(&j).contains(&Code::DimNpot), "{:?}", j.warnings);
    let j = cube_dims((600, 450), &[]);
    let w = warning(&j, Code::DimResized);
    assert!(w.message.contains("face 150 → 128"), "{}", w.message);
    assert!(!codes(&j).contains(&Code::DimNpot));
    let j = cube_dims((512, 512), &[]);
    assert!(
        !codes(&j)
            .iter()
            .any(|c| matches!(c, Code::DimNpot | Code::DimSmall | Code::DimResized)),
        "{:?}",
        j.warnings
    );
}

#[test]
fn hdr_cube_source_without_clamp_warns() {
    let float = SourceInfo {
        bit_depth: 32,
        channels: 4,
        has_alpha: true,
        is_float: true,
    };
    let hot = |a: &mut engine::analyse::Analysis| a.peak_luminance = 40.0;
    let j = cube_src((1024, 768), float, hot, &[]);
    let w = warning(&j, Code::LossClampHdr);
    assert!(
        w.message.contains("cubemap.highlight_clamp"),
        "{}",
        w.message
    );
    let j = cube_src((1024, 768), float, hot, &["cubemap.highlight_clamp=4"]);
    assert!(!codes(&j).contains(&Code::LossClampHdr), "{:?}", j.warnings);
    let j = cube_dims((1024, 768), &[]);
    assert!(!codes(&j).contains(&Code::LossClampHdr));
}

/// DECISIONS DN7: a cube DDS is a cubemap; an explicit other sub-type is CFG-INVALID.
#[test]
fn cube_dds_is_decided_as_a_cubemap() {
    let cube_dds = |a: &mut engine::analyse::Analysis| a.dds_cube = true;
    let j = cube_src((48, 8), RGBA8, cube_dds, &[]);
    assert_eq!(j.profile.subtype, Subtype::Cubemap);
    assert!(!codes(&j).contains(&Code::DimNpot), "{:?}", j.warnings);
    // A cube DDS is always its six faces side by side: a face tag in the name does not make it six files, and an
    // explicit other layout is refused.
    let j = cube_src_named("foo_px.dds", (48, 8), RGBA8, cube_dds, &["subtype=cubemap"]);
    assert!(!codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
    assert!(!codes(&j).contains(&Code::DimSmall));
    let j = cube_src((48, 8), RGBA8, cube_dds, &["cubemap.layout=hcross"]);
    assert!(
        j.warnings
            .iter()
            .any(|w| w.code == Code::CfgInvalid && w.message.contains("remove cubemap.layout")),
        "{:?}",
        j.warnings
    );
    let j = cube_src((48, 8), RGBA8, cube_dds, &["cubemap.layout=hstrip"]);
    assert!(!codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
    let j = cube_src((48, 8), RGBA8, cube_dds, &["subtype=diffuse"]);
    assert_eq!(j.profile.subtype, Subtype::Diffuse);
    assert!(
        j.warnings
            .iter()
            .any(|w| w.code == Code::CfgInvalid && w.message.contains("cube DDS")),
        "{:?}",
        j.warnings
    );
}
