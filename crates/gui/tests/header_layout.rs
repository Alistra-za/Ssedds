//! AG126: the header row never overlaps itself at any window width, and every control stays in the row. Layout v2 A4:
//! one row, Export to ▾ · Save · Export at the right edge; the header is the same in the Meshes view (C7).
mod support;
use eframe::egui::{self, accesskit::Role, containers::panel::PanelState, Id};
use egui_kittest::kittest::{by, NodeT as _, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::meshes::MeshesModel;
use sdc_gui::model::AppModel;
use sdc_gui::settings::MeshesSettings;
use sdc_gui::ui::header::elide_middle;
use sdc_gui::ui::State;
use support::*;

type H = Harness<'static, (AppModel, State, MeshesModel)>;

const WIDTHS: [f32; 4] = [1440.0, 1024.0, 800.0, 640.0];

/// The controls every Textures header must show (plus "Export to: …", matched by its start).
const CONTROLS: [&str; 12] = [
    "Open",
    "Undo",
    "Redo",
    "MO2…",
    "Mode",
    "Preset",
    "Textures view",
    "Meshes view",
    "Textures column",
    "Sets column",
    "Save",
    "Export",
];

fn harness(m: AppModel, w: f32, meshes_view: bool) -> H {
    harness_sized(m, egui::vec2(w, 800.0), meshes_view)
}

fn harness_sized(m: AppModel, size: egui::Vec2, meshes_view: bool) -> H {
    let mut meshes = MeshesModel::new(None, MeshesSettings::default());
    meshes.procs = sdc_gui::model::meshes::no_procs;
    let mut h = Harness::builder().with_size(size).build_ui_state(
        move |ui, (m, s, me): &mut (AppModel, State, MeshesModel)| {
            sdc_gui::ui::header::set_meshes_view(ui.ctx(), meshes_view);
            if meshes_view {
                // K6: the app's Meshes branch (K3): the header, the bottom panel, then the view.
                sdc_gui::ui::header::draw(ui, m, Some(&mut *me));
                sdc_gui::ui::bottom::draw(ui, m, &mut s.drawer);
                sdc_gui::ui::meshes::show(ui, me);
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
        h.get_by_label_contains("Export to: ");
        // R14: Messages, Presets…, Recent and PG JSON Editor left the header.
        let header = outer(&h, "header");
        for gone in ["Messages", "Presets…", "Recent", "PG JSON Editor"] {
            assert!(
                !h.query_all_by_label(gone)
                    .any(|n| header.contains_rect(n.rect())),
                "at {w} the header still has {gone}"
            );
        }
        // Note BH7: the JSON name field moved into the editor window.
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

#[test]
fn elide_middle_keeps_the_drive_and_the_tail() {
    let p = r"C:\Users\Curti\AppData\Local\Temp\mods\My Mod";
    assert_eq!(elide_middle(p, 100), p);
    assert_eq!(elide_middle(p, 20), r"C:\…\mods\My Mod");
    assert_eq!(elide_middle(p, 24), r"C:\…\Temp\mods\My Mod");
    assert_eq!(elide_middle(r"C:\a\VeryLongFolderName", 10), "…olderName");
}

/// A4 (K2): one row: every control's rect holds Open's centre line, and they run left to right in the spec's order.
#[test]
fn the_header_is_one_row_in_the_spec_order() {
    let d = tempfile::tempdir().unwrap();
    let h = harness_sized(pbr_opened(d.path()), egui::vec2(1600.0, 1000.0), false);
    let order = [
        ("Open", h.get_by_label("Open").rect()),
        ("Undo", h.get_by_label("Undo").rect()),
        ("Redo", h.get_by_label("Redo").rect()),
        ("Mode", h.get_by_label("Mode").rect()),
        ("Preset", h.get_by_label("Preset").rect()),
        ("Textures view", h.get_by_label("Textures view").rect()),
        ("Meshes view", h.get_by_label("Meshes view").rect()),
        ("Export to", h.get_by_label_contains("Export to: ").rect()),
        ("Save", h.get_by_label("Save").rect()),
        ("Export", h.get_by_label("Export").rect()),
    ];
    let y = order[0].1.center().y;
    for (name, r) in order {
        assert!(
            r.min.y <= y && y <= r.max.y,
            "{name} {r:?} is off the row at y {y}"
        );
        // K2: the single-height controls share the centre line (the tabs are 6 pt taller, Save/Export two rows).
        if !matches!(name, "Textures view" | "Meshes view" | "Save" | "Export") {
            assert!((r.center().y - y).abs() < 2.0, "{name} {r:?} vs y {y}");
        }
    }
    for pair in order.windows(2) {
        assert!(
            pair[0].1.center().x < pair[1].1.center().x,
            "{} {:?} is not left of {} {:?}",
            pair[0].0,
            pair[0].1,
            pair[1].0,
            pair[1].1
        );
    }
    // Export ends the row at the right edge, only the panel margin past it.
    assert!(1600.0 - order[9].1.right() <= 16.5, "{:?}", order[9].1);
}

/// A4: Save sits right before Export, as tall as it (two rows).
#[test]
fn save_sits_next_to_export_at_its_height() {
    let d = tempfile::tempdir().unwrap();
    for w in [1600.0, 1024.0] {
        let h = harness(pbr_opened(&d.path().join(format!("{w}"))), w, false);
        let (s, e) = (
            h.get_by_label("Save").rect(),
            h.get_by_label("Export").rect(),
        );
        assert!((s.height() - e.height()).abs() < 1.0, "at {w}: {s:?} {e:?}");
        assert!(
            s.right() <= e.left() && e.left() - s.right() < 12.0,
            "at {w}: {s:?} {e:?}"
        );
        assert!(
            (s.center().y - e.center().y).abs() < 1.0,
            "at {w}: {s:?} {e:?}"
        );
        let open = h.get_by_label("Open").rect();
        assert!(
            e.height() > 1.5 * open.height(),
            "at {w}: {e:?} vs {open:?}"
        );
        assert!(e.width() >= 230.0, "at {w}: {e:?}");
    }
}

/// A4, K1, K8: a long export folder is cut in the middle so Export to, Save and Export stay on one row; the hover and
/// the accessible name keep the whole path.
#[test]
fn the_export_folder_shows_its_tail_and_the_whole_path_on_hover() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    // Far longer than the window is wide (300+ characters): uncut, the button would run off it.
    let mut deep = d.path().to_path_buf();
    for i in 0..8 {
        deep = deep.join(format!("a rather long folder name number {i}"));
    }
    let deep = deep.join("mods").join("My Mod");
    m.settings.export_folder = Some(deep.clone());
    let full = deep.display().to_string();
    let mut h = harness(m, 1024.0, false);
    let b = h.get_by_label_contains(r"\mods\My Mod");
    assert_eq!(
        b.accesskit_node().label().unwrap_or_default(),
        format!("Export to: {full}")
    );
    // Cut: the button shares Export's row and ends before Save and Export, inside the window.
    assert!(full.chars().count() > 300);
    let r = b.rect();
    let (save, export) = (
        h.get_by_label("Save").rect(),
        h.get_by_label("Export").rect(),
    );
    assert!(
        r.min.y <= export.center().y && export.center().y <= r.max.y,
        "{r:?} {export:?}"
    );
    assert!(
        r.right() <= save.left() && export.right() <= 1024.0,
        "{r:?} {save:?} {export:?}"
    );
    assert!(h.query_by_label(&full).is_none());
    h.get_by_label_contains(r"\mods\My Mod").hover();
    h.run();
    // The tooltip: the whole path, alone.
    h.get_by_label(&full);
}

/// Note BH15: the columns' eye toggles keep their accessible names and say whether the column shows.
#[test]
fn the_column_toggles_are_eyes_named_for_their_columns() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()), 1440.0, false);
    let toggled = |h: &H| h.get_by_label("Textures column").accesskit_node().toggled();
    assert_eq!(
        h.get_by_label("Textures column").accesskit_node().role(),
        Role::CheckBox
    );
    assert_eq!(toggled(&h), Some(egui::accesskit::Toggled::True));
    // R14: the eyes follow the view tabs.
    let (meshes, eye) = (
        h.get_by_label("Meshes view").rect(),
        h.get_by_label("Textures column").rect(),
    );
    assert!(
        eye.left() >= meshes.right() && eye.left() - meshes.right() < 24.0,
        "{meshes:?} {eye:?}"
    );
    h.get_by_label("Textures column").click();
    h.run_steps(2);
    assert!(h.state().0.settings.textures_hidden);
    assert_eq!(toggled(&h), Some(egui::accesskit::Toggled::False));
    // The visible text is the bare column name.
    assert!(button_text_is_short(&h));
}

/// The toggles show "Textures" and "Sets" (shorter than the old "… column" labels).
fn button_text_is_short(h: &H) -> bool {
    let w = h.get_by_label("Textures column").rect().width();
    w < 100.0
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

/// Note BH7, R14: Export to ▾ → PGPatcher JSON editor… opens the editor, whose top field renames the JSON.
#[test]
fn pg_json_editor_opens_the_editor_with_the_name_field() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()), 1440.0, false);
    assert!(h
        .query_by_label(sdc_gui::ui::json_editor::NAME_LABEL)
        .is_none());
    h.get_by_label_contains("Export to: ").click();
    h.run_steps(2);
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

/// C7: the Meshes view draws the same header (no eyes: they hide Textures-view columns).
#[test]
fn the_meshes_header_never_overlaps() {
    let d = tempfile::tempdir().unwrap();
    for w in WIDTHS {
        let h = harness(pbr_opened(&d.path().join(format!("{w}"))), w, true);
        assert_no_overlap(&h, "header", w);
        for c in [
            "Open",
            "Undo",
            "MO2…",
            "Mode",
            "Meshes view",
            "Save",
            "Export",
        ] {
            h.get_by_label(c);
        }
        assert!(h.query_by_label("Textures column").is_none());
        assert!(PanelState::load(&h.ctx, Id::new("meshes-header")).is_none());
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
