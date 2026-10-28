//! Cubemap GUI debts: the sub-type dropdown offers Cubemap, the size estimate counts six faces, a six-face set is named so.
mod support;
use egui_kittest::kittest::{NodeT as _, Queryable};
use egui_kittest::Harness;
use engine::config::{Subtype, Target};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::settings::GuiSettings;
use sdc_gui::ui::settings_panel::{offered_groups, OTHER_HEADING};
use std::time::Duration;

fn open(dir: &std::path::Path) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.join("textures"));
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

/// Bytes of a full BC7 chain of an `n` x `n` image: 16 bytes per 4x4 block, blocks rounded up.
fn bc7_chain_bytes(mut n: u32) -> u64 {
    let mut sum = 0;
    loop {
        let b = u64::from(n.div_ceil(4));
        sum += b * b * 16;
        if n == 1 {
            return sum;
        }
        n /= 2;
    }
}

#[test]
fn cubemap_is_offered_in_the_subtype_dropdown() {
    let groups = offered_groups(Target::Cs);
    let other = groups.iter().find(|(h, _)| *h == OTHER_HEADING).unwrap();
    assert!(other.1.contains(&Subtype::Cubemap), "{other:?}");
}

#[test]
fn cube_size_estimate_counts_six_faces() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/a");
    std::fs::create_dir_all(&r).unwrap();
    image::RgbImage::from_fn(384, 64, |x, _| image::Rgb([(x % 256) as u8, 9, 9]))
        .save(r.join("chrome.png"))
        .unwrap();
    let mut m = open(d.path());
    m.decide("a/chrome.png", Answer::Subtype(Subtype::Cubemap));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_field(
        "a/chrome.png",
        "cubemap.layout",
        serde_json::json!("hstrip"),
    );
    m.set_field("a/chrome.png", "format", serde_json::json!("bc7"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let e = loop {
        if let Some(e) = m.size_of("a/chrome.png") {
            break e;
        }
        assert!(m.wait_idle(Duration::from_secs(60)));
    };
    assert_eq!(e.chain_bytes, 6 * bc7_chain_bytes(64));
}

#[test]
fn six_face_item_says_six_faces() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/sky");
    std::fs::create_dir_all(&r).unwrap();
    for f in ["px", "nx", "py", "ny", "pz", "nz"] {
        image::RgbImage::from_pixel(16, 16, image::Rgb([5, 6, 7]))
            .save(r.join(format!("env_{f}.png")))
            .unwrap();
    }
    let mut m = open(d.path());
    for f in ["px", "nx", "py", "ny", "pz", "nz"] {
        m.decide(
            &format!("sky/env_{f}.png"),
            Answer::Subtype(Subtype::Cubemap),
        );
        assert!(m.wait_idle(Duration::from_secs(60)));
    }
    let names: Vec<_> = m.items.iter().map(|i| i.name.clone()).collect();
    assert_eq!(
        names.iter().filter(|n| n.ends_with("(6 faces)")).count(),
        1,
        "{names:?}"
    );
}

// ---- the cubemap settings section (Task 3) ----

const CUBE: &str = "a/chrome.png";
const FLAT: &str = "a/flat.png";

/// A cube (a 6:1 strip decided as Cubemap) and a plain 2D texture, planned, with the panel drawn over `selected`.
fn section(d: &std::path::Path, selected: &str) -> Harness<'static, AppModel> {
    let m = cube_model(d, selected);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1600.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h
}

fn cube_model(d: &std::path::Path, selected: &str) -> AppModel {
    let r = d.join("textures/a");
    std::fs::create_dir_all(&r).unwrap();
    image::RgbImage::from_fn(384, 64, |x, _| image::Rgb([(x % 256) as u8, 9, 9]))
        .save(r.join("chrome.png"))
        .unwrap();
    image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 90]))
        .save(r.join("flat.png"))
        .unwrap();
    let mut m = open(d);
    m.decide(CUBE, Answer::Subtype(Subtype::Cubemap));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.select_item(selected);
    m
}

