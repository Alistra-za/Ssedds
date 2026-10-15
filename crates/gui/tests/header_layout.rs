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
    "Editor…",
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
        // The JSON name field keeps a usable width.
        let header = PanelState::load(&h.ctx, Id::new("header"))
            .unwrap()
            .outer_rect;
        let field = h
            .query_all_by_role(Role::TextInput)
            .map(|n| n.rect())
            .find(|r| header.contains_rect(*r))
            .expect("the JSON name field");
        assert!(field.width() >= 80.0, "at {w} the JSON field is {field:?}");
    }
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
