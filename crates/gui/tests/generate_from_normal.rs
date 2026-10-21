//! v2 Track B Task 5: the "Generate from normal map…" entries, their disabled reasons, the set's normal and the
//! `[generated]` provenance table of `sdc-project.toml`.
mod support;
use engine::config::{GreenConvention, Target};
use engine::from_normal::{HeightOpts, Neutral, Opts, Tiling};
use engine::shader::Shader;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::bulk::{MenuAction, MenuEntry};
use sdc_gui::model::checks;
use sdc_gui::model::from_normal::*;
use sdc_gui::model::sets::SlotKind;
use sdc_gui::model::AppModel;
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::*;

const ROCK_SET: &str = "landscape/rock01";
const LEAF_SET: &str = "landscape/leaf";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn land(d: &Path) -> PathBuf {
    d.join("textures/landscape")
}

/// Plan again after files changed on disk (unsaved edits stay).
fn reopen(m: &mut AppModel, _d: &Path) {
    m.replan();
    idle(m);
}

fn shader(m: &mut AppModel, s: Shader) {
    m.set_shader(ROCK_SET, Some(s));
    idle(m);
}

fn gen_open(menu: &[MenuEntry]) -> Option<&MenuEntry> {
    menu.iter().find(|e| {
        matches!(
            e.action,
            MenuAction::Generated {
                op: GenOp::Open(_),
                ..
            }
        )
    })
}

fn height(m: &AppModel, set: &str) -> Result<(), String> {
    m.gen_offer(set, GenMap::Height)
}

fn has_real(name: &str) -> Result<(), String> {
    Err(format!("this set already has a height map ({name})"))
}

/// A BC1 DDS of `w`×`h` written with the engine's DDS writer (blocks all zero).
fn bc1_dds(path: &Path, w: u32, h: u32) {
    use engine::config::Format;
    use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
    let d = DdsDesc {
        width: w,
        height: h,
        format: Format::Bc1,
        srgb: false,
        header: ResolvedHeader::Legacy,
    };
    let level = vec![0u8; (w.div_ceil(4) * h.div_ceil(4) * 8) as usize];
    std::fs::write(path, dds_bytes(&d, &[level]).unwrap()).unwrap();
}

#[test]
fn a_pbr_set_with_a_normal_and_no_height_offers_height_on_its_row_and_set_menu() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    let menu = m.slot_menu(ROCK_SET, SlotKind::Height);
    let row = gen_open(&menu).expect("the Height row offers Generate");
    assert_eq!(row, &m.gen_entry(ROCK_SET, Some(GenMap::Height)));
    assert_eq!(row.label, "Generate from normal map…");
    assert_eq!(row.disabled, None);
    assert_eq!(
        row.action,
        MenuAction::Generated {
            set: ROCK_SET.into(),
            op: GenOp::Open(Some(GenMap::Height))
        }
    );
    // P12: the entry is appended after the PGPatcher lock, so the existing rows keep their places.
    let at = |f: fn(&MenuAction) -> bool| menu.iter().position(|e| f(&e.action)).unwrap();
    assert!(
        at(|a| matches!(a, MenuAction::Generated { .. }))
            > at(|a| matches!(a, MenuAction::PgpLock { .. }))
    );
    let set = m.gen_entry(ROCK_SET, None);
    assert_eq!(set.disabled, None);
    assert_eq!(set.label, "Generate from normal map…");
    assert_eq!(
        set.action,
        MenuAction::Generated {
            set: ROCK_SET.into(),
            op: GenOp::Open(None)
        }
    );
    // Other rows do not offer it.
    assert!(gen_open(&m.slot_menu(ROCK_SET, SlotKind::Diffuse)).is_none());
    assert_eq!(
        m.set_normal(ROCK_SET).as_deref(),
        Some("landscape/rock01_normal.png")
    );
    assert_eq!(m.run_menu(set.action), None, "a no-op until the dialog");
}

