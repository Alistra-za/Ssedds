//! M2.5 Task 3: which recipes a set's raw maps make per target, and what each channel holds (spec §6.4).
use engine::config::*;
use engine::pack::*;
use engine::presets::Provenance;
use std::collections::BTreeSet;
use std::path::PathBuf;
use ChannelNeed::*;
use RawMap::*;

fn raw(files: &[(&str, RawMap)]) -> Vec<RawInput> {
    files
        .iter()
        .map(|(f, r)| RawInput {
            path: PathBuf::from(f),
            role: *r,
        })
        .collect()
}

fn src(f: &str, role: RawMap, invert: bool) -> ChannelNeed {
    Source {
        path: PathBuf::from(f),
        role,
        invert,
    }
}

fn kinds(r: &[Recipe]) -> Vec<RecipeKind> {
    r.iter().map(|r| r.kind).collect()
}

fn unused_names(u: &[Unused]) -> Vec<String> {
    u.iter()
        .map(|u| u.path.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn cs_packs_rmaos_and_p_from_a_full_set() {
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[
            ("iron_roughness.png", Roughness),
            ("iron_metallic.png", Metallic),
            ("iron_ao.png", Ao),
            ("iron_specular.png", Specular),
            ("iron_height.png", Height),
        ]),
    );
    assert_eq!(kinds(&r), [RecipeKind::Rmaos, RecipeKind::Parallax]);
    assert_eq!(
        r[0].channels,
        [
            src("iron_roughness.png", Roughness, false),
            src("iron_metallic.png", Metallic, false),
            src("iron_ao.png", Ao, false),
            src("iron_specular.png", Specular, false),
        ]
    );
    assert_eq!(
        r[1].channels,
        [
            src("iron_height.png", Height, false),
            MirrorR,
            MirrorR,
            Opaque
        ]
    );
    assert!(u.is_empty(), "{u:?}");
    assert_eq!(r[0].sources().len(), 4);
}

/// Spec §6.4 (owner): missing roughness does not block when an AO or metallic source exists.
#[test]
fn cs_missing_roughness_does_not_block() {
    for only in [("iron_ao.png", Ao), ("iron_metallic.png", Metallic)] {
        let (r, _) = recipes_for(Target::Cs, &raw(&[only]));
        assert_eq!(kinds(&r), [RecipeKind::Rmaos], "{only:?}");
        assert_eq!(r[0].channels[0], Missing(Slot::Roughness));
        assert_eq!(r[0].channels[3], Missing(Slot::Specular));
    }
}

#[test]
fn cs_glossiness_is_inverted_and_roughness_wins_over_it() {
    let (r, u) = recipes_for(Target::Cs, &raw(&[("iron_gloss.png", Glossiness)]));
    assert_eq!(r[0].channels[0], src("iron_gloss.png", Glossiness, true));
    assert!(u.is_empty());
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[
            ("iron_gloss.png", Glossiness),
            ("iron_rough.png", Roughness),
        ]),
    );
    assert_eq!(r[0].channels[0], src("iron_rough.png", Roughness, false));
    assert_eq!(unused_names(&u), ["iron_gloss.png"]);
    assert!(u[0].reason.contains("roughness is used"), "{}", u[0].reason);
}

#[test]
fn cs_fuzz_makes_rfaos_and_metallic_is_unused() {
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[
            ("iron_rough.png", Roughness),
            ("iron_fuzz.png", Fuzz),
            ("iron_metal.png", Metallic),
        ]),
    );
    assert_eq!(kinds(&r), [RecipeKind::Rfaos]);
    assert_eq!(r[0].channels[1], src("iron_fuzz.png", Fuzz, false));
    assert_eq!(unused_names(&u), ["iron_metal.png"]);
    assert!(
        u[0].reason.contains("no metallic channel"),
        "{}",
        u[0].reason
    );
}

