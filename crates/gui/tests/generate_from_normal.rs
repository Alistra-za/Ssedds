//! v2 Track B Task 5: the "Generate from normal map…" entries, their disabled reasons, the set's normal and the
//! `[generated]` provenance table of `sdc-project.toml`.
mod support;
use engine::config::{GreenConvention, Target};
use engine::from_normal::{HeightOpts, Neutral, Opts, Tiling};
use engine::shader::Shader;
use image::{ImageBuffer, Luma, Rgb, Rgba};
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

/// The open menu's entry. The set editor's Generate button has the same accessible name, but its text is the short
/// "Generate…", so the menu entry is the widest match.
fn menu_generate<'a>(
    h: &'a egui_kittest::Harness<'static, AppModel>,
) -> Option<egui_kittest::Node<'a>> {
    use egui_kittest::kittest::Queryable;
    h.query_all_by_label(GENERATE)
        .find(|n| n.rect().width() > 120.0)
}

#[test]
fn the_set_card_menu_offers_generate_with_its_reason() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened_plain(d.path()));
    h.run();
    card_menu(&mut h, ROCK_SET);
    assert!(!menu_generate(&h).unwrap().accesskit_node().is_disabled());
    menu_generate(&h).unwrap().click();
    h.run_steps(3);
    assert_eq!(
        h.state().dialog,
        Some(Dialog::GenerateFromNormal(ROCK_SET.to_string())),
        "the set entry's own action opens the dialog"
    );
    let g = h.state().gen.as_ref().expect("the dialog state");
    assert_eq!(g.set, ROCK_SET);
    assert!(g.ticked.is_empty(), "the set entry pre-ticks nothing");
    assert!(menu_generate(&h).is_none(), "the menu closed");
    h.key_press(eframe::egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    // The leaf set has no normal: off, and the hover says why.
    card_menu(&mut h, LEAF_SET);
    let b = menu_generate(&h).unwrap();
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
    assert!(menu_generate(&h).unwrap().accesskit_node().is_disabled());
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
    assert!(
        m.sets_doc.as_ref().unwrap().is_dirty(),
        "the unsaved shader pick"
    );
    generate_height(&mut m);
    // P14: unsaved edits are the user's; the record waits for the next Save.
    assert_eq!(m.status_note.as_deref(), Some(PROVENANCE_DEFERRED));
    assert!(m.status_line.starts_with(PROVENANCE_DEFERRED));
    assert!(m.sets_doc.as_ref().unwrap().is_dirty());
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
    // A write that fails (its folder is a file: absent to the check, unwritable) removes the files this call wrote.
    std::fs::write(d.path().join("afile"), b"x").unwrap();
    let bad = d.path().join("afile").join("x.png");
    let c = d.path().join("c.png");
    let e = write_all(
        &[(c.clone(), vec![1], None), (bad.clone(), vec![1], None)],
        &go,
    )
    .unwrap_err();
    assert!(
        e.starts_with(&bad.display().to_string()),
        "the write failed: {e}"
    );
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
    // M5: the `_m` alpha follows the `_m`'s size at no extra parallax cost: no cost warning at any size.
    m.gen.as_mut().unwrap().opts.size = 1024;
    assert_eq!(m.gen_size_warning(), None);
    m.gen.as_mut().unwrap().opts.size = 64;
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
    assert!(!m.sets_doc.as_ref().unwrap().is_dirty());
    m.gen_generate();
    idle(&mut m);
    assert!(land(d.path()).join("rock01_p_generated.png").exists());
    // P14: a clean sdc-project.toml is saved at once with the record.
    let saved = std::fs::read_to_string(m.root.clone().unwrap().join("sdc-project.toml")).unwrap();
    assert!(saved.contains("[generated]"), "{saved}");
    assert_eq!(m.status_note, None);
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
        // P17: Height, AO and Curvature ticked, so the AO group and three preview tiles grow the body.
        let mut m = relief_rock(d.path());
        std::fs::remove_file(land(d.path()).join("rock01_ao.png")).unwrap();
        reopen(&mut m, d.path());
        m.open_gen(ROCK_SET, Some(GenMap::Height));
        m.gen_tick(GenMap::Ao, true);
        m.gen_tick(GenMap::Curvature, true);
        let mut h = gen_harness(m, w, 800.0);
        h.run_steps(3);
        assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
        // The tallest state: a refusal naming a long path below the body (CO61).
        let long = land(d.path()).join(format!("{}rock01_p_generated.png", "deep/".repeat(30)));
        h.state_mut().gen.as_mut().unwrap().error = Some(format!(
            "{} already exists; nothing was written",
            long.display()
        ));
        h.run_steps(5);
        assert!(
            h.query_by_label("AO preview").is_some(),
            "the AO group and tile"
        );
        assert!(h.query_by_label("Curvature preview").is_some());
        let win = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(w, 800.0),
        );
        for b in ["Generate", "Cancel"] {
            let r = h.get_by_label(b).rect();
            assert!(win.contains_rect(r), "{b} at {r:?} in {w}×800");
        }
        // The refusal sits between the scrolled body and the buttons, never scrolled away under them (CO61).
        let e = h
            .get_by_label_contains("already exists; nothing was written")
            .rect();
        let g = h.get_by_label("Generate").rect();
        assert!(
            win.contains_rect(e) && e.max.y <= g.min.y,
            "the refusal at {e:?}, Generate at {g:?} in {w}×800"
        );
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

// ---- Task 6 fix round 1 ----

/// `opened` (a clean sdc-project.toml) with a 64² relief normal.
fn relief_clean(d: &Path) -> AppModel {
    let mut m = opened(d);
    relief_normal(&land(d), "rock01_normal.png", 64);
    reopen(&mut m, d);
    assert!(!m.sets_doc.as_ref().unwrap().is_dirty());
    m
}

fn project_text(root: &Path) -> String {
    std::fs::read_to_string(root.join("sdc-project.toml")).unwrap_or_default()
}

