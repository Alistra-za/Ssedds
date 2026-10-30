//! GUI tweaks G3–G6: the TruePBR JSON editor, a centre-column tab (or its own window when popped out, Note FB7) over
//! the whole PGPatcher JSON: every PBR set's entry and the delete sections, each in its own pill, each key a small
//! pill of its own. It edits the same settings as the set editor's TruePBR box (`AppModel::set_json_value`), so the
//! two always agree. Drawing only.
use super::columns::{AMBER, RED};
use super::path_picker;
use super::pgp_fields::{alpha_default, context, control, disabled_reason, edit_key, Ctx};
use crate::model::pbr_export::{json_issues, json_name, written_json_entries, JsonIssues};
use crate::model::sections::{delete_hits, not_written_reason, DeleteHit};
use crate::model::sets::{slot_matches, SetView};
use crate::model::AppModel;
use crate::pgp::prefix::{
    active_matches, json_rename, match_toggle, stored_set_match, NOT_WRITTEN,
    PATH_CONTAINS_NOT_WRITTEN, RENAMED_MATCH_NOTE, RENAMED_ONE_MATCH,
};
use crate::pgp::schema::{grass_ignores, spec, KeyKind, KeySpec, NumRange, KEYS};
use crate::pgp::settings::{number_default, rgb_default, Settings};
use crate::pgp::writer::{document, to_text, EntryInput, FIXED_NUMBERS};
use eframe::egui;
use engine::config::Target;
use engine::sets::same_id;
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::Arc;

pub const WINDOW_TITLE: &str = "TruePBR JSON";
const ENTRY_RADIUS: u8 = 12;
const PILL_RADIUS: u8 = 10;
/// The slot names of `match1`–`match8`.
pub(crate) const SLOT_NAMES: [&str; 8] = [
    "diffuse",
    "normal",
    "glow",
    "parallax",
    "cubemap",
    "env mask",
    "multilayer",
    "backlight",
];

/// Whose key an edit changes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    Set(String),
    Section(usize),
}

impl Owner {
    fn salt(&self) -> String {
        match self {
            Owner::Set(id) => format!("set:{id}"),
            Owner::Section(i) => format!("section:{i}"),
        }
    }
}

/// How a texture match matches: exactly in slot N (1-based), or anywhere in the diffuse path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Exact(u8),
    Contains,
}

impl Mode {
    fn key(self) -> String {
        match self {
            Mode::Exact(n) => format!("match{n}"),
            Mode::Contains => "path_contains".into(),
        }
    }

    fn label(self) -> String {
        match self {
            Mode::Exact(n) => format!("exact · slot {n} {}", SLOT_NAMES[n as usize - 1]),
            Mode::Contains => "path contains".into(),
        }
    }
}

enum Act {
    /// Fold or unfold a set's entry.
    Fold(String),
    Edit(Owner, String, Option<Value>),
    /// Several keys as one undo step (trace-debug #56).
    Edits(Owner, Vec<(String, Option<Value>)>),
    AddSection,
    RemoveSection(usize),
    Select(String),
    /// A key added from the + menu was removed before it had a value.
    Unpend(Owner, String),
    /// A key (or a match mode switched on a pill with no path) waits for its value.
    Pend(Owner, String),
}

/// The JSON name field's width; it is never squeezed (AG126).
const NAME_FIELD_WIDTH: f32 = 160.0;
/// The JSON name field's label (and how tests find the row).
pub const NAME_LABEL: &str = "JSON name:";

/// Note BH7: the PGPatcher JSON name, the editor's top field (it was in the header). The default shows as hint text;
/// empty restores it. One undo step per committed edit (`AppModel::set_json_name`), on focus loss.
fn name_field(ui: &mut egui::Ui, model: &mut AppModel) {
    let id = egui::Id::new("json-name");
    let mut t = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| {
        model
            .sets_doc
            .as_ref()
            .and_then(|d| d.json_name())
            .unwrap_or_default()
    });
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = model
        .sets_doc
        .as_ref()
        .and_then(|d| d.error())
        .unwrap_or("sdc-project.toml is read-only")
        .to_string();
    // Note FB1: no path to show until a folder is chosen (`sdc-out` is only a name).
    let path = match model.export_folder_chosen() {
        Some(root) => format!(
            "{}\\PBRNIFPatcher\\{}.json",
            root.display(),
            json_name(model)
        ),
        None => crate::model::review::NO_EXPORT_FOLDER.to_string(),
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(NAME_LABEL);
        let r = ui
            .add_enabled(
                editable,
                egui::TextEdit::singleline(&mut t)
                    .hint_text(json_name(model))
                    .desired_width(NAME_FIELD_WIDTH),
            )
            .on_hover_text(if model.export_folder_chosen().is_some() {
                format!("The PGPatcher JSON is written to {path}")
            } else {
                path.clone()
            })
            .on_disabled_hover_text(why);
        ui.weak(&path);
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id, t.clone()));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<String>(id));
            // Esc cancels (AG10/AG11, trace-debug #57): egui drops the focus on it.
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                model.set_json_name(&t);
            }
        }
    });
}

