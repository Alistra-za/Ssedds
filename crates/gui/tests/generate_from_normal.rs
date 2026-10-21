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
use sdc_gui::model::{AppModel, Dialog};
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
    assert_eq!(m.run_menu(set.action), None);
    assert_eq!(
        m.dialog,
        Some(Dialog::GenerateFromNormal(ROCK_SET.into())),
        "the entry opens the dialog"
    );
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
    h.run_steps(3);
    assert_eq!(
        h.state().dialog,
        Some(Dialog::GenerateFromNormal(ROCK_SET.to_string())),
        "the set entry's own action opens the dialog"
    );
    let g = h.state().gen.as_ref().expect("the dialog state");
    assert_eq!(g.set, ROCK_SET);
    assert!(g.ticked.is_empty(), "the set entry pre-ticks nothing");
    assert!(h.query_by_label(GENERATE).is_none(), "the menu closed");
    h.key_press(eframe::egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    // The leaf set has no normal: off, and the hover says why.
    card_menu(&mut h, LEAF_SET);
    let b = h.get_by_label(GENERATE);
    assert!(b.accesskit_node().is_disabled());
    let r = b.rect();
    h.hover_at(r.center());
    h.run();
    assert!(h.query_by_label(NO_NORMAL).is_some(), "the reason on hover");
    assert!(h.state().gen.is_none(), "a disabled entry opens nothing");

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

// ---- Task 6: the dialog, preview and Generate ----

const P_KEY: &str = "landscape/rock01_p_generated.png";

/// `pbr_opened_plain` with a 64² relief normal in place of the flat one.
fn relief_rock(d: &Path) -> AppModel {
    let mut m = pbr_opened_plain(d);
    relief_normal(&land(d), "rock01_normal.png", 64);
    reopen(&mut m, d);
    m
}

fn generate_height(m: &mut AppModel) {
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    idle(m);
}

fn rock_view(m: &AppModel) -> sdc_gui::model::sets::SetView {
    sdc_gui::model::sets::views(m)
        .iter()
        .find(|v| v.id == ROCK_SET)
        .expect("the rock set")
        .clone()
}

fn generated_pngs(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("_generated"))
        .collect()
}

fn regen_label(m: &AppModel) -> String {
    gen_open(&m.slot_menu(ROCK_SET, SlotKind::Height))
        .expect("the Height row's entry")
        .label
        .clone()
}

#[test]
fn generate_writes_a_16_bit_p_next_to_the_normal_and_records_provenance() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    let p = land(d.path()).join("rock01_p_generated.png");
    let img = image::open(&p).expect("the generated file").into_luma16();
    assert_eq!(img.dimensions(), (64, 64), "the normal's size (< 256)");
    let codes: Vec<u16> = img.pixels().map(|p| p.0[0]).collect();
    let (lo, hi) = (
        (8.0 / 255.0 * 65535.0f64).round() as u16,
        (247.0 / 255.0 * 65535.0f64).round() as u16,
    );
    assert!(codes.iter().all(|&c| (lo..=hi).contains(&c)));
    let distinct: std::collections::BTreeSet<u16> = codes.iter().copied().collect();
    assert!(distinct.len() > 1000, "{} codes", distinct.len());
    let mean = codes.iter().map(|&c| f64::from(c)).sum::<f64>() / codes.len() as f64 / 65535.0;
    assert!((mean - 0.5).abs() <= 0.02, "mean {mean}");
    let rec = &m.sets_doc.as_ref().unwrap().generated()[P_KEY];
    assert_eq!(rec["from"], "landscape/rock01_normal.png");
    assert_eq!(rec["kind"], "height");
    assert_eq!(rec["opts"]["height"]["neutral"], "half");
    assert_eq!(
        rec["sha256"],
        sdc_gui::model::history::sha256_of(&p).unwrap().as_str()
    );
    assert_eq!(
        m.selection,
        sdc_gui::model::Selection::Item(P_KEY.into()),
        "selected after the re-plan"
    );
    assert_eq!(
        rock_view(&m).slots.get(&SlotKind::Height),
        Some(&sdc_gui::model::sets::SlotFill::Own { item: P_KEY.into() })
    );
    assert_eq!(regen_label(&m), "Regenerate…");
    assert!(m.gen.is_none() && m.dialog.is_none(), "the dialog closed");

    // The flat fixture normal gives a constant Half.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    generate_height(&mut m);
    let img = image::open(land(d.path()).join("rock01_p_generated.png"))
        .unwrap()
        .into_luma16();
    assert!(img.pixels().all(|p| p.0[0] == 32768));
}