/// Wait without polling until the job has written `p` (the worker passed its hash, its decode and the cancel check).
fn until_written(p: &Path) {
    let end = std::time::Instant::now() + Duration::from_secs(60);
    while !p.exists() {
        assert!(
            std::time::Instant::now() < end,
            "{} never written",
            p.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_generate_never_records_into_another_project() {
    // I1: the project changed under a running Generate (a direct open, as the plan-failure fallback does).
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut m = relief_clean(da.path());
    let root_a = m.root.clone().unwrap();
    let root_b = mod_folder(db.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    // Past the cancel check, so the open's cancel cannot stop it and it lands with a written file.
    until_written(&land(da.path()).join("rock01_p_generated.png"));
    m.open_folder(root_b.clone());
    idle(&mut m);
    assert_eq!(m.root.as_ref(), Some(&root_b));
    assert!(
        m.sets_doc.as_ref().unwrap().generated().is_empty(),
        "not in B"
    );
    assert!(!project_text(&root_b).contains("[generated]"));
    assert!(!project_text(&root_a).contains("[generated]"), "not in A");
    // Said on the status line (the next plan's count replaces it) and kept in Messages.
    assert!(
        m.messages.entries().any(|e| e.text.contains("unrecorded")),
        "said so"
    );

    // Open and close wait for a running Generate, as they wait for an export.
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut m = relief_clean(da.path());
    let root_a = m.root.clone().unwrap();
    let root_b = mod_folder(db.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    assert!(!m.request_open(root_b.clone()), "waits");
    assert_eq!(m.root.as_ref(), Some(&root_a));
    idle(&mut m);
    assert!(project_text(&root_a).contains("[generated]"), "A recorded");
    assert_eq!(m.root.as_ref(), Some(&root_b), "then B opened");
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_clean(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    assert!(!m.request_close(), "waits");
    assert!(!m.take_close_ready());
    idle(&mut m);
    assert!(project_text(&m.root.clone().unwrap()).contains("[generated]"));
    assert!(m.take_close_ready(), "then closes");
    // A drop opens through `request_open`, so it waits too.
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut m = relief_clean(da.path());
    let root_b = mod_folder(db.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    assert!(!m.dropped(std::slice::from_ref(&root_b)));
    idle(&mut m);
    assert_eq!(m.root.as_ref(), Some(&root_b));
}

#[test]
fn the_record_hashes_the_normal_the_job_read() {
    // I2: a normal re-saved while the job runs leaves the record stale.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_clean(d.path());
    let normal = land(d.path()).join("rock01_normal.png");
    let read = sdc_gui::model::history::sha256_of(&normal).unwrap();
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    until_written(&land(d.path()).join("rock01_p_generated.png"));
    relief_normal(&land(d.path()), "rock01_normal.png", 32);
    assert_ne!(sdc_gui::model::history::sha256_of(&normal).unwrap(), read);
    idle(&mut m);
    let rec = &m.sets_doc.as_ref().unwrap().generated()[P_KEY];
    assert_eq!(rec["from_sha256"], read.as_str());
}

#[test]
fn a_cancel_after_writing_started_still_records() {
    // I3 (CO50).
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_clean(d.path());
    let p = land(d.path()).join("rock01_p_generated.png");
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    until_written(&p);
    m.gen_cancel();
    assert!(m.dialog.is_none(), "closed");
    assert!(m.gen_pending(), "the job still lands");
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(
        m.status_line,
        "Generate is still finishing; try again in a moment"
    );
    assert!(m.dialog.is_none(), "refused while it lands");
    idle(&mut m);
    assert!(m.gen.is_none());
    assert!(m.sets_doc.as_ref().unwrap().generated().contains_key(P_KEY));
    // Cancelled at once: either nothing was written, or what was written is recorded.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_clean(d.path());
    let p = land(d.path()).join("rock01_p_generated.png");
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    m.gen_cancel();
    idle(&mut m);
    assert!(m.gen.is_none());
    assert_eq!(
        p.exists(),
        m.sets_doc.as_ref().unwrap().generated().contains_key(P_KEY)
    );
}

#[test]
fn the_disabled_generate_says_why() {
    // M1.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_clean(d.path());
    m.open_gen(ROCK_SET, None);
    assert_eq!(m.gen_generate_hover(), NOTHING_TICKED);
    m.gen.as_mut().unwrap().ticked.insert(GenMap::Height);
    m.gen_frame();
    assert_eq!(m.gen_generate_hover(), PREVIEW_RUNNING);
    idle(&mut m);
    m.gen_generate();
    assert_eq!(m.gen_generate_hover(), GENERATING);
    idle(&mut m);
}

#[test]
fn the_deferred_note_goes_with_save_and_open() {
    // M2.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    assert_eq!(m.status_note.as_deref(), Some(PROVENANCE_DEFERRED));
    m.save().unwrap();
    assert_eq!(m.status_note, None);
    assert!(project_text(&m.root.clone().unwrap()).contains("[generated]"));
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    m.save().unwrap();
    m.status_note = Some(PROVENANCE_DEFERRED.into());
    let other = tempfile::tempdir().unwrap();
    m.open_folder(mod_folder(other.path()));
    assert_eq!(m.status_note, None);
}

#[test]
fn an_error_after_cancel_is_shown() {
    // M3: a Generate that failed after its dialog was cancelled says so.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_clean(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen.as_mut().unwrap().running = Some((999, false));
    m.gen_cancel();
    assert!(m.gen.is_some(), "kept for its landing");
    m.sender()
        .send(sdc_gui::worker::WorkerMsg::Generated {
            id: 999,
            preview: false,
            result: Err("x.png: disk full".into()),
        })
        .unwrap();
    m.poll();
    assert!(m.gen.is_none());
    assert!(m.status_line.contains("disk full"), "{}", m.status_line);
    assert!(m.messages.entries().any(|e| e.text.contains("disk full")));
}

#[test]
fn a_rollback_never_removes_a_file_that_was_there() {
    // M4.
    let d = tempfile::tempdir().unwrap();
    let go = std::sync::atomic::AtomicBool::new(false);
    std::fs::write(d.path().join("afile"), b"x").unwrap();
    let bad = d.path().join("afile").join("x.png");
    // Equal bytes already there: nothing to write, nothing to roll back.
    let same = d.path().join("same.png");
    std::fs::write(&same, [7]).unwrap();
    assert!(write_all(
        &[(same.clone(), vec![7], None), (bad.clone(), vec![1], None)],
        &go
    )
    .is_err());
    assert_eq!(std::fs::read(&same).unwrap(), [7], "kept");
    // Our own earlier file (Regenerate) is put back as it was.
    let own = d.path().join("own.png");
    std::fs::write(&own, [1, 1]).unwrap();
    let sha = sdc_gui::model::history::sha256_of(&own);
    assert!(write_all(&[(own.clone(), vec![2, 2], sha), (bad, vec![1], None)], &go).is_err());
    assert_eq!(std::fs::read(&own).unwrap(), [1, 1], "restored");
}

#[test]
fn a_preview_is_at_most_512_and_a_closed_dialogs_preview_is_dropped() {
    // M7 (a), (b): CO35/CO57.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    relief_normal(&land(d.path()), "rock01_normal.png", 1024);
    reopen(&mut m, d.path());
    let width = |m: &AppModel| {
        let g = m.gen.as_ref().unwrap();
        g.preview
            .as_ref()
            .unwrap()
            .maps
            .height
            .as_ref()
            .unwrap()
            .width
    };
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_frame();
    idle(&mut m);
    assert_eq!(width(&m), 512);
    m.gen.as_mut().unwrap().opts.size = 256;
    m.gen_frame();
    idle(&mut m);
    assert_eq!(width(&m), 256);
    // A preview of a closed dialog lands after the new dialog's job started: it is dropped.
    m.gen_cancel();
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_frame(); // 512
    m.gen_cancel();
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen.as_mut().unwrap().opts.size = 256;
    m.gen_frame(); // 256
    idle(&mut m);
    assert_eq!(width(&m), 256, "the new dialog's own preview");
    assert_eq!(m.gen.as_ref().unwrap().started, 1);
}

#[test]
fn a_close_asked_while_a_refused_generate_runs_still_closes() {
    // Fix round 2, N1: the job fails with its dialog open; the close waiting for it runs, and nothing stays armed.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_clean(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    std::fs::write(land(d.path()).join("rock01_p_generated.png"), b"theirs").unwrap();
    m.gen_generate();
    assert!(!m.request_close(), "waits for Generate");
    idle(&mut m);
    assert!(
        m.gen.as_ref().is_some_and(|g| g.error.is_some()),
        "refused in the open dialog"
    );
    assert!(m.take_close_ready(), "the close runs");
    assert!(!m.take_close_ready(), "once");
    // The same for an Open.
    let (d, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut m = relief_clean(d.path());
    let root_b = mod_folder(other.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    std::fs::write(land(d.path()).join("rock01_p_generated.png"), b"theirs").unwrap();
    m.gen_generate();
    assert!(!m.request_open(root_b.clone()));
    idle(&mut m);
    assert_eq!(m.root.as_ref(), Some(&root_b), "the open runs");
}

#[test]
fn a_running_preview_does_not_hold_up_open_or_close() {
    // Fix round 2, N3: only a Generate that writes makes Open and Close wait.
    let (d, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut m = relief_clean(d.path());
    let root_b = mod_folder(other.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_frame();
    assert!(m.gen_pending(), "a preview runs");
    assert!(m.request_close(), "close is not held");
    assert!(m.request_open(root_b.clone()), "open is not held");
    assert_eq!(m.root.as_ref(), Some(&root_b));
    assert!(m.gen.is_none() && m.dialog.is_none());
    idle(&mut m);
}

// ---- Task 7: provenance on the slot row, stale, Regenerate, Delete generated file ----

const NORMAL: &str = "rock01_normal.png";

fn select_rock(m: &mut AppModel) {
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with("landscape/rock01"))
        .expect("the rock set")
        .key
        .clone();
    m.select_set(&key);
}

/// The Height row's status dot: (its AccessKit label, its rect), the dot nearest the row label's height to its right.
fn height_dot(h: &egui_kittest::Harness<'static, AppModel>) -> (String, eframe::egui::Rect) {
    use egui_kittest::kittest::Queryable;
    let row = slot_rect(h, "Height _p");
    ["ready", "warning", "blocks export"]
        .iter()
        .flat_map(|l| {
            h.query_all_by_label(l)
                .map(move |n| (l.to_string(), n.rect()))
        })
        .filter(|(_, r)| r.min.x > row.max.x)
        .min_by(|a, b| {
            let dy = |r: &eframe::egui::Rect| (r.center().y - row.center().y).abs();
            dy(&a.1).total_cmp(&dy(&b.1))
        })
        .expect("the Height row's dot")
}

/// Rewrite the normal with other content (a new stamp) and plan again; returns its old bytes.
fn rewrite_normal(m: &mut AppModel, d: &Path) -> Vec<u8> {
    let p = land(d).join(NORMAL);
    let old = std::fs::read(&p).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgb([100 + x as u8, 128u8, 240]))
        .save(&p)
        .unwrap();
    reopen(m, d);
    old
}

fn stale(m: &AppModel) -> Option<String> {
    m.generated_info(P_KEY).expect("a generated item").stale
}

const EARLIER: &str = "generated from an earlier rock01_normal.png: Regenerate… to update it";
const NO_HEIGHT: &str = "this set's shader reads no height map; the generated file is unused";

fn delete_op(menu: &[MenuEntry]) -> Option<&MenuEntry> {
    menu.iter().find(|e| {
        matches!(
            e.action,
            MenuAction::Generated {
                op: GenOp::Delete(_),
                ..
            }
        )
    })
}

/// The fixture plans with suggestions unaccepted (`opened`): decide the generated `_p` so its tile can be ready.
fn decide_p(m: &mut AppModel) {
    m.decide(
        P_KEY,
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Height),
    );
    idle(m);
}

fn add_real_p(m: &mut AppModel, d: &Path) {
    ImageBuffer::from_fn(64, 64, |_, _| Luma([128u8]))
        .save(land(d).join("rock01_p.png"))
        .unwrap();
    reopen(m, d);
}

#[test]
fn the_slot_row_says_where_a_generated_map_came_from() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    assert_eq!(
        m.generated_info(P_KEY),
        Some(GenInfo {
            from: NORMAL.into(),
            stale: None,
            replaced_by: None
        })
    );
    assert_eq!(
        m.sets_doc.as_ref().unwrap().generated()[P_KEY]["from"],
        "landscape/rock01_normal.png",
        "the record keeps the item key"
    );
    assert_eq!(m.generated_info("landscape/rock01_normal.png"), None);
    decide_p(&mut m);
    select_rock(&mut m);
    let mut h = harness(m);
    h.run();
    let row = slot_rect(&h, "Height _p");
    let marker = h
        .query_all_by_label("Generated from rock01_normal.png")
        .map(|n| n.rect())
        .find(|r| r.min.x > row.max.x && (r.center().y - row.center().y).abs() < 40.0)
        .expect("the Flags cell's generated marker");
    let (dot, _) = height_dot(&h);
    assert_eq!(dot, "ready");
    let file = h
        .query_all_by_label("rock01_p_generated.png")
        .map(|n| n.rect())
        .filter(|r| r.min.x > row.max.x && r.max.x < marker.min.x)
        .max_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the File cell (right of the thumbnail)");
    h.hover_at(file.center());
    h.run();
    assert!(
        h.query_all_by_label_contains("Generated from rock01_normal.png")
            .any(|n| {
                let a = n.accesskit_node();
                a.label()
                    .or(a.value())
                    .is_some_and(|l| l.contains("rock01_p_generated.png"))
            }),
        "the File cell's hover names the normal"
    );
}

#[test]
fn a_stale_generated_map_is_a_regenerate_row_its_set_dot_reads() {
    use egui_kittest::kittest::Queryable;
    use sdc_gui::model::bottom;
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    decide_p(&mut m);
    // Fill the packed channels that would default, so the set is ready before the map goes stale.
    for (map, v) in [
        (engine::config::RawMap::Metallic, 0),
        (engine::config::RawMap::Specular, 255),
    ] {
        m.set_pack_fill("landscape/rock01_rmaos", map, Some(v));
    }
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let set_key = rock_view(&m).key;
    // The rows about the generated item or its set (the rewrite adds rows about the normal itself).
    let rows = |m: &AppModel| -> Vec<bottom::Problem> {
        bottom::problems(m)
            .iter()
            .filter(|p| match &p.at {
                bottom::At::Texture(k) => k == P_KEY,
                bottom::At::Set { key, .. } => *key == set_key,
                _ => false,
            })
            .cloned()
            .collect()
    };
    let rows_before = rows(&m);
    let dots = |m: &AppModel| {
        (
            bottom::set_ready(m, &rock_view(m)),
            bottom::tile_ready(m.item(P_KEY).unwrap()),
            m.export_is_clean(),
        )
    };
    let before = dots(&m);
    let all_before = bottom::problems(&m);
    rewrite_normal(&mut m, d.path());
    assert_eq!(stale(&m).as_deref(), Some(EARLIER));
    // Note FC3 (supersedes CO70/CO71): a stale map is a Warning row with Regenerate, so its set is not ready; the
    // tile and the export do not read it.
    let after = dots(&m);
    assert_eq!((before.1, before.2), (after.1, after.2));
    assert!(before.0, "the set was ready before: {all_before:?}");
    assert!(!after.0, "the set dot reads the row");
    let new: Vec<_> = rows(&m)
        .into_iter()
        .filter(|p| !rows_before.contains(p))
        .collect();
    assert_eq!(new.len(), 1, "{new:?}");
    assert_eq!(new[0].code, Some(engine::codes::Code::GenStale));
    assert_eq!(new[0].level, bottom::Level::Warning);
    assert_eq!(new[0].text, EARLIER);
    assert_eq!(new[0].fix, Some(bottom::Fix::Regenerate(P_KEY.into())));
    select_rock(&mut m);
    let mut h = harness(m);
    h.run();
    let (dot, r) = height_dot(&h);
    assert_eq!(dot, "warning");
    h.hover_at(r.center());
    h.run();
    assert!(h
        .query_by_label_contains(&format!("stale: {EARLIER}"))
        .is_some());
}

#[test]
fn gen_info_results_are_hover_only() {
    use engine::codes::Code;
    use sdc_gui::model::bottom;
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    rewrite_normal(&mut m, d.path());
    assert!(stale(&m).is_some());
    add_real_p(&mut m, d.path());
    assert!(m
        .item(P_KEY)
        .unwrap()
        .warnings
        .iter()
        .any(|w| w.code == Code::GenReplaced));
    assert!(stale(&m).is_some());
    let codes: Vec<_> = bottom::problems(&m).iter().filter_map(|p| p.code).collect();
    assert!(!codes.contains(&Code::GenReplaced), "{codes:?}");
    // Note FC3: GEN-STALE is a row; GEN-REPLACED stays on hover.
    assert!(codes.contains(&Code::GenStale), "{codes:?}");
}

/// Note FC3, Review Focus 3: `Regenerate…` on a stale row opens Generate with that map; nothing is written from the
/// row, and the button is off while a plan is in flight.
#[test]
fn the_stale_row_regenerates_only_from_its_own_click() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    decide_p(&mut m);
    rewrite_normal(&mut m, d.path());
    assert!(stale(&m).is_some());
    let p = land(d.path()).join("rock01_p_generated.png");
    let bytes = std::fs::read(&p).unwrap();
    m.open_problems(None);
    let mut h = harness(m);
    h.run();
    // A plan in flight: the row's button is off. The UI never polls, so the plan stays in flight until `idle`.
    h.state_mut().replan();
    assert!(h.state().fix_off().is_some(), "a plan is in flight");
    h.run();
    assert!(h.get_by_label("Regenerate…").accesskit_node().is_disabled());
    idle(h.state_mut());
    h.run();
    assert!(h.state().gen.is_none());
    // Enter on the focused stale row does nothing: only the button's own click opens Generate.
    let name = format!(
        "{P_KEY}: {}",
        sdc_gui::model::code_text::code_sentence(engine::codes::Code::GenStale)
    );
    h.get_by_label(&name).focus();
    h.run();
    assert!(
        h.get_by_label(&name).accesskit_node().is_focused(),
        "the row has the keyboard"
    );
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert!(h.state().gen.is_none(), "Enter does not regenerate");
    assert_eq!(std::fs::read(&p).unwrap(), bytes, "Enter wrote nothing");
    h.get_by_label("Regenerate…").click();
    h.run();
    let g = h.state().gen.as_ref().expect("the Generate dialog");
    assert!(g.ticked.contains(&GenMap::Height), "{:?}", g.ticked);
    assert_eq!(std::fs::read(&p).unwrap(), bytes, "nothing written yet");
}

#[test]
fn delete_confirm_defaults_to_cancel_and_outlines_delete() {
    use eframe::egui::Key;
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    let p = land(d.path()).join("rock01_p_generated.png");
    let open = |m: &mut AppModel| {
        m.run_menu(MenuAction::Generated {
            set: ROCK_SET.into(),
            op: GenOp::Delete(P_KEY.into()),
        });
        assert!(matches!(m.dialog, Some(Dialog::ConfirmDeleteGenerated(_))));
    };
    open(&mut m);
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(3);
    assert!(h
        .query_by_label("Delete rock01_p_generated.png? It was generated from rock01_normal.png.")
        .is_some());
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "Enter is Cancel");
    assert!(p.exists());
    open(h.state_mut());
    h.run_steps(3);
    h.get_by_label("Delete rock01_p_generated.png").click();
    h.run_steps(3);
    assert!(!p.exists());
    assert!(h.state().dialog.is_none());
    for (label, role, _) in sdc_gui::ui::dialogs::delete_generated_buttons("x.png") {
        let want = if label == "Cancel" {
            sdc_gui::ui::dialogs::Role::Default
        } else {
            sdc_gui::ui::dialogs::Role::Destructive
        };
        assert_eq!(role, want, "{label}");
    }
}

#[test]
fn stale_follows_the_normal_hash_and_the_neutral_flags() {
    use engine::codes::{Code, Severity};
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    assert_eq!(stale(&m), None);
    let hashed = m.gen_hashes();
    let gen_stale = |m: &AppModel| {
        assert!(
            !m.item(P_KEY)
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.code == Code::GenStale),
            "never on the item"
        );
        m.item_messages(P_KEY)
            .into_iter()
            .find(|w| w.code == Code::GenStale)
    };
    assert_eq!(gen_stale(&m), None);

    let old = rewrite_normal(&mut m, d.path());
    assert_eq!(stale(&m).as_deref(), Some(EARLIER));
    let w = gen_stale(&m).expect("in the message list");
    assert_eq!(
        (w.message.as_str(), w.code.severity()),
        (EARLIER, Severity::Info)
    );
    let n = m.gen_hashes();
    assert_eq!(n, hashed + 1, "hashed once for the new stamp");
    for _ in 0..50 {
        stale(&m);
    }
    assert_eq!(m.gen_hashes(), n, "not once per frame");

    std::fs::write(land(d.path()).join(NORMAL), old).unwrap();
    reopen(&mut m, d.path());
    assert_eq!(stale(&m), None, "restored");
    assert_eq!(gen_stale(&m), None);

    let n = m.gen_hashes();
    m.set_json_values(
        ROCK_SET,
        vec![
            ("multilayer".into(), Some(json!(true))),
            ("coat_parallax".into(), Some(json!(true))),
        ],
    );
    idle(&mut m);
    let want = "generated for a height neutral of 0.5; this set now wants 1.0";
    assert_eq!(stale(&m).as_deref(), Some(want));
    assert_eq!(gen_stale(&m).unwrap().message, want);
    assert_eq!(m.gen_hashes(), n, "a set edit reads no normal again");

    shader(&mut m, Shader::Multilayer);
    assert_eq!(stale(&m).as_deref(), Some(NO_HEIGHT));
    assert_eq!(gen_stale(&m).unwrap().message, NO_HEIGHT);
    // Review M1: a 0.5-neutral shader without a Height slot.
    shader(&mut m, Shader::Glow);
    assert_eq!(stale(&m).as_deref(), Some(NO_HEIGHT));
}

#[test]
fn a_missing_normal_is_stale() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    std::fs::remove_file(land(d.path()).join(NORMAL)).unwrap();
    reopen(&mut m, d.path());
    assert_eq!(stale(&m).as_deref(), Some("the normal map is gone"));
}

#[test]
fn regenerate_reuses_the_stored_settings_and_replaces_only_its_own_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen
        .as_mut()
        .unwrap()
        .opts
        .height
        .as_mut()
        .unwrap()
        .strength = 0.5;
    m.gen_generate();
    idle(&mut m);
    let p = land(d.path()).join("rock01_p_generated.png");
    let first = std::fs::read(&p).unwrap();
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(
        m.gen
            .as_ref()
            .unwrap()
            .opts
            .height
            .as_ref()
            .unwrap()
            .strength,
        0.5
    );
    m.gen
        .as_mut()
        .unwrap()
        .opts
        .height
        .as_mut()
        .unwrap()
        .strength = 1.0;
    m.gen_generate();
    idle(&mut m);
    assert!(
        m.gen.is_none(),
        "{:?}",
        m.gen.as_ref().and_then(|g| g.error.clone())
    );
    assert_ne!(first, std::fs::read(&p).unwrap(), "replaced");
    let rec = m.provenance(P_KEY).unwrap();
    assert_eq!(
        rec.sha256,
        sdc_gui::model::history::sha256_of(&p).unwrap(),
        "the new sha"
    );
    assert_eq!(rec.opts.height.unwrap().strength, 1.0);
}

#[test]
fn regenerate_follows_the_sets_neutral_and_a_shrunk_normal() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    relief_normal(&land(d.path()), NORMAL, 1024);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen.as_mut().unwrap().opts.size = 1024;
    assert_eq!(
        m.gen
            .as_ref()
            .unwrap()
            .opts
            .height
            .as_ref()
            .unwrap()
            .neutral,
        Neutral::Half
    );
    m.gen_generate();
    idle(&mut m);
    assert!(m.gen.is_none());
    m.set_json_values(
        ROCK_SET,
        vec![
            ("multilayer".into(), Some(json!(true))),
            ("coat_parallax".into(), Some(json!(true))),
        ],
    );
    idle(&mut m);
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    let o = m.gen.as_ref().unwrap().opts.clone();
    assert_eq!(o.height.unwrap().neutral, Neutral::White);
    assert_eq!(o.size, 1024);
    m.gen_cancel();
    relief_normal(&land(d.path()), NORMAL, 512);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(m.gen.as_ref().unwrap().opts.size, 512, "never upsampled");
}

#[test]
fn regenerate_refuses_a_generated_file_the_user_edited() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    let p = land(d.path()).join("rock01_p_generated.png");
    let mut bytes = std::fs::read(&p).unwrap();
    bytes.push(0);
    std::fs::write(&p, &bytes).unwrap();
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen
        .as_mut()
        .unwrap()
        .opts
        .height
        .as_mut()
        .unwrap()
        .strength = 0.5;
    m.gen_generate();
    idle(&mut m);
    let err = m
        .gen
        .as_ref()
        .and_then(|g| g.error.clone())
        .expect("refused");
    assert_eq!(
        err,
        "rock01_p_generated.png was edited since it was generated, so it is not overwritten; delete or rename it first"
    );
    assert_eq!(std::fs::read(&p).unwrap(), bytes, "unchanged");
}