/// The editor, in the centre column or in its popped-out window (Note FB7).
pub fn view(ui: &mut egui::Ui, model: &mut AppModel) {
    let mut acts: Vec<Act> = vec![];
    let read_only = model.sets_doc.as_ref().and_then(|d| {
        d.read_only().then(|| {
            d.error()
                .unwrap_or("sdc-project.toml is read-only")
                .to_string()
        })
    });
    ui.horizontal_wrapped(|ui| {
        ui.heading(WINDOW_TITLE);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            super::centre_tabs::controls(ui, model, crate::model::CentreTab::Json);
            let r = ui
                .add_enabled(read_only.is_none(), egui::Button::new("+ Delete section"))
                .on_hover_text("An entry of its own that deletes the shapes it matches");
            if r.clicked() {
                acts.push(Act::AddSection);
            }
        });
    });
    if model.root.is_some() {
        name_field(ui, model);
    }
    if let Some(why) = &read_only {
        ui.colored_label(AMBER, why);
    }
    if model.target != Target::Cs {
        ui.weak("The TruePBR JSON is written in cs mode only.");
        return;
    }
    let built = built(ui, model);
    let (entries, issues) = (&built.entries, &built.issues);
    let sections = model.sections();
    let hits = delete_hits(model);
    let nav_id = egui::Id::new("json-editor-nav");
    let mut nav = ui.data(|d| d.get_temp::<Nav>(nav_id)).unwrap_or_default();
    ui.weak(format!(
        "{} set entr{} · {} delete section{}. Edits here and in a set's TruePBR settings are the same edits.",
        entries.len(),
        if entries.len() == 1 { "y" } else { "ies" },
        sections.len(),
        if sections.len() == 1 { "" } else { "s" },
    ));
    let spots = error_spots(entries, issues);
    let (prev, next) = toolbar(ui, &mut nav, issues, entries, &spots);
    // Trace #19: egui matches plain F8 with an extra Shift, so Shift+F8 is consumed first.
    let key_prev = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F8));
    let key_next = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F8));
    let dir = if prev || key_prev {
        Some(false)
    } else if next || key_next {
        Some(true)
    } else {
        None
    };
    if let Some(spot) = dir.and_then(|fwd| step(&spots, &nav.current, fwd)) {
        jump(&mut nav, entries, spot);
    }
    json_text(ui, &built.text);
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("json-editor-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_enabled_ui(read_only.is_none(), |ui| {
                if entries.is_empty() {
                    ui.weak("No PBR set has an entry yet.");
                }
                general_box(ui, issues, &mut nav);
                let q = nav.query.clone();
                for (v, _) in entries.iter().filter(|(v, e)| matches(v, e, &q)) {
                    set_entry(ui, model, v, &hits, issues, &mut nav, &mut acts);
                    ui.add_space(6.0);
                }
                if !sections.is_empty() {
                    ui.label(egui::RichText::new("Delete sections").strong());
                }
                for (i, s) in sections.iter().enumerate() {
                    section_entry(ui, model, i, s, &hits, &mut acts);
                    ui.add_space(6.0);
                }
            });
        });
    nav.scroll = None;
    // Trace-debug #12: sections are addressed by index, so removals run last (a stable sort keeps the rest in order);
    // otherwise a focus-loss edit pushed after a Remove lands on the shifted neighbour.
    acts.sort_by_key(|a| matches!(a, Act::RemoveSection(_)));
    for a in acts {
        match a {
            Act::Edit(Owner::Set(id), k, v) => model.set_json_value(&id, &k, v),
            Act::Edit(Owner::Section(i), k, v) => model.set_section_value(i, &k, v),
            Act::Edits(Owner::Set(id), e) => model.set_json_values(&id, e),
            Act::Edits(Owner::Section(i), e) => model.set_section_values(i, e),
            Act::AddSection => model.add_section(""),
            Act::RemoveSection(i) => {
                // Trace #21: pending keys are held per section index, so the later sections' move up with them.
                let n = sections.len();
                for j in i..n {
                    let next = if j + 1 < n {
                        pending(ui, &Owner::Section(j + 1))
                    } else {
                        Vec::new()
                    };
                    set_pending(ui, &Owner::Section(j), next);
                }
                model.remove_section(i);
            }
            Act::Select(key) => {
                model.select_set(&key);
                // Note FB7: the set's editor comes to the front of the centre.
                model.centre_tab = crate::model::CentreTab::Selection;
            }
            Act::Fold(id) => {
                if !nav.closed.remove(&id) {
                    nav.closed.insert(id);
                }
            }
            Act::Unpend(owner, key) => {
                let left = pending(ui, &owner)
                    .into_iter()
                    .filter(|k| *k != key)
                    .collect();
                set_pending(ui, &owner, left);
            }
            Act::Pend(owner, key) => {
                let w = pending(ui, &owner);
                add_pending(ui, &owner, &w, &key);
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(nav_id, nav));
}

/// The file as it would be written now (every written set as if exported), built once per revision (`built`).
fn json_text(ui: &mut egui::Ui, text: &Arc<String>) {
    egui::CollapsingHeader::new("JSON text")
        .id_salt("json-editor-text")
        .show(ui, |ui| {
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(text.to_string());
            }
            egui::ScrollArea::vertical()
                .id_salt("json-editor-text-scroll")
                .max_height(240.0)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(text.as_str()).monospace()).wrap());
                });
        });
}

fn entry_frame(ui: &egui::Ui, stroke: egui::Color32) -> egui::Frame {
    egui::Frame::group(ui.style())
        .corner_radius(ENTRY_RADIUS)
        .stroke(egui::Stroke::new(1.5, stroke))
}

