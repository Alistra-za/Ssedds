//! Owner amendment J (Note FB9): Set type and the texture panel's Sub-type share one picker: a popup anchored to its
//! button, the suggestion pinned on top, a search box focused on open, the grouped list below; Esc closes it.
mod support;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::config::{Subtype, Target};
use sdc_gui::model::{AppModel, Selection};
use sdc_gui::ui::settings_panel::subtype_label;
use sdc_gui::ui::type_picker::{self, TypeKind};
use support::*;

const PICKER: &str = "test-picker";

struct State {
    top: f32,
    picked: Option<Subtype>,
}

/// A "Set type…" button `top` px down a 600×700 window, with the picker on it (subsurface tint suggested).
fn bare(top: f32) -> Harness<'static, State> {
    Harness::builder()
        .with_size(egui::vec2(600.0, 700.0))
        .build_ui_state(
            |ui, s: &mut State| {
                ui.add_space(s.top);
                let b = ui.button("Set type…");
                let id = egui::Id::new(PICKER);
                let suggested = Some(Subtype::SubsurfaceTint);
                if let Some(st) =
                    type_picker::show(ui, id, &b, None, suggested, TypeKind::SetType, Target::Cs)
                {
                    s.picked = Some(st);
                }
            },
            State { top, picked: None },
        )
}

fn popup_rect(h: &Harness<'static, State>) -> egui::Rect {
    h.ctx
        .memory(|m| m.area_rect(egui::Id::new(PICKER)))
        .expect("the picker is drawn")
}

fn open(h: &mut Harness<'static, State>) -> egui::Rect {
    h.run();
    let b = h.get_by_label("Set type…").rect();
    h.get_by_label("Set type…").click();
    h.run();
    b
}

/// Every type the cs target offers, by its label.
fn offered_labels() -> Vec<String> {
    Subtype::ALL
        .into_iter()
        .filter(|s| engine::policy::offered(Target::Cs, *s))
        .map(subtype_label)
        .collect()
}

#[test]
fn type_picker_opens_at_its_anchor() {
    // Room below: the popup's top is just under the button.
    let mut h = bare(10.0);
    let b = open(&mut h);
    let p = popup_rect(&h);
    assert!(
        p.top() >= b.bottom() - 1.0 && p.top() - b.bottom() <= 40.0,
        "popup {p:?} under button {b:?}"
    );
    let search = h.get_by_role(Role::TextInput);
    assert!(search.is_focused(), "the search box has focus on open");
    // The suggestion is the first entry, above every listed type.
    let first = h.get_by_label("subsurface tint (suggested)").rect();
    assert!(first.top() > search.rect().top());
    for l in offered_labels() {
        if let Some(n) = h.query_by_label(&l) {
            assert!(n.rect().top() > first.top(), "{l} above the suggestion");
        }
    }
    // Typing narrows the list to the entries containing the text.
    h.get_by_role(Role::TextInput).type_text("tint");
    h.run();
    for l in offered_labels() {
        assert_eq!(
            h.query_by_label(&l).is_some(),
            l.contains("tint"),
            "{l} after typing tint"
        );
    }
    h.get_by_label("subsurface tint (suggested)");
    // Esc closes it, and a pick returns the type and closes it.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_role(Role::TextInput).is_none(), "Esc closes it");
    open(&mut h);
    h.get_by_label("diffuse");
    // The list scrolls: narrow it so the entry is on screen, as a user would.
    h.get_by_role(Role::TextInput).type_text("back");
    h.run();
    h.get_by_label("backlight tint").click();
    h.run();
    assert_eq!(h.state().picked, Some(Subtype::BacklightTint));
    assert!(
        h.query_by_role(Role::TextInput).is_none(),
        "a pick closes it"
    );

    // No room below: it flips above, just over the button.
    let mut h = bare(650.0);
    let b = open(&mut h);
    let p = popup_rect(&h);
    assert!(
        p.bottom() <= b.top() + 1.0 && b.top() - p.bottom() <= 40.0,
        "popup {p:?} over button {b:?}"
    );
}

/// Review C7 round 1: the Sub-type button keeps the old combo's gating. A read-only sdc.toml disables it (the
/// panel's `add_enabled_ui(!read_only)`); `busy` never disabled the combo (only sliders defer their commit), so it
/// stays enabled while a plan runs, like the other combos in the panel.
#[test]
fn subtype_button_is_disabled_on_a_read_only_sdc_toml() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(
        root.join("sdc.toml"),
        "[files.\"landscape/rock01.png\"]\nbogus_field = 1\n",
    )
    .unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..Default::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(m.project.as_ref().unwrap().read_only());
    m.select_item("landscape/rock01_normal.png");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let b = h.get_by_role_and_label(Role::ComboBox, "Sub-type");
    assert!(b.accesskit_node().is_disabled(), "Sub-type is enabled");
    b.click();
    h.run();
    h.run();
    assert!(
        h.query_all_by_role(Role::TextInput)
            .all(|n| !n.is_focused()),
        "a disabled button opened the picker"
    );
}

#[test]
fn subtype_combo_uses_the_same_picker() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    let key = "landscape/rock01.png";
    m.select_item(key);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let inputs = |h: &Harness<'static, AppModel>| h.query_all_by_role(Role::TextInput).count();
    let before = inputs(&h);
    let combo = || {
        h.query_all_by_role(Role::ComboBox)
            .find(|n| n.value().as_deref() == Some("diffuse"))
            .expect("the sub-type control shows the current sub-type")
            .rect()
    };
    let at = combo();
    h.query_all_by_role(Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("diffuse"))
        .unwrap()
        .click();
    h.run();
    h.run();
    assert_eq!(inputs(&h), before + 1, "the picker's search box");
    let search = h
        .query_all_by_role(Role::TextInput)
        .find(|n| n.is_focused())
        .expect("the search box has focus");
    let gap = (search.rect().top() - at.bottom()).min((at.top() - search.rect().bottom()).abs());
    assert!(gap <= 40.0, "search {:?} far from {at:?}", search.rect());
    // Esc closes only the picker: the texture stays selected (owner amendment G).
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(inputs(&h), before);
    assert!(matches!(&h.state().selection, Selection::Item(k) if k == key));
    // A pick edits the field.
    h.query_all_by_role(Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("diffuse"))
        .unwrap()
        .click();
    h.run();
    h.run();
    h.query_all_by_role(Role::TextInput)
        .find(|n| n.is_focused())
        .unwrap()
        .type_text("tint");
    h.run();
    h.get_by_label("subsurface tint").click();
    h.run();
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    let v = h
        .state()
        .project
        .as_ref()
        .unwrap()
        .file_value(key, "subtype");
    assert_eq!(v, Some(serde_json::json!("subsurface_tint")));
}