#[test]
fn an_in_place_export_still_offers_regenerate() {
    use engine::codes::Code;
    let d = tempfile::tempdir().unwrap();
    // A Parallax set: its `_p` exports to `textures/landscape/` (a PBR one to `textures/pbr/…`).
    let mut m = relief_clean(d.path());
    shader(&mut m, Shader::Parallax);
    generate_height(&mut m);
    let root = m.root.clone().unwrap();
    m.sets_doc.as_mut().unwrap().set_export_folder(&root);
    reopen(&mut m, d.path());
    let out = m.item(P_KEY).unwrap().job.as_ref().unwrap().output.clone();
    assert_eq!(out, land(d.path()).join("rock01_p.dds"), "next to the PNG");
    // What an in-place export writes there (the GUI's own Export refuses the opened folder; `sdc convert` does not).
    bc1_dds(&out, 64, 64);
    reopen(&mut m, d.path());
    assert!(
        rock_view(&m)
            .members
            .iter()
            .any(|k| k == "landscape/rock01_p.dds"),
        "a set member"
    );
    assert!(!m
        .item(P_KEY)
        .unwrap()
        .warnings
        .iter()
        .any(|w| w.code == Code::GenReplaced));
    let e = m.gen_entry(ROCK_SET, Some(GenMap::Height));
    assert_eq!((e.label.as_str(), e.disabled), ("Regenerate…", None));
}

fn delete_rows(m: &AppModel) -> Vec<DeleteGenerated> {
    match &m.dialog {
        Some(Dialog::ConfirmDeleteGenerated(d)) => vec![(**d).clone()],
        d => panic!("{d:?}"),
    }
}

#[test]
fn a_real_height_added_later_replaces_the_generated_one_and_delete_is_offered() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    let p = land(d.path()).join("rock01_p_generated.png");
    add_real_p(&mut m, d.path());
    assert_eq!(
        m.generated_info(P_KEY).unwrap().replaced_by.as_deref(),
        Some("rock01_p.png")
    );
    let entry = delete_op(&m.slot_menu(ROCK_SET, SlotKind::Height))
        .expect("Delete generated file…")
        .clone();
    assert_eq!(entry.label, "Delete generated file…");
    assert_eq!(entry.disabled, None);
    m.run_menu(entry.action.clone());
    let rows = delete_rows(&m);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].body,
        "Delete rock01_p_generated.png? It was generated from rock01_normal.png."
    );
    assert_eq!(rows[0].refused, None);
    let steps = m.history.undo.len();
    m.confirm_delete_generated();
    idle(&mut m);
    assert!(!p.exists());
    assert!(m.provenance(P_KEY).is_none());
    assert_eq!(m.history.undo.len(), steps, "not undoable");
    assert!(m.dialog.is_none());

    // An edited file and one with no record: the box says so, and the user's confirm deletes it (spec-S8, P13).
    for edited in [true, false] {
        let d = tempfile::tempdir().unwrap();
        let mut m = relief_rock(d.path());
        generate_height(&mut m);
        let p = land(d.path()).join("rock01_p_generated.png");
        let want = if edited {
            let mut b = std::fs::read(&p).unwrap();
            b.push(0);
            std::fs::write(&p, b).unwrap();
            "Delete rock01_p_generated.png? It was generated from rock01_normal.png. It was edited since."
        } else {
            m.sets_doc.as_mut().unwrap().set_generated(P_KEY, None);
            "Delete rock01_p_generated.png? Tessalith has no record of generating it."
        };
        add_real_p(&mut m, d.path());
        let entry = delete_op(&m.slot_menu(ROCK_SET, SlotKind::Height))
            .expect("offered")
            .clone();
        m.run_menu(entry.action);
        let rows = delete_rows(&m);
        assert_eq!(rows[0].body, want);
        assert_eq!(rows[0].refused, None);
        m.confirm_delete_generated();
        idle(&mut m);
        assert!(!p.exists(), "edited {edited}: deleted on confirm");
        assert!(m.provenance(P_KEY).is_none());
    }

    // A file that changes after the box opened is kept, and the status line says so.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    let p = land(d.path()).join("rock01_p_generated.png");
    m.ask_delete_generated(P_KEY);
    let mut b = std::fs::read(&p).unwrap();
    b.push(0);
    std::fs::write(&p, &b).unwrap();
    m.confirm_delete_generated();
    assert_eq!(std::fs::read(&p).unwrap(), b, "kept");
    assert!(m.provenance(P_KEY).is_some());
    assert_eq!(
        m.status_line,
        "rock01_p_generated.png changed since; it was not deleted"
    );
}

#[test]
fn delete_takes_a_moved_in_generated_file_out_of_the_set() {
    // Review M4: no `add` entry is left behind for a file that is gone (SET-EDIT-STALE).
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    std::fs::remove_file(land(d.path()).join(NORMAL)).unwrap();
    relief_normal(&land(d.path()), "stone_normal.png", 64);
    reopen(&mut m, d.path());
    m.edit_docs("Move into", None, true, |_, s| {
        s.add_member(ROCK_SET, "landscape/stone_normal.png")
    });
    idle(&mut m);
    generate_height(&mut m);
    let key = "landscape/stone_p_generated.png";
    let added = |m: &AppModel| {
        m.sets_doc
            .as_ref()
            .unwrap()
            .entries()
            .iter()
            .any(|e| e.add.iter().any(|f| f == key))
    };
    assert!(added(&m), "moved in with its normal");
    let steps = m.history.undo.len();
    m.ask_delete_generated(key);
    m.confirm_delete_generated();
    idle(&mut m);
    assert!(!land(d.path()).join("stone_p_generated.png").exists());
    assert!(!added(&m), "out of the set's add list");
    assert!(!m.sets_doc.as_ref().unwrap().generated().contains_key(key));
    assert_eq!(m.history.undo.len(), steps, "not undoable");
}

#[test]
fn a_read_only_project_deletes_nothing() {
    // Review M5.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    let p = land(d.path()).join("rock01_p_generated.png");
    std::fs::write(
        m.root.clone().unwrap().join("sdc-project.toml"),
        "[[sets]\nbroken",
    )
    .unwrap();
    m.reload_documents();
    idle(&mut m);
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    let e = delete_op(&m.slot_menu(ROCK_SET, SlotKind::Height))
        .expect("listed")
        .clone();
    assert!(
        e.disabled
            .as_deref()
            .is_some_and(|r| r.starts_with("sdc-project.toml is read-only: ")),
        "{:?}",
        e.disabled
    );
    m.ask_delete_generated(P_KEY);
    m.confirm_delete_generated();
    assert!(p.exists(), "kept");
    assert!(m.status_line.starts_with("sdc-project.toml is read-only: "));
}

#[test]
fn the_set_card_submenu_deletes_the_m_alpha_height() {
    // Review I1: the set card's submenu is the only Delete for an `_m`-alpha height (it fills no row).
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    idle(&mut m);
    relief_normal(&land(d.path()), NORMAL, 64);
    reopen(&mut m, d.path());
    complex_material(&mut m, "rock01");
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen.as_mut().unwrap().opts.size = 64;
    m.gen_generate();
    idle(&mut m);
    let key = "landscape/rock01_height_generated.png";
    let file = land(d.path()).join("rock01_height_generated.png");
    assert!(file.exists());
    let mut h = harness(m);
    h.run();
    card_menu(&mut h, "landscape/rock01");
    h.get_by_label("Delete generated file ⏵").hover();
    h.run();
    h.get_by_role_and_label(
        eframe::egui::accesskit::Role::Button,
        "rock01_height_generated.png",
    )
    .click();
    h.run();
    let rows = delete_rows(h.state());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, key);
    h.get_by_label("Delete rock01_height_generated.png").click();
    h.run();
    assert!(!file.exists());
}

#[cfg(windows)]
#[test]
fn an_unreadable_normal_is_not_called_earlier() {
    // Review M3: a normal that cannot be read says so; it is not "generated from an earlier …".
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    rewrite_normal(&mut m, d.path());
    // No sharing: every other open fails while this handle lives.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(land(d.path()).join(NORMAL))
        .unwrap();
    assert_eq!(
        stale(&m).as_deref(),
        Some("the normal map rock01_normal.png cannot be read")
    );
    drop(lock);
    // Not cached: the next rebuild reads it.
    m.invalidate_views();
    assert_eq!(stale(&m).as_deref(), Some(EARLIER));
}

#[test]
fn a_dds_normal_goes_stale_and_regenerate_makes_it_fresh() {
    // Review M6 (a, c): the DDS stamp branch, and stale -> Regenerate -> fresh.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    std::fs::remove_file(land(d.path()).join(NORMAL)).unwrap();
    let dds = land(d.path()).join("rock01_n.dds");
    normal_dds(&dds, &blob_normals(), 128, true);
    reopen(&mut m, d.path());
    generate_height(&mut m);
    assert_eq!(stale(&m), None);
    let flat = vec![[0.0, 0.0, 1.0]; 128 * 128];
    normal_dds(&dds, &flat, 128, false);
    reopen(&mut m, d.path());
    assert_eq!(
        stale(&m).as_deref(),
        Some("generated from an earlier rock01_n.dds: Regenerate… to update it")
    );
    let before = m.provenance(P_KEY).unwrap().from_sha256;
    generate_height(&mut m);
    assert!(
        m.gen.is_none(),
        "{:?}",
        m.gen.as_ref().and_then(|g| g.error.clone())
    );
    assert_eq!(stale(&m), None, "fresh after Regenerate");
    assert_ne!(m.provenance(P_KEY).unwrap().from_sha256, before);
}

#[test]
fn the_set_card_menu_lists_generated_files_to_delete() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    assert!(m.delete_generated_entries(ROCK_SET).is_empty());
    generate_height(&mut m);
    let es = m.delete_generated_entries(ROCK_SET);
    assert_eq!(es.len(), 1);
    assert_eq!(es[0].label, "rock01_p_generated.png");
    assert_eq!(
        es[0].action,
        MenuAction::Generated {
            set: ROCK_SET.into(),
            op: GenOp::Delete(P_KEY.into())
        }
    );
}