/// One key as a small rounded pill.
fn pill<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .corner_radius(PILL_RADIUS)
        .fill(ui.visuals().faint_bg_color)
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| ui.horizontal(add).inner)
        .inner
}

fn remove_button(ui: &mut egui::Ui) -> bool {
    ui.small_button("×")
        .on_hover_text("Remove this key")
        .clicked()
}

/// Keys added from the + menu that have no value yet (a text key cannot be stored empty).
fn pending(ui: &egui::Ui, owner: &Owner) -> Vec<String> {
    ui.data(|d| d.get_temp::<Vec<String>>(egui::Id::new(("json-pending", owner.salt()))))
        .unwrap_or_default()
}

fn set_pending(ui: &egui::Ui, owner: &Owner, keys: Vec<String>) {
    ui.data_mut(|d| d.insert_temp(egui::Id::new(("json-pending", owner.salt())), keys));
}

fn text_of<'a>(s: &'a Settings, key: &str) -> Option<&'a str> {
    s.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// A text box committed on Enter or focus loss: the new text, when it changed.
fn text_box(ui: &mut egui::Ui, id: egui::Id, current: &str, width: f32) -> Option<String> {
    let mut t = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(width));
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, t.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        // Esc cancels (AG10/AG11, trace-debug #57).
        if t.trim() != current.trim() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Some(t.trim().to_string());
        }
    }
    None
}

/// A known-paths list for a picker, cached while the scan and the plan stay the same.
fn known(
    ui: &egui::Ui,
    model: &AppModel,
    which: &str,
    make: impl FnOnce() -> Vec<String>,
) -> Arc<Vec<String>> {
    let key = (Arc::as_ptr(&model.known) as usize, model.items_rev());
    path_picker::cached(ui, which, key, make)
}

/// The edits that store `path` under `mode` for `owner` (a set's `match1` of its derived path stays derived).
fn store(owner: &Owner, mode: Mode, path: &str, derived: &str) -> (String, Option<Value>) {
    match (owner, mode) {
        (Owner::Set(_), Mode::Exact(n)) => {
            (format!("match{n}"), stored_set_match(n, path, derived))
        }
        (_, m) => {
            let t = path.trim().replace('/', "\\");
            (m.key(), (!t.is_empty()).then(|| json!(t)))
        }
    }
}

/// The edits that store a path picked from a known-paths list. The lists use `/`; the app's own pick is stored in
/// PGPatcher's `\` (wave 2 #8), while a typed `/` still blocks (P26).
fn picked(owner: &Owner, mode: Mode, path: &str, derived: &str) -> (String, Option<Value>) {
    store(owner, mode, &path.replace('/', "\\"), derived)
}

/// The edit that removes `mode` for `owner` (a set's slot 1 match turns off as `match1 = ""`).
fn unstore(owner: &Owner, mode: Mode) -> (String, Option<Value>) {
    match (owner, mode) {
        (Owner::Set(_), Mode::Exact(n)) => match_toggle(n, false, ""),
        (_, m) => (m.key(), None),
    }
}

/// A texture match pill: how it matches (a drop-down: exact in a slot, or path contains), the path, a picker of
/// known paths for that kind of match, and ×.
#[allow(clippy::too_many_arguments)]
fn match_pill(
    ui: &mut egui::Ui,
    model: &AppModel,
    owner: &Owner,
    mode: Mode,
    path: &str,
    derived: &str,
    taken: &[Mode],
    acts: &mut Vec<Act>,
) {
    pill(ui, |ui| {
        let mut picked_mode = mode;
        egui::ComboBox::from_id_salt(("json-mode", owner.salt(), mode.key()))
            .selected_text(egui::RichText::new(mode.label()).small())
            .show_ui(ui, |ui| {
                for m in (1..=8u8).map(Mode::Exact).chain([Mode::Contains]) {
                    let free = m == mode || !taken.contains(&m);
                    ui.add_enabled_ui(free, |ui| {
                        ui.selectable_value(&mut picked_mode, m, m.label());
                    });
                }
            })
            .response
            .on_hover_text("Exact: a mesh's texture in this slot is exactly this path. Path contains: the diffuse path contains this text.");
        if picked_mode != mode && path.is_empty() {
            // Trace #20: a pill with no path yet is pending; the new mode takes its place (a store of "" is a no-op).
            acts.push(Act::Unpend(owner.clone(), mode.key()));
            acts.push(Act::Pend(owner.clone(), picked_mode.key()));
            let (k, v) = unstore(owner, mode);
            acts.push(Act::Edit(owner.clone(), k, v));
        } else if picked_mode != mode {
            let edits = vec![
                unstore(owner, mode),
                store(owner, picked_mode, path, derived),
            ];
            acts.push(Act::Edits(owner.clone(), edits));
        }
        let id = egui::Id::new(("json-match", owner.salt(), mode.key()));
        if let Some(t) = text_box(ui, id, path, 260.0) {
            let (k, v) = if t.is_empty() {
                acts.push(Act::Unpend(owner.clone(), mode.key()));
                unstore(owner, mode)
            } else {
                store(owner, mode, &t, derived)
            };
            acts.push(Act::Edit(owner.clone(), k, v));
        }
        if matches!(owner, Owner::Set(_)) && mode == Mode::Exact(1) && path == derived {
            ui.weak("derived")
                .on_hover_text("The diffuse's own path; type another to change it");
        }
        let hover = match mode {
            Mode::Exact(_) => "Pick an exact texture path the scan found in this slot",
            Mode::Contains => "Pick a texture path; any diffuse path containing it matches",
        };
        path_picker::menu(ui, "▾", |ui| {
            let list = match mode {
                Mode::Exact(n) => known(ui, model, &format!("slot{n}"), || {
                    model.known_textures(Some(n - 1))
                }),
                Mode::Contains => known(ui, model, "any", || model.known_textures(None)),
            };
            if let Some(p) =
                path_picker::list(ui, ("json-pick", owner.salt(), mode.key()), &list, false)
            {
                let (k, v) = picked(owner, mode, &p, derived);
                acts.push(Act::Edit(owner.clone(), k, v));
                ui.close();
            }
        })
        .response
        .on_hover_text(hover);
        if remove_button(ui) {
            acts.push(Act::Unpend(owner.clone(), mode.key()));
            let (k, v) = unstore(owner, mode);
            acts.push(Act::Edit(owner.clone(), k, v));
        }
    });
}

