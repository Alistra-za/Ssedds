//! The Slot commands dialog (M3b spec §4.2): per slot 1–8, from this set / a path / clear, and the slot lock (F3a
//! Task 8).
use crate::model::bulk::MenuAction;
use crate::model::sets::{slot_gives, SlotCommand, SLOT_LOCKED};
use crate::model::AppModel;
use eframe::egui;
use engine::sets::same_id;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct SlotCmdState {
    /// The set whose dialog is open.
    pub open: Option<String>,
    /// Per slot: the text being typed for a Path, and the last refusal.
    pub text: [String; 8],
    pub error: [Option<String>; 8],
    /// Per slot: a choice made in the combo that is not written yet (Path waits for Apply).
    pub mode: [Option<usize>; 8],
}

const NAMES: [&str; 8] = [
    "Diffuse",
    "Normal",
    "Emissive",
    "Height",
    "Cubemap",
    "RMAOS",
    "Coat normal / fuzz",
    "Subsurface",
];
const CHOICES: [&str; 3] = ["From this set", "Path", "Clear"];

pub fn draw(ctx: &egui::Context, model: &mut AppModel, st: &mut SlotCmdState) {
    let Some(set) = st.open.clone() else { return };
    let entry = model
        .sets_doc
        .as_ref()
        .and_then(|d| d.entries().into_iter().find(|e| same_id(&e.id, &set)))
        .unwrap_or_default();
    let (settings, locks) = (entry.settings, entry.lock);
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model.banner.clone().unwrap_or_default();
    let given: Vec<String> = (1..=8u8).map(|n| slot_gives(model, &set, n)).collect();
    let mut close = false;
    let mut apply: Vec<(u8, SlotCommand)> = vec![];
    let mut lock: Vec<(u8, bool)> = vec![];
    let modal = egui::Modal::new(egui::Id::new("slot-commands")).show(ctx, |ui| {
        ui.set_max_width(640.0);
        ui.heading(format!("Slot commands: {set}"));
        ui.label("A path is relative to Data\\ and ends in .dds; textures\\ is added when missing (except for the cubemap). Base-game files in a BSA work too.");
        ui.add_space(6.0);
        egui::Grid::new("slot-cmd-grid").num_columns(5).show(ui, |ui| {
            for n in 1..=8u8 {
                let i = usize::from(n - 1);
                ui.label(format!("Slot {n} ({})", NAMES[i]));
                ui.weak(&given[i]);
                let locked = locks.contains(&n);
                let mut on = locked;
                let r = ui.add_enabled(editable, egui::Checkbox::new(&mut on, "🔒"));
                // Named per slot for screen readers (and tests): eight boxes share the icon.
                r.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        editable,
                        on,
                        format!("Lock slot {n}"),
                    )
                });
                if r.on_hover_text("Locked: drops, slot commands and bulk edits leave this slot as it is")
                    .on_disabled_hover_text(&why)
                    .changed()
                {
                    lock.push((n, on));
                }
                // A locked slot's commands are off, with the reason on hover.
                let why = if editable && locked {
                    SLOT_LOCKED.to_string()
                } else {
                    why.clone()
                };
                let editable = editable && !locked;
                let current = match settings.get(&format!("slot{n}")) {
                    None => 0,
                    Some(Value::String(s)) if s.is_empty() => 2,
                    Some(_) => 1,
                };
                if st.text[i].is_empty() {
                    if let Some(Value::String(s)) = settings.get(&format!("slot{n}")) {
                        st.text[i] = s.clone();
                    }
                }
                let mut choice = st.mode[i].unwrap_or(current);
                let before = choice;
                ui.add_enabled_ui(editable, |ui| {
                    egui::ComboBox::from_id_salt(("slot-cmd", n))
                        .selected_text(CHOICES[choice])
                        .show_ui(ui, |ui| {
                            for (c, name) in CHOICES.iter().enumerate() {
                                ui.selectable_value(&mut choice, c, *name);
                            }
                        });
                })
                .response
                .on_disabled_hover_text(&why);
                if choice != before {
                    st.error[i] = None;
                    if choice == 1 {
                        st.mode[i] = Some(1);
                    } else {
                        st.mode[i] = None;
                        apply.push((
                            n,
                            if choice == 0 {
                                SlotCommand::FromSet
                            } else {
                                SlotCommand::Clear
                            },
                        ));
                    }
                }
                ui.vertical(|ui| {
                    if choice == 1 {
                        ui.add_enabled_ui(editable, |ui| {
                            let r = ui.text_edit_singleline(&mut st.text[i]);
                            let enter =
                                r.lost_focus() && ui.input(|k| k.key_pressed(egui::Key::Enter));
                            if ui.small_button("Apply").clicked() || enter {
                                apply.push((n, SlotCommand::Path(st.text[i].clone())));
                            }
                        })
                        .response
                        .on_disabled_hover_text(&why);
                    }
                    if let Some(e) = &st.error[i] {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                });
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        close = ui.button("Close").clicked();
    });
    for (n, on) in lock {
        st.error[usize::from(n - 1)] = model.set_slot_lock(&set, n, on).err();
    }
    for (n, cmd) in apply {
        let i = usize::from(n - 1);
        let is_path = matches!(cmd, SlotCommand::Path(_));
        st.error[i] = model.set_slot_command(&set, n, cmd).err();
        if is_path && st.error[i].is_none() {
            st.mode[i] = None;
        }
    }
    if close || modal.should_close() {
        *st = SlotCmdState::default();
    }
}

/// F3a Task 8: run a slot menu pick. "Path…" opens this window on that slot to type the path; a refused command
/// opens it with the reason on that slot's row. Any other pick runs as is.
pub fn run(model: &mut AppModel, st: &mut SlotCmdState, a: MenuAction) {
    let (set, n) = match &a {
        MenuAction::Slot { set, n, .. }
        | MenuAction::SlotPath { set, n }
        | MenuAction::SlotLink { set, n, .. }
        | MenuAction::SlotLock { set, n, .. } => (set.clone(), *n),
        _ => {
            model.run_menu(a);
            return;
        }
    };
    let path = matches!(a, MenuAction::SlotPath { .. });
    let err = model.run_menu(a);
    if !path && err.is_none() {
        return;
    }
    if st.open.as_deref() != Some(set.as_str()) {
        *st = SlotCmdState::default();
        st.open = Some(set);
    }
    let i = usize::from(n.clamp(1, 8) - 1);
    if path {
        st.mode[i] = Some(1);
    }
    st.error[i] = err;
}