#[test]
fn each_disabled_reason_shows_in_order() {
    // (a) a read-only sdc-project.toml comes first.
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("textures")).unwrap();
    std::fs::write(
        d.path().join("textures/sdc-project.toml"),
        "[[sets]\nbroken",
    )
    .unwrap();
    let m = opened(d.path());
    let r = height(&m, LEAF_SET).unwrap_err();
    assert!(r.starts_with("sdc-project.toml is read-only: "), "{r}");
    assert_eq!(m.gen_entry(LEAF_SET, None).disabled, Some(r));

    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    // (b) a locked slot 4, before the missing normal of the leaf set.
    m.set_slot_lock(LEAF_SET, 4, true).unwrap();
    m.set_slot_lock(ROCK_SET, 4, true).unwrap();
    idle(&mut m);
    assert_eq!(height(&m, ROCK_SET), Err("slot locked".into()));
    assert_eq!(height(&m, LEAF_SET), Err("slot locked".into()));
    m.set_slot_lock(LEAF_SET, 4, false).unwrap();
    m.set_slot_lock(ROCK_SET, 4, false).unwrap();
    idle(&mut m);
    // (c) no normal.
    assert_eq!(height(&m, LEAF_SET), Err(NO_NORMAL.into()));
    assert_eq!(m.gen_entry(LEAF_SET, None).disabled, Some(NO_NORMAL.into()));
    // (d) the shader rules.
    shader(&mut m, Shader::Multilayer);
    assert_eq!(height(&m, ROCK_SET), Err(MULTILAYER.into()));
    shader(&mut m, Shader::Glow);
    assert_eq!(
        height(&m, ROCK_SET),
        Err("a height map would switch this set from Glow to Parallax".into())
    );
    shader(&mut m, Shader::Landscape);
    assert_eq!(height(&m, ROCK_SET), Err(VANILLA_LANDSCAPE.into()));
    shader(&mut m, Shader::PbrGrass);
    assert_eq!(
        height(&m, ROCK_SET),
        Err("PBR grass reads no height map".into())
    );
    // (e) complex material reads the `_m` alpha, packed only on vanilla.
    shader(&mut m, Shader::ComplexMaterial);
    assert_eq!(height(&m, ROCK_SET), Err(COMPLEX_CS.into()));
    shader(&mut m, Shader::Parallax);
    assert_eq!(height(&m, ROCK_SET), Ok(()));
    // (f) a real `_p`.
    ImageBuffer::from_fn(64, 64, |_, _| Luma([128u8]))
        .save(land(d.path()).join("rock01_p.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(height(&m, ROCK_SET), has_real("rock01_p.png"));

    // (e) on vanilla: the packed `rock01_m` member (from rock01_roughness.png) is no `_m` file.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    idle(&mut m);
    complex_material(&mut m, "rock01");
    assert!(
        sdc_gui::model::sets::views(&m)
            .iter()
            .find(|v| v.id == ROCK_SET)
            .unwrap()
            .members
            .iter()
            .any(|k| k == "landscape/rock01_m"),
        "the fixture packs a complex _m"
    );
    assert_eq!(height(&m, ROCK_SET), Ok(()));
    assert_eq!(
        m.gen_size(ROCK_SET, GenMap::Height),
        Ok(GenSize::Locked(64)),
        "the _m alpha matches its recipe's size (P6)"
    );
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([0u8, 0, 0]))
        .save(land(d.path()).join("rock01_m.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(
        height(&m, ROCK_SET),
        Err(
            "this set has a pre-packed _m (rock01_m.png); its alpha already decides the height"
                .into()
        )
    );

    // (g) a generated height of the other kind (an earlier `_m`-alpha choice) blocks a `_p`.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    shader(&mut m, Shader::Default);
    assert_eq!(height(&m, ROCK_SET), Ok(()));
    ImageBuffer::from_fn(64, 64, |_, _| Luma([128u16]))
        .save(land(d.path()).join("rock01_height_generated.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(
        height(&m, ROCK_SET),
        has_real("rock01_height_generated.png")
    );
    assert_eq!(
        m.generated_items(ROCK_SET),
        vec![("landscape/rock01_height_generated.png".to_string(), None)]
    );
}

#[test]
fn a_generated_height_without_provenance_is_unrecorded_and_a_recorded_one_regenerates() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    ImageBuffer::from_fn(64, 64, |_, _| Luma([128u16]))
        .save(land(d.path()).join("rock01_p_generated.png"))
        .unwrap();
    reopen(&mut m, d.path());
    let key = "landscape/rock01_p_generated.png";
    assert_eq!(
        height(&m, ROCK_SET),
        Err(
            "rock01_p_generated.png exists but was not generated here; delete it to generate again"
                .to_string()
        )
    );
    let doc = m.sets_doc.as_mut().unwrap();
    doc.set_generated(key, Some(&serde_json::to_value(provenance()).unwrap()));
    assert_eq!(height(&m, ROCK_SET), Ok(()));
    assert_eq!(
        m.generated_items(ROCK_SET),
        vec![(key.into(), Some(provenance()))]
    );
    assert_eq!(
        m.gen_entry(ROCK_SET, Some(GenMap::Height)).label,
        "Regenerate…"
    );
    assert_eq!(m.gen_entry(ROCK_SET, None).label, "Regenerate…");
    // A real `_p` replaces it: the engine's GEN-REPLACED names the rival.
    ImageBuffer::from_fn(64, 64, |_, _| Luma([128u8]))
        .save(land(d.path()).join("rock01_p.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(height(&m, ROCK_SET), has_real("rock01_p.png"));
    assert_eq!(
        m.generated_items(ROCK_SET).len(),
        1,
        "found by set key though no longer a member (P5)"
    );
}

#[test]
fn an_npot_normal_disables_the_entry_with_its_size() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    ImageBuffer::from_fn(48, 48, |_, _| Rgb([128u8, 128, 255]))
        .save(land(d.path()).join("rock01_normal.png"))
        .unwrap();
    reopen(&mut m, d.path());
    let why = "the normal map is 48×48; only power-of-two sizes can be generated";
    assert_eq!(height(&m, ROCK_SET), Err(why.into()));
    let row = m.slot_menu(ROCK_SET, SlotKind::Height);
    assert_eq!(gen_open(&row).unwrap().disabled.as_deref(), Some(why));
    // Review M5: the size comes before the shader rules.
    shader(&mut m, Shader::Glow);
    assert_eq!(height(&m, ROCK_SET), Err(why.into()));
}

#[test]
fn an_unreadable_dds_normal_says_so() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_normal.png")).unwrap();
    std::fs::write(land(d.path()).join("rock01_n.dds"), [7u8; 16]).unwrap();
    reopen(&mut m, d.path());
    shader(&mut m, Shader::Default);
    assert_eq!(
        height(&m, ROCK_SET),
        Err("the normal map rock01_n.dds cannot be read".into())
    );
}

#[test]
fn a_height_under_a_user_alias_is_a_real_rival() {
    // Review M1: an alias names only a §6.1 suffix (`hgt = "height"` is CFG-INVALID), so an aliased height is a
    // `_p` slot file with a job, and its canonical suffix comes from the plan's alias use.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    let presets = d.path().join("presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(
        presets.join("mine.toml"),
        "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\n[suffix_aliases]\nhgt = \"p\"\n",
    )
    .unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([x as u8]))
        .save(land(d.path()).join("rock01_hgt.png"))
        .unwrap();
    m.user_presets = Some(presets);
    m.set_preset(Some("mine".into()));
    reopen(&mut m, d.path());
    assert!(
        m.item("landscape/rock01_hgt.png")
            .and_then(|i| i.job.as_ref()?.suffix_alias.as_ref())
            .is_some(),
        "the engine reads the alias"
    );
    assert_eq!(height(&m, ROCK_SET), has_real("rock01_hgt.png"));
}