/// The `nif_filter` pill: the text, a picker of known mesh folders, and ×.
fn nif_pill(ui: &mut egui::Ui, model: &AppModel, owner: &Owner, value: &str, acts: &mut Vec<Act>) {
    pill(ui, |ui| {
        ui.label(egui::RichText::new("mesh path contains").small())
            .on_hover_text("nif_filter: only meshes whose path contains this text");
        let id = egui::Id::new(("json-nif", owner.salt()));
        if let Some(t) = text_box(ui, id, value, 220.0) {
            let t = t.replace('/', "\\");
            acts.push(Act::Edit(
                owner.clone(),
                "nif_filter".into(),
                (!t.is_empty()).then(|| json!(t)),
            ));
        }
        path_picker::menu(ui, "▾", |ui| {
            let dirs: Vec<String> = model
                .known_mesh_dirs()
                .iter()
                .map(|d| d.replace('/', "\\"))
                .collect();
            if let Some(p) = path_picker::list(ui, ("json-nif-pick", owner.salt()), &dirs, false) {
                acts.push(Act::Edit(
                    owner.clone(),
                    "nif_filter".into(),
                    Some(json!(p)),
                ));
                ui.close();
            }
        })
        .response
        .on_hover_text("Pick a mesh folder the scan found");
        if remove_button(ui) {
            acts.push(Act::Unpend(owner.clone(), "nif_filter".into()));
            acts.push(Act::Edit(owner.clone(), "nif_filter".into(), None));
        }
    });
}

/// A schema key's pill on a set: the same control as the TruePBR box.
fn key_pill(ui: &mut egui::Ui, k: &KeySpec, c: &Ctx, set: &str, acts: &mut Vec<Act>) {
    let key = edit_key(k.key);
    pill(ui, |ui| {
        if k.kind == KeyKind::Slot {
            // A slot command: set in the Slot commands dialog, shown here as written.
            let val = c.own.get(key).map(Value::to_string).unwrap_or_default();
            ui.label(egui::RichText::new(format!("{}: {val}", k.label)).small())
                .on_hover_text("A slot command; change it with Slot commands…");
            let locked = key
                .strip_prefix("slot")
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| c.locks.contains(&n));
            if locked {
                ui.label(egui::RichText::new("locked").small().weak())
                    .on_hover_text("The slot is locked; unlock it to change its command");
            } else if c.read_only.is_none() && remove_button(ui) {
                acts.push(Act::Edit(
                    Owner::Set(set.to_string()),
                    key.to_string(),
                    None,
                ));
            }
            return;
        }
        if !matches!(k.kind, KeyKind::OnlyTrue | KeyKind::Object) {
            ui.label(egui::RichText::new(k.label).small())
                .on_hover_text(k.key);
        }
        let reason = disabled_reason(k, c);
        let mut edits = vec![];
        let r = ui.add_enabled_ui(reason.is_none(), |ui| {
            control(ui, k, c.own.get(key).cloned(), c, &mut edits);
        });
        if let Some(why) = reason {
            r.response.on_disabled_hover_text(why);
        }
        if c.read_only.is_none() && remove_button(ui) {
            acts.push(Act::Unpend(Owner::Set(set.to_string()), k.key.to_string()));
            edits.push((key.to_string(), None));
        }
        acts.extend(
            edits
                .into_iter()
                .map(|(k, v)| Act::Edit(Owner::Set(set.to_string()), k, v)),
        );
    });
}

/// The match and filter keys a pill of their own draws.
fn is_match_key(key: &str) -> bool {
    key == "path_contains" || key == "nif_filter" || key.starts_with("match")
}

/// The keys a set has, other than the matches: in schema order.
fn set_keys(own: &Settings) -> Vec<&'static KeySpec> {
    KEYS.iter()
        .filter(|k| !is_match_key(k.key))
        .filter(|k| own.contains_key(edit_key(k.key)))
        .collect()
}