#[test]
fn an_m_alpha_height_shows_its_provenance_stale_and_delete() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    idle(&mut m);
    relief_normal(&land(d.path()), NORMAL, 64);
    reopen(&mut m, d.path());
    complex_material(&mut m, "rock01");
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen.as_mut().unwrap().opts.size = 64;
    m.gen_generate();
    idle(&mut m);
    let key = "landscape/rock01_height_generated.png";
    assert_eq!(
        m.generated_info(key).map(|g| (g.from, g.stale)),
        Some((NORMAL.to_string(), None))
    );
    assert!(m
        .delete_generated_entries(ROCK_SET)
        .iter()
        .any(|e| e.label == "rock01_height_generated.png"));
    // No longer complex material: the `_m` alpha is unused.
    shader(&mut m, Shader::Parallax);
    assert_eq!(
        m.generated_info(key).unwrap().stale.as_deref(),
        Some(NO_HEIGHT)
    );
    shader(&mut m, Shader::ComplexMaterial);
    rewrite_normal(&mut m, d.path());
    select_rock(&mut m);
    let mut h = harness(m);
    h.run();
    assert!(
        h.query_by_label(&format!("stale: {EARLIER}")).is_some(),
        "a weak stale line in the _m block"
    );
}

// ---- Task 10: AO ----

const AO_KEY: &str = "landscape/rock01_ao_generated.png";

/// `pbr_opened_plain` without the fixture's `rock01_ao.png` (a real AO source).
fn ao_rock(d: &Path) -> AppModel {
    let mut m = pbr_opened_plain(d);
    std::fs::remove_file(land(d).join("rock01_ao.png")).unwrap();
    reopen(&mut m, d);
    m
}

fn ao(m: &AppModel) -> Result<(), String> {
    m.gen_offer(ROCK_SET, GenMap::Ao)
}

fn ao_open(menu: &[MenuEntry]) -> Option<&MenuEntry> {
    menu.iter().find(|e| {
        matches!(
            e.action,
            MenuAction::Generated {
                op: GenOp::Open(Some(GenMap::Ao)),
                ..
            }
        )
    })
}

/// The set's packed `_rmaos` output.
fn rmaos(m: &AppModel) -> &sdc_gui::model::items::TextureItem {
    m.items
        .iter()
        .find(|i| {
            i.key.ends_with("rock01_rmaos") && i.job.as_ref().is_some_and(|j| j.pack.is_some())
        })
        .expect("the packed _rmaos")
}

fn rmaos_codes(m: &AppModel) -> Vec<(engine::codes::Code, String)> {
    rmaos(m)
        .job
        .as_ref()
        .unwrap()
        .warnings
        .iter()
        .map(|w| (w.code, w.message.clone()))
        .collect()
}

fn flat_normal(path: &Path, size: u32) {
    ImageBuffer::from_fn(size, size, |_, _| Rgb([128u8, 128, 255]))
        .save(path)
        .unwrap();
}

#[test]
fn a_pbr_set_without_ao_offers_ao_on_the_rmaos_row() {
    use engine::codes::Code;
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    let menu = m.slot_menu(ROCK_SET, SlotKind::Rmaos);
    let e = ao_open(&menu).expect("the _rmaos row offers AO");
    assert_eq!((e.label.as_str(), e.disabled.clone()), (GENERATE, None));
    assert_eq!(e, &m.gen_entry(ROCK_SET, Some(GenMap::Ao)));
    assert!(ao_open(&m.slot_menu(ROCK_SET, SlotKind::Height)).is_none());
    assert!(rmaos_codes(&m)
        .iter()
        .any(|(c, t)| *c == Code::PackDefaulted && t.contains(" B: ")));
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    assert_eq!(
        m.gen.as_ref().unwrap().ticked,
        [GenMap::Ao].into_iter().collect()
    );
    m.gen_generate();
    idle(&mut m);
    let p = land(d.path()).join("rock01_ao_generated.png");
    let img = image::open(&p).expect("the generated AO");
    assert!(
        matches!(img, image::DynamicImage::ImageLuma16(_)),
        "16-bit grey"
    );
    assert_eq!((img.width(), img.height()), (64, 64));
    assert_eq!(
        generated_pngs(&land(d.path())),
        vec!["rock01_ao_generated.png"]
    );
    let rec = &m.sets_doc.as_ref().unwrap().generated()[AO_KEY];
    assert_eq!(rec["kind"], "ao");
    assert_eq!(rec["opts"]["ao"]["slices"], 16, "the file at full quality");
    // After the re-plan the generated AO is the `_rmaos` B.
    let codes = rmaos_codes(&m);
    assert!(codes.iter().any(|(c, _)| *c == Code::GenAo), "{codes:?}");
    assert!(
        !codes
            .iter()
            .any(|(c, t)| *c == Code::PackDefaulted && t.contains(" B: ")),
        "{codes:?}"
    );
    let e = ao_open(&m.slot_menu(ROCK_SET, SlotKind::Rmaos))
        .cloned()
        .unwrap();
    assert_eq!((e.label.as_str(), e.disabled), ("Regenerate…", None));
}

#[test]
fn a_generated_ao_fills_a_flat_prepacked_rmaos_and_offers_regenerate() {
    use engine::codes::Code;
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_roughness.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgba([100u8 + x as u8, 0, 255, 77]))
        .save(land(d.path()).join("rock01_rmaos.png"))
        .unwrap();
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    m.gen_generate();
    idle(&mut m);
    assert_eq!(
        generated_pngs(&land(d.path())),
        vec!["rock01_ao_generated.png"]
    );
    let codes = rmaos_codes(&m);
    assert!(codes.iter().any(|(c, _)| *c == Code::GenAo), "{codes:?}");
    assert!(
        !codes
            .iter()
            .any(|(c, _)| matches!(c, Code::PackDefaulted | Code::PackConflict)),
        "{codes:?}"
    );
    let pk = rmaos(&m).job.as_ref().unwrap().pack.as_ref().unwrap();
    assert!(
        pk.sources
            .iter()
            .any(|s| s.path.ends_with("rock01_rmaos.png")),
        "R/G/A from the image"
    );
    let e = ao_open(&m.slot_menu(ROCK_SET, SlotKind::Rmaos))
        .cloned()
        .unwrap();
    assert_eq!((e.label.as_str(), e.disabled), ("Regenerate…", None));
}

#[test]
fn a_loose_roughness_beside_a_flat_prepacked_rmaos_keeps_the_plain_reason() {
    // CO108: the engine joins the image only beside a lone generated AO; with a loose roughness it does not.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([100u8, 0, 255]))
        .save(land(d.path()).join("rock01_rmaos.png"))
        .unwrap();
    let real = Err("this set already has an AO source (rock01_rmaos.png)".to_string());
    // packing.prefer = raw (the default): the raw recipe replaces the image.
    reopen(&mut m, d.path());
    assert_eq!(ao(&m), real, "prefer = raw");
    // packing.prefer = prepacked: the image is kept, and a generated AO would go unused.
    let toml = m.root.clone().unwrap().join("sdc.toml");
    std::fs::write(&toml, "[defaults.packing]\nprefer = \"prepacked\"\n").unwrap();
    reopen(&mut m, d.path());
    assert!(
        m.items
            .iter()
            .any(|i| i.key.ends_with("rock01_rmaos.png") && i.analysis.is_some()),
        "the image keeps its own job under prefer = prepacked"
    );
    assert_eq!(ao(&m), real, "prefer = prepacked");
}

#[test]
fn the_rmaos_row_offers_ao_on_a_flat_prepacked_rmaos() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    // The pre-packed image is the set's only R/G/A source.
    std::fs::remove_file(land(d.path()).join("rock01_roughness.png")).unwrap();
    let rm = land(d.path()).join("rock01_rmaos.png");
    let row = |m: &AppModel| {
        m.slot_menu(ROCK_SET, SlotKind::Rmaos)
            .last()
            .cloned()
            .unwrap()
    };
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([100u8, 0, 255]))
        .save(&rm)
        .unwrap();
    reopen(&mut m, d.path());
    let e = row(&m);
    assert_eq!(ao_open(std::slice::from_ref(&e)), Some(&e), "the AO entry");
    assert_eq!((e.label.as_str(), e.disabled), (GENERATE, None));
    // The AO takes the image's size, so the recipe's sources match.
    assert_eq!(m.gen_size(ROCK_SET, GenMap::Ao), Ok(GenSize::Locked(32)));
    // B varies 100..255: it already holds AO.
    ImageBuffer::from_fn(64, 64, |x, y| {
        Rgb([100u8, 0, 100 + ((x + y) * 155 / 126) as u8])
    })
    .save(&rm)
    .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(
        row(&m).disabled.as_deref(),
        Some("this set already has an AO source (rock01_rmaos.png (B))")
    );
    // A DDS `_rmaos` instead: DDS_RMAOS.
    std::fs::remove_file(&rm).unwrap();
    bc1_dds(&land(d.path()).join("rock01_rmaos.dds"), 64, 64);
    reopen(&mut m, d.path());
    assert_eq!(
        row(&m).disabled.as_deref(),
        Some("this set's _rmaos is a DDS (rock01_rmaos.dds), which is never re-encoded")
    );
}

#[test]
fn ao_reasons_cover_vanilla_non_pbr_real_ao_and_prepacked_rmaos() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    let real = |n: &str| Err(format!("this set already has an AO source ({n})"));
    // The support fixture's rock01_ao.png.
    assert_eq!(ao(&m), real("rock01_ao.png"));
    std::fs::remove_file(land(d.path()).join("rock01_ao.png")).unwrap();
    reopen(&mut m, d.path());
    assert_eq!(ao(&m), Ok(()));
    // No normal.
    assert_eq!(m.gen_offer(LEAF_SET, GenMap::Ao), Err(NO_NORMAL.into()));
    // Slot 6 locked, before everything but read-only.
    m.set_slot_lock(ROCK_SET, 6, true).unwrap();
    idle(&mut m);
    assert_eq!(ao(&m), Err("slot locked".into()));
    m.set_slot_lock(ROCK_SET, 6, false).unwrap();
    idle(&mut m);
    // NOT_PBR from the row's menu and from the set menu's AO checkbox (the dialog's offers), on vanilla...
    let row = |m: &AppModel| {
        ao_open(&m.slot_menu(ROCK_SET, SlotKind::Rmaos))
            .cloned()
            .unwrap()
    };
    m.set_target(Target::Vanilla);
    idle(&mut m);
    assert_eq!(ao(&m), Err(NOT_PBR.into()));
    assert_eq!(row(&m).disabled, Some(NOT_PBR.into()));
    m.set_target(Target::Cs);
    idle(&mut m);
    // ... and for a default-shader set on cs.
    shader(&mut m, Shader::Default);
    assert_eq!(ao(&m), Err(NOT_PBR.into()));
    assert_eq!(row(&m).disabled, Some(NOT_PBR.into()));
    m.open_gen(ROCK_SET, None);
    assert_eq!(m.gen_facts().offers[1], (GenMap::Ao, Err(NOT_PBR.into())));
    m.gen_cancel();
    // PbrGrass reads `_rmaos` (gap ruling).
    shader(&mut m, Shader::PbrGrass);
    assert_eq!(ao(&m), Ok(()));
    shader(&mut m, Shader::Pbr);
    // Task 11: a pre-packed `_rmaos` image beside the fixture's loose roughness is a real AO source (CO108).
    let rm = land(d.path()).join("rock01_rmaos.png");
    ImageBuffer::from_fn(64, 64, |x, _| Rgb([100u8, 0, 100 + x as u8]))
        .save(&rm)
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(ao(&m), real("rock01_rmaos.png"));
    std::fs::remove_file(&rm).unwrap();
    // A DDS `_rmaos` is never re-encoded.
    bc1_dds(&land(d.path()).join("rock01_rmaos.dds"), 64, 64);
    reopen(&mut m, d.path());
    assert_eq!(
        ao(&m),
        Err("this set's _rmaos is a DDS (rock01_rmaos.dds), which is never re-encoded".into())
    );
    std::fs::remove_file(land(d.path()).join("rock01_rmaos.dds")).unwrap();
    // A hand-made `_ao_generated`.
    ImageBuffer::from_fn(64, 64, |_, _| Luma([200u8]))
        .save(land(d.path()).join("rock01_ao_generated.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(
        ao(&m),
        Err(
            "rock01_ao_generated.png exists but was not generated here; delete it to generate again"
                .into()
        )
    );
    std::fs::remove_file(land(d.path()).join("rock01_ao_generated.png")).unwrap();
    // NPOT comes before NOT_PBR.
    m.set_target(Target::Vanilla);
    flat_normal(&land(d.path()).join("rock01_normal.png"), 48);
    reopen(&mut m, d.path());
    assert_eq!(
        ao(&m),
        Err("the normal map is 48×48; only power-of-two sizes can be generated".into())
    );
}

#[test]
fn a_generated_ao_matches_the_rmaos_sources_size() {
    use engine::codes::Code;
    // P6: a 64² set with a 128² relief normal, AO ticked after opening for Height.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    relief_normal(&land(d.path()), "rock01_normal.png", 128);
    reopen(&mut m, d.path());
    assert_eq!(m.gen_size(ROCK_SET, GenMap::Ao), Ok(GenSize::Locked(64)));
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(m.gen.as_ref().unwrap().opts.size, 128);
    m.gen_tick(GenMap::Ao, true);
    let dims: Vec<(GenMap, (u32, u32))> =
        m.gen_paths().into_iter().map(|(g, _, d)| (g, d)).collect();
    assert_eq!(
        dims,
        vec![(GenMap::Height, (128, 128)), (GenMap::Ao, (64, 64))]
    );
    m.gen_generate();
    idle(&mut m);
    let dim = |n: &str| image::image_dimensions(land(d.path()).join(n)).unwrap();
    assert_eq!(dim("rock01_ao_generated.png"), (64, 64));
    assert_eq!(dim("rock01_p_generated.png"), (128, 128));
    assert_eq!(
        m.sets_doc.as_ref().unwrap().generated()[AO_KEY]["opts"]["size"],
        64,
        "each record keeps its file's size"
    );
    let rm = rmaos(&m);
    assert!(
        matches!(rm.status, sdc_gui::model::items::ItemStatus::Ready),
        "{:?}",
        rm.status
    );
    let codes = rmaos_codes(&m);
    assert!(
        !codes.iter().any(|(c, _)| *c == Code::PackSizeMismatch),
        "{codes:?}"
    );
    assert!(codes.iter().any(|(c, _)| *c == Code::GenAo), "{codes:?}");

    // Opened for AO (Size 64, its locked size), then Height ticked: the Size row shows what the height writes at.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    relief_normal(&land(d.path()), "rock01_normal.png", 128);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    assert_eq!(m.gen.as_ref().unwrap().opts.size, 64);
    m.gen_tick(GenMap::Height, true);
    let f = m.gen_facts();
    assert_eq!(f.shown_size, 128);
    assert_eq!(f.locked, vec![(GenMap::Ao, 64)]);
    assert_eq!(f.paths[0].2, (128, 128));

    // A 32² normal cannot make a 64² AO.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    flat_normal(&land(d.path()).join("rock01_normal.png"), 32);
    reopen(&mut m, d.path());
    let locked = "this set's maps are 64×64; a generated AO must match them (the normal is 32×32)";
    assert_eq!(ao(&m), Err(locked.into()));
    assert_eq!(m.gen_size(ROCK_SET, GenMap::Ao), Err(locked.into()));
}