#[test]
fn a_dds_normal_is_the_sets_normal() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_normal.png")).unwrap();
    bc1_dds(&land(d.path()).join("rock01_n.dds"), 64, 64);
    reopen(&mut m, d.path());
    assert_eq!(
        m.set_normal(ROCK_SET).as_deref(),
        Some("landscape/rock01_n.dds")
    );
    // `_msn` (model-space) never counts.
    std::fs::remove_file(land(d.path()).join("rock01_n.dds")).unwrap();
    bc1_dds(&land(d.path()).join("rock01_msn.dds"), 64, 64);
    reopen(&mut m, d.path());
    assert_eq!(m.set_normal(ROCK_SET), None);
}

#[test]
fn height_neutral_maps_track_cs_rule() {
    // P8: one rule, Track C's; `tests/checks.rs` pins its flags (`_cnr` included).
    let coat: sdc_gui::pgp::settings::Settings = [
        ("multilayer".to_string(), json!(true)),
        ("coat_parallax".to_string(), json!(true)),
    ]
    .into();
    for (s, flags) in [
        (Some(Shader::Pbr), &coat),
        (Some(Shader::Multilayer), &Default::default()),
        (Some(Shader::Parallax), &Default::default()),
        (None, &Default::default()),
    ] {
        let want = match checks::height_neutral(s, flags) {
            checks::HeightNeutral::Half => Some(Neutral::Half),
            checks::HeightNeutral::White => Some(Neutral::White),
            checks::HeightNeutral::NotHeight => None,
        };
        assert_eq!(height_neutral(s, flags), want, "{s:?}");
    }
    assert_eq!(
        height_neutral(Some(Shader::Pbr), &coat),
        Some(Neutral::White)
    );
    assert_eq!(
        height_neutral(Some(Shader::Multilayer), &Default::default()),
        None
    );
    assert_eq!(
        height_neutral(Some(Shader::Parallax), &Default::default()),
        Some(Neutral::Half)
    );
}

