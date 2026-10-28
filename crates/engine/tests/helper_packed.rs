//! Helper textures Task 2 (Note DX): hand-set packed fill targets, `helper.read` and `helper.invert` on packed sources.
use engine::api::*;
use engine::config::*;
use engine::events::*;
use engine::helper::{FillTarget, HelperRead, HelperSel};
use engine::image::{Rgba32F, SourceInfo};
use engine::pack::assemble::assemble_channels_with;
use engine::pack::*;
use engine::policy::*;
use image::{ImageBuffer, Luma};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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

fn fills(ids: &[&str]) -> HelperSel {
    HelperSel {
        fills: Some(ids.iter().map(|t| FillTarget::parse(t).unwrap()).collect()),
        ..Default::default()
    }
}

fn sel(entries: Vec<(&str, HelperSel)>) -> BTreeMap<PathBuf, HelperSel> {
    entries
        .into_iter()
        .map(|(p, s)| (PathBuf::from(p), s))
        .collect()
}

fn src(f: &str, role: RawMap) -> ChannelNeed {
    Source {
        path: PathBuf::from(f),
        role,
        invert: false,
        channel: None,
    }
}

fn reason<'a>(u: &'a [Unused], f: &str) -> &'a str {
    &u.iter()
        .find(|u| u.path == Path::new(f))
        .unwrap_or_else(|| panic!("{f} is not unused: {u:?}"))
        .reason
}

#[test]
fn no_selection_is_byte_for_byte_recipes_for_with() {
    let roles: Vec<RawMap> = RawMap::INPUTS.into_iter().chain([Rmaos]).collect();
    let empty = BTreeMap::new();
    for mask in 0u32..(1 << roles.len()) {
        let mut files: Vec<RawInput> = roles
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, r)| RawInput {
                path: PathBuf::from(format!("x_{}.png", r.id())),
                role: *r,
            })
            .collect();
        // A second source of the first role (M13).
        if let Some(first) = files.first().cloned() {
            files.push(RawInput {
                path: PathBuf::from(format!("b_{}.png", first.role.id())),
                role: first.role,
            });
        }
        for target in [Target::Cs, Target::Vanilla] {
            for recipe in [PackRecipe::Auto, PackRecipe::Rmaos, PackRecipe::Rfaos] {
                for complex in [false, true] {
                    assert_eq!(
                        recipes_for_sel(target, &files, recipe, complex, &empty),
                        recipes_for_with(target, &files, recipe, complex),
                        "{target:?} {recipe:?} {complex} {files:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn hand_fills_move_roughness_to_rmaos_g() {
    let files = raw(&[("x_metallic.png", Metallic), ("x_roughness.png", Roughness)]);
    let (r, u) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_roughness.png", fills(&["rmaos.g"]))]),
    );
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].kind, RecipeKind::Rmaos);
    assert_eq!(r[0].channels[0], Missing(Slot::Roughness));
    assert_eq!(r[0].channels[1], src("x_roughness.png", Roughness));
    assert_eq!(
        reason(&u, "x_metallic.png"),
        "rmaos.g is filled by x_roughness.png"
    );
}

#[test]
fn hand_fill_creates_parallax() {
    let files = raw(&[("x_mask.png", Height)]);
    let s = HelperSel {
        role: Some(engine::helper::HelperRole::Raw(Height)),
        ..fills(&["p.r"])
    };
    let (r, u) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_mask.png", s)]),
    );
    assert!(u.is_empty(), "{u:?}");
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].kind, RecipeKind::Parallax);
    assert_eq!(r[0].channels[0], src("x_mask.png", Height));
}

#[test]
fn hand_fill_creates_rmaos_with_suggested_sources() {
    // A lone specular starts no `_rmaos`; a hand-set fill does, and the specular then takes its A as today.
    let files = raw(&[("x_mask.png", Height), ("x_specular.png", Specular)]);
    let (r, u) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_mask.png", fills(&["rmaos.b"]))]),
    );
    assert!(u.is_empty(), "{u:?}");
    assert_eq!(r[0].kind, RecipeKind::Rmaos);
    assert_eq!(r[0].channels[2], src("x_mask.png", Height));
    assert_eq!(r[0].channels[3], src("x_specular.png", Specular));
    assert_eq!(r[0].channels[0], Missing(Slot::Roughness));
}

