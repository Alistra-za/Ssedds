//! Helper textures Task 8 (Note DY17): the helper panel edits the helper file's `helper.*` keys in `sdc.toml` (role,
//! fills, read from, invert; `rnm.target` for normal-type roles), one undo step each. Temp dirs only.
mod support;
use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::items::ItemKind;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use support::session;

type H = Harness<'static, AppModel>;

/// A folder with `files` (a diffuse for `<base>.png`, flat normals for `_n` and `_cnr`, grey ramps for the rest) and
/// `toml` as its `sdc.toml`, opened with high-confidence suggestions accepted.
fn fixture(dir: &Path, files: &[&str], toml: &str) -> AppModel {
    for f in files {
        let p = dir.join(f);
        if f.ends_with("_n.png") || f.ends_with("_cnr.png") {
            ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
                .save(p)
                .unwrap();
        } else if !f.contains('_') {
            ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
                .save(p)
                .unwrap();
        } else {
            ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
                .save(p)
                .unwrap();
        }
    }
    if !toml.is_empty() {
        std::fs::write(dir.join("sdc.toml"), toml).unwrap();
    }
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

const XSET: [&str; 3] = ["x.png", "x_roughness.png", "x_opacity.png"];
const CNRSET: [&str; 3] = ["x.png", "x_roughness.png", "x_cnr.png"];

/// The app at `size` with `key` selected.
fn open(m: AppModel, key: &str, size: egui::Vec2) -> H {
    let mut m = m;
    m.select_item(key);
    let mut h = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.run();
    h
}

fn app(m: AppModel, key: &str) -> H {
    open(m, key, egui::vec2(1500.0, 900.0))
}

fn value(h: &H, key: &str, field: &str) -> Option<Value> {
    h.state().project.as_ref().unwrap().file_value(key, field)
}

/// Waits for the re-plan an edit started, then draws its result.
fn settle(h: &mut H) {
    h.run();
    idle(h.state_mut());
    h.run();
    h.run();
}

/// Opens the combo named `name` and picks `option`.
fn pick(h: &mut H, name: &str, option: &str) {
    let below = h.get_by_role_and_label(Role::ComboBox, name).rect().max.y;
    h.get_by_role_and_label(Role::ComboBox, name).click();
    h.run();
    // The option in the open popup: the nearest node of that label below the combo (a "G" or "CNR" may be elsewhere).
    h.query_all_by_label(option)
        .filter(|n| n.rect().min.y >= below - 1.0)
        .min_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .unwrap_or_else(|| panic!("no {option} under {name}"))
        .click();
    settle(h);
}

fn click(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::Button, name).click();
    settle(h);
}

/// The Textures column's helper row for `file`, by its accessible name.
fn row_name(h: &H, file: &str) -> Option<String> {
    let prefix = format!("Helper {file}:");
    h.query_all_by_role(Role::Button)
        .filter_map(|n| n.accesskit_node().label())
        .find(|l| l.starts_with(&prefix))
}

#[test]
fn role_combo_writes_helper_role() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(d.path(), &XSET, ""), "x_roughness.png");
    assert_eq!(
        h.query_all_by_label("Reset role").count(),
        0,
        "guessed role"
    );
    pick(&mut h, "Helper role", "Glossiness");
    assert_eq!(
        value(&h, "x_roughness.png", "helper.role"),
        Some(json!("glossiness"))
    );
    assert!(row_name(&h, "x_roughness.png")
        .unwrap()
        .contains("Glossiness (set by you)"));
    click(&mut h, "Reset role");
    assert_eq!(value(&h, "x_roughness.png", "helper.role"), None);
}

#[test]
fn read_and_invert_write_their_keys() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(d.path(), &XSET, ""), "x_roughness.png");
    let read = h.get_by_role_and_label(Role::ComboBox, "Read from");
    assert_eq!(read.accesskit_node().value().as_deref(), Some("Auto (R)"));
    pick(&mut h, "Read from", "G");
    assert_eq!(
        value(&h, "x_roughness.png", "helper.read"),
        Some(json!("g"))
    );
    pick(&mut h, "Read from", "Luminance");
    assert_eq!(
        value(&h, "x_roughness.png", "helper.read"),
        Some(json!("luma"))
    );
    pick(&mut h, "Read from", "Auto (R)");
    assert_eq!(value(&h, "x_roughness.png", "helper.read"), None);
    h.get_by_role_and_label(Role::CheckBox, "Invert").click();
    settle(&mut h);
    assert_eq!(
        value(&h, "x_roughness.png", "helper.invert"),
        Some(json!(true))
    );
    h.get_by_role_and_label(Role::CheckBox, "Invert").click();
    settle(&mut h);
    assert_eq!(value(&h, "x_roughness.png", "helper.invert"), None);
}