/// The value a key added from the + menu starts with (`None`: a text key, which waits for its text).
fn start_value(k: &KeySpec) -> Option<Value> {
    match k.kind {
        KeyKind::OnlyTrue | KeyKind::Object | KeyKind::Tri => Some(json!(true)),
        KeyKind::Number(NumRange::Int(_)) => Some(json!(alpha_default(k.key))),
        KeyKind::Number(_) => Some(json!(number_default(k.key)
            .or_else(|| {
                FIXED_NUMBERS
                    .iter()
                    .find(|(d, _)| *d == k.key)
                    .map(|(_, x)| *x)
            })
            .unwrap_or(0.0))),
        // AG133: floats; the fuzz colour starts from its default, the others from white.
        KeyKind::Rgb | KeyKind::Rgba => Some(json!(rgb_default(k.key).unwrap_or([1.0; 3]))),
        KeyKind::Text | KeyKind::Slot => None,
    }
}

fn set_entry(
    ui: &mut egui::Ui,
    model: &AppModel,
    v: &SetView,
    hits: &[DeleteHit],
    issues: &JsonIssues,
    nav: &mut Nav,
    acts: &mut Vec<Act>,
) {
    let c = context(model, &v.id);
    let owner = Owner::Set(v.id.clone());
    let mine: Vec<&DeleteHit> = hits.iter().filter(|h| same_id(&h.set_id, &v.id)).collect();
    let mine_of = |l: &[(String, String)]| -> Vec<String> {
        l.iter()
            .filter(|(id, _)| same_id(id, &v.id))
            .map(|(_, t)| t.clone())
            .collect()
    };
    let (errors, warnings) = (mine_of(&issues.errors), mine_of(&issues.warnings));
    let open = !nav.closed.contains(&v.id);
    let stroke = if !errors.is_empty() {
        RED
    } else if !mine.is_empty() || !warnings.is_empty() {
        AMBER
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    let framed = entry_frame(ui, stroke).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            let mut title = egui::RichText::new(&v.id).strong();
            if !mine.is_empty() {
                title = title.strikethrough().color(AMBER);
            }
            if ui
                .small_button(if open { "Collapse" } else { "Expand" })
                .clicked()
            {
                acts.push(Act::Fold(v.id.clone()));
            }
            ui.label(title);
            if let Some(p) = &v.prefix {
                ui.weak(format!("→ {p}"));
            }
            if ui
                .small_button("Show")
                .on_hover_text("Select this set in the main window")
                .clicked()
            {
                acts.push(Act::Select(v.key.clone()));
            }
        });
        for h in &mine {
            ui.colored_label(
                AMBER,
                format!(
                    "⚠ Delete section {} deletes the shapes this set is for: {}",
                    h.section + 1,
                    h.why
                ),
            );
        }
        issue_lines(ui, errors.iter(), warnings.iter());
        if !open {
            return;
        }
        let derived = v.derived_match.clone().unwrap_or_default();
        let mut taken: Vec<Mode> = active_matches(&c.own, &derived)
            .iter()
            .map(|(n, _)| Mode::Exact(*n))
            .collect();
        let contains = text_of(&c.own, "path_contains").map(str::to_string);
        if contains.is_some() {
            taken.push(Mode::Contains);
        }
        let waiting = pending(ui, &owner);
        if let Some((d, _)) = json_rename(&c.own, &derived) {
            ui.weak(RENAMED_MATCH_NOTE).on_hover_text(format!(
                "Written as match_diffuse = {d}; other match slots are not written"
            ));
        }
        ui.horizontal_wrapped(|ui| {
            // AN2: a renamed set's matches keep their mode (another would be refused), and only its first is written.
            let renamed = json_rename(&c.own, &derived).is_some();
            let blocked: Vec<Mode> = if renamed {
                (1..=8u8).map(Mode::Exact).chain([Mode::Contains]).collect()
            } else {
                taken.clone()
            };
            let first = active_matches(&c.own, &derived).first().map(|(n, _)| *n);
            for (n, path) in active_matches(&c.own, &derived) {
                match_pill(
                    ui,
                    model,
                    &owner,
                    Mode::Exact(n),
                    &path,
                    &derived,
                    &blocked,
                    acts,
                );
                if renamed && Some(n) != first {
                    ui.colored_label(AMBER, NOT_WRITTEN);
                }
            }
            // Trace #18: an exact slot with no known path is a pending pill, not an invisible `matchN = ""`.
            // Slot 1 too: a pill switched to slot 1 (turned off, `match1 = ""`) would otherwise vanish.
            for n in (1..=8u8).filter(|n| {
                waiting.contains(&format!("match{n}")) && !taken.contains(&Mode::Exact(*n))
            }) {
                match_pill(
                    ui,
                    model,
                    &owner,
                    Mode::Exact(n),
                    "",
                    &derived,
                    &blocked,
                    acts,
                );
            }
            if contains.is_some() || waiting.iter().any(|k| k == "path_contains") {
                let p = contains.clone().unwrap_or_default();
                match_pill(
                    ui,
                    model,
                    &owner,
                    Mode::Contains,
                    &p,
                    &derived,
                    &blocked,
                    acts,
                );
                if renamed {
                    ui.colored_label(AMBER, PATH_CONTAINS_NOT_WRITTEN);
                }
            }
            let nif = text_of(&c.own, "nif_filter").map(str::to_string);
            if nif.is_some() || waiting.iter().any(|k| k == "nif_filter") {
                nif_pill(ui, model, &owner, nif.as_deref().unwrap_or(""), acts);
            }
            for k in set_keys(&c.own) {
                key_pill(ui, k, &c, &v.id, acts);
            }
            for key in waiting.iter().filter(|k| !is_match_key(k)) {
                if let Some(k) = spec(key).filter(|_| !c.own.contains_key(key.as_str())) {
                    key_pill(ui, k, &c, &v.id, acts);
                }
            }
            // Hand-written keys sdc does not know: shown, and removable.
            for (k, val) in c
                .own
                .iter()
                .filter(|(k, _)| spec(k).is_none() && !k.ends_with("_on") && !is_match_key(k))
            {
                pill(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("{k}: {val}"))
                            .small()
                            .color(AMBER),
                    )
                    .on_hover_text("A key Tessalith does not know; it is kept as written");
                    if remove_button(ui) {
                        acts.push(Act::Edit(owner.clone(), k.clone(), None));
                    }
                });
            }
            add_set_key_menu(ui, v, &c, &owner, &taken, &waiting, acts);
        });
    });
    if nav.scroll == Some(Spot::Entry(v.id.clone())) {
        ui.scroll_to_rect(framed.response.rect, Some(egui::Align::Center));
    }
    // A pending key that now has a value is no longer pending (read again: the + menu may have added one).
    let left: Vec<String> = pending(ui, &owner)
        .into_iter()
        .filter(|k| {
            // A stored empty match (slot 1 turned off, `match1 = ""`) has no pill, so it keeps waiting.
            !c.own.contains_key(edit_key(k))
                || (is_match_key(k)
                    && matches!(c.own.get(edit_key(k)), Some(Value::String(s)) if s.is_empty()))
        })
        .collect();
    set_pending(ui, &owner, left);
}