#[test]
fn cancel_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    let before = m.sets_doc.as_ref().unwrap().text();
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_cancel();
    idle(&mut m);
    assert!(m.gen.is_none() && m.dialog.is_none());
    assert!(generated_pngs(&land(d.path())).is_empty());
    assert_eq!(m.sets_doc.as_ref().unwrap().text(), before);
    // Cancel after `generate` but before the first write (spec-S9).
    let t = land(d.path()).join("rock01_p_generated.png");
    let cancel = std::sync::atomic::AtomicBool::new(true);
    assert!(write_all(&[(t.clone(), vec![1, 2, 3], None)], &cancel).is_err());
    assert!(!t.exists());
    let go = std::sync::atomic::AtomicBool::new(false);
    let shas = write_all(&[(t.clone(), vec![1, 2, 3], None)], &go).unwrap();
    assert_eq!(std::fs::read(&t).unwrap(), [1, 2, 3]);
    assert_eq!(shas, vec![sdc_gui::model::history::sha256_of(&t).unwrap()]);
}

#[test]
fn write_all_checks_every_target_first_and_rolls_back() {
    let d = tempfile::tempdir().unwrap();
    let (a, b) = (d.path().join("a.png"), d.path().join("b.png"));
    std::fs::write(&b, b"theirs").unwrap();
    let go = std::sync::atomic::AtomicBool::new(false);
    let e = write_all(
        &[(a.clone(), vec![1], None), (b.clone(), vec![2], None)],
        &go,
    )
    .unwrap_err();
    assert_eq!(
        e,
        format!("{} already exists; nothing was written", b.display())
    );
    assert!(!a.exists(), "nothing written");
    assert_eq!(std::fs::read(&b).unwrap(), b"theirs");
    // Our own recorded file is replaced; an edited one is not.
    let own = sdc_gui::model::history::sha256_of(&b);
    write_all(&[(b.clone(), vec![2], own)], &go).unwrap();
    assert_eq!(std::fs::read(&b).unwrap(), [2]);
    let e = write_all(&[(b.clone(), vec![3], Some("00".repeat(32)))], &go).unwrap_err();
    assert!(e.contains("b.png was edited since it was generated"), "{e}");
    // A write that fails removes the files this call wrote.
    let dir_target = d.path().join("adir");
    std::fs::create_dir(&dir_target).unwrap();
    let c = d.path().join("c.png");
    assert!(write_all(
        &[(c.clone(), vec![1], None), (dir_target, vec![1], None)],
        &go
    )
    .is_err());
    assert!(!c.exists(), "rolled back");
}

#[test]
fn nothing_ticked_disables_generate() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, None);
    assert!(m.gen.as_ref().unwrap().ticked.is_empty());
    m.gen_generate();
    assert!(m.gen.as_ref().unwrap().running.is_none());
    assert!(!m.gen_pending());
    assert!(generated_pngs(&land(d.path())).is_empty());
    assert!(!sdc_gui::ui::from_normal::gen_buttons(false, false)[1].2);
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(4);
    let b = h.get_by_label("Generate");
    assert!(b.accesskit_node().is_disabled());
    let r = b.rect();
    h.hover_at(r.center());
    h.run_steps(30);
    assert!(
        h.query_by_label(NOTHING_TICKED).is_some(),
        "the reason on hover"
    );
}