#[test]
fn not_a_helper_makes_a_tile() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(d.path(), &XSET, ""), "x_opacity.png");
    assert!(row_name(&h, "x_opacity.png").is_some());
    pick(&mut h, "Helper role", "Not a helper");
    assert_eq!(
        value(&h, "x_opacity.png", "helper.role"),
        Some(json!("none"))
    );
    let m = h.state();
    let item = m.items.iter().find(|i| i.name == "x_opacity.png").unwrap();
    assert_ne!(item.kind, ItemKind::Helper);
    assert!(m
        .visible_items()
        .iter()
        .any(|&i| m.items[i].name == "x_opacity.png"));
    assert!(row_name(&h, "x_opacity.png").is_none(), "no helper row");
    h.get_by_label("Not a helper (set by you)");
    click(&mut h, "Reset role");
    assert_eq!(value(&h, "x_opacity.png", "helper.role"), None);
    assert!(row_name(&h, "x_opacity.png").is_some(), "the row is back");
}

/// The brief's "a `subtype` key already on that file is left untouched throughout". With a `subtype` key, clearing
/// `helper.role` leaves the file ordinary (Ruling 13: the sub-type escape hatch), so the row cannot come back here.
#[test]
fn not_a_helper_leaves_a_subtype_key_alone() {
    let d = tempfile::tempdir().unwrap();
    let toml = "[files.\"x_opacity.png\"]\nsubtype = \"diffuse\"\nhelper.role = \"opacity\"\n";
    let mut h = app(fixture(d.path(), &XSET, toml), "x_opacity.png");
    let subtype = Some(json!("diffuse"));
    assert!(
        row_name(&h, "x_opacity.png").is_some(),
        "hand-set role wins"
    );
    pick(&mut h, "Helper role", "Not a helper");
    assert_eq!(value(&h, "x_opacity.png", "subtype"), subtype);
    assert!(row_name(&h, "x_opacity.png").is_none());
    click(&mut h, "Reset role");
    assert_eq!(value(&h, "x_opacity.png", "helper.role"), None);
    assert_eq!(value(&h, "x_opacity.png", "subtype"), subtype);
}

#[test]
fn fills_add_and_remove_write_the_list() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(d.path(), &CNRSET, ""), "x_roughness.png");
    h.get_by_label("suggested");
    pick(&mut h, "Add fill target", "x_cnr alpha");
    assert_eq!(
        value(&h, "x_roughness.png", "helper.fills"),
        Some(json!(["rmaos.r", "cnr.a"]))
    );
    assert_eq!(h.query_all_by_label("suggested").count(), 0, "hand-set");
    click(&mut h, "Stop filling x_rmaos R");
    assert_eq!(
        value(&h, "x_roughness.png", "helper.fills"),
        Some(json!(["cnr.a"]))
    );
    click(&mut h, "Stop filling x_cnr alpha");
    assert_eq!(
        value(&h, "x_roughness.png", "helper.fills"),
        Some(json!([]))
    );
    let row = row_name(&h, "x_roughness.png").unwrap();
    assert!(row.contains("not used"), "{row}");
    h.get_by_label("not used: no targets chosen");
}

/// Final review M1: a hand-set list shows every target from `sdc.toml` (DY18b), so ✕ on one never drops a hand target
/// that did not land (`n.a` under vanilla with no `_n`).
#[test]
fn hand_targets_that_did_not_land_stay_listed() {
    let d = tempfile::tempdir().unwrap();
    let toml = "[files.\"x_opacity.png\"]\nhelper.fills = [\"diffuse.a\", \"n.a\"]\n";
    let mut m = fixture(d.path(), &["x.png", "x_opacity.png"], toml);
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    let mut h = app(m, "x_opacity.png");
    h.get_by_role_and_label(Role::Button, "Stop filling x alpha");
    h.get_by_role_and_label(Role::Button, "Stop filling x_n alpha");
    click(&mut h, "Stop filling x alpha");
    assert_eq!(
        value(&h, "x_opacity.png", "helper.fills"),
        Some(json!(["n.a"]))
    );
}