#[test]
fn env_m_rejects_g() {
    let files = raw(&[("x_ao.png", Ao)]);
    let (r, u) = recipes_for_sel(
        Target::Vanilla,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_ao.png", fills(&["m.g"]))]),
    );
    assert!(r.is_empty(), "{r:?}");
    assert!(
        reason(&u, "x_ao.png").contains("the env-mask _m has only R"),
        "{u:?}"
    );
    // `rmaos.r` under vanilla is no target either.
    let (r, u) = recipes_for_sel(
        Target::Vanilla,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_ao.png", fills(&["rmaos.r"]))]),
    );
    assert!(r.is_empty(), "{r:?}");
    assert!(reason(&u, "x_ao.png").contains("rmaos.r"), "{u:?}");
}

#[test]
fn hand_fill_glossiness_into_roughness_is_inverted() {
    let files = raw(&[("x_mask.png", Glossiness), ("x_ao.png", Ao)]);
    let (r, _) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_mask.png", fills(&["rmaos.r"]))]),
    );
    assert_eq!(
        r[0].channels[0],
        Source {
            path: "x_mask.png".into(),
            role: Glossiness,
            invert: true,
            channel: None
        }
    );
}

#[test]
fn read_and_invert_apply_to_suggested_slot() {
    let files = raw(&[("x_gloss.png", Glossiness)]);
    let s = HelperSel {
        read: Some(HelperRead::G),
        invert: true,
        ..Default::default()
    };
    let (r, _) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_gloss.png", s)]),
    );
    assert_eq!(
        r[0].channels[0],
        Source {
            path: "x_gloss.png".into(),
            role: Glossiness,
            invert: false,
            channel: Some(1)
        }
    );
}

#[test]
fn two_assigned_helpers_first_by_path_wins() {
    let files = raw(&[("b_ao.png", Ao), ("a_ao.png", Ao)]);
    let (r, u) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![
            ("a_ao.png", fills(&["rmaos.b"])),
            ("b_ao.png", fills(&["rmaos.b"])),
        ]),
    );
    assert_eq!(r[0].channels[2], src("a_ao.png", Ao));
    assert_eq!(reason(&u, "b_ao.png"), "rmaos.b is filled by a_ao.png");
}

fn role(r: RawMap) -> HelperSel {
    HelperSel {
        role: Some(engine::helper::HelperRole::Raw(r)),
        ..Default::default()
    }
}

/// Fix round 1 (Ruling 8): a hand-set role is assigned and beats a name-guessed source of its role.
#[test]
fn hand_role_beats_name_guessed_source() {
    let files = raw(&[("x_ao.png", Ao), ("x_mask.png", Ao)]);
    let (r, u) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![("x_mask.png", role(Ao))]),
    );
    assert_eq!(r[0].channels[2], src("x_mask.png", Ao));
    assert!(reason(&u, "x_ao.png").contains("x_mask.png"), "{u:?}");
}

/// Fix round 1 (Ruling 8): a hand-set role and a hand-set fill on one channel are both assigned; first by path wins.
#[test]
fn hand_role_and_hand_fill_first_by_path_wins() {
    // The role sorts first: it keeps `rmaos.b`.
    let files = raw(&[("a_mask.png", Ao), ("b_mask.png", Height)]);
    let (r, u) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![
            ("a_mask.png", role(Ao)),
            ("b_mask.png", fills(&["rmaos.b"])),
        ]),
    );
    assert_eq!(r[0].channels[2], src("a_mask.png", Ao));
    assert_eq!(reason(&u, "b_mask.png"), "rmaos.b is filled by a_mask.png");
    // The fill sorts first: it takes `rmaos.b`.
    let files = raw(&[("a_mask.png", Height), ("b_mask.png", Ao)]);
    let (r, u) = recipes_for_sel(
        Target::Cs,
        &files,
        PackRecipe::Auto,
        false,
        &sel(vec![
            ("a_mask.png", fills(&["rmaos.b"])),
            ("b_mask.png", role(Ao)),
        ]),
    );
    assert_eq!(r[0].channels[2], src("a_mask.png", Height));
    assert_eq!(reason(&u, "b_mask.png"), "rmaos.b is filled by a_mask.png");
}

/// One in-memory pure-red 64x64 source assembled into R.
fn assemble_red(channel: Option<u8>) -> engine::pack::assemble::Assembled {
    let p = PathBuf::from("x_red.png");
    let channels = [
        ChannelSource::Source {
            path: p.clone(),
            role: Roughness,
            invert: false,
            channel,
        },
        ChannelSource::MirrorR,
        ChannelSource::MirrorR,
        ChannelSource::Opaque,
    ];
    let sources = [PackSource {
        read: None,
        path: p,
        role: Roughness,
        stamp: None,
    }];
    let info = SourceInfo {
        bit_depth: 8,
        channels: 3,
        has_alpha: false,
        is_float: false,
    };
    assemble_channels_with(
        &channels,
        &sources,
        GraySource::R,
        |_| Ok((64, 64)),
        |_| Ok((Rgba32F::new(64, 64, [1.0, 0.0, 0.0, 1.0]), info)),
    )
    .unwrap()
}