#[test]
fn generate_refuses_when_a_target_appeared_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    let before = m.sets_doc.as_ref().unwrap().text();
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    let t = land(d.path()).join("rock01_p_generated.png");
    std::fs::write(&t, b"made by hand").unwrap();
    m.gen_generate();
    idle(&mut m);
    let g = m.gen.as_ref().expect("the dialog stays open");
    assert!(
        g.error
            .as_deref()
            .is_some_and(|e| e.ends_with("already exists; nothing was written")),
        "{:?}",
        g.error
    );
    assert!(matches!(
        m.dialog,
        Some(sdc_gui::model::Dialog::GenerateFromNormal(_))
    ));
    assert_eq!(std::fs::read(&t).unwrap(), b"made by hand");
    assert!(m.sets_doc.as_ref().unwrap().generated().is_empty());
    assert_eq!(m.sets_doc.as_ref().unwrap().text(), before);
}

#[test]
fn undo_after_generate_keeps_the_provenance() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    let steps = m.history.undo.len();
    generate_height(&mut m);
    assert_eq!(m.history.undo.len(), steps, "no undo step for the file");
    let p = land(d.path()).join("rock01_p_generated.png");
    m.undo();
    idle(&mut m);
    assert!(p.exists());
    assert!(m.sets_doc.as_ref().unwrap().generated().contains_key(P_KEY));
    assert_eq!(regen_label(&m), "Regenerate…");

    // A moved-in normal: the generated file joins the set by membership, and undo removes the membership only.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_normal.png")).unwrap();
    relief_normal(&land(d.path()), "stone_normal.png", 64);
    reopen(&mut m, d.path());
    m.edit_docs("Move into", None, true, |_, s| {
        s.add_member(ROCK_SET, "landscape/stone_normal.png")
    });
    idle(&mut m);
    assert_eq!(
        m.set_normal(ROCK_SET).as_deref(),
        Some("landscape/stone_normal.png")
    );
    let steps = m.history.undo.len();
    generate_height(&mut m);
    let key = "landscape/stone_p_generated.png";
    assert!(rock_view(&m).members.iter().any(|k| k == key), "a member");
    assert_eq!(
        m.history.undo.len(),
        steps + 1,
        "the membership is one step"
    );
    m.undo();
    idle(&mut m);
    assert!(land(d.path()).join("stone_p_generated.png").exists());
    assert!(m.sets_doc.as_ref().unwrap().generated().contains_key(key));
    assert!(!rock_view(&m).members.iter().any(|k| k == key));
}

#[test]
fn the_paths_and_the_size_warning_show_before_generate() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    relief_normal(&land(d.path()), "rock01_normal.png", 1024);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(
        m.gen.as_ref().unwrap().opts.size,
        1024,
        "the normal's own size"
    );
    assert_eq!(
        m.gen_paths(),
        vec![(
            GenMap::Height,
            land(d.path()).join("rock01_p_generated.png"),
            (1024, 1024)
        )]
    );
    assert_eq!(
        m.gen_size_warning(),
        Some("Above 512 px, parallax costs more GPU time for little visible gain")
    );
    m.gen.as_mut().unwrap().opts.size = 512;
    assert_eq!(m.gen_size_warning(), None);
    assert_eq!(m.gen_paths()[0].2, (512, 512));
}

/// The first file named `name` under `dir`.
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()? {
        let p = e.ok()?.path();
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
}