#[test]
fn fills_reset_returns_to_suggestion() {
    let d = tempfile::tempdir().unwrap();
    let toml = "[files.\"x_roughness.png\"]\nhelper.fills = [\"cnr.a\"]\n";
    let mut h = app(fixture(d.path(), &CNRSET, toml), "x_roughness.png");
    assert_eq!(h.query_all_by_label("suggested").count(), 0);
    click(&mut h, "Reset fills");
    assert_eq!(value(&h, "x_roughness.png", "helper.fills"), None);
    h.get_by_label("suggested");
    assert_eq!(h.query_all_by_label("Reset fills").count(), 0);
}

#[test]
fn read_and_invert_hidden_for_normal_roles() {
    let d = tempfile::tempdir().unwrap();
    let files = ["x.png", "x_n.png", "x_cnr.png", "x_detailnorm.png"];
    let mut h = app(fixture(d.path(), &files, ""), "x_detailnorm.png");
    assert_eq!(h.query_all_by_label("Read from").count(), 0);
    assert_eq!(h.query_all_by_label("Invert").count(), 0);
    assert_eq!(h.query_all_by_label("Add fill target").count(), 0);
    pick(&mut h, "Layer target", "CNR");
    assert_eq!(
        value(&h, "x_detailnorm.png", "rnm.target"),
        Some(json!("cnr"))
    );
}

#[test]
fn read_only_project_disables_controls() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(d.path(), &XSET, "target = [\n"), "x_roughness.png");
    for (role, name) in [
        (Role::ComboBox, "Helper role"),
        (Role::ComboBox, "Add fill target"),
        (Role::ComboBox, "Read from"),
        (Role::CheckBox, "Invert"),
        (Role::Button, "Stop filling x_rmaos R"),
    ] {
        let n = h.get_by_role_and_label(role, name);
        assert!(n.accesskit_node().is_disabled(), "{name} is enabled");
    }
    h.get_by_role_and_label(Role::ComboBox, "Helper role")
        .hover();
    h.run();
    assert!(
        h.query_by_label_contains("sdc.toml is read-only").is_some(),
        "the reason on hover"
    );
}

#[test]
fn panel_controls_inside_panel_at_1100() {
    let d = tempfile::tempdir().unwrap();
    // A long set name makes every fill label long.
    let base = format!("x{}", "a".repeat(64));
    let long = format!("{base}_roughness.png");
    let (diffuse, cnr) = (format!("{base}.png"), format!("{base}_cnr.png"));
    let files = [diffuse.as_str(), cnr.as_str(), long.as_str()];
    let toml = format!("[files.\"{long}\"]\nhelper.fills = [\"rmaos.r\", \"cnr.a\"]\n");
    let h = open(
        fixture(d.path(), &files, &toml),
        &long,
        egui::vec2(1100.0, 800.0),
    );
    let centre = sdc_gui::ui::centre_rect(&h.ctx).expect("the centre is drawn");
    let mut seen = vec![];
    for n in h.query_all(egui_kittest::kittest::by()) {
        let ak = n.accesskit_node();
        if !matches!(
            ak.role(),
            Role::Button | Role::ComboBox | Role::CheckBox | Role::Link
        ) {
            continue;
        }
        let r = n.rect();
        if r.min.x < centre.min.x || r.min.x >= centre.max.x || r.min.y < centre.min.y {
            continue;
        }
        let name = ak.label().unwrap_or_default();
        assert!(
            r.max.x <= centre.max.x,
            "{name} ends at {} past {}",
            r.max.x,
            centre.max.x
        );
        seen.push(name);
    }
    let stop_cnr = format!("Stop filling {base}_cnr alpha");
    let stop_rmaos = format!("Stop filling {base}_rmaos R");
    for want in [
        "Helper role",
        "Add fill target",
        "Read from",
        "Invert",
        &stop_cnr,
        &stop_rmaos,
        "Reset fills",
    ] {
        assert!(seen.iter().any(|s| s == want), "{want} not in {seen:?}");
    }
}