fn file_value(h: &Harness<'static, AppModel>, field: &str) -> Option<serde_json::Value> {
    h.state().project.as_ref().unwrap().file_value(CUBE, field)
}

fn settle(h: &mut Harness<'static, AppModel>) {
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
}

fn headers(h: &Harness<'static, AppModel>) -> usize {
    h.query_all_by_label("Cubemap").count()
}

#[test]
fn cube_section_shows_only_for_cubes() {
    let d = tempfile::tempdir().unwrap();
    let mut h = section(d.path(), FLAT);
    assert_eq!(headers(&h), 0, "a 2D item has no cube section");
    h.state_mut().select_item(CUBE);
    h.run();
    assert_eq!(headers(&h), 1, "a cube has one");
}

#[test]
fn mixed_selection_hides_the_cube_section() {
    let d = tempfile::tempdir().unwrap();
    let mut h = section(d.path(), CUBE);
    assert_eq!(headers(&h), 1);
    h.state_mut().click_item(FLAT, true, false);
    h.run();
    assert!(h.state().in_bulk(), "two textures are selected");
    assert_eq!(headers(&h), 0, "a mixed selection has no cube section");
    // A bulk edit reaches only the bulk panel's fields.
    let keys = vec![CUBE.to_string(), FLAT.to_string()];
    h.state_mut().apply_bulk_edit(
        &keys,
        "encoder.effort",
        sdc_gui::model::bulk::BulkOp::Set(serde_json::json!("fast")),
    );
    settle(&mut h);
    let text = h.state().project.as_ref().unwrap().text();
    assert!(text.contains("encoder.effort"), "the bulk edit ran: {text}");
    assert!(!text.contains("cubemap."), "{text}");
}

/// The section's own guard, drawn with no bulk panel in front of it: the active item is a cube but the selection is
/// not all cubes, so nothing shows and nothing is written. Fails when `all_cubes` is removed.
#[test]
fn the_section_guard_hides_a_mixed_selection() {
    let d = tempfile::tempdir().unwrap();
    let mut m = cube_model(d.path(), CUBE);
    m.click_item(FLAT, true, false);
    assert!(m.in_bulk());
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(800.0, 800.0))
        .build_ui_state(
            |ui, m: &mut AppModel| {
                let edits = sdc_gui::ui::cube_section::cube_section(ui, m, CUBE);
                assert!(edits.is_empty());
            },
            m,
        );
    h.run();
    assert_eq!(headers(&h), 0, "cube + 2D selected: no section");
    h.state_mut().select_item(CUBE);
    h.run();
    assert_eq!(headers(&h), 1, "the cube alone: the section");
}

#[test]
fn the_layout_combo_offers_no_six_face_option() {
    let d = tempfile::tempdir().unwrap();
    let mut h = section(d.path(), CUBE);
    h.get_by_label("Cubemap layout").click();
    h.run();
    assert_eq!(h.query_all_by_label("Six face files").count(), 0);
    assert!(h.query_all_by_label("Horizontal cross").count() >= 1);
}

#[test]
fn yaw_edit_writes_the_dotted_key() {
    let d = tempfile::tempdir().unwrap();
    let mut h = section(d.path(), CUBE);
    assert_eq!(file_value(&h, "cubemap.yaw"), None);
    let yaw = h.get_by_label("Cubemap yaw");
    yaw.click();
    h.run();
    h.get_by_label("Cubemap yaw").type_text("90");
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    settle(&mut h);
    assert_eq!(file_value(&h, "cubemap.yaw"), Some(serde_json::json!(90.0)));
    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(file_value(&h, "cubemap.yaw"), None, "undo removes the key");
}

#[test]
fn ground_fill_colour_writes_codes() {
    let d = tempfile::tempdir().unwrap();
    let mut h = section(d.path(), CUBE);
    h.get_by_label("Cubemap ground fill").click();
    h.run();
    h.query_all_by_label("Colour").last().unwrap().click();
    h.run();
    settle(&mut h);
    assert_eq!(
        file_value(&h, "cubemap.ground_fill"),
        Some(serde_json::json!([40, 30, 20]))
    );
    // With a fill the horizon row opens and the colour button shows.
    assert!(!h
        .get_by_label("Cubemap horizon")
        .accesskit_node()
        .is_disabled());
    assert_eq!(h.query_all_by_label("Cubemap ground colour").count(), 1);
}