#[test]
fn use_as_m_alpha_writes_height_generated_on_a_vanilla_complex_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    idle(&mut m);
    relief_normal(&land(d.path()), "rock01_normal.png", 64);
    reopen(&mut m, d.path());
    complex_material(&mut m, "rock01");
    assert_eq!(height(&m, ROCK_SET), Ok(()));
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert!(m.gen.as_ref().unwrap().m_alpha);
    assert_eq!(
        m.gen_paths(),
        vec![(
            GenMap::Height,
            land(d.path()).join("rock01_height_generated.png"),
            (64, 64)
        )]
    );
    m.gen_generate();
    idle(&mut m);
    assert!(land(d.path()).join("rock01_height_generated.png").exists());
    export_through(&mut m);
    idle(&mut m);
    let out = find(&d.path().join("export"), "rock01_m.dds").expect("rock01_m.dds exported");
    let (img, _) = engine::load::load_image(&out).unwrap();
    let a: Vec<f32> = img.px.iter().map(|p| p[3] * 255.0).collect();
    assert!(
        a.iter().all(|&v| v > 4.0 && v < 251.0),
        "alpha inside (4, 251)"
    );
    let (lo, hi) = a
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    assert!(hi - lo > 8.0, "not constant: {lo}..{hi}");
}

/// An uncompressed 8-bit DDS of unit normals `n` (`size`²), green stored flipped when `gl`.
fn normal_dds(path: &Path, n: &[[f64; 3]], size: u32, gl: bool) {
    use engine::config::Format;
    use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
    let d = DdsDesc {
        width: size,
        height: size,
        format: Format::Rgba8,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    let level: Vec<u8> = n
        .iter()
        .flat_map(|&[x, y, z]| {
            let [r, g, b] = normal_rgb([x, if gl { -y } else { y }, z]);
            // The engine's `Rgba8` is DXGI 87, B8G8R8A8.
            [b, g, r, 255]
        })
        .collect();
    std::fs::write(path, dds_bytes(&d, &[level]).unwrap()).unwrap();
}

/// The engine's asymmetric `blobs` fixture (128²): elongated diagonal Gaussians.
fn blob_normals() -> Vec<[f64; 3]> {
    let w = 128usize;
    let hf: Vec<f64> = (0..w * w)
        .map(|i| {
            let (x, y) = ((i % w) as f64, (i / w) as f64);
            let mut v = 0.0;
            for (cx, cy, a) in [(40.0, 40.0, 5.0), (90.0, 70.0, -4.0), (50.0, 100.0, 3.0)] {
                let (u, t) = (
                    (x - cx + y - cy) / 2f64.sqrt(),
                    (x - cx - y + cy) / 2f64.sqrt(),
                );
                v += a * (-(u * u) / (2.0 * 16.0 * 16.0) - t * t / (2.0 * 4.0 * 4.0)).exp();
            }
            v
        })
        .collect();
    slopes_to_normals(&hf, w, 2.0)
}

fn preview(m: &mut AppModel) {
    m.gen_frame();
    idle(m);
    let g = m.gen.as_ref().unwrap();
    assert!(g.preview.is_some(), "{:?}", g.error);
}

#[test]
fn a_dds_normal_generates_with_green_detected() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_normal.png")).unwrap();
    let dds = land(d.path()).join("rock01_n.dds");
    normal_dds(&dds, &blob_normals(), 128, true);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(m.gen.as_ref().unwrap().opts.green, None);
    preview(&mut m);
    assert_eq!(
        m.gen_format_line(),
        "Normal: looks OpenGL (detected)",
        "{:?}",
        m.gen
            .as_ref()
            .unwrap()
            .preview
            .as_ref()
            .unwrap()
            .maps
            .report
    );
    m.gen_generate();
    idle(&mut m);
    assert!(land(d.path()).join("rock01_p_generated.png").exists());
    // A flat DDS proves nothing about detection (spec-S7).
    let flat = vec![[0.0, 0.0, 1.0]; 128 * 128];
    std::fs::remove_file(land(d.path()).join("rock01_p_generated.png")).unwrap();
    m.sets_doc.as_mut().unwrap().set_generated(P_KEY, None);
    normal_dds(&dds, &flat, 128, false);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    preview(&mut m);
    assert_eq!(
        m.gen_format_line(),
        "Normal: green undetermined, assumed DirectX"
    );
    m.gen_cancel();
    // The name states the convention.
    std::fs::remove_file(&dds).unwrap();
    normal_dds(
        &land(d.path()).join("rock01_normalgl.dds"),
        &flat,
        128,
        false,
    );
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(
        m.gen.as_ref().unwrap().opts.green,
        Some(GreenConvention::Opengl)
    );
    assert_eq!(m.gen_format_line(), "Normal: OpenGL (from the file name)");
}

