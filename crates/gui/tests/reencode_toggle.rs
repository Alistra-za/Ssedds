//! Note FI3: a DDS input is re-encoded from a button (texture panel, bulk panel), which writes the S6 `reencode`
//! selector; the panel says a skipped DDS is not written, and its header issues read in words.
mod support;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::bottom::{problems, Level};
use sdc_gui::model::items::ItemStatus;
use sdc_gui::model::AppModel;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::*;

const REENCODE: &str = "Re-encode this file";

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn png(path: &Path) {
    image::ImageBuffer::from_fn(64, 64, |x, y| image::Rgb([x as u8 * 4, y as u8 * 4, 90u8]))
        .save(path)
        .unwrap();
}

/// `textures/` with `a.dds`, `b.dds` and `c.png` (no two share an output), plus `extra` written before opening.
fn folder(dir: &Path, extra: impl FnOnce(&Path)) -> PathBuf {
    let root = dir.join("textures");
    bc1_dds(&root.join("a.dds"));
    bc1_dds(&root.join("b.dds"));
    png(&root.join("c.png"));
    extra(&root);
    root
}

fn open(dir: &Path, extra: impl FnOnce(&Path)) -> AppModel {
    opened_as_the_app(folder(dir, extra), dir.join("export"))
}

fn toml(m: &AppModel) -> String {
    m.project.as_ref().unwrap().text().to_string()
}

/// Run the frame that takes a queued click, wait for its plan, and draw the result.
fn idle(h: &mut Harness<'static, AppModel>) {
    h.run();
    assert!(h.state_mut().wait_idle(Duration::from_secs(60)));
    h.run();
}

fn has_job(m: &AppModel, key: &str) -> bool {
    m.item(key).is_some_and(|i| i.job.is_some())
}

fn hover_shows(h: &mut Harness<'static, AppModel>, label: &str, text: &str) {
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.get_by_label(label).hover();
    h.run();
    h.run();
    h.get_by_label_contains(text);
}

#[test]
fn reencode_button_writes_the_selector() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |_| {});
    assert!(!has_job(&m, "a.dds"), "a skipped DDS has no job");
    m.select_item("a.dds");
    let mut h = harness(m);
    h.run();
    h.get_by_label("Already a DDS file: it is left as it is and not written to the export.");
    let (before, steps) = (toml(h.state()), h.state().history.undo.len());
    h.get_by_label(REENCODE).click();
    idle(&mut h);
    let t = toml(h.state());
    assert!(
        t.contains("[files.\"a.dds\"]\nreencode = true\nsubtype = \"diffuse\""),
        "the selector and the FC1 type it caused: {t}"
    );
    assert!(has_job(h.state(), "a.dds"), "a re-encoded DDS has a job");
    // Note FI3: the plan's FC1 recording joined the re-encode: one step, one Undo, one Redo.
    assert_eq!(h.state().history.undo.len(), steps + 1, "one undo step");
    assert_eq!(h.state().undo_label(), Some("Re-encode 1 DDS file"));
    h.state_mut().undo();
    idle(&mut h);
    assert_eq!(toml(h.state()), before, "one Undo takes back both");
    assert!(!has_job(h.state(), "a.dds"));
    h.state_mut().redo();
    idle(&mut h);
    assert_eq!(toml(h.state()), t, "one Redo puts back both");
    assert!(has_job(h.state(), "a.dds"));
}

#[test]
fn stop_reencoding_clears_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |_| {});
    m.select_item("a.dds");
    let mut h = harness(m);
    h.run();
    h.get_by_label(REENCODE).click();
    idle(&mut h);
    assert!(has_job(h.state(), "a.dds"));
    h.get_by_label("Stop re-encoding").click();
    idle(&mut h);
    assert!(!toml(h.state()).contains("reencode"), "{}", toml(h.state()));
    assert!(!has_job(h.state(), "a.dds"), "skipped again");
    assert_eq!(h.state().undo_label(), Some("Stop re-encoding 1 DDS file"));
    h.get_by_label(REENCODE);
}

#[test]
fn bulk_reencode_counts_only_dds() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |_| {});
    m.click_item("a.dds", false, false);
    m.click_item("b.dds", true, false);
    m.click_item("c.png", true, false);
    let mut h = harness(m);
    h.run();
    let steps = h.state().history.undo.len();
    h.get_by_label("Re-encode 2 DDS files").click();
    idle(&mut h);
    assert_eq!(h.state().history.undo.len(), steps + 1, "one undo step");
    assert_eq!(h.state().undo_label(), Some("Re-encode 2 DDS files"));
    let t = toml(h.state());
    assert!(
        t.contains("[files.\"a.dds\"]") && t.contains("[files.\"b.dds\"]"),
        "{t}"
    );
    assert_eq!(t.matches("reencode = true").count(), 2, "{t}");
    assert!(has_job(h.state(), "a.dds") && has_job(h.state(), "b.dds"));
}

#[test]
fn reencode_is_off_when_read_only() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |r| {
        std::fs::write(r.join("sdc.toml"), "target = [\n").unwrap()
    });
    let before = toml(&m);
    m.select_item("a.dds");
    let mut h = harness(m);
    h.run();
    assert!(h.get_by_label(REENCODE).accesskit_node().is_disabled());
    h.get_by_label(REENCODE).click();
    h.run();
    assert_eq!(toml(h.state()), before, "nothing is written");
    hover_shows(&mut h, REENCODE, "sdc.toml is read-only");
}