/// DECISIONS Note M2: specular-only and fuzz-only cs sets, and opacity anywhere, pack nothing.
#[test]
fn sets_without_a_trigger_pack_nothing() {
    for files in [
        vec![("iron_spec.png", Specular)],
        vec![("iron_fuzz.png", Fuzz)],
        vec![("iron_opacity.png", Opacity)],
    ] {
        let (r, u) = recipes_for(Target::Cs, &raw(&files));
        assert!(r.is_empty(), "{files:?}");
        assert_eq!(u.len(), 1, "{files:?}");
    }
    let (_, u) = recipes_for(
        Target::Cs,
        &raw(&[("iron_ao.png", Ao), ("iron_opacity.png", Opacity)]),
    );
    assert_eq!(unused_names(&u), ["iron_opacity.png"]);
    assert!(u[0].reason.contains("opacity"), "{}", u[0].reason);
}

#[test]
fn cpm_packs_a_complex_m() {
    let (r, u) = recipes_for(
        Target::Cpm,
        &raw(&[
            ("iron_rough.png", Roughness),
            ("iron_ao.png", Ao),
            ("iron_fuzz.png", Fuzz),
        ]),
    );
    assert_eq!(kinds(&r), [RecipeKind::CpmM]);
    assert_eq!(
        r[0].channels,
        [
            Missing(Slot::EnvMask),
            src("iron_rough.png", Roughness, true),
            Missing(Slot::Metallic),
            Missing(Slot::Height),
        ]
    );
    assert_eq!(unused_names(&u), ["iron_ao.png", "iron_fuzz.png"]);
    // Note M1: env mask from metallic (also B), else specular.
    let (r, u) = recipes_for(
        Target::Cpm,
        &raw(&[("iron_metal.png", Metallic), ("iron_spec.png", Specular)]),
    );
    assert_eq!(r[0].channels[0], src("iron_metal.png", Metallic, false));
    assert_eq!(r[0].channels[2], src("iron_metal.png", Metallic, false));
    assert_eq!(r[0].sources().len(), 1);
    assert_eq!(unused_names(&u), ["iron_spec.png"]);
    let (r, _) = recipes_for(
        Target::Cpm,
        &raw(&[("iron_spec.png", Specular), ("iron_height.png", Height)]),
    );
    assert_eq!(r[0].channels[0], src("iron_spec.png", Specular, false));
}

#[test]
fn vanilla_packs_an_env_mask_only_from_a_source() {
    let (r, u) = recipes_for(Target::Vanilla, &raw(&[("iron_metal.png", Metallic)]));
    assert_eq!(kinds(&r), [RecipeKind::VanillaM]);
    assert_eq!(
        r[0].channels,
        [
            src("iron_metal.png", Metallic, false),
            MirrorR,
            MirrorR,
            Opaque
        ]
    );
    assert!(u.is_empty());
    let (r, u) = recipes_for(Target::Vanilla, &raw(&[("iron_height.png", Height)]));
    assert!(r.is_empty());
    assert_eq!(unused_names(&u), ["iron_height.png"]);
}

/// Review Focus 2, DECISIONS Note M13.
#[test]
fn a_second_source_of_one_role_is_unused() {
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[
            ("iron_roughness.png", Roughness),
            ("iron_rough.png", Roughness),
        ]),
    );
    assert_eq!(r[0].channels[0], src("iron_rough.png", Roughness, false));
    assert_eq!(unused_names(&u), ["iron_roughness.png"]);
    assert!(u[0].reason.contains("iron_rough.png"), "{}", u[0].reason);
}

/// DECISIONS Note M7.
#[test]
fn predicted_suffixes_follow_the_recipes() {
    let set = |r: &[RawMap]| r.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        predicted_suffixes(Target::Cs, &set(&[Ao, Height])),
        ["rmaos", "p"]
    );
    assert_eq!(predicted_suffixes(Target::Cs, &set(&[Fuzz, Ao])), ["rfaos"]);
    assert_eq!(predicted_suffixes(Target::Cpm, &set(&[Height])), ["m"]);
    assert!(predicted_suffixes(Target::Vanilla, &set(&[Height])).is_empty());
}

