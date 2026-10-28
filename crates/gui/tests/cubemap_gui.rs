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
    // DT32: only the layouts an unclear image can be; a cross or strip would never fit it.
    for name in [
        "360° panorama",
        "Mirror ball",
        "Angular map",
        "Single photo",
    ] {
        assert_eq!(h.query_all_by_label(name).count(), 1, "{name}");
    }
    for name in [
        "Six face files",
        "Auto",
        "Horizontal cross",
        "Vertical cross",
        "Horizontal strip",
        "Vertical strip",
    ] {
        assert_eq!(h.query_all_by_label(name).count(), 0, "{name}");
    }
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

/// T4 minor 1 (DT35): a Review row whose only question is the layout says where to answer it; a 2D row does not.
#[test]
fn a_review_row_for_the_layout_points_to_settings() {
    const HINT: &str = "Pick its cube layout in settings";
    for (key, want) in [(UNCLEAR, 1), ("a/flat.png", 0)] {
        let d = tempfile::tempdir().unwrap();
        let mut h = Harness::builder()
            .with_size(eframe::egui::vec2(800.0, 200.0))
            .build_ui_state(
                move |ui, m: &mut AppModel| {
                    sdc_gui::ui::review::undecided_row(ui, m, key, None);
                },
                unclear_model(d.path()),
            );
        h.run();
        assert_eq!(h.query_all_by_label(HINT).count(), want, "{key}");
    }
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

// ---- the source pane beside the cross (Task 5 fix round 1) ----

/// A six-file cube (face i a solid colour whose red is `30 + 40 i`), default format, planned and expanded.
fn six_face_model(d: &std::path::Path) -> (AppModel, String) {
    let r = d.join("textures/a");
    std::fs::create_dir_all(&r).unwrap();
    for (i, f) in ["px", "nx", "py", "ny", "pz", "nz"].iter().enumerate() {
        image::RgbImage::from_pixel(16, 16, image::Rgb([30 + 40 * i as u8, 20, 10]))
            .save(r.join(format!("chrome_e_{f}.png")))
            .unwrap();
    }
    let mut m = open(d);
    let key = m
        .items
        .iter()
        .find(|i| i.job.is_some())
        .expect("the six-face item")
        .key
        .clone();
    m.expand(&key);
    (m, key)
}

/// Final review C1: an edit on a six-file cube goes to all six face entries, so the faces still resolve alike.
#[test]
fn a_six_face_edit_writes_all_six_entries_in_one_step() {
    use sdc_gui::model::items::ItemStatus;
    let faces = ["px", "nx", "py", "ny", "pz", "nz"].map(|f| format!("a/chrome_e_{f}.png"));
    for (field, value) in [
        ("cubemap.yaw", serde_json::json!(90.0)),
        ("encoder.effort", serde_json::json!("fast")),
    ] {
        let d = tempfile::tempdir().unwrap();
        let (mut m, key) = six_face_model(d.path());
        // Deciding the merged item decides all six faces (`decide` goes through `edit_subtype`).
        m.decide(&key, Answer::Subtype(Subtype::Cubemap));
        assert!(m.wait_idle(Duration::from_secs(60)));
        let it = m.item(&key).expect("the decided item keeps its key");
        assert_eq!(it.status, ItemStatus::Ready, "{:?}", it.decisions);
        let p = m.project.as_ref().unwrap();
        for f in &faces {
            assert_eq!(
                p.file_value(f, "subtype"),
                Some(serde_json::json!("cubemap")),
                "{f}"
            );
        }
        m.set_field(&key, field, value.clone());
        assert!(m.wait_idle(Duration::from_secs(60)));
        let it = m
            .item(&key)
            .unwrap_or_else(|| panic!("{field}: the item is gone"));
        assert_eq!(it.status, ItemStatus::Ready, "{field}: {:?}", it.status);
        let p = m.project.as_ref().unwrap();
        for f in &faces {
            assert_eq!(p.file_value(f, field), Some(value.clone()), "{field} {f}");
        }
        // The reset clears all six, as one more step.
        m.clear_field(&key, field);
        assert!(m.wait_idle(Duration::from_secs(60)));
        let p = m.project.as_ref().unwrap();
        assert!(
            faces.iter().all(|f| p.file_value(f, field).is_none()),
            "{field}: reset"
        );
        assert_eq!(m.item(&key).unwrap().status, ItemStatus::Ready);
        // Undo the reset, then undo the edit: one step each.
        m.undo();
        assert!(m.wait_idle(Duration::from_secs(60)));
        let p = m.project.as_ref().unwrap();
        assert!(
            faces.iter().all(|f| p.file_value(f, field).is_some()),
            "{field}: undo reset"
        );
        m.undo();
        assert!(m.wait_idle(Duration::from_secs(60)));
        let p = m.project.as_ref().unwrap();
        assert!(
            faces.iter().all(|f| p.file_value(f, field).is_none()),
            "{field}: undo"
        );
        assert_eq!(m.item(&key).unwrap().status, ItemStatus::Ready);
    }
}

fn source_red(m: &mut AppModel) -> f32 {
    use sdc_gui::model::preview::SourceState;
    // The debounce counts from the first tick that sees something scheduled.
    let t0 = std::time::Instant::now();
    m.tick(t0);
    m.tick(t0 + Duration::from_secs(5));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let SourceState::Ready(s) = &m.preview.source else {
        panic!("source not ready: {:?}", m.preview.source)
    };
    s.levels[0].px[0][0]
}

#[test]
fn the_source_face_selector_shows_the_processed_face_of_its_cell() {
    use sdc_gui::model::preview::EncodedState;
    use sdc_gui::ui::preview::{GpuStatus, PreviewUi};
    let d = tempfile::tempdir().unwrap();
    let (mut m, _) = six_face_model(d.path());
    let face0 = source_red(&mut m);
    m.encode_now();
    assert!(m.wait_idle(Duration::from_secs(60)));
    // The selector is a row of named buttons in the source pane (drawn once a renderer is there).
    let gpu = GpuStatus {
        renderer: true,
        source: true,
        encoded: Some(sdc_gui::render::UploadPath::CpuDecoded("")),
        ..GpuStatus::default()
    };
    let mut ui_state = PreviewUi::default();
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1600.0, 900.0))
        .build_ui_state(
            move |ui, model: &mut AppModel| {
                sdc_gui::ui::preview::draw(ui, model, &mut ui_state, &gpu)
            },
            m,
        );
    h.run();
    h.get_by_label("Source face +Y north").click();
    h.run();
    assert_eq!(h.state().view.source_face, 2);
    let face2 = source_red(h.state_mut());
    assert!(
        face2 > face0 + 0.1,
        "face 2 is not face 0: {face2} vs {face0}"
    );
    // It is the image of that face's cell of the cross.
    let EncodedState::Done(out) = &h.state().preview.encoded else {
        panic!("not encoded")
    };
    let (col, row) = engine::cube::layout::HCROSS[2];
    let cell = out.decoded[0][(row as usize * 16 + 8) * 64 + col as usize * 16 + 8][0];
    assert!((cell - face2).abs() < 0.03, "cell {cell} vs source {face2}");
}

