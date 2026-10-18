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
        channel: None,
    }
}

/// The recipes of a vanilla set whose shader is complex material (the old cpm target, Note AZ).
fn complex(raw: &[RawInput]) -> (Vec<Recipe>, Vec<Unused>) {
    recipes_for_with(Target::Vanilla, raw, PackRecipe::Auto, true)
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
fn vanilla_complex_material_packs_a_complex_m() {
    let (r, u) = complex(&raw(&[
        ("iron_rough.png", Roughness),
        ("iron_ao.png", Ao),
        ("iron_fuzz.png", Fuzz),
    ]));
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
    let (r, u) = complex(&raw(&[
        ("iron_metal.png", Metallic),
        ("iron_spec.png", Specular),
    ]));
    assert_eq!(r[0].channels[0], src("iron_metal.png", Metallic, false));
    assert_eq!(r[0].channels[2], src("iron_metal.png", Metallic, false));
    assert_eq!(r[0].sources().len(), 1);
    assert_eq!(unused_names(&u), ["iron_spec.png"]);
    let (r, _) = complex(&raw(&[
        ("iron_spec.png", Specular),
        ("iron_height.png", Height),
    ]));
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
        predicted_suffixes(Target::Cs, false, &set(&[Ao, Height])),
        ["rmaos", "p"]
    );
    assert_eq!(
        predicted_suffixes(Target::Cs, false, &set(&[Fuzz, Ao])),
        ["rfaos"]
    );
    assert_eq!(
        predicted_suffixes(Target::Vanilla, true, &set(&[Height])),
        ["m"]
    );
    assert!(predicted_suffixes(Target::Vanilla, false, &set(&[Height])).is_empty());
}