#[test]
fn one_size_caps_each_map() {
    // No `_rmaos` source: AO offers up to 2048, the height stays at its own cap (1024 beside a 64² albedo).
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_roughness.png")).unwrap();
    flat_normal(&land(d.path()).join("rock01_normal.png"), 2048);
    reopen(&mut m, d.path());
    let free = GenSize::Free {
        sizes: vec![256, 512, 1024, 2048],
        default: 2048,
    };
    assert_eq!(m.gen_size(ROCK_SET, GenMap::Ao), Ok(free.clone()));
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    assert_eq!(
        m.gen.as_ref().unwrap().opts.size,
        2048,
        "the AO cap when only AO is ticked"
    );
    m.gen_tick(GenMap::Height, true);
    let dims: Vec<(GenMap, (u32, u32))> =
        m.gen_paths().into_iter().map(|(g, _, d)| (g, d)).collect();
    assert_eq!(
        dims,
        vec![(GenMap::Height, (1024, 1024)), (GenMap::Ao, (2048, 2048))]
    );
    assert_eq!(m.gen_size_warning(), Some(SIZE_WARNING));
    assert_eq!(
        m.gen_facts().size,
        Ok(free),
        "the Size row offers the larger cap"
    );
    m.gen_tick(GenMap::Ao, false);
    assert_eq!(m.gen.as_ref().unwrap().opts.ao, None);
}

#[test]
fn ao_preview_reports_mean_and_dark_share() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    m.gen_frame();
    idle(&mut m);
    let g = m.gen.as_ref().unwrap();
    assert!(g.preview.as_ref().unwrap().maps.ao.is_some());
    assert_eq!(
        m.gen_ao_summary().as_deref(),
        Some("AO: mean 1.00, 0 % of texels below 0.5")
    );
    // The preview runs at 8 slices × 8 steps; the dialog keeps the file's settings.
    assert_eq!(
        m.gen_preview_opts()
            .unwrap()
            .ao
            .map(|a| (a.slices, a.steps)),
        Some((8, 8))
    );
    let g = m.gen.as_ref().unwrap();
    assert_eq!(
        g.opts.ao.as_ref().map(|a| (a.slices, a.steps)),
        Some((16, 16))
    );
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(3);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(3);
    assert!(h
        .query_by_label("AO: mean 1.00, 0 % of texels below 0.5")
        .is_some());
    assert!(h.query_by_label("AO preview").is_some());
    for l in ["Radius", "Quality", "Depth", "Intensity"] {
        assert!(h.query_by_label(l).is_some(), "{l}");
    }
}

#[test]
fn the_ao_sliders_stay_inside_the_engine_guards() {
    // CO84: radius 0–0.25, slices and steps 1–64, depth 0.25–4, intensity 0.5–3.
    use sdc_gui::ui::from_normal::{AO_DEPTH, AO_INTENSITY, AO_RADIUS_PCT, AO_SLICES};
    assert_eq!(
        (*AO_RADIUS_PCT.start(), *AO_RADIUS_PCT.end()),
        (1.5625, 25.0)
    );
    assert!(*AO_RADIUS_PCT.end() / 100.0 <= 0.25);
    assert_eq!(AO_SLICES, [8, 16, 32]);
    assert_eq!((*AO_DEPTH.start(), *AO_DEPTH.end()), (0.25, 4.0));
    assert_eq!((*AO_INTENSITY.start(), *AO_INTENSITY.end()), (0.5, 3.0));
    assert_eq!(engine::from_normal::AoOpts::default().steps, 16);
}

/// The rnm_ui fixture (P18): `m/iron` with or without a bent layer.
fn bent_fixture(dir: &Path, bent: bool) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
        .save(r.join("iron.png"))
        .unwrap();
    let mut files = vec!["iron_n.png"];
    if bent {
        files.push("iron_bent.png");
    }
    for f in files {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
            .save(r.join(f))
            .unwrap();
    }
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(dir.join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

#[test]
fn the_bent_layer_note_shows_only_for_bent_layer_sets() {
    use egui_kittest::kittest::Queryable;
    // (bent layer, cs PBR so AO is offered): the note shows only where both hold (review M5: not where AO is refused).
    for (bent, pbr) in [(true, true), (false, true), (true, false)] {
        let d = tempfile::tempdir().unwrap();
        let mut m = bent_fixture(d.path(), bent);
        if pbr {
            m.set_shader("m/iron", Some(Shader::Pbr));
            idle(&mut m);
        }
        assert_eq!(m.has_bent_layer("m/iron"), bent);
        assert_eq!(m.gen_offer("m/iron", GenMap::Ao).is_ok(), pbr);
        m.open_gen("m/iron", None);
        assert!(m.gen.as_ref().unwrap().ticked.is_empty());
        let mut h = gen_harness(m, 1500.0, 900.0);
        h.run_steps(3);
        let want = bent && pbr;
        assert_eq!(
            h.query_by_label(BENT_NOTE).is_some(),
            want,
            "nothing ticked ({bent}, {pbr})"
        );
        if pbr {
            h.state_mut().gen_tick(GenMap::Ao, true);
            h.run_steps(3);
            assert_eq!(h.query_by_label(BENT_NOTE).is_some(), want, "AO ticked");
        }
        assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    }
}

#[test]
fn a_height_and_ao_write_roll_back_together() {
    // Two targets: the AO write fails, so the height this call wrote is removed, and one that already held the
    // same bytes is kept.
    let d = tempfile::tempdir().unwrap();
    let go = std::sync::atomic::AtomicBool::new(false);
    std::fs::write(d.path().join("afile"), b"x").unwrap();
    let ao = d.path().join("afile").join("rock01_ao_generated.png");
    let p = d.path().join("rock01_p_generated.png");
    let e = write_all(
        &[(p.clone(), vec![1], None), (ao.clone(), vec![2], None)],
        &go,
    )
    .unwrap_err();
    assert!(e.starts_with(&ao.display().to_string()), "{e}");
    assert!(!p.exists(), "rolled back");
    std::fs::write(&p, [1]).unwrap();
    assert!(write_all(&[(p.clone(), vec![1], None), (ao, vec![2], None)], &go).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), [1], "kept");
}

/// A slot row's status dot: (its AccessKit label, its rect), nearest the row label's height to its right, outside
/// the Sets column (whose chips share the label).
fn row_dot(
    h: &egui_kittest::Harness<'static, AppModel>,
    label: &str,
) -> (String, eframe::egui::Rect) {
    use egui_kittest::kittest::Queryable;
    let row = h
        .query_all_by_label(label)
        .map(|n| n.rect())
        .filter(|r| !in_sets_column(h, *r))
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the row");
    ["ready", "warning", "blocks export"]
        .iter()
        .flat_map(|l| {
            h.query_all_by_label(l)
                .map(move |n| (l.to_string(), n.rect()))
        })
        .filter(|(_, r)| r.min.x > row.max.x)
        .min_by(|a, b| {
            let dy = |r: &eframe::egui::Rect| (r.center().y - row.center().y).abs();
            dy(&a.1).total_cmp(&dy(&b.1))
        })
        .expect("the row's dot")
}

#[test]
fn a_generated_ao_shows_on_the_rmaos_row() {
    // P20: the AO fills no row of its own; the `_rmaos` row carries its provenance, stale dot and Delete.
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    relief_normal(&land(d.path()), "rock01_normal.png", 64);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    m.gen_generate();
    idle(&mut m);
    let rm = rmaos(&m).key.clone();
    assert_eq!(
        m.generated_feed(&rm).map(|g| g.from),
        Some(NORMAL.to_string())
    );
    let del = delete_op(&m.slot_menu(ROCK_SET, SlotKind::Rmaos))
        .expect("Delete generated file…")
        .clone();
    assert_eq!(
        del.action,
        MenuAction::Generated {
            set: ROCK_SET.into(),
            op: GenOp::Delete(AO_KEY.into())
        }
    );
    rewrite_normal(&mut m, d.path());
    assert_eq!(
        m.generated_info(AO_KEY).unwrap().stale.as_deref(),
        Some(EARLIER)
    );
    let rm_name = rmaos(&m).name.clone();
    select_rock(&mut m);
    let mut h = harness(m);
    h.run();
    // Review M1: no "generated" marker on the packed row; its File hover says "AO generated from …".
    assert!(
        h.query_all_by_label("Generated from rock01_normal.png")
            .all(|n| in_sets_column(&h, n.rect())),
        "no Flags marker on the _rmaos row"
    );
    let row = h
        .query_all_by_label("RMAOS")
        .map(|n| n.rect())
        .filter(|r| !in_sets_column(&h, *r))
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the _rmaos row");
    let file = h
        .query_all_by_label(&rm_name)
        .map(|n| n.rect())
        .filter(|r| !in_sets_column(&h, *r) && r.min.x > row.max.x)
        .max_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the _rmaos File cell");
    h.hover_at(file.center());
    h.run();
    assert!(h
        .query_by_label_contains("AO generated from rock01_normal.png")
        .is_some());
    let (dot, r) = row_dot(&h, "RMAOS");
    assert_eq!(dot, "warning");
    // The AO fill line already says it; the dot's hover adds one more.
    let stale_lines = |h: &egui_kittest::Harness<'static, AppModel>| {
        h.query_all_by_label_contains(&format!("stale: {EARLIER}"))
            .count()
    };
    let before = stale_lines(&h);
    assert!(before >= 1, "the fill line");
    h.hover_at(r.center());
    h.run();
    assert_eq!(stale_lines(&h), before + 1, "the dot's hover");
}

#[test]
fn the_dialog_looks_up_its_set_once_per_frame() {
    // Task 6 M6: one set lookup per frame, and the alias map once per plan.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    m.gen_tick(GenMap::Height, true);
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(3);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(2);
    let (views, aliases) = h.state().gen_lookups();
    let pass = h.ctx.cumulative_pass_nr();
    h.run_steps(5);
    let passes = h.ctx.cumulative_pass_nr() - pass;
    let (views2, aliases2) = h.state().gen_lookups();
    assert!(passes >= 5);
    assert_eq!(
        u64::from(views2 - views),
        passes,
        "one set lookup per frame"
    );
    assert_eq!(aliases2, aliases, "no alias rebuild without a new plan");
}

// ---- Task 10 fix round 1 ----

/// The support fixture's rock01 (no `_ao`) under `textures/pbr/landscape/`, as a cs PBR set `pbr/landscape/rock01`.
fn pbr_dir_rock(d: &Path) -> (AppModel, PathBuf) {
    let r = d.join("textures/pbr/landscape");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
        .save(r.join("rock01.png"))
        .unwrap();
    relief_normal(&r, "rock01_normal.png", 64);
    ImageBuffer::from_fn(64, 64, |x, _| Luma([120u8.wrapping_add(x as u8)]))
        .save(r.join("rock01_roughness.png"))
        .unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(d.join("textures"));
    idle(&mut m);
    m.decide(
        "pbr/landscape/rock01.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::PbrAlbedo),
    );
    m.decide(
        "pbr/landscape/rock01_normal.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::PbrNormal),
    );
    m.decide(
        "pbr/landscape/rock01_normal.png",
        sdc_gui::model::Answer::Green(GreenConvention::Directx),
    );
    idle(&mut m);
    m.set_shader(PBR_SET, Some(Shader::Pbr));
    idle(&mut m);
    (m, r)
}

const PBR_SET: &str = "pbr/landscape/rock01";

/// Review I1: the set's own earlier `_rmaos` export, written in place, is neither a DDS `_rmaos` nor a real AO source.
fn in_place_rmaos_keeps_ao(m: &mut AppModel, set: &str, dir: &Path, d: &Path, export: &Path) {
    m.sets_doc.as_mut().unwrap().set_export_folder(export);
    reopen(m, d);
    let out = m
        .items
        .iter()
        .find(|i| {
            i.key.ends_with("rock01_rmaos") && i.job.as_ref().is_some_and(|j| j.pack.is_some())
        })
        .expect("the packed _rmaos")
        .job
        .as_ref()
        .unwrap()
        .output
        .clone();
    assert_eq!(out, dir.join("rock01_rmaos.dds"), "in place");
    bc1_dds(&out, 64, 64);
    reopen(m, d);
    let row = |m: &AppModel| {
        ao_open(&m.slot_menu(set, SlotKind::Rmaos))
            .cloned()
            .expect("the _rmaos row's entry")
    };
    // Before any AO is generated (the `real_rival` scan and DDS_RMAOS).
    let e = row(m);
    assert_eq!((e.label.as_str(), e.disabled), (GENERATE, None));
    m.open_gen(set, Some(GenMap::Ao));
    m.gen_generate();
    idle(m);
    assert!(dir.join("rock01_ao_generated.png").exists());
    let e = row(m);
    assert_eq!((e.label.as_str(), e.disabled), ("Regenerate…", None));
}