fn add_set_key_menu(
    ui: &mut egui::Ui,
    v: &SetView,
    c: &Ctx,
    owner: &Owner,
    taken: &[Mode],
    waiting: &[String],
    acts: &mut Vec<Act>,
) {
    let derived = v.derived_match.clone().unwrap_or_default();
    ui.menu_button("+ key", |ui| {
        ui.menu_button("Match", |ui| {
            let (_, addable) = slot_matches(v);
            // AN2: a renamed set takes no second match.
            let renamed = json_rename(&c.own, &derived).is_some();
            let refused = (renamed && taken.iter().any(|m| matches!(m, Mode::Exact(_))))
                .then_some(RENAMED_ONE_MATCH);
            for n in 1..=8u8 {
                if taken.contains(&Mode::Exact(n)) {
                    continue;
                }
                if ui
                    .add_enabled(refused.is_none(), egui::Button::new(Mode::Exact(n).label()))
                    .on_disabled_hover_text(refused.unwrap_or_default())
                    .clicked()
                {
                    let path = if n == 1 {
                        derived.clone()
                    } else {
                        addable
                            .iter()
                            .find(|a| a.n == n)
                            .map(|a| a.path.clone())
                            .unwrap_or_default()
                    };
                    if n != 1 && path.is_empty() {
                        add_pending(ui, owner, waiting, &format!("match{n}"));
                    } else {
                        let (k, val) = match_toggle(n, true, &path);
                        acts.push(Act::Edit(owner.clone(), k, val));
                    }
                    ui.close();
                }
            }
            let pc = renamed.then_some(RENAMED_ONE_MATCH);
            if !taken.contains(&Mode::Contains)
                && ui
                    .add_enabled(pc.is_none(), egui::Button::new("path contains"))
                    .on_disabled_hover_text(pc.unwrap_or_default())
                    .clicked()
            {
                add_pending(ui, owner, waiting, "path_contains");
                ui.close();
            }
            if !c.own.contains_key("nif_filter")
                && ui.button("mesh path contains (nif_filter)").clicked()
            {
                add_pending(ui, owner, waiting, "nif_filter");
                ui.close();
            }
        });
        ui.separator();
        for k in KEYS.iter().filter(|k| {
            !is_match_key(k.key) && k.kind != KeyKind::Slot && !c.own.contains_key(edit_key(k.key))
        }) {
            // PBR grass: a key grass ignores would be stored but never shown, and come back on a switch to pbr.
            if c.grass && grass_ignores(k.key) {
                continue;
            }
            // A sub-key of glint or fuzz is offered while the feature is on.
            if let Some((obj, _)) = k.key.split_once('.') {
                if c.own.get(&format!("{obj}_on")).and_then(Value::as_bool) != Some(true) {
                    continue;
                }
            }
            let reason = disabled_reason(k, c);
            let r = ui
                .add_enabled(reason.is_none(), egui::Button::new(k.label))
                .on_hover_text(k.key);
            let r = match reason {
                Some(why) => r.on_disabled_hover_text(why),
                None => r,
            };
            if r.clicked() {
                match start_value(k) {
                    Some(val) => acts.push(Act::Edit(
                        owner.clone(),
                        edit_key(k.key).to_string(),
                        Some(val),
                    )),
                    None => add_pending(ui, owner, waiting, k.key),
                }
                ui.close();
            }
        }
    });
}

fn add_pending(ui: &egui::Ui, owner: &Owner, waiting: &[String], key: &str) {
    let mut w = waiting.to_vec();
    if !w.iter().any(|k| k == key) {
        w.push(key.to_string());
    }
    set_pending(ui, owner, w);
}