#[test]
fn luma_read_uses_luma_channel() {
    let a = assemble_red(Some(LUMA_CHANNEL));
    let r = a.image.px[0][0];
    assert!((r - 0.2126).abs() <= 1.0 / 255.0, "{r}");
    assert_eq!(
        ChannelSource::Source {
            path: "x_red.png".into(),
            role: Roughness,
            invert: false,
            channel: Some(LUMA_CHANNEL)
        }
        .provenance(GraySource::R),
        Some(engine::presets::Provenance::Source("x_red.png:luma".into()))
    );
}

#[test]
fn explicit_read_is_not_chan_not_gray() {
    assert!(assemble_red(Some(0)).non_gray.is_empty());
    assert_eq!(assemble_red(None).non_gray, [PathBuf::from("x_red.png")]);
}

fn gray(dir: &Path, name: &str) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
        .save(p)
        .unwrap();
}

fn plan(d: &Path, target: Target) -> Plan {
    let mut r = BatchRequest::new(vec![d.join("in")], d.join("out"), target);
    r.recursive = true;
    let rep = Reporter::new(Arc::new(VecSink::default()), "helper-packed");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap()
}

fn packed<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.pack.is_some() && j.output.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no packed {name}"))
}

#[test]
fn value_fill_applies_only_without_helper() {
    let d = tempfile::tempdir().unwrap();
    for f in ["x.png", "x_roughness.png", "x_metallic.png"] {
        gray(d.path(), f);
    }
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[files.\"x_rmaos\".packing.fill]\nmetallic = 200\n",
    )
    .unwrap();
    let p = plan(d.path(), Target::Cs);
    let ch = &packed(&p, "x_rmaos.dds").pack.as_ref().unwrap().channels;
    assert!(matches!(ch[1], ChannelSource::Source { .. }), "{ch:?}");
    std::fs::remove_file(d.path().join("in/x_metallic.png")).unwrap();
    let p = plan(d.path(), Target::Cs);
    let ch = &packed(&p, "x_rmaos.dds").pack.as_ref().unwrap().channels;
    assert_eq!(
        ch[1],
        ChannelSource::Fill {
            role: Metallic,
            code: 200
        }
    );
}

/// Fix round 1: `packing.recipe = rfaos` holds for an `_rfaos` only a hand-set fill builds (no source starts one).
#[test]
fn configured_rfaos_builds_for_hand_fill_alone() {
    for (file, key, ch) in [
        ("x_specular.png", "x_specular", 3usize),
        ("x_metallic.png", "x_metallic", 0),
    ] {
        let d = tempfile::tempdir().unwrap();
        for f in ["x.png", file] {
            gray(d.path(), f);
        }
        let target = ["rfaos.r", "rfaos.g", "rfaos.b", "rfaos.a"][ch];
        std::fs::write(
            d.path().join("in/sdc.toml"),
            format!(
                "[files.\"x_rfaos\".packing]\nrecipe = \"rfaos\"\n[files.\"{key}\"]\nhelper.fills = [\"{target}\"]\n"
            ),
        )
        .unwrap();
        let p = plan(d.path(), Target::Cs);
        let ch_src = &packed(&p, "x_rfaos.dds").pack.as_ref().unwrap().channels[ch];
        match ch_src {
            ChannelSource::Source { path, .. } => assert!(path.ends_with(file), "{path:?}"),
            other => panic!("{file}: {target} is {other:?}"),
        }
        assert!(
            !p.jobs
                .iter()
                .any(|j| j.pack.is_some() && j.output.ends_with("x_rmaos.dds")),
            "{file}: no _rmaos"
        );
    }
}

