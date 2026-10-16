//! AG126: the header rows never overlap themselves at any window width, and every control stays in the row.
mod support;
use eframe::egui::{self, accesskit::Role, containers::panel::PanelState, Id};
use egui_kittest::kittest::{by, NodeT as _, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::meshes::MeshesModel;
use sdc_gui::model::AppModel;
use sdc_gui::settings::MeshesSettings;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State, MeshesModel)>;

const WIDTHS: [f32; 4] = [1440.0, 1024.0, 800.0, 640.0];

/// The controls every Textures header must show.
const CONTROLS: [&str; 15] = [
    "Presets…",
    "Preset",
    "MO2…",
    "Open folder…",
    "Recent",
    "Undo",
    "Redo",
    "Textures view",
    "Meshes view",
    "Textures column",
    "Sets column",
    "Save",
    "PG JSON Editor",
    "Export",
    "Messages",
];

fn harness(m: AppModel, w: f32, meshes_view: bool) -> H {
    let mut meshes = MeshesModel::new(None, MeshesSettings::default());
    meshes.procs = sdc_gui::model::meshes::no_procs;
    let mut h = Harness::builder()
        .with_size(egui::vec2(w, 800.0))
        .build_ui_state(
            move |ui, (m, s, me): &mut (AppModel, State, MeshesModel)| {
                sdc_gui::ui::header::set_meshes_view(ui.ctx(), meshes_view);
                if meshes_view {
                    sdc_gui::ui::meshes::page(ui, me);
                } else {
                    sdc_gui::ui::draw_with_scan(ui, m, s, Some(me));
                }
            },
            (m, State::default(), meshes),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(4);
    h
}

/// The widgets inside the panel `id`, not the containers: (role, label, rect).
fn widgets(h: &H, id: &str) -> Vec<(Role, String, egui::Rect)> {
    let panel = PanelState::load(&h.ctx, Id::new(id)).unwrap().outer_rect;
    h.query_all(by().predicate(|n| n.bounding_box().is_some()))
        .filter(|n| {
            matches!(
                n.accesskit_node().role(),
                Role::Button | Role::CheckBox | Role::ComboBox | Role::TextInput | Role::Label
            )
        })
        .map(|n| {
            let a = n.accesskit_node();
            (a.role(), a.label().unwrap_or_default(), n.rect())
        })
        .filter(|(_, _, r)| panel.contains_rect(*r))
        .collect()
}

fn assert_no_overlap(h: &H, id: &str, w: f32) {
    let ws = widgets(h, id);
    assert!(!ws.is_empty(), "{id} at {w}: no widgets");
    for (i, a) in ws.iter().enumerate() {
        assert!(
            a.2.right() <= w + 0.5,
            "{id} at {w}: {a:?} runs off the window"
        );
        for b in &ws[i + 1..] {
            let overlap = a.2.shrink(0.5).intersects(b.2.shrink(0.5));
            assert!(!overlap, "{id} at {w}: {a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn the_textures_header_never_overlaps() {
    let d = tempfile::tempdir().unwrap();
    for w in WIDTHS {
        let h = harness(pbr_opened(&d.path().join(format!("{w}"))), w, false);
        assert_no_overlap(&h, "header", w);
        // Note AM: an idle scan row is not drawn (the source moved to MO2…).
        assert!(PanelState::load(&h.ctx, Id::new("textures-scan")).is_none());
        for c in CONTROLS {
            h.get_by_label(c);
        }
        h.get_by_label_contains("Export folder");
        // Note BH7: the JSON name field moved into the editor window.
        let header = outer(&h, "header");
        assert!(
            !h.query_all_by_role(Role::TextInput)
                .any(|n| header.contains_rect(n.rect())),
            "at {w} the header still has a text field"
        );
    }
}

fn outer(h: &H, id: &str) -> egui::Rect {
    PanelState::load(&h.ctx, Id::new(id)).unwrap().outer_rect
}

/// Note BH3/BH4: Export spans both rows at the right edge; Messages sits on Save, both as wide, together as tall.
#[test]
fn export_spans_two_rows_at_the_right_with_messages_over_save() {
    let d = tempfile::tempdir().unwrap();
    for w in [1440.0, 1024.0] {
        let h = harness(pbr_opened(&d.path().join(format!("{w}"))), w, false);
        let export = h.get_by_label("Export").rect();
        let save = h.get_by_label("Save").rect();
        let msgs = h.get_by_label("Messages").rect();
        let open = h.get_by_label("Open folder…").rect();
        let cols = h.get_by_label("Textures column").rect();
        // Right-anchored: right of every other header widget, only the panel margin past it.
        assert!(widgets(&h, "header")
            .iter()
            .all(|(_, _, r)| r.right() <= export.right() + 0.5));
        assert!(w - export.right() <= 16.5, "at {w}: {export:?}");
        assert!(export.width() >= 230.0, "at {w}: {export:?}");
        // Two rows: from the top row's top to the bottom row's bottom.
        assert!(
            export.top() <= open.top() + 1.0,
            "at {w}: {export:?} vs {open:?}"
        );
        assert!(
            export.bottom() >= cols.bottom() - 1.0,
            "at {w}: {export:?} vs {cols:?}"
        );
        assert!(export.height() >= 2.0 * open.height(), "at {w}: {export:?}");
        // The stack: Messages over Save, directly left of Export.
        assert!(
            (save.width() - msgs.width()).abs() < 0.5,
            "{save:?} {msgs:?}"
        );
        assert!(msgs.bottom() <= save.top() + 0.5, "{msgs:?} {save:?}");
        assert!((save.left() - msgs.left()).abs() < 0.5);
        assert!(save.right() < export.left() && export.left() - save.right() < 12.0);
        assert!(
            (msgs.top() - export.top()).abs() < 1.0
                && (save.bottom() - export.bottom()).abs() < 1.0
        );
        // Note BH5: the column checkboxes start the bottom row.
        assert!((cols.left() - open.left()).abs() < 1.0 && cols.top() > open.bottom());
    }
}

/// Note BH9: a running export shows its progress under the Export button, in the header.
#[test]
fn a_running_export_shows_progress_in_the_header() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()), 1440.0, false);
    let header = outer(&h, "header");
    let bar = |h: &H| {
        h.query_all_by_role(Role::ProgressIndicator)
            .any(|n| header.contains_rect(n.rect()))
    };
    assert!(!bar(&h));
    h.state_mut().0.export.running = Some(Default::default());
    h.run_steps(2);
    assert!(bar(&h));
    h.get_by_label("0/0");
    assert!(h.get_by_label("Export").accesskit_node().is_disabled());
    let export = h.get_by_label("Export").rect();
    let p = h.get_by_label("0/0").rect();
    assert!(p.top() >= export.bottom() - 0.5 && (p.left() - export.left()).abs() < 1.0);
}

/// Note BH7: PG JSON Editor opens the editor, whose top field renames the JSON.
#[test]
fn pg_json_editor_opens_the_editor_with_the_name_field() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()), 1440.0, false);
    assert!(h
        .query_by_label(sdc_gui::ui::json_editor::NAME_LABEL)
        .is_none());
    h.get_by_label(sdc_gui::ui::header::JSON_EDITOR).click();
    h.run_steps(2);
    assert!(h.state().0.json_editor_open);
    let label = h.get_by_label(sdc_gui::ui::json_editor::NAME_LABEL).rect();
    // The field right of the label on its row.
    h.query_all_by_role(Role::TextInput)
        .filter(|n| (n.rect().center().y - label.center().y).abs() < 6.0)
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the JSON name field")
        .click();
    h.run();
    h.event(egui::Event::Text("My Pack".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let name = h.state().0.sets_doc.as_ref().unwrap().json_name();
    assert_eq!(name.as_deref(), Some("My Pack"));
    assert!(h.state().0.can_undo());
    h.state_mut().0.undo();
    assert_ne!(
        h.state()
            .0
            .sets_doc
            .as_ref()
            .unwrap()
            .json_name()
            .as_deref(),
        Some("My Pack")
    );
}

#[test]
fn the_meshes_header_never_overlaps() {
    let d = tempfile::tempdir().unwrap();
    for w in WIDTHS {
        let h = harness(pbr_opened(&d.path().join(format!("{w}"))), w, true);
        assert_no_overlap(&h, "meshes-header", w);
        h.get_by_label("MO2…");
        // Note AM: an idle bar with no status holds nothing.
        if !widgets(&h, "meshes-bar").is_empty() {
            assert_no_overlap(&h, "meshes-bar", w);
        }
    }
}

/// The 3D view's height under this row (at least 72 % of the centre at 1024×768) is checked by
/// `set_preview_layout::in_3d_the_viewport_takes_most_of_the_centre`.
/// Note AO: the set header's input name box and its ▾ never overlap each other or run off the window, and the box keeps
/// at least 160 pt (the locked folder label included).
#[test]
fn the_set_header_input_name_box_never_overlaps() {
    let d = tempfile::tempdir().unwrap();
    for w in [1024.0, 800.0, 640.0] {
        let mut m = pbr_opened(&d.path().join(format!("{w}")));
        let key = sdc_gui::model::pbr_export::json_entries(&m)
            .into_iter()
            .find(|(v, _)| v.id == "landscape/rock01")
            .unwrap()
            .0
            .key;
        m.select_set(&key);
        let h = harness(m, w, false);
        let edit = h
            .get(
                by().role(Role::TextInput)
                    .label(sdc_gui::ui::input_name::HINT),
            )
            .rect();
        let locked = h.get_by_label("textures/landscape/").rect();
        let drop = h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).rect();
        let all = [locked, edit, drop];
        for (i, a) in all.iter().enumerate() {
            assert!(a.right() <= w + 0.5, "at {w}: {a:?} runs off the window");
            for b in &all[i + 1..] {
                assert!(
                    !a.shrink(0.5).intersects(b.shrink(0.5)),
                    "at {w}: {a:?} overlaps {b:?}"
                );
            }
        }
        assert!(
            edit.right() - locked.left() >= 160.0 - 1.0,
            "at {w} the box is {locked:?}..{edit:?}"
        );
    }
}