#[test]
fn a_single_image_cube_shows_its_input_in_the_source_pane() {
    use sdc_gui::model::preview::SourceState;
    let d = tempfile::tempdir().unwrap();
    let mut m = cube_model(d.path(), CUBE);
    m.expand(CUBE);
    m.tick(std::time::Instant::now() + Duration::from_secs(5));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let SourceState::Ready(s) = &m.preview.source else {
        panic!("source not ready")
    };
    // The 384x64 strip as the file is, not a 64-px face.
    assert_eq!((s.levels[0].width, s.levels[0].height), (384, 64));
}

#[test]
fn the_tile_button_toggles_on_a_2d_item() {
    let d = tempfile::tempdir().unwrap();
    let mut h = preview(d.path(), FLAT);
    assert!(!h.state().view.tile);
    h.get_by_label("Tile 3×3").click();
    h.run();
    assert!(h.state().view.tile, "the first click turns tiling on");
    h.get_by_label("Tile 3×3").click();
    h.run();
    assert!(!h.state().view.tile, "the second click turns it off");
}

#[test]
fn a_cube_leaves_the_stored_tile_setting_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut h = preview(d.path(), CUBE);
    h.state_mut().view.tile = true;
    h.run();
    assert!(h.state().view.tile, "drawing a cube never writes tile");
}

// ---- the marker cube tool (Task 6) ----

mod marker {
    use super::*;
    use eframe::egui;
    use engine::cube::marker::{alpha_of, marker_bytes};
    use sdc_gui::model::marker::{make_marker, MarkerDraft, MarkerOutcome};
    use sdc_gui::model::picker::{PickerBackend, PickerKind, PickerPurpose, PickerSpec};
    use sdc_gui::model::Dialog;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// A Save dialog that answers `path` and records what it was asked. Never an OS dialog.
    struct Fake {
        path: Option<PathBuf>,
        asked: Mutex<Vec<PickerSpec>>,
    }

    impl PickerBackend for Fake {
        fn pick(&self, spec: &PickerSpec) -> Option<PathBuf> {
            self.asked.lock().unwrap().push(spec.clone());
            self.path.clone()
        }
    }

    fn fake(path: Option<PathBuf>) -> Arc<Fake> {
        Arc::new(Fake {
            path,
            asked: Mutex::default(),
        })
    }