#[test]
fn an_image_normals_green_comes_from_its_settings() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(
        m.gen.as_ref().unwrap().opts.green,
        Some(GreenConvention::Directx)
    );
    assert_eq!(
        m.gen_format_line(),
        "Normal: DirectX (from the normal's settings)"
    );
    // `opened` states no convention: IN-GREEN-UNSTATED, so auto.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(m.gen.as_ref().unwrap().opts.green, None);
}

fn gen_harness(m: AppModel, w: f32, h: f32) -> egui_kittest::Harness<'static, AppModel> {
    let h = egui_kittest::Harness::builder()
        .with_size(eframe::egui::vec2(w, h))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_dialog_draws_and_previews_off_the_ui_thread() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(3);
    assert_eq!(
        h.state().gen.as_ref().unwrap().started,
        1,
        "one preview job"
    );
    assert!(h.state().gen_pending(), "on a worker");
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(5);
    assert!(h.query_by_label("Generate").is_some());
    assert!(
        h.query_by_label("Height preview").is_some(),
        "a preview tile texture"
    );
    assert!(h.query_by_label_contains("Fit: mean").is_some());
    assert_eq!(h.state().gen.as_ref().unwrap().started, 1, "settled");
    // While a slider drags, a changed setting starts nothing.
    let m = h.state_mut();
    let g = m.gen.as_mut().unwrap();
    g.dragging = true;
    g.opts.relief = 0.7;
    m.gen_frame();
    assert_eq!(m.gen.as_ref().unwrap().started, 1);
    m.gen.as_mut().unwrap().dragging = false;
    m.gen_frame();
    assert_eq!(m.gen.as_ref().unwrap().started, 2, "released: it previews");
}

#[test]
fn enter_never_generates_and_a_click_outside_keeps_the_dialog() {
    use eframe::egui::{pos2, Event, Key, Modifiers, PointerButton};
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(3);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let g = h.state().gen.as_ref().expect("still open");
    assert!(!matches!(g.running, Some((_, false))), "no Generate job");
    assert!(generated_pngs(&land(d.path())).is_empty());
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos: pos2(5.0, 890.0),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(3);
    assert!(h.state().gen.is_some(), "a click outside keeps it");
    assert!(h.query_by_label("Generate").is_some());
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(
        h.state().gen.is_none() && h.state().dialog.is_none(),
        "Esc closes"
    );
    assert!(generated_pngs(&land(d.path())).is_empty());
    // Neither footer button is the accent-filled Enter default.
    for on in [false, true] {
        for b in sdc_gui::ui::from_normal::gen_buttons(on, false) {
            assert_eq!(b.1, sdc_gui::ui::dialogs::Role::Plain);
        }
    }
}

#[test]
fn the_footer_stays_on_screen_at_1280x800() {
    use egui_kittest::kittest::Queryable;
    for w in [1280.0, 1100.0] {
        let d = tempfile::tempdir().unwrap();
        let mut m = relief_rock(d.path());
        m.open_gen(ROCK_SET, Some(GenMap::Height));
        let mut h = gen_harness(m, w, 800.0);
        h.run_steps(3);
        assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
        h.run_steps(5);
        let win = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(w, 800.0),
        );
        for b in ["Generate", "Cancel"] {
            let r = h.get_by_label(b).rect();
            assert!(win.contains_rect(r), "{b} at {r:?} in {w}×800");
        }
    }
}

#[test]
fn generate_jobs_are_polled_without_greying_advanced() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    assert!(m.gen_pending() && m.v2_jobs_pending());
    assert!(!m.busy(), "Advanced stays live");
    idle(&mut m);
    assert!(land(d.path()).join("rock01_p_generated.png").exists());
    assert!(!m.gen_pending());
}