/// Note BH: renders the header (with rock01 selected, so the set row shows under it) to `SDC_TOOLBAR_PNG` for the
/// owner. Needs a GPU: `cargo test -p sdc-gui --test header_layout -- --ignored render_the_toolbar`.
#[test]
#[ignore]
fn render_the_toolbar() {
    let out = std::env::var("SDC_TOOLBAR_PNG").unwrap_or_else(|_| "toolbar-after.png".into());
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = sdc_gui::model::pbr_export::json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    let scale = 1.5;
    let mut meshes = MeshesModel::new(None, MeshesSettings::default());
    meshes.procs = sdc_gui::model::meshes::no_procs;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1440.0, 800.0))
        .with_pixels_per_point(scale)
        .wgpu()
        .build_ui_state(
            |ui, (m, s, me): &mut (AppModel, State, MeshesModel)| {
                sdc_gui::ui::draw_with_scan(ui, m, s, Some(me))
            },
            (m, State::default(), meshes),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(6);
    let edit = h
        .get(
            by().role(Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .rect();
    let img = h.render().expect("render");
    let bottom = ((edit.bottom() + 12.0) * scale) as u32;
    let crop =
        image::imageops::crop_imm(&img, 0, 0, img.width(), bottom.min(img.height())).to_image();
    crop.save(&out).expect("save png");
}