    #[test]
    fn marker_matches_the_cli_bytes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a_e.dds");
        let draft = MarkerDraft {
            rgb: [200, 100, 50],
            roughness: Some(0.5),
        };
        assert_eq!(make_marker(&p, &draft), MarkerOutcome::Written);
        let want = marker_bytes([200, 100, 50], alpha_of(Some(0.5)).unwrap());
        assert_eq!(std::fs::read(&p).unwrap(), want);
        // Its own file is replaced.
        let again = MarkerDraft {
            rgb: [1, 2, 3],
            roughness: None,
        };
        assert_eq!(make_marker(&p, &again), MarkerOutcome::Written);
        assert_eq!(std::fs::read(&p).unwrap(), marker_bytes([1, 2, 3], 255));
    }

    #[test]
    fn marker_keeps_a_foreign_file() {
        use engine::config::Format;
        use engine::dds::{header_bytes_with, DdsDesc, ResolvedHeader};
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a_e.dds");
        // A real DDS Tessalith did not write: a flat 1x1 texture, not a cube.
        let desc = DdsDesc {
            width: 1,
            height: 1,
            format: Format::Rgba8,
            srgb: false,
            header: ResolvedHeader::Legacy,
        };
        let mut foreign = header_bytes_with(&desc, 1, false).unwrap();
        foreign.extend_from_slice(&[9, 8, 7, 6]);
        std::fs::write(&p, &foreign).unwrap();
        let out = make_marker(&p, &MarkerDraft::default());
        assert!(
            matches!(out, MarkerOutcome::Kept(ref n) if !n.is_empty()),
            "{out:?}"
        );
        assert_eq!(std::fs::read(&p).unwrap(), foreign, "left untouched");
    }

    #[test]
    fn marker_without_roughness_is_alpha_255() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a_e.dds");
        let draft = MarkerDraft {
            rgb: [10, 20, 30],
            roughness: None,
        };
        assert_eq!(make_marker(&p, &draft), MarkerOutcome::Written);
        let b = std::fs::read(&p).unwrap();
        // The +X texel is the first after the 128-byte header, stored BGRA.
        assert_eq!(&b[128..132], &[30, 20, 10, 255]);
    }

    #[test]
    fn marker_with_a_bad_roughness_fails_and_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a_e.dds");
        let draft = MarkerDraft {
            rgb: [0; 3],
            roughness: Some(1.5),
        };
        assert!(matches!(make_marker(&p, &draft), MarkerOutcome::Failed(_)));
        assert!(!p.exists());
    }

    fn dialog_harness(m: AppModel) -> Harness<'static, AppModel> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(900.0, 700.0))
            .build_ui_state(
                |ui, m: &mut AppModel| sdc_gui::ui::marker_dialog::draw(ui.ctx(), m),
                m,
            );
        sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
        // The modal's first frames size and fade it in; a click before they pass lands on the old layout.
        h.run_steps(6);
        h
    }

    fn wait_picker(m: &AppModel) {
        let end = std::time::Instant::now() + Duration::from_secs(10);
        while m.picker.busy() {
            assert!(std::time::Instant::now() < end, "the picker never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_dialog_controls_have_names() {
        let mut m = AppModel::placeholder(GuiSettings::default());
        m.open_marker_dialog();
        let mut h = dialog_harness(m);
        for name in ["Marker colour", "Set roughness", "Save…", "Cancel"] {
            assert!(h.query_by_label(name).is_some(), "{name}");
        }
        assert!(h.query_by_label("Roughness").is_none(), "off: no slider");
        assert!(h.query_by_label("F0 = (code/255)^1.6").is_some());
        h.get_by_label("Set roughness").click();
        h.run();
        assert_eq!(h.state().marker.roughness, Some(0.5));
        assert!(
            h.query_all_by_label_contains("Roughness").next().is_some(),
            "the slider"
        );
        h.get_by_label("Set roughness").click();
        h.run();
        assert_eq!(h.state().marker.roughness, None, "off again is alpha 255");
    }

    #[test]
    fn save_writes_the_picked_file_and_closes() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("out_e.dds");
        let mut m = AppModel::placeholder(GuiSettings::default());
        let f = fake(Some(target.clone()));
        m.picker.set_backend(f.clone());
        m.marker = MarkerDraft {
            rgb: [200, 100, 50],
            roughness: Some(0.5),
        };
        m.open_marker_dialog();
        let mut h = dialog_harness(m);
        h.get_by_label("Save…").click();
        h.run();
        wait_picker(h.state());
        h.state_mut().poll();
        assert_eq!(
            std::fs::read(&target).unwrap(),
            marker_bytes([200, 100, 50], alpha_of(Some(0.5)).unwrap())
        );
        assert_eq!(h.state().dialog, None);
        let asked = f.asked.lock().unwrap();
        let PickerKind::Save { file_name, .. } = &asked[0].kind else {
            panic!("not a Save dialog")
        };
        assert_eq!(*file_name, "marker_e.dds");
    }

    #[test]
    fn a_kept_file_leaves_the_dialog_open_with_the_note() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("out_e.dds");
        std::fs::write(&target, b"not ours").unwrap();
        let mut m = AppModel::placeholder(GuiSettings::default());
        m.picker.set_backend(fake(Some(target.clone())));
        m.open_marker_dialog();
        assert!(m.pick_marker_file(&egui::Context::default()));
        wait_picker(&m);
        m.poll();
        assert_eq!(std::fs::read(&target).unwrap(), b"not ours");
        assert_eq!(m.dialog, Some(Dialog::MakeMarker), "pick another name");
        assert!(m.status_line.contains("out_e.dds"), "{}", m.status_line);
    }

    /// T6 M5 (DT34): a kept write's note shows inside the dialog, not only behind its backdrop.
    #[test]
    fn a_kept_note_shows_in_the_dialog() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("out_e.dds");
        std::fs::write(&target, b"not ours").unwrap();
        let mut m = AppModel::placeholder(GuiSettings::default());
        m.picker.set_backend(fake(Some(target.clone())));
        m.open_marker_dialog();
        let mut h = dialog_harness(m);
        h.get_by_label("Save…").click();
        h.run();
        wait_picker(h.state());
        h.state_mut().poll();
        h.run();
        let note = h.state().status_line.clone();
        assert!(note.contains("out_e.dds"), "{note}");
        assert_eq!(
            h.query_all_by_label(&note).count(),
            1,
            "the note in the dialog"
        );
        // Opening the dialog again starts without it.
        h.state_mut().dialog = None;
        h.state_mut().open_marker_dialog();
        h.run();
        assert_eq!(h.query_all_by_label(&note).count(), 0);
    }

    /// Final review M2 (DT33): a Save result that lands after the dialog was cancelled writes nothing, and closes
    /// no other dialog.
    #[test]
    fn a_save_after_cancel_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("out_e.dds");
        let mut m = AppModel::placeholder(GuiSettings::default());
        m.picker.set_backend(fake(Some(target.clone())));
        m.open_marker_dialog();
        let mut h = dialog_harness(m);
        h.get_by_label("Save…").click();
        h.run();
        h.get_by_label("Cancel").click();
        h.run();
        assert_eq!(h.state().dialog, None);
        // Another dialog opened meanwhile stays open.
        h.state_mut().dialog = Some(Dialog::Error("other".into()));
        wait_picker(h.state());
        h.state_mut().poll();
        assert!(!target.exists(), "nothing is written after a cancel");
        assert_eq!(h.state().dialog, Some(Dialog::Error("other".into())));
    }

    #[test]
    fn escape_and_cancel_close_without_asking_for_a_file() {
        for esc in [true, false] {
            let mut m = AppModel::placeholder(GuiSettings::default());
            let f = fake(None);
            m.picker.set_backend(f.clone());
            m.open_marker_dialog();
            let mut h = dialog_harness(m);
            if esc {
                h.key_press(egui::Key::Escape);
            } else {
                h.get_by_label("Cancel").click();
            }
            h.run();
            assert_eq!(h.state().dialog, None, "esc={esc}");
            assert!(f.asked.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn enter_asks_for_a_file_in_the_cubemaps_folder_and_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("textures/cubemaps")).unwrap();
        let mut m = open(d.path());
        let f = fake(None);
        m.picker.set_backend(f.clone());
        m.open_marker_dialog();
        let mut h = dialog_harness(m);
        h.key_press(egui::Key::Enter);
        h.run();
        wait_picker(h.state());
        let asked = f.asked.lock().unwrap();
        assert_eq!(asked.len(), 1, "Enter is Save…");
        let PickerKind::Save { dir, .. } = &asked[0].kind else {
            panic!("not a Save dialog")
        };
        assert_eq!(
            dir.as_deref(),
            Some(d.path().join("textures/cubemaps").as_path())
        );
        drop(asked);
        assert_eq!(
            h.state().dialog,
            Some(Dialog::MakeMarker),
            "a cancel leaves it open"
        );
        assert!(!d.path().join("textures/cubemaps/marker_e.dds").exists());
        let _ = PickerPurpose::MakeMarker;
    }

    #[test]
    fn the_export_menu_offers_the_tool() {
        let d = tempfile::tempdir().unwrap();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .build_ui_state(
                |ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m),
                cube_model(d.path(), FLAT),
            );
        sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
        h.run();
        h.get_by_label_contains("Export to").click();
        h.run();
        h.get_by_label("Make marker cube…").click();
        h.run();
        assert_eq!(h.state().dialog, Some(Dialog::MakeMarker));
    }
}