fn section_entry(
    ui: &mut egui::Ui,
    model: &AppModel,
    i: usize,
    s: &Settings,
    hits: &[DeleteHit],
    acts: &mut Vec<Act>,
) {
    let owner = Owner::Section(i);
    entry_frame(ui, RED).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("Delete section {}", i + 1)).strong());
            if ui
                .small_button("Remove")
                .on_hover_text("Remove this section")
                .clicked()
            {
                acts.push(Act::RemoveSection(i));
            }
        });
        if let Some(why) = not_written_reason(s) {
            ui.colored_label(AMBER, why);
        }
        for h in hits.iter().filter(|h| h.section == i) {
            ui.colored_label(
                AMBER,
                format!("⚠ Deletes shapes of set {}: {}", h.set_id, h.why),
            );
        }
        let mut taken: Vec<Mode> = (1..=8u8)
            .filter(|n| s.contains_key(&format!("match{n}")))
            .map(Mode::Exact)
            .collect();
        if s.contains_key("path_contains") {
            taken.push(Mode::Contains);
        }
        let waiting = pending(ui, &owner);
        ui.horizontal_wrapped(|ui| {
            for m in taken.clone() {
                let p = text_of(s, &m.key()).unwrap_or("").to_string();
                match_pill(ui, model, &owner, m, &p, "", &taken, acts);
            }
            for key in waiting
                .iter()
                .filter(|k| is_match_key(k) && *k != "nif_filter")
            {
                let m = if key == "path_contains" {
                    Mode::Contains
                } else {
                    Mode::Exact(key[5..].parse().unwrap_or(1))
                };
                if !taken.contains(&m) {
                    match_pill(ui, model, &owner, m, "", "", &taken, acts);
                }
            }
            let nif = text_of(s, "nif_filter").map(str::to_string);
            if nif.is_some() || waiting.iter().any(|k| k == "nif_filter") {
                nif_pill(ui, model, &owner, nif.as_deref().unwrap_or(""), acts);
            }
            pill(ui, |ui| {
                ui.label(egui::RichText::new("delete: true").small())
                    .on_hover_text("PGPatcher deletes every trishape this section matches");
            });
            ui.menu_button("+ key", |ui| {
                for n in 1..=8u8 {
                    if !taken.contains(&Mode::Exact(n))
                        && ui.button(Mode::Exact(n).label()).clicked()
                    {
                        add_pending(ui, &owner, &waiting, &format!("match{n}"));
                        ui.close();
                    }
                }
                if !taken.contains(&Mode::Contains) && ui.button("path contains").clicked() {
                    add_pending(ui, &owner, &waiting, "path_contains");
                    ui.close();
                }
                if nif.is_none() && ui.button("mesh path contains (nif_filter)").clicked() {
                    add_pending(ui, &owner, &waiting, "nif_filter");
                    ui.close();
                }
            });
        });
    });
    let left: Vec<String> = pending(ui, &owner)
        .into_iter()
        .filter(|k| !s.contains_key(k.as_str()))
        .collect();
    set_pending(ui, &owner, left);
}

thread_local! {
    // Per thread: tests run side by side, each drawing on its own thread.
    static BUILDS: Cell<usize> = const { Cell::new(0) };
}

/// How often this thread's editor rebuilt its JSON text and issues (tests: once per revision, not once per frame).
pub fn cache_builds() -> usize {
    BUILDS.get()
}

/// Everything the editor derives from the model, rebuilt only when `AppModel::json_key` changes (DECISIONS AG109).
#[derive(Clone)]
struct Built {
    key: (crate::model::ViewKey, u64, String),
    entries: Arc<Vec<(SetView, EntryInput)>>,
    text: Arc<String>,
    issues: Arc<JsonIssues>,
}

fn built(ui: &egui::Ui, model: &AppModel) -> Built {
    let id = egui::Id::new("json-editor-built");
    let key = model.json_key();
    if let Some(b) = ui
        .data(|d| d.get_temp::<Built>(id))
        .filter(|b| b.key == key)
    {
        return b;
    }
    BUILDS.set(BUILDS.get() + 1);
    // Only the entries the file gets: the statics rule's dropped sets are left out, as export does (design §5.1).
    let entries = written_json_entries(model);
    let mut inputs: Vec<EntryInput> = entries.iter().map(|(_, e)| e.clone()).collect();
    inputs.extend(
        model
            .sections()
            .iter()
            .filter(|s| crate::model::sections::written(s))
            .map(|s| EntryInput {
                settings: s.clone(),
                section: true,
                ..EntryInput::default()
            }),
    );
    let ids: Vec<&str> = entries.iter().map(|(v, _)| v.id.as_str()).collect();
    let b = Built {
        key,
        text: Arc::new(to_text(&document(&inputs))),
        issues: Arc::new(json_issues(model, &ids)),
        entries: Arc::new(entries.clone()),
    };
    ui.data_mut(|d| d.insert_temp(id, b.clone()));
    b
}

/// A place Next error can go to: the General box, or a set's entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Spot {
    General,
    Entry(String),
}

/// The editor's view state: the search text, the folded entries and where Next error is.
#[derive(Clone, Default)]
struct Nav {
    query: String,
    closed: BTreeSet<String>,
    current: Option<Spot>,
    /// Scroll to this at the next draw.
    scroll: Option<Spot>,
}

type Entries = [(SetView, EntryInput)];

