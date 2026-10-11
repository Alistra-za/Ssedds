//! M3b polish Task 2: a set's inferred members follow its diffuse to PBR types, and PBR types lead the dropdown.
mod support;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::config::Subtype;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use support::session;

const DIFFUSE: &str = "m/rock01.png";
const N: &str = "m/rock01_n.png";
const G: &str = "m/rock01_g.png";
const P: &str = "m/rock01_p.png";

fn fixture(dir: &Path) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
        .save(r.join("rock01.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(r.join("rock01_n.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([200u8, 100, 0]))
        .save(r.join("rock01_g.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([x as u8]))
        .save(r.join("rock01_p.png"))
        .unwrap();
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
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

fn subtype(m: &AppModel, key: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(key, "subtype")
}

fn auto(m: &AppModel) -> Vec<String> {
    m.sets_doc.as_ref().unwrap().auto_pbr()
}

fn toml_text(m: &AppModel) -> (String, String) {
    (
        m.project.as_ref().unwrap().text(),
        m.sets_doc.as_ref().unwrap().text(),
    )
}

#[test]
fn a_pbr_diffuse_switches_inferred_members_and_height_stays() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    for k in [DIFFUSE, N, G, P] {
        assert!(
            m.item(k).is_some(),
            "fixture item {k}: {:?}",
            m.items.iter().map(|i| &i.key).collect::<Vec<_>>()
        );
    }
    m.decide(DIFFUSE, Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    assert_eq!(subtype(&m, N), Some(json!("pbr_normal")));
    assert_eq!(subtype(&m, G), Some(json!("pbr_emissive")));
    assert_eq!(subtype(&m, P), None, "height stays height");
    let mut a = auto(&m);
    a.sort();
    assert_eq!(a, vec![G.to_string(), N.to_string()]);
    assert_eq!(
        m.item(N).unwrap().job.as_ref().unwrap().profile.subtype,
        Subtype::PbrNormal
    );
}

#[test]
fn a_member_the_user_set_is_left_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.decide(N, Answer::Subtype(Subtype::Msn));
    idle(&mut m);
    m.decide(DIFFUSE, Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    assert_eq!(subtype(&m, N), Some(json!("msn")));
    assert_eq!(subtype(&m, G), Some(json!("pbr_emissive")));
    assert_eq!(auto(&m), vec![G.to_string()]);
}

#[test]
fn reverting_the_diffuse_reverts_only_the_auto_switched_members() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.decide(DIFFUSE, Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    // The user changes the emissive afterwards; the normal stays automatic.
    m.set_field(G, "subtype", json!("glow"));
    idle(&mut m);
    assert_eq!(
        auto(&m),
        vec![N.to_string()],
        "a hand edit takes it off the list"
    );
    m.set_field(DIFFUSE, "subtype", json!("diffuse"));
    idle(&mut m);
    assert_eq!(subtype(&m, N), None, "the auto member returns to inferred");
    assert_eq!(
        subtype(&m, G),
        Some(json!("glow")),
        "the hand-set one stays"
    );
    assert!(auto(&m).is_empty());
    assert!(
        !m.sets_doc.as_ref().unwrap().text().contains("auto_pbr"),
        "the emptied list is removed"
    );
}

#[test]
fn a_member_changed_by_hand_after_the_switch_is_not_reverted() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.decide(DIFFUSE, Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    // Changed out of band to another value while still on the list (a hand-edited file).
    m.project
        .as_mut()
        .unwrap()
        .set_file_value(N, "subtype", &json!("msn"));
    m.clear_field(DIFFUSE, "subtype");
    idle(&mut m);
    assert_eq!(subtype(&m, N), Some(json!("msn")));
    assert_eq!(subtype(&m, G), None);
}

#[test]
fn one_undo_restores_both_files_and_redo_reapplies() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    let before = toml_text(&m);
    m.decide(DIFFUSE, Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    let after = toml_text(&m);
    assert_ne!(before, after);
    m.undo();
    idle(&mut m);
    assert_eq!(
        toml_text(&m),
        before,
        "one undo restores the diffuse, members and list"
    );
    assert!(!m.can_undo());
    m.redo();
    idle(&mut m);
    assert_eq!(toml_text(&m), after);
}

#[test]
fn a_read_only_project_file_changes_the_diffuse_only() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc-project.toml"), "[[sets]\nbroken").unwrap();
    let mut m = fixture(d.path());
    assert!(m.sets_doc.as_ref().unwrap().read_only());
    m.decide(DIFFUSE, Answer::Subtype(Subtype::PbrAlbedo));
    idle(&mut m);
    assert_eq!(subtype(&m, DIFFUSE), Some(json!("pbr_albedo")));
    assert_eq!(subtype(&m, N), None);
    assert_eq!(subtype(&m, G), None);
}

#[test]
fn the_subtype_combo_lists_the_pbr_group_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path());
    m.select_item(DIFFUSE);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let combo = h
        .query_all_by_role(eframe::egui::accesskit::Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("diffuse"))
        .expect("the sub-type combo shows the current sub-type");
    combo.click();
    h.run();
    h.run();
    let pbr = h.get_by_label("PBR (Community Shaders)");
    let first_vanilla = h.get_by_label("Colour maps");
    let y = |n: &egui_kittest::Node<'_>| n.accesskit_node().raw_bounds().unwrap().y0;
    assert!(y(&pbr) < y(&first_vanilla), "the PBR heading comes first");
    let pbr_albedo = h.get_by_label("pbr albedo");
    let diffuse = h.get_by_label("diffuse");
    assert!(y(&pbr_albedo) < y(&first_vanilla));
    assert!(
        y(&first_vanilla) < y(&diffuse),
        "vanilla types follow their heading"
    );
}