#[test]
fn horizon_is_disabled_without_a_fill() {
    let d = tempfile::tempdir().unwrap();
    let h = section(d.path(), CUBE);
    assert!(h
        .get_by_label("Cubemap horizon")
        .accesskit_node()
        .is_disabled());
    assert_eq!(h.query_all_by_label("needs a ground fill").count(), 1);
}

#[test]
fn out_of_range_yaw_shows_the_engine_message() {
    let d = tempfile::tempdir().unwrap();
    let mut h = section(d.path(), CUBE);
    h.state_mut()
        .set_field(CUBE, "cubemap.yaw", serde_json::json!(400.0));
    settle(&mut h);
    settle(&mut h);
    // The engine's text sits under the yaw row, in the section.
    let yaw_y = h.get_by_label("Cubemap yaw").rect().max.y;
    let under: Vec<_> = h
        .query_all_by_label_contains("cubemap.yaw 400 outside")
        // The panel's general warning list repeats it far below; the row's own copy is right under the row.
        .filter(|n| (yaw_y..yaw_y + 40.0).contains(&n.rect().min.y))
        .collect();
    assert_eq!(under.len(), 1, "one message under the row");
    // The GUI neither clamps nor rewrites the file value.
    assert_eq!(
        file_value(&h, "cubemap.yaw"),
        Some(serde_json::json!(400.0))
    );
}

// ---- the unclear-layout decision (Task 4) ----

const UNCLEAR: &str = "a/room_e.png";

/// A square `_e.png` (could be a panorama, a mirror ball or a photo) next to a 2D diffuse, planned.
fn unclear_model(d: &std::path::Path) -> AppModel {
    let r = d.join("textures/a");
    std::fs::create_dir_all(&r).unwrap();
    image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 7]))
        .save(r.join("room_e.png"))
        .unwrap();
    image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([y as u8, x as u8, 90]))
        .save(r.join("flat.png"))
        .unwrap();
    let mut m = open(d);
    m.decide(UNCLEAR, Answer::Subtype(Subtype::Cubemap));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.select_item(UNCLEAR);
    m
}

#[test]
fn unclear_cube_offers_a_layout_decision() {
    let d = tempfile::tempdir().unwrap();
    let m = unclear_model(d.path());
    let it = m.item(UNCLEAR).expect("item");
    assert_eq!(it.status, sdc_gui::model::items::ItemStatus::NeedsDecision);
    assert!(
        it.decisions
            .contains(&sdc_gui::model::items::Decision::CubeLayout),
        "{:?}",
        it.decisions
    );
    assert_eq!(it.decisions, [sdc_gui::model::items::Decision::CubeLayout]);
    let flat = &m.item("a/flat.png").unwrap().decisions;
    assert!(!flat.contains(&sdc_gui::model::items::Decision::CubeLayout));
}

#[test]
fn picking_a_layout_resolves_only_that_file() {
    let d = tempfile::tempdir().unwrap();
    let m = unclear_model(d.path());
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1600.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    assert_eq!(
        h.query_all_by_label_contains("could be several cube layouts")
            .count(),
        1
    );
    h.get_by_label("Cube layout").click();
    h.run();
    assert_eq!(h.query_all_by_label("Six face files").count(), 0);
    assert_eq!(h.query_all_by_label("Auto").count(), 0);
    h.get_by_label("Mirror ball").click();
    h.run();
    settle(&mut h);
    let p = h.state().project.as_ref().unwrap();
    assert_eq!(
        p.file_value(UNCLEAR, "cubemap.layout"),
        Some(serde_json::json!("mirror_ball"))
    );
    assert_eq!(p.file_value("a/flat.png", "cubemap.layout"), None);
    let it = h.state().item(UNCLEAR).unwrap();
    assert!(
        !it.decisions
            .contains(&sdc_gui::model::items::Decision::CubeLayout),
        "{:?}",
        it.decisions
    );
    assert_eq!(
        it.status,
        sdc_gui::model::items::ItemStatus::Ready,
        "{:?}",
        it.decisions
    );
}