#[test]
fn one_undo_step_per_edit() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(fixture(d.path(), &XSET, ""), "x_roughness.png");
    // Steps before the edit: the opening's "Set n types automatically" (Note FC1), if any.
    let steps = h.state().history.undo.len();
    pick(&mut h, "Helper role", "Glossiness");
    assert!(value(&h, "x_roughness.png", "helper.role").is_some());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    settle(&mut h);
    assert_eq!(value(&h, "x_roughness.png", "helper.role"), None);
    assert_eq!(h.state().history.undo.len(), steps, "one step");
}

/// Review pictures of the helper panel at 1100, 1280 and 1600 px (egui-ui-rules step 5). Needs a GPU and
/// `SDC_SHOTS_DIR`: `cargo test -p sdc-gui --test helper_panel -- --ignored shots`.
#[test]
#[ignore]
fn shots() {
    let Ok(out) = std::env::var("SDC_SHOTS_DIR") else {
        return;
    };
    let d = tempfile::tempdir().unwrap();
    let files = [
        "x.png",
        "x_roughness.png",
        "x_cnr.png",
        "x_n.png",
        "x_detailnorm.png",
    ];
    let toml = "[files.\"x_roughness.png\"]\nhelper.fills = [\"rmaos.r\", \"cnr.a\"]\nhelper.read = \"g\"\n";
    for (w, hgt) in [(1100.0, 800.0), (1280.0, 800.0), (1600.0, 900.0)] {
        for key in ["x_roughness.png", "x_detailnorm.png"] {
            let mut m = fixture(d.path(), &files, toml);
            m.select_item(key);
            let mut h = Harness::builder()
                .with_size(egui::vec2(w, hgt))
                .with_pixels_per_point(1.0)
                .wgpu()
                .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
            sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
            h.run_steps(10);
            let img = h.render().expect("render");
            let stem = key.trim_end_matches(".png");
            img.save(Path::new(&out).join(format!("helper-panel-{stem}-{w}.png")))
                .expect("save png");
        }
    }
}

/// Review fix (DY17): a layer with no prefix in a set with `_cnr` and no `_n` blends into `_cnr` (Note W4); the panel
/// shows the engine's target, and picking a value writes `rnm.target` even when it is the one shown.
#[test]
fn layer_target_follows_the_engine() {
    let d = tempfile::tempdir().unwrap();
    let files = ["x.png", "x_cnr.png", "x_detailnorm.png"];
    let mut h = app(fixture(d.path(), &files, ""), "x_detailnorm.png");
    let combo = h.get_by_role_and_label(Role::ComboBox, "Layer target");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("CNR"));
    pick(&mut h, "Layer target", "CNR");
    assert_eq!(
        value(&h, "x_detailnorm.png", "rnm.target"),
        Some(json!("cnr"))
    );
    pick(&mut h, "Layer target", "Normal");
    assert_eq!(
        value(&h, "x_detailnorm.png", "rnm.target"),
        Some(json!("n"))
    );
    let rep = h
        .state()
        .item("x_detailnorm.png")
        .unwrap()
        .helper
        .clone()
        .unwrap();
    assert_eq!(rep.rnm_target, Some(engine::config::RnmTarget::N));
    let combo = h.get_by_role_and_label(Role::ComboBox, "Layer target");
    assert_eq!(combo.accesskit_node().value().as_deref(), Some("Normal"));
}

/// Review fix (DY17): picking the guessed role writes it, so it becomes assigned and replaces the diffuse's own
/// varying alpha, which a suggestion never does.
#[test]
fn picking_the_guessed_role_hand_sets_it() {
    let d = tempfile::tempdir().unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 4) as u8, 90u8, (x * 4) as u8])
    })
    .save(d.path().join("x.png"))
    .unwrap();
    let mut h = app(fixture(d.path(), &["x_opacity.png"], ""), "x_opacity.png");
    let rep = |h: &H| {
        h.state()
            .item("x_opacity.png")
            .unwrap()
            .helper
            .clone()
            .unwrap()
    };
    assert!(!rep(&h).role_hand_set);
    assert!(
        rep(&h).fills.is_empty(),
        "a suggestion leaves real alpha: {:?}",
        rep(&h)
    );
    pick(&mut h, "Helper role", "Opacity");
    assert_eq!(
        value(&h, "x_opacity.png", "helper.role"),
        Some(json!("opacity"))
    );
    assert!(rep(&h).role_hand_set);
    assert_eq!(
        rep(&h).fills,
        [engine::helper::FillTarget::Alpha(
            engine::helper::AlphaTarget::Diffuse
        )]
    );
    h.get_by_label("✎ Opacity");
}