/// The spots with an error, in the order they are shown (never filtered, AG108).
fn error_spots(entries: &Entries, issues: &JsonIssues) -> Vec<Spot> {
    let general = (!issues.general_errors.is_empty()).then_some(Spot::General);
    general
        .into_iter()
        .chain(
            entries
                .iter()
                .filter(|(v, _)| issues.errors.iter().any(|(id, _)| same_id(id, &v.id)))
                .map(|(v, _)| Spot::Entry(v.id.clone())),
        )
        .collect()
}

/// The spot after (or before) `cur`, wrapping; the first (or last) when `cur` is none or gone.
fn step(spots: &[Spot], cur: &Option<Spot>, forward: bool) -> Option<Spot> {
    let n = spots.len();
    let at = cur.as_ref().and_then(|c| spots.iter().position(|s| s == c));
    let i = match (at, forward) {
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
        (None, true) => 0,
        (None, false) => n.checked_sub(1)?,
    };
    spots.get(i).cloned()
}

/// Go to `spot`: unfold it, scroll to it, and drop a search that hides it.
fn jump(nav: &mut Nav, entries: &Entries, spot: Spot) {
    if let Spot::Entry(id) = &spot {
        nav.closed.remove(id);
        let hidden = entries
            .iter()
            .any(|(v, e)| same_id(&v.id, id) && !matches(v, e, &nav.query));
        if hidden {
            nav.query.clear();
        }
    }
    nav.scroll = Some(spot.clone());
    nav.current = Some(spot);
}

/// Whether the entry shows under the search text: a case-insensitive substring of the set's name or of any path in it.
fn matches(v: &SetView, e: &EntryInput, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let paths = active_matches(&e.settings, &e.derived_match)
        .into_iter()
        .map(|(_, p)| p)
        .chain(e.slots.values().cloned())
        .chain(
            e.settings
                .iter()
                .filter(|(k, _)| is_match_key(k) || k.starts_with("slot"))
                .filter_map(|(_, val)| val.as_str().map(str::to_string)),
        );
    std::iter::once(v.id.clone())
        .chain(paths)
        .any(|s| s.to_lowercase().contains(&q))
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// The counts, Prev / Next error, and the search box. Returns whether Prev and Next were clicked.
fn toolbar(
    ui: &mut egui::Ui,
    nav: &mut Nav,
    issues: &JsonIssues,
    entries: &Entries,
    spots: &[Spot],
) -> (bool, bool) {
    let (errors, warnings) = (issues.error_count(), issues.warning_count());
    let mut clicked = (false, false);
    ui.horizontal_wrapped(|ui| {
        let text = format!(
            "{}, {}",
            plural(errors, "error"),
            plural(warnings, "warning")
        );
        let text = egui::RichText::new(text).strong();
        ui.label(match (errors, warnings) {
            (0, 0) => text.weak(),
            (0, _) => text.color(AMBER),
            _ => text.color(RED),
        });
        let on = !spots.is_empty();
        clicked.0 = ui
            .add_enabled(on, egui::Button::new("Prev"))
            .on_hover_text("The previous entry with an error (Shift+F8)")
            .clicked();
        clicked.1 = ui
            .add_enabled(on, egui::Button::new("Next error"))
            .on_hover_text("The next entry with an error, wrapping at the end (F8)")
            .clicked();
        ui.add(
            egui::TextEdit::singleline(&mut nav.query)
                .hint_text("Search sets and paths")
                .desired_width(220.0),
        );
        let shown = entries
            .iter()
            .filter(|(v, e)| matches(v, e, &nav.query))
            .count();
        ui.weak(format!("showing {shown} of {}", entries.len()));
    });
    clicked
}

/// Errors and warnings that belong to no entry.
fn general_box(ui: &mut egui::Ui, issues: &JsonIssues, nav: &mut Nav) {
    if issues.general_errors.is_empty() && issues.general_warnings.is_empty() {
        return;
    }
    let stroke = if issues.general_errors.is_empty() {
        AMBER
    } else {
        RED
    };
    let r = entry_frame(ui, stroke).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new("General").strong());
        issue_lines(
            ui,
            issues.general_errors.iter(),
            issues.general_warnings.iter(),
        );
    });
    if nav.scroll == Some(Spot::General) {
        ui.scroll_to_rect(r.response.rect, Some(egui::Align::Center));
    }
    ui.add_space(6.0);
}

/// Red lines for errors, amber for warnings.
fn issue_lines<'a>(
    ui: &mut egui::Ui,
    errors: impl Iterator<Item = &'a String>,
    warnings: impl Iterator<Item = &'a String>,
) {
    for t in errors {
        ui.colored_label(RED, format!("✖ {t}"));
    }
    for t in warnings {
        ui.colored_label(AMBER, format!("⚠ {t}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trace-debug wave 2 #8: the picker lists `textures/...` with `/`; a pick stores PGPatcher's `\`, and slot 1's
    /// own diffuse picked from the list stays derived.
    #[test]
    fn exact_pick_on_set_stores_backslashes() {
        let set = Owner::Set("landscape/rock01".into());
        let derived = r"landscape\rock01";
        assert_eq!(
            picked(&set, Mode::Exact(2), "textures/architecture/x_n", derived),
            ("match2".into(), Some(json!(r"textures\architecture\x_n")))
        );
        assert_eq!(
            picked(&set, Mode::Exact(1), "textures/landscape/rock01", derived),
            ("match1".into(), None)
        );
    }
}