// ---- the cross preview (Task 5) ----

/// The cube planned, expanded and drawn in the preview (no GPU, so the panes say so; the controls still draw).
fn preview(d: &std::path::Path, key: &str) -> Harness<'static, AppModel> {
    let mut m = cube_model(d, key);
    m.expand(key);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1600.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h
}

#[test]
fn cube_preview_draws_a_cross() {
    use sdc_gui::model::preview::EncodedState;
    let d = tempfile::tempdir().unwrap();
    let mut h = preview(d.path(), CUBE);
    h.state_mut().encode_now();
    settle(&mut h);
    let EncodedState::Done(out) = &h.state().preview.encoded else {
        panic!("not encoded")
    };
    // 64-px faces: six chains of 7 mips, one 4x3-face cross decoded per mip.
    assert_eq!((out.result.faces, out.result.levels.len()), (6, 42));
    assert_eq!(out.decoded.len(), 7);
    for (i, d) in out.decoded.iter().enumerate() {
        let face = 64usize >> i;
        assert_eq!(d.len(), 4 * face * 3 * face, "mip {i}");
    }
    // The toolbar: face labels on offer, heatmap and tile off for a cube, the mip counted per face.
    assert!(h.query_by_label("Face labels").is_some());
    for name in ["Heatmap", "Tile 3×3"] {
        assert!(
            h.get_by_label(name).accesskit_node().is_disabled(),
            "{name} is off for a cube"
        );
    }
    assert!(h.query_by_label_contains("64×64 per face").is_some());
}

#[test]
fn face_labels_toggle_is_a_named_button_and_cube_only() {
    let d = tempfile::tempdir().unwrap();
    let mut h = preview(d.path(), CUBE);
    assert!(h.state().view.face_labels, "on by default");
    h.get_by_label("Face labels").click();
    h.run();
    assert!(!h.state().view.face_labels);
    h.state_mut().expand(FLAT);
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
    assert!(
        h.query_by_label("Face labels").is_none(),
        "a 2D item has none"
    );
    assert!(!h.get_by_label("Tile 3×3").accesskit_node().is_disabled());
}

#[test]
fn broken_six_face_set_shows_the_error() {
    use sdc_gui::model::items::ItemStatus;
    use sdc_gui::model::preview::EncodedState;
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/a");
    std::fs::create_dir_all(&r).unwrap();
    for f in ["px", "nx", "py", "ny", "pz", "nz"] {
        let n = if f == "pz" { 32 } else { 16 };
        image::RgbImage::from_pixel(n, n, image::Rgb([5, 6, 7]))
            .save(r.join(format!("chrome_e_{f}.png")))
            .unwrap();
    }
    let mut m = open(d.path());
    let key = m
        .items
        .iter()
        .find(|i| matches!(i.status, ItemStatus::Error(_)))
        .expect("the broken set is an error item")
        .key
        .clone();
    m.expand(&key);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1600.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    assert!(matches!(
        h.state().preview.encoded,
        EncodedState::NotEncoded
    ));
    // The encoded pane says why instead of "Not encoded": the engine's reason, no Encode button, no cross.
    assert!(h.query_by_label("Not encoded").is_none());
    assert!(h
        .query_by_label("Encode now")
        .is_none_or(|n| n.accesskit_node().is_disabled()));
    let xs: Vec<f32> = h
        .query_all_by_label_contains("faces differ in size")
        .map(|n| n.rect().min.x)
        .collect();
    let mid = h.get_by_label("Encoded").rect().min.x;
    assert!(
        xs.iter().any(|&x| x >= mid),
        "the reason is in the encoded pane: {xs:?} vs {mid}"
    );
    assert!(h.query_by_label("Face labels").is_none());
}