#[test]
fn output_names_follow_the_map() {
    assert_eq!(
        output_name("rock01", GenMap::Height, false),
        "rock01_p_generated.png"
    );
    assert_eq!(
        output_name("rock01", GenMap::Height, true),
        "rock01_height_generated.png"
    );
    assert_eq!(
        output_name("rock01", GenMap::Ao, false),
        "rock01_ao_generated.png"
    );
    assert_eq!(
        output_name("rock01", GenMap::Curvature, false),
        "rock01_curves_generated.png"
    );
}

#[test]
fn a_dds_normals_size_is_read_once() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_normal.png")).unwrap();
    let dds = land(d.path()).join("rock01_n.dds");
    bc1_dds(&dds, 48, 48);
    reopen(&mut m, d.path());
    shader(&mut m, Shader::Default);
    let why = Err("the normal map is 48×48; only power-of-two sizes can be generated".to_string());
    assert_eq!(height(&m, ROCK_SET), why);
    assert_eq!(height(&m, ROCK_SET), why);
    assert_eq!(m.gen_dims_len(), 1, "one header read, cached");
    bc1_dds(&dds, 64, 64);
    // Review M2: within the 1 s TTL the cached answer stands without a stat.
    assert_eq!(height(&m, ROCK_SET), why);
    m.clear_gen_dims();
    assert_eq!(height(&m, ROCK_SET), Ok(()), "a new stamp is read again");
    assert_eq!(m.gen_dims_len(), 1, "one entry per file");
}

fn harness(m: AppModel) -> egui_kittest::Harness<'static, AppModel> {
    let h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(1500.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Right-click set `id`'s card; the card menu opens.
fn card_menu(h: &mut egui_kittest::Harness<'static, AppModel>, id: &str) {
    use egui_kittest::kittest::Queryable;
    let card = |h: &egui_kittest::Harness<'static, AppModel>| {
        h.query_all_by_label(id)
            .find(|n| in_sets_column(h, n.rect()))
            .unwrap_or_else(|| panic!("{id}'s card"))
            .rect()
    };
    let r = card(h);
    h.hover_at(r.center());
    h.run();
    h.query_all_by_label(id)
        .find(|n| in_sets_column(h, n.rect()))
        .unwrap()
        .click_secondary();
    h.run();
}

const GENERATE: &str = "Generate from normal map…";

#[test]
fn the_set_card_menu_offers_generate_with_its_reason() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened_plain(d.path()));
    h.run();
    card_menu(&mut h, ROCK_SET);
    assert!(!h.get_by_label(GENERATE).accesskit_node().is_disabled());
    h.get_by_label(GENERATE).click();
    h.run();
    assert_eq!(
        h.state().gen_requested,
        Some((ROCK_SET.to_string(), None)),
        "the set entry's own action"
    );
    assert!(h.query_by_label(GENERATE).is_none(), "the menu closed");
    // The leaf set has no normal: off, and the hover says why.
    card_menu(&mut h, LEAF_SET);
    let b = h.get_by_label(GENERATE);
    assert!(b.accesskit_node().is_disabled());
    let r = b.rect();
    h.hover_at(r.center());
    h.run();
    assert!(h.query_by_label(NO_NORMAL).is_some(), "the reason on hover");
    assert_eq!(h.state().gen_requested, Some((ROCK_SET.to_string(), None)));

    // A read-only project: off (with the menu's own reason).
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    std::fs::write(
        m.root.clone().unwrap().join("sdc-project.toml"),
        "[[sets]\nbroken",
    )
    .unwrap();
    m.reload_documents();
    idle(&mut m);
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    let mut h = harness(m);
    h.run();
    card_menu(&mut h, ROCK_SET);
    assert!(h.get_by_label(GENERATE).accesskit_node().is_disabled());
}

