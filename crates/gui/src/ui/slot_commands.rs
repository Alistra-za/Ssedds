//! The Slot commands dialog (M3b spec §4.2): per slot 1–8, from this set / a path / clear.
use crate::model::sets::{set_views, SlotCommand, SlotFill, SlotKind};
use crate::model::AppModel;
use eframe::egui;
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

/// What the set itself gives at slot `n`: the file names of the slot cards that fill it.
fn gives(model: &AppModel, set: &str, n: u8) -> String {
    let Some(v) = set_views(model)
        .into_iter()
        .find(|v| v.id.eq_ignore_ascii_case(set))
    else {
        return String::new();
    };
    let names: Vec<String> = SlotKind::ALL
        .iter()
        .filter(|s| s.slot_number() == n)
        .filter_map(|s| match v.slots.get(s)? {
            SlotFill::Own { item } => model.item(item).map(|i| i.name.clone()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        "nothing".into()
    } else {
        names.join(", ")
    }
}

pub fn draw(ctx: &egui::Context, model: &mut AppModel, st: &mut SlotCmdState) {
    let Some(set) = st.open.clone() else { return };
    let settings = model
        .sets_doc
        .as_ref()
        .and_then(|d| {
            d.entries()
                .into_iter()
                .find(|e| e.id.eq_ignore_ascii_case(&set))
        })
        .map(|e| e.settings)
        .unwrap_or_default();
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model.banner.clone().unwrap_or_default();
    let given: Vec<String> = (1..=8u8).map(|n| gives(model, &set, n)).collect();
    let mut close = false;
    let mut apply: Vec<(u8, SlotCommand)> = vec![];
    let modal = egui::Modal::new(egui::Id::new("slot-commands")).show(ctx, |ui| {
        ui.set_max_width(640.0);
        ui.heading(format!("Slot commands: {set}"));
        ui.label("A path is relative to Data\\ and ends in .dds; textures\\ is added when missing (except for the cubemap). Base-game files in a BSA work too.");
        ui.add_space(6.0);
        egui::Grid::new("slot-cmd-grid").num_columns(4).show(ui, |ui| {
            for n in 1..=8u8 {
                let i = usize::from(n - 1);
                ui.label(format!("Slot {n} ({})", NAMES[i]));
                ui.weak(&given[i]);
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