#[test]
fn an_in_place_rmaos_export_under_textures_pbr_keeps_regenerate() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, dir) = pbr_dir_rock(d.path());
    // PBR outputs go to `textures\pbr\…` under the export root: the mod folder above the opened `textures`. The
    // engine reads the moved packed output too (CO102), so the generated AO is kept and Regenerate offered.
    in_place_rmaos_keeps_ao(&mut m, PBR_SET, &dir, d.path(), d.path());
}

#[test]
fn an_in_place_rmaos_export_of_a_set_with_no_prefix_keeps_ao_offered() {
    // A rename with a folder is a prefix error: the set has no PBR prefix and its outputs mirror the input keys.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    m.set_json_value(ROCK_SET, "rename", Some(json!("a/b")));
    idle(&mut m);
    let root = m.root.clone().unwrap();
    in_place_rmaos_keeps_ao(&mut m, ROCK_SET, &land(d.path()), d.path(), &root);
}

#[test]
fn only_ao_ticked_from_the_set_menu_takes_aos_size() {
    // Review M2: opened from the set menu (Height's Size), ticking only AO switches to AO's default.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_roughness.png")).unwrap();
    flat_normal(&land(d.path()).join("rock01_normal.png"), 2048);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, None);
    assert_eq!(m.gen.as_ref().unwrap().opts.size, 1024, "Height's default");
    m.gen_tick(GenMap::Ao, true);
    assert_eq!(m.gen.as_ref().unwrap().opts.size, 2048);
    assert_eq!(m.gen_paths()[0].2, (2048, 2048));
    // With Height ticked too, the user's Size stays.
    m.gen.as_mut().unwrap().opts.size = 512;
    m.gen_tick(GenMap::Height, true);
    assert_eq!(m.gen.as_ref().unwrap().opts.size, 512);
}

#[test]
fn progress_runs_once_across_two_generate_calls() {
    // Review M3: part `i` of `n` maps its 0–1000 into its share, so the line never restarts.
    assert_eq!(overall_progress(0, 0, 1), 0);
    assert_eq!(overall_progress(1000, 0, 1), 1000);
    assert_eq!(overall_progress(1000, 0, 2), 500);
    assert_eq!(overall_progress(0, 1, 2), 500);
    assert_eq!(overall_progress(400, 1, 2), 700);
    assert_eq!(overall_progress(1000, 1, 2), 1000);
    // The two-size job sets its parts: a 128 height beside a 64 AO.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    relief_normal(&land(d.path()), "rock01_normal.png", 128);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_tick(GenMap::Ao, true);
    m.gen_generate();
    let part = m.gen.as_ref().unwrap().part.clone();
    idle(&mut m);
    assert_eq!(
        part.load(std::sync::atomic::Ordering::Relaxed),
        (1 << 8) | 2,
        "ended on part 2 of 2"
    );
}

#[test]
fn only_ao_ticked_hides_strength_and_the_height_preview() {
    // Review M4.
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    m.gen_frame();
    idle(&mut m);
    assert_eq!(m.gen_preview_opts().unwrap().height, None);
    let g = m.gen.as_ref().unwrap();
    assert!(g.preview.as_ref().unwrap().maps.height.is_none());
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(3);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(3);
    assert!(h.query_by_label("Strength").is_none());
    assert!(h.query_by_label("Height preview").is_none());
    assert!(
        h.query_by_label_contains("Fit: mean").is_some(),
        "the solve's fit stays"
    );
    h.state_mut().gen_tick(GenMap::Height, true);
    h.run_steps(3);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(3);
    assert!(h.query_by_label("Strength").is_some());
    assert!(h.query_by_label("Height preview").is_some());
}

// ---- Task 12: curvature ----

const CURVES_KEY: &str = "landscape/rock01_curves_generated.png";

fn curves(m: &AppModel) -> Result<(), String> {
    m.gen_offer(ROCK_SET, GenMap::Curvature)
}

/// Replace the fixture's 64² albedo with a `size`² one.
fn albedo(d: &Path, size: u32) {
    ImageBuffer::from_fn(size, size, |x, y| Rgb([x as u8, y as u8, 90u8]))
        .save(land(d).join("rock01.png"))
        .unwrap();
}

#[test]
fn the_curvature_box_writes_curves_generated() {
    use egui_kittest::kittest::{NodeT, Queryable};
    // P10: Track A's role exists, so the box is always there, with Also's use on hover.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    assert_eq!(curves(&m), Ok(()));
    m.open_gen(ROCK_SET, None);
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(4);
    let b = h.get_by_label("Curvature");
    assert!(!b.accesskit_node().is_disabled());
    let r = b.rect();
    h.hover_at(r.center());
    h.run_steps(30);
    assert!(
        h.query_by_label(CURVATURE_HOVER).is_some(),
        "Also's use on hover"
    );
    assert_eq!(
        CURVATURE_HOVER,
        "used by Also export as: Vanilla / Complex material"
    );
    h.get_by_label("Curvature").click();
    // The open's preview lands, then the tick's preview starts and lands.
    for _ in 0..2 {
        h.run_steps(3);
        assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    }
    h.run_steps(3);
    assert!(
        h.query_by_label("Curvature preview").is_some(),
        "the curvature preview tile"
    );
    let mut m = h.into_state();
    let g = m.gen.as_ref().unwrap();
    assert_eq!(g.ticked, [GenMap::Curvature].into_iter().collect());
    assert!(g.opts.curvature);
    m.gen_generate();
    idle(&mut m);
    let p = land(d.path()).join("rock01_curves_generated.png");
    let img = image::open(&p).expect("the generated file");
    assert_eq!(img.color(), image::ColorType::L16, "16-bit grey");
    assert_eq!((img.width(), img.height()), (64, 64), "the albedo's size");
    assert_eq!(
        generated_pngs(&land(d.path())),
        vec!["rock01_curves_generated.png".to_string()],
        "only the ticked map"
    );
    let rec = &m.sets_doc.as_ref().unwrap().generated()[CURVES_KEY];
    assert_eq!(rec["kind"], "curvature");
    assert_eq!(rec["opts"]["curvature"], true);
    assert_eq!(
        rec["sha256"],
        sdc_gui::model::history::sha256_of(&p).unwrap().as_str()
    );
    // The re-plan reads it as Track A's Curves input: unused in a plain run (expected).
    let item = m.item(CURVES_KEY).expect("the generated item");
    assert!(
        item.warnings
            .iter()
            .any(|w| w.code == engine::codes::Code::PackUnusedInput
                && w.message
                    .contains("read only by Also export as (Vanilla / Complex material)")),
        "{:?}",
        item.warnings
    );
    // FC7 (CN3): an Info row with no button, not the generic "remove it" warning.
    use sdc_gui::model::bottom::{problems, At, Level};
    let rows: Vec<_> = problems(&m)
        .iter()
        .filter(|p| p.at == At::Texture(CURVES_KEY.into()))
        .cloned()
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].level, rows[0].text.as_str(), rows[0].fix.is_none()),
        (
            Level::Info,
            "Read only by Also export; not written on its own",
            true
        )
    );
}

#[test]
fn curvature_matches_the_albedo_size() {
    // P6: Also's diffuse reads the albedo and the curvature together, so the curvature takes the albedo's size.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    relief_normal(&land(d.path()), NORMAL, 128);
    reopen(&mut m, d.path());
    assert_eq!(
        m.gen_size(ROCK_SET, GenMap::Curvature),
        Ok(GenSize::Locked(64))
    );
    m.open_gen(ROCK_SET, None);
    m.gen_tick(GenMap::Curvature, true);
    let f = m.gen_facts();
    assert_eq!(f.locked, vec![(GenMap::Curvature, 64)]);
    assert_eq!(f.paths.len(), 1);
    assert_eq!(f.paths[0].2, (64, 64));
    m.gen_generate();
    idle(&mut m);
    assert_eq!(
        image::image_dimensions(land(d.path()).join("rock01_curves_generated.png")).unwrap(),
        (64, 64)
    );

    // A normal smaller than the albedo is never upsampled.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    flat_normal(&land(d.path()).join(NORMAL), 32);
    reopen(&mut m, d.path());
    let locked =
        "this set's maps are 64×64; a generated curvature map must match them (the normal is 32×32)";
    assert_eq!(curves(&m), Err(locked.into()));
    assert_eq!(m.gen_size(ROCK_SET, GenMap::Curvature), Err(locked.into()));
}