#[test]
fn a_cs_pbr_height_may_follow_a_larger_albedo_up_to_2048() {
    // OWNER-Q1: elsewhere the free offer is `sizes(normal, HEIGHT_MAX)`.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    shader(&mut m, Shader::Default);
    assert_eq!(
        m.gen_size(ROCK_SET, GenMap::Height),
        Ok(GenSize::Free {
            sizes: vec![64],
            default: 64
        })
    );
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    let big = |n: u32, name: &str| {
        ImageBuffer::from_fn(n, n, |x, y| Rgb([(x % 251) as u8, (y % 251) as u8, 200u8]))
            .save(land(d.path()).join(name))
            .unwrap();
    };
    big(2048, "rock01.png");
    big(2048, "rock01_normal.png");
    reopen(&mut m, d.path());
    assert_eq!(
        m.gen_size(ROCK_SET, GenMap::Height),
        Ok(GenSize::Free {
            sizes: vec![256, 512, 1024, 2048],
            default: 2048
        })
    );
    shader(&mut m, Shader::Parallax);
    assert_eq!(
        m.gen_size(ROCK_SET, GenMap::Height),
        Ok(GenSize::Free {
            sizes: vec![256, 512, 1024],
            default: 1024
        })
    );
}

#[test]
fn a_complex_m_alpha_smaller_normal_is_size_locked() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    idle(&mut m);
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([128u8, 128, 255]))
        .save(land(d.path()).join("rock01_normal.png"))
        .unwrap();
    reopen(&mut m, d.path());
    complex_material(&mut m, "rock01");
    assert_eq!(
        height(&m, ROCK_SET),
        Err("this set's maps are 64×64; a generated height map must match them (the normal is 32×32)".into())
    );
}

fn provenance() -> Provenance {
    Provenance {
        from: "landscape/rock01_normal.png".into(),
        from_sha256: "ab".repeat(32),
        sha256: "cd".repeat(32),
        kind: GenMap::Height,
        m_alpha: false,
        opts: Opts {
            size: 64,
            tiling: Tiling::Auto,
            relief: 0.3,
            green: Some(GreenConvention::Directx),
            height: Some(HeightOpts {
                strength: 0.75,
                neutral: Neutral::Half,
            }),
            ao: None,
            curvature: false,
        },
    }
}

#[test]
fn the_generated_table_round_trips_and_older_files_load() {
    let d = tempfile::tempdir().unwrap();
    let older = "[[sets]]\nid = \"landscape/rock01\"\nshader = \"pbr\"\n";
    std::fs::write(d.path().join("sdc-project.toml"), older).unwrap();
    let mut doc = SetsDoc::open(d.path());
    assert!(
        doc.generated().is_empty(),
        "a file without [generated] loads"
    );
    let v = serde_json::to_value(provenance()).unwrap();
    let key = "landscape/rock01_p_generated.png";
    doc.set_generated(key, Some(&v));
    doc.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    assert!(text.contains("[generated]"), "{text}");
    assert!(!text.contains("null"), "{text}");
    let mut doc = SetsDoc::open(d.path());
    assert_eq!(doc.generated(), [(key.to_string(), v.clone())].into());
    let back: Provenance = serde_json::from_value(doc.generated()[key].clone()).unwrap();
    assert_eq!(back, provenance());
    // A set edit leaves the table alone.
    doc.set_shader("landscape/rock01", Some(Shader::Parallax));
    assert_eq!(doc.generated(), [(key.to_string(), v.clone())].into());
    // Undo/redo to text from before the record keeps it (it describes a file on disk).
    doc.restore_keeping_json_sha(older);
    assert_eq!(doc.generated(), [(key.to_string(), v.clone())].into());
    // Keys match by Unicode lowercase; `None` removes.
    let mut w = v.clone();
    w["sha256"] = json!("ee".repeat(32));
    doc.set_generated("Landscape/ROCK01_P_GENERATED.png", Some(&w));
    assert_eq!(doc.generated().len(), 1);
    assert_eq!(doc.generated().values().next(), Some(&w));
    doc.set_generated(key, None);
    assert!(doc.generated().is_empty());
}