fn packing() -> PackingSpec {
    PackingSpec {
        enabled: true,
        roughness_default: 0.5,
        prefer: PackPrefer::Prepacked,
        fill: FillSpec::default(),
    }
}

#[test]
fn missing_channels_take_neutral_defaults_or_fills() {
    let (r, _) = recipes_for(Target::Cs, &raw(&[("iron_ao.png", Ao)]));
    let (ch, defaulted) = resolve_channels(&r[0], &packing());
    assert_eq!(ch[0], ChannelSource::Default { value: 0.5 });
    assert_eq!(ch[1], ChannelSource::Default { value: 0.0 });
    assert_eq!(ch[3], ChannelSource::Default { value: 1.0 });
    assert_eq!(
        defaulted,
        [
            (0, Slot::Roughness),
            (1, Slot::Metallic),
            (3, Slot::Specular)
        ]
    );
    // Note M5: a fill replaces the default and is not reported as defaulted.
    let mut p = packing();
    p.fill.metallic = Some(255);
    p.roughness_default = 0.7;
    let (ch, defaulted) = resolve_channels(&r[0], &p);
    assert_eq!(
        ch[1],
        ChannelSource::Fill {
            role: Metallic,
            code: 255
        }
    );
    assert_eq!(ch[0], ChannelSource::Default { value: 0.7 });
    assert_eq!(defaulted, [(0, Slot::Roughness), (3, Slot::Specular)]);
    // A glossiness fill feeds a roughness channel inverted.
    let mut p = packing();
    p.fill.glossiness = Some(51);
    let (ch, _) = resolve_channels(&r[0], &p);
    assert_eq!(
        ch[0],
        ChannelSource::Fill {
            role: Glossiness,
            code: 204
        }
    );
    // cpm: glossiness default is 1 - roughness_default; env mask and height defaults (Note M1).
    let (r, _) = recipes_for(Target::Cpm, &raw(&[("iron_rough.png", Roughness)]));
    let (ch, _) = resolve_channels(&r[0], &packing());
    assert_eq!(ch[0], ChannelSource::Default { value: 0.0 });
    assert_eq!(ch[3], ChannelSource::Default { value: 0.5 });
    let mut p = packing();
    p.fill.metallic = Some(40);
    let (ch, _) = resolve_channels(&r[0], &p);
    assert_eq!(
        ch[0],
        ChannelSource::Fill {
            role: Metallic,
            code: 40
        }
    );
}

/// DECISIONS Note M12.
#[test]
fn channel_provenance_strings() {
    let s = |path: &str, invert| ChannelSource::Source {
        path: PathBuf::from(format!("C:/in/{path}")),
        role: Roughness,
        invert,
    };
    assert_eq!(
        s("iron_rough.png", false).provenance(GraySource::R),
        Some(Provenance::Source("iron_rough.png:R".into()))
    );
    assert_eq!(
        s("iron_gloss.png", true).provenance(GraySource::R),
        Some(Provenance::Source("1-iron_gloss.png:R".into()))
    );
    assert_eq!(
        s("iron_rough.png", false).provenance(GraySource::Luma),
        Some(Provenance::Source("iron_rough.png:luma".into()))
    );
    assert_eq!(
        ChannelSource::Default { value: 0.5 }.provenance(GraySource::R),
        Some(Provenance::Default("0.5".into()))
    );
    assert_eq!(
        ChannelSource::Fill {
            role: Metallic,
            code: 255
        }
        .provenance(GraySource::R),
        Some(Provenance::Fill("255".into()))
    );
    assert_eq!(ChannelSource::MirrorR.provenance(GraySource::R), None);
    assert_eq!(ChannelSource::Opaque.provenance(GraySource::R), None);
}