/// Final review I2: `if-changed` sees a helper moved to another packed channel, added to a second one, or taken off
/// one it shared; a default placement records nothing, so an unchanged project stays up to date. The steps use G:
/// a move into A (the review's input) already re-encoded before this fix by another path, so it is checked last.
#[test]
fn if_changed_sees_a_packed_channel_move() {
    let d = tempfile::tempdir().unwrap();
    for f in ["x_roughness.png", "x_ao.png"] {
        gray(d.path(), f);
    }
    let run = |fills: Option<&str>| {
        let toml = fills.map_or(String::new(), |f| {
            format!("[files.\"x_ao\"]\nhelper.fills = [{f}]\n")
        });
        std::fs::write(d.path().join("in/sdc.toml"), toml).unwrap();
        let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
        r.recursive = true;
        r.on_existing = OnExisting::IfChanged;
        let rep = Reporter::new(Arc::new(NullSink), "helper-packed");
        let e = Engine::new().unwrap();
        let p = e.plan(r, &rep, &CancelToken::default()).unwrap();
        let s = e.run(p, &rep, &CancelToken::default()).unwrap();
        (s.converted, s.up_to_date)
    };
    assert_eq!(run(None), (1, 0));
    assert_eq!(run(None), (0, 1), "nothing changed");
    let m = engine::manifest::Manifest::at(&d.path().join("out"))
        .load()
        .unwrap();
    let e = m.values().next().unwrap();
    assert!(
        e.sources.iter().all(|s| s.fill.is_none()),
        "a default placement records nothing: {:?}",
        e.sources
    );
    // A hand fill on the suggested channel builds the same output.
    assert_eq!(run(Some("\"rmaos.b\"")), (0, 1), "hand-set, same channel");
    assert_eq!(run(Some("\"rmaos.g\"")), (1, 0), "(a) B moved to G");
    assert_eq!(run(Some("\"rmaos.g\"")), (0, 1));
    assert_eq!(run(Some("\"rmaos.b\"")), (1, 0), "G moved back to B");
    assert_eq!(
        run(Some("\"rmaos.b\", \"rmaos.g\"")),
        (1, 0),
        "(b) a second channel"
    );
    assert_eq!(run(Some("\"rmaos.b\", \"rmaos.g\"")), (0, 1));
    assert_eq!(run(Some("\"rmaos.b\"")), (1, 0), "(c) Value on G");
    assert_eq!(run(None), (0, 1), "suggested again, same channel");
    assert_eq!(run(Some("\"rmaos.a\"")), (1, 0), "the review's B to A");
    assert_eq!(run(Some("\"rmaos.a\"")), (0, 1));
}

/// Final review I2: every recipe built without hand-set selectors records no placement, so existing manifests stay
/// up to date.
#[test]
fn suggested_recipes_record_no_placement() {
    let spec = PackingSpec {
        enabled: true,
        roughness_default: 0.5,
        prefer: PackPrefer::Prepacked,
        fill: FillSpec::default(),
        recipe: Default::default(),
    };
    let roles: Vec<RawMap> = RawMap::INPUTS.into_iter().chain([Rmaos]).collect();
    for mask in 0u32..(1 << roles.len()) {
        let files: Vec<RawInput> = roles
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, r)| RawInput {
                path: PathBuf::from(format!("x_{}.png", r.id())),
                role: *r,
            })
            .collect();
        for target in [Target::Cs, Target::Vanilla] {
            for recipe in [PackRecipe::Auto, PackRecipe::Rmaos, PackRecipe::Rfaos] {
                for complex in [false, true] {
                    for r in recipes_for_with(target, &files, recipe, complex).0 {
                        let pk = PackJob {
                            kind: r.kind,
                            channels: resolve_channels(&r, &spec).0,
                            gray: GraySource::R,
                            sources: r
                                .sources()
                                .into_iter()
                                .map(|(path, role)| PackSource {
                                    path,
                                    role,
                                    stamp: None,
                                    read: None,
                                })
                                .collect(),
                        };
                        for s in &pk.sources {
                            assert_eq!(
                                source_fill(&pk, s),
                                None,
                                "{target:?} {recipe:?} {complex} {s:?} in {pk:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn plan_honours_hand_set_fills() {
    let d = tempfile::tempdir().unwrap();
    for f in ["x.png", "x_roughness.png", "x_metallic.png"] {
        gray(d.path(), f);
    }
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[files.\"x_roughness\"]\nhelper.fills = [\"rmaos.g\"]\nhelper.read = \"luma\"\n",
    )
    .unwrap();
    let p = plan(d.path(), Target::Cs);
    let ch = &packed(&p, "x_rmaos.dds").pack.as_ref().unwrap().channels;
    match &ch[1] {
        ChannelSource::Source { path, channel, .. } => {
            assert!(path.ends_with("x_roughness.png"), "{path:?}");
            assert_eq!(*channel, Some(LUMA_CHANNEL));
        }
        other => panic!("G is {other:?}"),
    }
    assert!(
        !matches!(ch[0], ChannelSource::Source { .. }),
        "R lost its source: {ch:?}"
    );
}
