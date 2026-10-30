//! One type picker (owner amendment J, Note FB9) for Problems' "Set type…" and the texture panel's Sub-type: a popup
//! anchored to its button, the suggestion pinned on top, a search box focused on open, the grouped list below.
use super::settings_panel::{subtype_groups, subtype_label};
use eframe::egui;
use engine::config::{Subtype, Target};
use engine::policy::offered_for;

/// Which control the picker serves. Not `PickerKind`: `model::picker` names its file pickers that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    /// Deciding an undecided texture (Problems): the types the target offers.
    SetType,
    /// The texture panel's Sub-type field: the current type stays listed even where the target no longer offers it.
    SubType,
}

/// The list's height before it scrolls.
pub const LIST_HEIGHT: f32 = 320.0;

/// The picker on `anchor`: a click on it opens or closes the popup `id`. Returns the type picked this frame. Esc, a
/// click outside or a pick closes it; Esc does nothing else (owner amendment G). `grass` also offers `complex_grass`
/// (`engine::policy::grass_context`, Note FC10).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    anchor: &egui::Response,
    current: Option<Subtype>,
    suggested: Option<Subtype>,
    kind: TypeKind,
    target: Target,
    grass: bool,
) -> Option<Subtype> {
    let offered = |t: Target, s: Subtype| offered_for(t, s, grass);
    let query_id = id.with("query");
    let was_open = egui::Popup::is_id_open(ui.ctx(), id);
    let mut picked = None;
    egui::Popup::from_toggle_button_response(anchor)
        .id(id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .width(260.0)
        .show(|ui| {
            let mut q: String = ui.data(|d| d.get_temp(query_id)).unwrap_or_default();
            let hint = "Search types";
            let r = ui.add(
                egui::TextEdit::singleline(&mut q)
                    .id(query_id.with("edit"))
                    .hint_text(hint),
            );
            let name = match kind {
                TypeKind::SetType => "Search the types to set",
                TypeKind::SubType => "Search the sub-types",
            };
            r.widget_info(|| {
                let mut i = egui::WidgetInfo::text_edit(true, "", &q, hint);
                i.label = Some(name.to_string());
                i
            });
            if !was_open {
                r.request_focus();
            }
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(query_id, q.clone()));
            }
            // Labels are the ids with spaces, so "subsurface_tint" finds "subsurface tint" too.
            let needle = q.trim().to_lowercase().replace('_', " ");
            let hit = |s: Subtype| subtype_label(s).to_lowercase().contains(&needle);
            // A suggestion the target doesn't offer is not pinned: picking it would be refused.
            if let Some(s) = suggested.filter(|s| hit(*s) && offered(target, *s)) {
                let text = format!("{} (suggested)", subtype_label(s));
                if ui.selectable_label(current == Some(s), text).clicked() {
                    picked = Some(s);
                }
                ui.separator();
            }
            let groups = subtype_groups(|s| {
                offered(target, s) || (kind == TypeKind::SubType && current == Some(s))
            });
            // ui-rules 3.2: a bar drawn without hover, so a list that overflows says so.
            ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
            egui::ScrollArea::vertical()
                .id_salt(id.with("list"))
                .max_height(LIST_HEIGHT)
                .show(ui, |ui| {
                    let mut any = false;
                    for (heading, members) in groups {
                        let shown: Vec<Subtype> = members.into_iter().filter(|s| hit(*s)).collect();
                        if shown.is_empty() {
                            continue;
                        }
                        any = true;
                        ui.label(egui::RichText::new(heading).weak());
                        for s in shown {
                            if ui
                                .selectable_label(current == Some(s), subtype_label(s))
                                .on_hover_text(s.id())
                                .clicked()
                            {
                                picked = Some(s);
                            }
                        }
                    }
                    if !any {
                        ui.weak("No type matches");
                    }
                });
        });
    if picked.is_some() {
        egui::Popup::close_id(ui.ctx(), id);
    }
    if !egui::Popup::is_id_open(ui.ctx(), id) {
        ui.data_mut(|d| d.remove::<String>(query_id));
    }
    picked
}