fn packing() -> PackingSpec {
    PackingSpec {
        enabled: true,
        roughness_default: 0.5,
        prefer: PackPrefer::Prepacked,
        fill: FillSpec::default(),
        recipe: Default::default(),
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
    // The complex _m with a roughness source: G is that source inverted, so only env mask and height take defaults (Note M1).
    let (r, _) = complex(&raw(&[("iron_rough.png", Roughness)]));
    let (ch, _) = resolve_channels(&r[0], &packing());
    assert_eq!(ch[0], ChannelSource::Default { value: 0.0 });
    assert_eq!(ch[3], ChannelSource::Default { value: 0.0 });
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

/// Spec §6.4 defaults table: glossiness defaults to 1 - roughness_default; provenance shows the exact decimal.
#[test]
fn glossiness_default_is_one_minus_roughness_default() {
    let (r, _) = complex(&raw(&[("iron_height.png", Height)]));
    let mut p = packing();
    p.roughness_default = 0.6;
    let (ch, defaulted) = resolve_channels(&r[0], &p);
    assert_eq!(ch[1], ChannelSource::Default { value: 0.4 });
    assert_eq!(
        ch[1].provenance(GraySource::R),
        Some(Provenance::Default("0.4".into()))
    );
    assert!(defaulted.contains(&(1, Slot::Glossiness)), "{defaulted:?}");
}

/// DECISIONS Note M5: a roughness fill feeds a glossiness channel inverted (255 - v).
#[test]
fn roughness_fill_feeds_a_glossiness_channel_inverted() {
    let (r, _) = complex(&raw(&[("iron_height.png", Height)]));
    let mut p = packing();
    p.fill.roughness = Some(51);
    let (ch, defaulted) = resolve_channels(&r[0], &p);
    assert_eq!(
        ch[1],
        ChannelSource::Fill {
            role: Roughness,
            code: 204
        }
    );
    assert_eq!(
        ch[1].provenance(GraySource::R),
        Some(Provenance::Fill("204".into()))
    );
    assert!(!defaulted.iter().any(|(i, _)| *i == 1), "{defaulted:?}");
}

/// Spec §6.4 defaults table: AO defaults to 1.0 (no occlusion).
#[test]
fn ao_default_is_one() {
    let (r, _) = recipes_for(Target::Cs, &raw(&[("iron_metal.png", Metallic)]));
    let (ch, defaulted) = resolve_channels(&r[0], &packing());
    assert_eq!(ch[2], ChannelSource::Default { value: 1.0 });
    assert_eq!(
        ch[2].provenance(GraySource::R),
        Some(Provenance::Default("1".into()))
    );
    assert!(defaulted.contains(&(2, Slot::Ao)), "{defaulted:?}");
}

/// DECISIONS Note M12.
#[test]
fn channel_provenance_strings() {
    let s = |path: &str, invert| ChannelSource::Source {
        path: PathBuf::from(format!("C:/in/{path}")),
        role: Roughness,
        invert,
        channel: None,
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

fn orm(f: &str, channel: u8, invert: bool) -> ChannelNeed {
    Source {
        path: PathBuf::from(f),
        role: Orm,
        invert,
        channel: Some(channel),
    }
}

/// DECISIONS Note AL1: an `_orm` supplies AO (R), roughness (G) and metallic (B) to every recipe that reads them.
#[test]
fn an_orm_feeds_ao_roughness_and_metallic() {
    let set = raw(&[("iron_orm.png", Orm)]);
    let (r, u) = recipes_for(Target::Cs, &set);
    assert!(u.is_empty());
    assert_eq!(
        r[0].channels,
        [
            orm("iron_orm.png", 1, false),
            orm("iron_orm.png", 2, false),
            orm("iron_orm.png", 0, false),
            Missing(Slot::Specular),
        ]
    );
    assert!(r[0].notes.is_empty());
    let (r, _) = complex(&set);
    assert_eq!(
        r[0].channels,
        [
            orm("iron_orm.png", 2, false),
            orm("iron_orm.png", 1, true),
            orm("iron_orm.png", 2, false),
            Missing(Slot::Height),
        ]
    );
    let (r, _) = recipes_for(Target::Vanilla, &set);
    assert_eq!(r[0].channels[0], orm("iron_orm.png", 2, false));
    let (r, _) = recipes_for_with(Target::Cs, &set, PackRecipe::Rfaos, false);
    assert_eq!(kinds(&r), [RecipeKind::Rfaos]);
    assert_eq!(
        predicted_suffixes(Target::Cs, false, &BTreeSet::from([Orm])),
        ["rmaos"]
    );
}

/// DECISIONS Note AL2: a loose map wins its slot over the ORM channel, with a note; a fully overridden ORM is unused.
#[test]
fn a_loose_map_wins_over_the_orm_channel() {
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[("iron_orm.png", Orm), ("iron_rough.png", Roughness)]),
    );
    assert!(u.is_empty());
    assert_eq!(r[0].channels[0], src("iron_rough.png", Roughness, false));
    assert_eq!(r[0].channels[1], orm("iron_orm.png", 2, false));
    assert_eq!(r[0].notes.len(), 1);
    assert!(
        r[0].notes[0].contains("iron_rough.png"),
        "{}",
        r[0].notes[0]
    );
    let (r, u) = recipes_for(
        Target::Vanilla,
        &raw(&[("iron_orm.png", Orm), ("iron_metal.png", Metallic)]),
    );
    assert_eq!(r[0].channels[0], src("iron_metal.png", Metallic, false));
    assert_eq!(unused_names(&u), ["iron_orm.png"]);
    assert!(u[0].reason.contains("loose"), "{}", u[0].reason);
    // the ORM's metallic beats a loose specular for the env mask, and the reason says so
    for cm in [true, false] {
        let (_, u) = recipes_for_with(
            Target::Vanilla,
            &raw(&[("iron_orm.png", Orm), ("iron_spec.png", Specular)]),
            PackRecipe::Auto,
            cm,
        );
        assert_eq!(unused_names(&u), ["iron_spec.png"]);
        assert!(
            u[0].reason.contains("env mask comes from the metallic"),
            "{}",
            u[0].reason
        );
    }
}

/// DECISIONS Note AL5: `_rfaos` has no metallic channel, so the ORM's B is dropped with a note.
#[test]
fn rfaos_notes_the_dropped_orm_metallic() {
    let (r, _) = recipes_for(
        Target::Cs,
        &raw(&[("iron_orm.png", Orm), ("iron_fuzz.png", Fuzz)]),
    );
    assert_eq!(kinds(&r), [RecipeKind::Rfaos]);
    assert_eq!(r[0].notes.len(), 1, "{:?}", r[0].notes);
    assert!(r[0].notes[0].contains("metallic"), "{}", r[0].notes[0]);
    // an `_rmaos` from the same ORM drops nothing
    let (r, _) = recipes_for(Target::Cs, &raw(&[("iron_orm.png", Orm)]));
    assert!(r[0].notes.is_empty());
}

/// Note AZ (ruling 3): under vanilla a gloss/height set packs the complex `_m` only when its shader is complex
/// material; otherwise nothing packs (the env-mask `_m` needs metallic or specular) and the maps are unused.
#[test]
fn vanilla_packs_the_complex_m_only_for_a_complex_material_set() {
    let set = raw(&[("iron_gloss.png", Glossiness), ("iron_height.png", Height)]);
    let (r, u) = recipes_for(Target::Vanilla, &set);
    assert!(r.is_empty());
    assert_eq!(unused_names(&u), ["iron_gloss.png", "iron_height.png"]);
    let (r, u) = complex(&set);
    assert_eq!(kinds(&r), [RecipeKind::CpmM]);
    assert!(u.is_empty());
    let roles = BTreeSet::from([Glossiness, Height]);
    assert!(predicted_suffixes(Target::Vanilla, false, &roles).is_empty());
    assert_eq!(predicted_suffixes(Target::Vanilla, true, &roles), ["m"]);
    // cs ignores the flag.
    assert_eq!(
        kinds(&recipes_for_with(Target::Cs, &set, PackRecipe::Auto, true).0),
        [RecipeKind::Rmaos, RecipeKind::Parallax]
    );
}

/// Audit 08a-4: a packed output is `<base>_<suffix>` whether its first source is a vocabulary-named raw map or
/// already carries a §6.1 suffix (a pre-packed `_rmaos` used as a channel source).
#[test]
fn virtual_stem_strips_a_vocabulary_term_or_a_suffix() {
    use engine::pack::plan::virtual_stem;
    use std::path::Path;
    assert_eq!(
        virtual_stem(Path::new("rock_roughness.png"), RecipeKind::Rmaos),
        "rock_rmaos"
    );
    assert_eq!(
        virtual_stem(Path::new("a/rock_rmaos.png"), RecipeKind::Rmaos),
        "rock_rmaos"
    );
    assert_eq!(
        virtual_stem(Path::new("rock.png"), RecipeKind::Rmaos),
        "rock_rmaos"
    );
}