#[test]
fn curvature_reasons() {
    // A real `_curves` (or `_curvature`) map.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    ImageBuffer::from_fn(64, 64, |x, _| Luma([x as u8]))
        .save(land(d.path()).join("rock01_curvature.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(
        curves(&m),
        Err("this set already has a curvature map (rock01_curvature.png)".into())
    );
    // A `_curves_generated` Tessalith has no record of.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    ImageBuffer::from_fn(64, 64, |x, _| Luma([x as u8]))
        .save(land(d.path()).join("rock01_curves_generated.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(
        curves(&m),
        Err(
            "rock01_curves_generated.png exists but was not generated here; delete it to generate again"
                .into()
        )
    );
    // An NPOT normal; then no normal.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    flat_normal(&land(d.path()).join(NORMAL), 48);
    reopen(&mut m, d.path());
    assert_eq!(
        curves(&m),
        Err("the normal map is 48×48; only power-of-two sizes can be generated".into())
    );
    std::fs::remove_file(land(d.path()).join(NORMAL)).unwrap();
    reopen(&mut m, d.path());
    assert_eq!(curves(&m), Err(NO_NORMAL.into()));
}

#[test]
fn a_generated_curvature_shows_on_the_set_card() {
    // P20: curvature fills no slot row; the set card carries its Regenerate, Delete and stale reason.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, None);
    m.gen_tick(GenMap::Curvature, true);
    m.gen_generate();
    idle(&mut m);
    assert_eq!(
        m.generated_info(CURVES_KEY).map(|g| (g.from, g.stale)),
        Some((NORMAL.to_string(), None))
    );
    assert_eq!(m.gen_entry(ROCK_SET, None).label, "Regenerate…");
    assert!(m.delete_generated_entries(ROCK_SET).iter().any(|e| e.label
        == "rock01_curves_generated.png"
        && e.action
            == MenuAction::Generated {
                set: ROCK_SET.into(),
                op: GenOp::Delete(CURVES_KEY.into())
            }));
    rewrite_normal(&mut m, d.path());
    assert_eq!(
        m.generated_info(CURVES_KEY).unwrap().stale.as_deref(),
        Some(EARLIER)
    );
    // Regenerate from the set card ticks the recorded map (I1), replaces its own file (the recorded hash) and is
    // fresh again.
    m.open_gen(ROCK_SET, None);
    assert_eq!(
        m.gen.as_ref().unwrap().ticked,
        [GenMap::Curvature].into_iter().collect()
    );
    m.gen_generate();
    idle(&mut m);
    assert!(
        m.gen.is_none(),
        "{:?}",
        m.gen.as_ref().and_then(|g| g.error.clone())
    );
    assert_eq!(m.generated_info(CURVES_KEY).unwrap().stale, None);
}

#[test]
fn progress_runs_once_across_three_generate_calls() {
    // Height 256 (the normal's own), AO 64 (the roughness), curvature 128 (the albedo): three calls, one job.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    relief_normal(&land(d.path()), NORMAL, 256);
    albedo(d.path(), 128);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_tick(GenMap::Ao, true);
    m.gen_tick(GenMap::Curvature, true);
    let dims: Vec<(GenMap, (u32, u32))> =
        m.gen_paths().into_iter().map(|(g, _, d)| (g, d)).collect();
    assert_eq!(
        dims,
        vec![
            (GenMap::Height, (256, 256)),
            (GenMap::Ao, (64, 64)),
            (GenMap::Curvature, (128, 128))
        ]
    );
    m.gen_generate();
    let part = m.gen.as_ref().unwrap().part.clone();
    idle(&mut m);
    assert_eq!(
        part.load(std::sync::atomic::Ordering::Relaxed),
        (2 << 8) | 3,
        "ended on part 3 of 3"
    );
    assert_eq!(overall_progress(0, 1, 3), 333);
    assert_eq!(overall_progress(1000, 2, 3), 1000);
    let dim = |n: &str| image::image_dimensions(land(d.path()).join(n)).unwrap();
    assert_eq!(dim("rock01_p_generated.png"), (256, 256));
    assert_eq!(dim("rock01_ao_generated.png"), (64, 64));
    assert_eq!(dim("rock01_curves_generated.png"), (128, 128));
    let gen = m.sets_doc.as_ref().unwrap().generated();
    assert_eq!(gen[CURVES_KEY]["opts"]["size"], 128);
    assert_eq!(gen[AO_KEY]["opts"]["size"], 64);
}

// ---- Task 12 fix round 1 ----

#[test]
fn regenerate_from_the_set_card_loads_the_record() {
    // I1 (spec Goal 7): a curvature made with Mirror, OpenGL green and relief 0.8.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    m.open_gen(ROCK_SET, None);
    m.gen_tick(GenMap::Curvature, true);
    {
        let o = &mut m.gen.as_mut().unwrap().opts;
        o.tiling = Tiling::Mirror;
        o.green = Some(GreenConvention::Opengl);
        o.relief = 0.8;
    }
    m.gen_generate();
    idle(&mut m);
    assert!(m.gen.is_none());
    m.open_gen(ROCK_SET, None);
    let g = m.gen.as_ref().unwrap();
    assert_eq!(g.ticked, [GenMap::Curvature].into_iter().collect());
    assert_eq!(g.opts.tiling, Tiling::Mirror);
    assert_eq!(g.opts.green, Some(GreenConvention::Opengl));
    assert_eq!(g.opts.relief, 0.8);
    assert!(g.opts.curvature);

    // An `_m`-alpha height (its only entry is the set card) keeps its strength.
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    idle(&mut m);
    relief_normal(&land(d.path()), NORMAL, 64);
    reopen(&mut m, d.path());
    complex_material(&mut m, "rock01");
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    {
        let g = m.gen.as_mut().unwrap();
        g.opts.size = 64;
        g.opts.height.as_mut().unwrap().strength = 0.5;
    }
    m.gen_generate();
    idle(&mut m);
    m.open_gen(ROCK_SET, None);
    let g = m.gen.as_ref().unwrap();
    assert_eq!(g.ticked, [GenMap::Height].into_iter().collect());
    assert!(g.m_alpha);
    assert_eq!(g.opts.height.as_ref().unwrap().strength, 0.5);
}

#[test]
fn curvature_is_refused_where_also_export_reads_none() {
    // I2: Also derives only cs PBR, PBR landscape and PBR grass sets.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    shader(&mut m, Shader::Parallax);
    assert_eq!(curves(&m), Err(NOT_PBR_CURVES.into()));
    shader(&mut m, Shader::PbrLandscape);
    assert_eq!(curves(&m), Ok(()));
    m.set_target(Target::Vanilla);
    idle(&mut m);
    shader(&mut m, Shader::Pbr);
    assert_eq!(curves(&m), Err(NOT_PBR_CURVES.into()));

    // A PBR set with no albedo writes no Vanilla diffuse, so nothing would read the curvature.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    std::fs::remove_file(land(d.path()).join("rock01.png")).unwrap();
    reopen(&mut m, d.path());
    shader(&mut m, Shader::Pbr);
    assert_eq!(curves(&m), Err(NO_ALBEDO.into()));
    assert_eq!(
        m.gen_size(ROCK_SET, GenMap::Curvature),
        Err(NO_ALBEDO.into())
    );
}

#[test]
fn a_locked_size_that_is_not_a_power_of_two_is_refused() {
    // M1: a 1000² albedo beside a 1024² normal; never an "internal error" from the FFT.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    flat_normal(&land(d.path()).join(NORMAL), 1024);
    albedo(d.path(), 1000);
    reopen(&mut m, d.path());
    let locked = "this set's maps are 1000×1000; a generated curvature map must match them (not a power of two)";
    assert_eq!(curves(&m), Err(locked.into()));
    assert_eq!(m.gen_size(ROCK_SET, GenMap::Curvature), Err(locked.into()));
}

#[test]
fn curvature_takes_a_rectangular_or_dds_albedos_size() {
    // M2(c): a 64×32 albedo beside a 128×64 normal.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    ImageBuffer::from_fn(128, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(land(d.path()).join(NORMAL))
        .unwrap();
    ImageBuffer::from_fn(64, 32, |x, y| Rgb([x as u8, y as u8, 90u8]))
        .save(land(d.path()).join("rock01.png"))
        .unwrap();
    reopen(&mut m, d.path());
    assert_eq!(
        m.gen_size(ROCK_SET, GenMap::Curvature),
        Ok(GenSize::Locked(64))
    );
    m.open_gen(ROCK_SET, None);
    m.gen_tick(GenMap::Curvature, true);
    assert_eq!(m.gen_facts().paths[0].2, (64, 32));

    // A DDS albedo: Also derives no set with a DDS member (DERIVE-DDS-SOURCE), so the curvature is refused.
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    flat_normal(&land(d.path()).join(NORMAL), 128);
    std::fs::remove_file(land(d.path()).join("rock01.png")).unwrap();
    bc1_dds(&land(d.path()).join("rock01.dds"), 64, 64);
    reopen(&mut m, d.path());
    shader(&mut m, Shader::Pbr);
    assert_eq!(
        curves(&m),
        Err(
            "Also export skips a set with a DDS member (rock01.dds), so nothing would read a curvature"
                .into()
        )
    );
}

// ---- Final fix wave ----

#[test]
fn a_map_ticked_after_opening_loads_its_record() {
    // Final review M2: a height (Strength 0.6, 128) and an AO (Radius 0.1, Depth 2, 64) recorded in two calls.
    let d = tempfile::tempdir().unwrap();
    let mut m = ao_rock(d.path());
    relief_normal(&land(d.path()), NORMAL, 128);
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen
        .as_mut()
        .unwrap()
        .opts
        .height
        .as_mut()
        .unwrap()
        .strength = 0.6;
    m.gen_tick(GenMap::Ao, true);
    {
        let a = m.gen.as_mut().unwrap().opts.ao.as_mut().unwrap();
        (a.radius, a.depth) = (0.1, 2.0);
    }
    let ao_rec = m.gen.as_ref().unwrap().opts.ao.clone();
    m.gen_generate();
    idle(&mut m);
    assert!(m.gen.is_none());
    // Regenerate from the Height row, then tick AO: AO's recorded settings, not the defaults.
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    assert_eq!(m.gen.as_ref().unwrap().opts.ao, None);
    m.gen_tick(GenMap::Ao, true);
    assert_eq!(m.gen.as_ref().unwrap().opts.ao, ao_rec);
    // The other way round: opened for AO, then Height ticked keeps the recorded Strength.
    m.gen_cancel();
    m.open_gen(ROCK_SET, Some(GenMap::Ao));
    m.gen_tick(GenMap::Height, true);
    assert_eq!(
        m.gen
            .as_ref()
            .unwrap()
            .opts
            .height
            .as_ref()
            .unwrap()
            .strength,
        0.6
    );
}

#[test]
fn provenance_is_parsed_once_per_revision() {
    // Final review M4: the open dialog's offers read the generated table from a cache, not per frame.
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    generate_height(&mut m);
    m.open_gen(ROCK_SET, None);
    let _ = m.gen_facts();
    let before = m.gen_prov_builds();
    for _ in 0..5 {
        let _ = m.gen_facts();
        assert!(m.provenance(P_KEY).is_some());
    }
    assert_eq!(m.gen_prov_builds(), before, "no rebuild without an edit");
    m.gen_cancel();
    m.ask_delete_generated(P_KEY);
    m.confirm_delete_generated();
    idle(&mut m);
    assert!(m.provenance(P_KEY).is_none(), "a forgotten record is gone");
}

// ---- Generate button on the Height and RMAOS rows ----

const GEN_NAME: &str = "Generate from normal map…";

fn editor_of(mut m: AppModel) -> egui_kittest::Harness<'static, AppModel> {
    select_rock(&mut m);
    let mut h = harness(m);
    h.run();
    h
}

/// The set editor's slot row `label` (the leftmost match outside the Sets column).
fn editor_row(h: &egui_kittest::Harness<'static, AppModel>, label: &str) -> eframe::egui::Rect {
    use egui_kittest::kittest::Queryable;
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .filter(|r| !in_sets_column(h, *r))
        .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .unwrap_or_else(|| panic!("no editor row {label:?}"))
}

/// The Generate buttons (by accessible name) at or below row `label`'s top, nearest first.
fn gen_buttons_below(
    h: &egui_kittest::Harness<'static, AppModel>,
    label: &str,
) -> Vec<eframe::egui::Rect> {
    use egui_kittest::kittest::Queryable;
    let row = editor_row(h, label);
    let mut v: Vec<_> = h
        .query_all_by_label(GEN_NAME)
        .map(|n| n.rect())
        .filter(|r| {
            r.min.y >= row.min.y - 2.0 && r.min.y < row.min.y + 70.0 && !in_sets_column(h, *r)
        })
        .collect();
    v.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
    v
}

#[test]
fn the_height_row_has_a_generate_button_under_assign() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut h = editor_of(pbr_opened_plain(d.path()));
    let assign = h
        .query_all_by_label("Assign…")
        .map(|n| n.rect())
        .filter(|r| {
            let row = editor_row(&h, "Height _p");
            (r.center().y - row.center().y).abs() < 40.0
        })
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .expect("the Height row's Assign…");
    let btn = gen_buttons_below(&h, "Height _p")[0];
    assert!(btn.min.y >= assign.max.y, "{btn:?} under {assign:?}");
    assert!((btn.min.x - assign.min.x).abs() < 2.0, "{btn:?} {assign:?}");
    h.get_all_by_label(GEN_NAME)
        .find(|n| n.rect() == btn)
        .unwrap()
        .click();
    h.run();
    let g = h.state().gen.as_ref().expect("the Generate dialog opened");
    assert!(g.ticked.contains(&GenMap::Height), "{:?}", g.ticked);
}

/// Pre-packed `_rmaos.png` as the set's only R/G/A source, flat B (so AO is offered).
fn prepacked_rock(d: &Path) -> AppModel {
    let mut m = ao_rock(d);
    std::fs::remove_file(land(d).join("rock01_roughness.png")).unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([100u8, 0, 255]))
        .save(land(d).join("rock01_rmaos.png"))
        .unwrap();
    reopen(&mut m, d);
    m
}

fn rmaos_slot_kind(m: &AppModel) -> Option<sdc_gui::model::items::ItemKind> {
    use sdc_gui::model::sets::{set_views, SlotFill};
    let v = set_views(m).into_iter().find(|v| v.id == ROCK_SET)?;
    match v.slots.get(&SlotKind::Rmaos)? {
        SlotFill::Own { item } | SlotFill::Borrowed { item, .. } => m.item(item).map(|i| i.kind),
        SlotFill::Empty => None,
    }
}

#[test]
fn the_rmaos_row_has_the_button_unless_packed() {
    use egui_kittest::kittest::Queryable;
    use sdc_gui::model::items::ItemKind;
    // A plain `_rmaos` image: the button opens Generate with AO ticked.
    let d = tempfile::tempdir().unwrap();
    let m = prepacked_rock(d.path());
    assert_eq!(rmaos_slot_kind(&m), Some(ItemKind::Image));
    let mut h = editor_of(m);
    let btns = gen_buttons_below(&h, "RMAOS");
    assert_eq!(btns.len(), 1, "{btns:?}");
    h.get_all_by_label(GEN_NAME)
        .find(|n| n.rect() == btns[0])
        .unwrap()
        .click();
    h.run();
    let g = h.state().gen.as_ref().expect("the Generate dialog opened");
    assert!(g.ticked.contains(&GenMap::Ao), "{:?}", g.ticked);

    // A packed RMAOS (the auto recipe's output): no button on its row.
    let d = tempfile::tempdir().unwrap();
    let m = ao_rock(d.path());
    assert_eq!(rmaos_slot_kind(&m), Some(ItemKind::Packed));
    let h = editor_of(m);
    assert!(
        gen_buttons_below(&h, "RMAOS").is_empty(),
        "no Generate on a packed row"
    );
    assert_eq!(
        gen_buttons_below(&h, "Height _p").len(),
        1,
        "the Height row still has its own"
    );
}

#[test]
fn a_shared_packed_rmaos_has_no_button() {
    use sdc_gui::model::items::ItemKind;
    use sdc_gui::model::sets::{set_views, DropChoice, DropOutcome, SlotFill};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    let odd = m
        .items
        .iter()
        .find(|i| i.name == "odd.png")
        .unwrap()
        .key
        .clone();
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    assert!(matches!(
        m.drop_on_set(&rmaos, &id),
        DropOutcome::AskChoice { .. }
    ));
    assert_eq!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    );
    idle(&mut m);
    let v = set_views(&m).into_iter().find(|v| v.id == id).unwrap();
    let Some(SlotFill::Borrowed { item, .. }) = v.slots.get(&SlotKind::Rmaos) else {
        panic!(
            "the new set borrows the RMAOS: {:?}",
            v.slots.get(&SlotKind::Rmaos)
        );
    };
    assert_eq!(m.item(item).map(|i| i.kind), Some(ItemKind::Packed));
    let key = v.key.clone();
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    editor_row(&h, "RMAOS");
    assert!(gen_buttons_below(&h, "RMAOS").is_empty());
}
#[test]
fn an_npot_normal_shows_the_button_disabled_with_the_reason() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    ImageBuffer::from_fn(48, 48, |_, _| Rgb([128u8, 128, 255]))
        .save(land(d.path()).join("rock01_normal.png"))
        .unwrap();
    reopen(&mut m, d.path());
    let why = m
        .gen_entry(ROCK_SET, Some(GenMap::Height))
        .disabled
        .unwrap();
    let mut h = editor_of(m);
    let btn = gen_buttons_below(&h, "Height _p")[0];
    let node = h
        .get_all_by_label(GEN_NAME)
        .find(|n| n.rect() == btn)
        .unwrap();
    assert!(node.accesskit_node().is_disabled());
    h.hover_at(btn.center());
    h.run();
    assert!(
        h.query_all_by_label_contains(&why).next().is_some(),
        "the reason on hover"
    );
    assert!(h.state().gen.is_none());
}

// ---- Task 2: zoom and pan in the Generate preview ----

use sdc_gui::ui::from_normal::{zoom_uv, PreviewZoom, TILE, ZOOM_MAX};

fn zoom_key() -> eframe::egui::Id {
    eframe::egui::Id::new("gen-preview-zoom")
}

fn zoom_of(h: &egui_kittest::Harness<'static, AppModel>) -> PreviewZoom {
    h.ctx
        .data(|d| d.get_temp::<PreviewZoom>(zoom_key()))
        .unwrap_or_default()
}

/// An open dialog with its preview shown, `h` tall.
fn zoom_harness(d: &Path, h: f32) -> egui_kittest::Harness<'static, AppModel> {
    let mut m = relief_rock(d);
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    let mut h = gen_harness(m, 1500.0, h);
    h.run_steps(3);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(5);
    h
}

fn wheel(h: &mut egui_kittest::Harness<'static, AppModel>, label: &str, dy: f32) {
    use egui_kittest::kittest::Queryable;
    h.get_by_label(label).hover();
    h.run_steps(2);
    h.input_mut().events.push(eframe::egui::Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Point,
        delta: eframe::egui::vec2(0.0, dy),
        phase: eframe::egui::TouchPhase::Move,
        modifiers: eframe::egui::Modifiers::NONE,
    });
    // The wheel is smoothed over several frames; never idle meanwhile.
    h.run_steps(8);
}