/// Review Focus 4: a DDS whose image twin writes the same output is offered off, alone and in the bulk panel.
#[test]
fn reencode_with_an_image_twin_is_offered_off() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |r| png(&r.join("a.png")));
    m.select_item("a.dds");
    let mut h = harness(m);
    h.run();
    assert!(h.get_by_label(REENCODE).accesskit_node().is_disabled());
    hover_shows(&mut h, REENCODE, "a.png already writes this file");
    h.state_mut().click_item("a.dds", false, false);
    h.state_mut().click_item("a.png", true, false);
    h.run();
    let before = toml(h.state());
    let b = h.get_by_label("Re-encode DDS files");
    assert!(b.accesskit_node().is_disabled());
    b.click();
    h.run();
    hover_shows(
        &mut h,
        "Re-encode DDS files",
        "1 left out: another file already writes them",
    );
    assert_eq!(toml(h.state()), before, "nothing is written");
    assert!(
        !problems(h.state())
            .iter()
            .any(|p| p.code == Some(engine::codes::Code::InDuplicate)),
        "no IN-DUPLICATE row"
    );
}

#[test]
fn reencode_acts_on_an_accesskit_click() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |_| {});
    m.select_item("a.dds");
    let mut h = harness(m);
    h.run();
    h.get_by_label(REENCODE).click_accesskit();
    idle(&mut h);
    assert!(
        toml(h.state()).contains("reencode = true"),
        "{}",
        toml(h.state())
    );
}

/// A guard (passes before the fix too): the panel has no default, so Enter with nothing focused writes nothing.
#[test]
fn enter_with_nothing_focused_writes_no_reencode() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |_| {});
    m.select_item("a.dds");
    let mut h = harness(m);
    h.run();
    h.key_press(egui::Key::Enter);
    idle(&mut h);
    assert!(!toml(h.state()).contains("reencode"), "{}", toml(h.state()));
}

/// A23: the header issues read as one plain line; the raw `check_header` text is on hover only.
#[test]
fn dds_header_issues_read_in_words() {
    use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |r| {
        let desc = DdsDesc {
            width: 64,
            height: 64,
            format: engine::config::Format::Bc1,
            srgb: false,
            header: ResolvedHeader::Dx10,
        };
        let data = vec![[0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0].repeat(256)];
        let mut b = dds_bytes(&desc, &data).unwrap();
        // DX10 miscFlags2 (magic 4 + header 124 + dxgiFormat, resourceDimension, miscFlag, arraySize).
        b[144..148].copy_from_slice(&1u32.to_le_bytes());
        std::fs::write(r.join("h.dds"), b).unwrap();
    });
    m.select_item("h.dds");
    let mut h = harness(m);
    h.run();
    let line = "The DDS header has fields other tools may misread.";
    h.get_by_label_contains(line);
    assert_eq!(h.query_all_by_label_contains("DX10 header:").count(), 0);
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.get_by_label_contains(line).hover();
    h.run();
    h.run();
    h.get_by_label_contains("DX10 header: dimension 3, array 1, misc2 1");
}

/// Review Focus 4: into its own folder, S6 fails the file at plan: a red Failed row, never a silent skip.
#[test]
fn reencode_into_its_own_folder_is_a_failed_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = open(d.path(), |_| {});
    let root = m.root.clone().unwrap();
    choose_export(&mut m, root);
    m.select_item("a.dds");
    let mut h = harness(m);
    h.run();
    hover_shows(
        &mut h,
        REENCODE,
        "It cannot be written into the folder it comes from.",
    );
    h.get_by_label(REENCODE).click();
    idle(&mut h);
    let why = "re-encoding would overwrite the source DDS: choose another output folder";
    let item = h.state().item("a.dds").unwrap();
    assert_eq!(item.status, ItemStatus::Failed(why.into()));
    assert!(
        problems(h.state())
            .iter()
            .any(|p| p.level == Level::Blocks && p.text.contains(why)),
        "{:?}",
        problems(h.state())
    );
}

/// Note FI2 follow-up: re-encoding the diffuse of an all-DDS PBR set makes it a full PBR set with its JSON entry.
#[test]
fn reencoding_a_member_of_an_all_dds_set_makes_it_a_pbr_set() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::model::sets::{views, SlotKind};
    let d = tempfile::tempdir().unwrap();
    let root = dds_pbr_sets(d.path(), &["a"]);
    let mut m = opened_as_the_app(root, d.path().join("export"));
    let set = |m: &AppModel| {
        views(m)
            .iter()
            .find(|v| v.key.ends_with("pbr/a/a"))
            .cloned()
    };
    let before = set(&m).expect("the all-DDS set is listed");
    assert!(!before.pbr && before.slots.is_empty());
    assert!(json_entries(&m).is_empty());
    m.select_item("pbr/a/a.dds");
    let mut h = harness(m);
    h.run();
    h.get_by_label(REENCODE).click();
    idle(&mut h);
    let after = set(h.state()).unwrap();
    assert!(after.pbr, "{after:?}");
    assert!(
        after.slots.contains_key(&SlotKind::Diffuse),
        "{:?}",
        after.slots
    );
    assert_eq!(json_entries(h.state()).len(), 1);
}