/// A 4x4 uncompressed A8R8G8B8 DDS at `p`.
fn tiny_dds(p: &Path) {
    let mut b: Vec<u8> = b"DDS ".to_vec();
    let mut u = |v: u32| b.extend_from_slice(&v.to_le_bytes());
    for v in [124, 0x100F, 4, 4, 16, 0, 0] {
        u(v);
    }
    for _ in 0..11 {
        u(0);
    }
    for v in [
        32,
        0x41,
        0,
        32,
        0x00ff_0000,
        0x0000_ff00,
        0x0000_00ff,
        0xff00_0000,
        0x1000,
        0,
        0,
        0,
        0,
    ] {
        u(v);
    }
    b.extend(std::iter::repeat_n(0x80u8, 64));
    std::fs::write(p, b).unwrap();
}

/// Docs review (Note DY17): an image whose name has no helper suffix can be made a helper from its own panel.
#[test]
fn ordinary_image_can_become_a_helper() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(
        fixture(d.path(), &["x.png", "x_mask.png"], ""),
        "x_mask.png",
    );
    let item = |h: &H| h.state().item("x_mask.png").unwrap().clone();
    assert_ne!(item(&h).kind, ItemKind::Helper, "a tile to start with");
    let steps = h.state().history.undo.len();
    let opened_type = value(&h, "x_mask.png", "subtype");
    let combo = h.get_by_role_and_label(Role::ComboBox, "Helper role");
    assert_eq!(
        combo.accesskit_node().value().as_deref(),
        Some("Not a helper")
    );
    pick(&mut h, "Helper role", "Opacity");
    assert_eq!(
        value(&h, "x_mask.png", "helper.role"),
        Some(json!("opacity"))
    );
    let it = item(&h);
    assert_eq!(it.kind, ItemKind::Helper);
    assert_eq!(
        it.helper.unwrap().fills,
        [engine::helper::FillTarget::Alpha(
            engine::helper::AlphaTarget::Diffuse
        )]
    );
    assert!(row_name(&h, "x_mask.png")
        .unwrap()
        .contains("fills to x alpha"));
    // The selection stays on the file, now the helper panel.
    h.get_by_role_and_label(Role::ComboBox, "Read from");
    // One undo step, and no subtype key is left: the type the opening recorded (Note FC1) goes with the role.
    assert_eq!(value(&h, "x_mask.png", "subtype"), None);
    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(value(&h, "x_mask.png", "helper.role"), None);
    assert_eq!(value(&h, "x_mask.png", "subtype"), opened_type);
    assert_eq!(h.state().history.undo.len(), steps);
    // Note FC1: a file that stops being a helper by a new edit (not an Undo) gets its automatic type written again,
    // so `sdc convert` agrees.
    h.state_mut().redo();
    settle(&mut h);
    assert_eq!(value(&h, "x_mask.png", "subtype"), None);
    pick(&mut h, "Helper role", "Not a helper");
    assert_ne!(item(&h).kind, ItemKind::Helper);
    assert!(opened_type.is_some(), "the opening recorded a type");
    assert_eq!(value(&h, "x_mask.png", "subtype"), opened_type);
}

#[test]
fn dds_has_no_helper_role_row() {
    let d = tempfile::tempdir().unwrap();
    tiny_dds(&d.path().join("y_mask.dds"));
    let h = app(fixture(d.path(), &["x.png"], ""), "y_mask.dds");
    assert_eq!(h.state().item("y_mask.dds").unwrap().kind, ItemKind::Dds);
    assert_eq!(h.query_all_by_label("Helper role").count(), 0);
    h.get_by_label_contains("Already a DDS file");
}

#[test]
fn read_only_disables_the_helper_role_row() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app(
        fixture(d.path(), &["x.png", "x_mask.png"], "target = [\n"),
        "x_mask.png",
    );
    let n = h.get_by_role_and_label(Role::ComboBox, "Helper role");
    assert!(n.accesskit_node().is_disabled());
    n.hover();
    h.run();
    assert!(h.query_by_label_contains("sdc.toml is read-only").is_some());
}