#[test]
fn zoom_uv_stays_inside_the_texture() {
    use eframe::egui::{pos2, vec2, Rect};
    let near =
        |a: Rect, b: Rect| (a.min - b.min).length() < 1e-6 && (a.max - b.max).length() < 1e-6;
    let z = |zoom, x, y| PreviewZoom {
        zoom,
        centre: vec2(x, y),
    };
    let r = |a, b, c, d| Rect::from_min_max(pos2(a, b), pos2(c, d));
    assert!(near(zoom_uv(&z(1.0, 0.5, 0.5)), r(0.0, 0.0, 1.0, 1.0)));
    assert!(near(
        zoom_uv(&z(4.0, 0.5, 0.5)),
        r(0.375, 0.375, 0.625, 0.625)
    ));
    assert!(near(zoom_uv(&z(4.0, 0.0, 1.0)), r(0.0, 0.75, 0.25, 1.0)));
    assert_eq!(PreviewZoom::default().zoom, 1.0);
    assert_eq!(PreviewZoom::default().centre, vec2(0.5, 0.5));
    assert_eq!((TILE, ZOOM_MAX), (160.0, 16.0));
}

#[test]
fn a_wheel_over_a_tile_zooms_it_and_not_the_dialog() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    // Short, so the body scrolls.
    let mut h = zoom_harness(d.path(), 520.0);
    // Scroll the body down off its top with a wheel over a grid label, so a leaked wheel would move it back up.
    wheel(&mut h, "Size", -60.0);
    h.run_steps(3);
    let before = h.get_by_label("Height preview").rect();
    assert!(
        before.min.y < 400.0,
        "the tile is still on screen: {before:?}"
    );
    wheel(&mut h, "Height preview", 3.0);
    h.run_steps(3);
    assert!(zoom_of(&h).zoom > 1.0, "zoomed: {:?}", zoom_of(&h));
    let after = h.get_by_label("Height preview").rect();
    assert_eq!(before.min.y, after.min.y, "the dialog body did not scroll");
}

#[test]
fn a_wheel_down_over_a_tile_at_fit_scrolls_the_dialog_body() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut h = zoom_harness(d.path(), 520.0);
    // Scroll the body a little, so there is room to move both ways.
    wheel(&mut h, "Size", -20.0);
    h.run_steps(3);
    let before = h.get_by_label("Height preview").rect();
    // Zoom out at fit changes nothing, so the wheel must reach the body.
    wheel(&mut h, "Height preview", -20.0);
    h.run_steps(3);
    assert_eq!(zoom_of(&h).zoom, 1.0);
    let after = h.get_by_label("Height preview").rect();
    assert!(
        after.min.y < before.min.y,
        "the body scrolled: {before:?} -> {after:?}"
    );
}
/// Strength and the Height tile are inside the dialog body's visible part at once (final review, Important 2).
/// With Height alone the whole tile fits at 800 pt. With AO (and Curvature) ticked the options grow, and the 160 pt
/// tile row is cut at the fold (measured: 10 pt for Height + AO, 50 pt for all three at 1100x800): Strength stays in
/// view and more than half of each tile shows. Set `SDC_GEN_SHOTS` to a folder to also save the PNGs.
#[test]
fn the_controls_and_the_tiles_are_visible_together() {
    use egui_kittest::kittest::Queryable;
    for (w, hgt) in [(1100.0, 800.0), (1280.0, 800.0), (1600.0, 900.0)] {
        for ticks in [
            &[][..],
            &[GenMap::Ao][..],
            &[GenMap::Ao, GenMap::Curvature][..],
        ] {
            let d = tempfile::tempdir().unwrap();
            let mut m = relief_rock(d.path());
            std::fs::remove_file(land(d.path()).join("rock01_ao.png")).unwrap();
            reopen(&mut m, d.path());
            m.open_gen(ROCK_SET, Some(GenMap::Height));
            for &t in ticks {
                m.gen_tick(t, true);
            }
            let mut h = gen_harness(m, w, hgt);
            h.run_steps(3);
            assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
            h.run_steps(5);
            if let Ok(dir) = std::env::var("SDC_GEN_SHOTS") {
                let img = h.render().expect("render");
                let name = format!("generate-{w}x{hgt}-{}maps.png", ticks.len() + 1);
                img.save(Path::new(&dir).join(name)).expect("save png");
            }
            let strength = h.get_by_label("Strength").rect();
            let tile = h.get_by_label("Height preview").rect();
            let top = h.get_by_label("Generate from normal map").rect().max.y;
            let bottom = top + hgt * 0.6;
            let ctx = format!(
                "{} maps at {w}x{hgt}, body {top}..{bottom}",
                ticks.len() + 1
            );
            assert!(
                strength.min.y >= top && strength.max.y <= bottom,
                "Strength {strength:?} outside the body: {ctx}"
            );
            let need = if ticks.is_empty() {
                tile.max.y
            } else {
                tile.center().y
            };
            assert!(need <= bottom, "Height tile {tile:?} below the fold: {ctx}");
        }
    }
}
#[test]
fn the_zoom_survives_a_new_preview() {
    let d = tempfile::tempdir().unwrap();
    let mut h = zoom_harness(d.path(), 900.0);
    wheel(&mut h, "Height preview", 200.0);
    h.run_steps(3);
    let z = zoom_of(&h);
    assert!(z.zoom > 1.0);
    let started = h.state().gen.as_ref().unwrap().started;
    h.state_mut()
        .gen
        .as_mut()
        .unwrap()
        .opts
        .height
        .as_mut()
        .unwrap()
        .strength = 0.5;
    h.run_steps(3);
    assert!(
        h.state().gen.as_ref().unwrap().started > started,
        "re-previewed"
    );
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(5);
    assert_eq!(zoom_of(&h), z);
}

#[test]
fn double_click_resets_the_zoom() {
    use egui_kittest::kittest::Queryable;
    let d = tempfile::tempdir().unwrap();
    let mut h = zoom_harness(d.path(), 900.0);
    wheel(&mut h, "Height preview", 200.0);
    h.run_steps(3);
    assert!(zoom_of(&h).zoom > 1.0);
    let pos = h.get_by_label("Height preview").rect().center();
    h.get_by_label("Height preview").hover();
    h.run_steps(2);
    for pressed in [true, false, true, false] {
        h.input_mut()
            .events
            .push(eframe::egui::Event::PointerButton {
                pos,
                button: eframe::egui::PointerButton::Primary,
                pressed,
                modifiers: eframe::egui::Modifiers::NONE,
            });
    }
    h.run_steps(3);
    let z = zoom_of(&h);
    assert_eq!((z.zoom, z.centre), (1.0, eframe::egui::vec2(0.5, 0.5)));
}

#[test]
fn a_reopened_dialog_starts_at_fit() {
    let d = tempfile::tempdir().unwrap();
    let mut h = zoom_harness(d.path(), 900.0);
    wheel(&mut h, "Height preview", 200.0);
    h.run_steps(3);
    assert!(zoom_of(&h).zoom > 1.0);
    h.state_mut().gen_cancel();
    h.run_steps(3);
    h.state_mut().open_gen(ROCK_SET, Some(GenMap::Height));
    h.run_steps(3);
    assert_eq!(zoom_of(&h).zoom, 1.0);
}

#[test]
fn the_three_tiles_fit_the_dialog_body_without_a_horizontal_scroll() {
    use egui_kittest::kittest::Queryable;
    for w in [1100.0, 1280.0, 1600.0] {
        let d = tempfile::tempdir().unwrap();
        let mut m = relief_rock(d.path());
        std::fs::remove_file(land(d.path()).join("rock01_ao.png")).unwrap();
        reopen(&mut m, d.path());
        m.open_gen(ROCK_SET, Some(GenMap::Height));
        m.gen_tick(GenMap::Ao, true);
        m.gen_tick(GenMap::Curvature, true);
        let mut h = gen_harness(m, w, 900.0);
        h.run_steps(3);
        assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
        h.run_steps(5);
        let rects: Vec<_> = ["Height preview", "AO preview", "Curvature preview"]
            .iter()
            .map(|l| h.get_by_label(l).rect())
            .collect();
        let left = rects.iter().map(|r| r.min.x).fold(f32::MAX, f32::min);
        let right = rects.iter().map(|r| r.max.x).fold(0.0, f32::max);
        assert!(right - left <= 640.0, "tiles span {left}..{right} at {w}");
        assert!(right <= w, "{right} past the window at {w}");
    }
}

// ---- Task 2 fix round 1: the drawn crop ----

fn meshes(s: &eframe::egui::Shape, out: &mut Vec<eframe::egui::Rect>, tile: eframe::egui::Rect) {
    match s {
        eframe::egui::Shape::Vec(v) => v.iter().for_each(|s| meshes(s, out, tile)),
        // An egui Image paints a textured rect: the crop is the brush's uv.
        eframe::egui::Shape::Rect(r) => {
            if let Some(b) = &r.brush {
                if (r.rect.min - tile.min).length() < 0.6 && (r.rect.max - tile.max).length() < 0.6
                {
                    out.push(b.uv);
                }
            }
        }
        _ => {}
    }
}

/// The UV rect the tile's image mesh was painted with in the last frame.
fn drawn_uv(h: &egui_kittest::Harness<'static, AppModel>, label: &str) -> eframe::egui::Rect {
    use egui_kittest::kittest::Queryable;
    let tile = h.get_by_label(label).rect();
    let mut found = vec![];
    for cs in &h.output().shapes {
        meshes(&cs.shape, &mut found, tile);
    }
    assert_eq!(found.len(), 1, "one mesh over {label} at {tile:?}");
    found[0]
}

fn near_uv(a: eframe::egui::Rect, b: eframe::egui::Rect) -> bool {
    (a.min - b.min).length() < 1e-4 && (a.max - b.max).length() < 1e-4
}

fn wheel_at(h: &mut egui_kittest::Harness<'static, AppModel>, pos: eframe::egui::Pos2, dy: f32) {
    h.hover_at(pos);
    h.run_steps(2);
    h.input_mut().events.push(eframe::egui::Event::MouseWheel {
        unit: eframe::egui::MouseWheelUnit::Point,
        delta: eframe::egui::vec2(0.0, dy),
        phase: eframe::egui::TouchPhase::Move,
        modifiers: eframe::egui::Modifiers::NONE,
    });
    h.run_steps(10);
}

#[test]
fn the_drawn_crop_follows_the_zoom_and_every_tile_shows_the_same_one() {
    use eframe::egui::{pos2, vec2, Rect};
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    std::fs::remove_file(land(d.path()).join("rock01_ao.png")).unwrap();
    reopen(&mut m, d.path());
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_tick(GenMap::Ao, true);
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(3);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run_steps(5);
    use egui_kittest::kittest::Queryable;
    h.run_steps(10);
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    assert!(near_uv(drawn_uv(&h, "Height preview"), full));
    // Near the tile's top-left corner.
    let tile = h.get_by_label("Height preview").rect();
    let pos = tile.min + tile.size() * vec2(0.1, 0.15);
    let frac = vec2(0.1, 0.15);
    let under = |uv: Rect| uv.min.to_vec2() + frac * uv.size();
    let before = under(drawn_uv(&h, "Height preview"));
    wheel_at(&mut h, pos, 900.0);
    let z = zoom_of(&h);
    assert!(z.zoom > 2.0, "{z:?}");
    let uv = drawn_uv(&h, "Height preview");
    assert!(near_uv(uv, zoom_uv(&z)), "drawn {uv:?} vs stored {z:?}");
    assert!(uv.width() < 0.5, "the crop is zoomed: {uv:?}");
    assert!(
        (under(uv) - before).length() < 1e-3,
        "the UV under the pointer moved: {before:?} -> {:?}",
        under(uv)
    );
    // Both tiles show the one crop.
    assert!(near_uv(drawn_uv(&h, "AO preview"), uv));
    // Drag far past the edge: the crop stays inside the texture and the centre moved.
    let c0 = z.centre;
    let from = h.get_by_label("Height preview").rect().center();
    h.hover_at(from);
    h.run_steps(2);
    h.input_mut()
        .events
        .push(eframe::egui::Event::PointerButton {
            pos: from,
            button: eframe::egui::PointerButton::Primary,
            pressed: true,
            modifiers: eframe::egui::Modifiers::NONE,
        });
    h.run_steps(2);
    for i in 1..=25 {
        h.hover_at(from + vec2(30.0 * i as f32, 0.0));
        h.run_steps(1);
    }
    let z2 = zoom_of(&h);
    h.run_steps(3);
    let z2 = zoom_of(&h).centre.x.min(z2.centre.x);
    assert!(z2 < c0.x - 0.05, "panned: {c0:?} -> {z2}");
    let uv = drawn_uv(&h, "Height preview");
    assert!(
        uv.min.x >= 0.0 && uv.min.y >= 0.0 && uv.max.x <= 1.0 + 1e-6 && uv.max.y <= 1.0 + 1e-6,
        "inside the texture: {uv:?}"
    );
    assert!(near_uv(uv, zoom_uv(&zoom_of(&h))));
    assert!(uv.min.x < 1e-3, "pinned at the left edge: {uv:?}");
}

/// Review C19 (Note FB13): the greyed AO box says why on hover, with the real rule from the model.
#[test]
fn a_greyed_ao_box_says_why_on_hover() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    // The support fixture's rock01_ao.png is a real AO source, so the offer is off.
    let why = "this set already has an AO source (rock01_ao.png)";
    assert_eq!(m.gen_offer(ROCK_SET, GenMap::Ao), Err(why.into()));
    m.open_gen(ROCK_SET, None);
    let mut h = gen_harness(m, 1500.0, 900.0);
    h.run_steps(4);
    let b = h.get_by_role_and_label(eframe::egui::accesskit::Role::CheckBox, "AO");
    assert!(b.accesskit_node().is_disabled());
    h.hover_at(b.rect().center());
    h.run_steps(30);
    assert!(
        h.query_by_label(why).is_some(),
        "the rule is on the greyed box's hover"
    );
}
