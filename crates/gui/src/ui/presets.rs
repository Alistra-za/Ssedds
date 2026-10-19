//! The preset creator window (Note AM, owner design §2). Drawing only: every edit is collected and applied to the
//! `PresetsModel` after the window is drawn.
use super::columns::RED;
use super::settings_panel::{combo, options_of, subtype_label, Options};
use crate::model::fields::{
    encoding_options, field_lock, filter_option_lock, format_label, is_cutout,
};
use crate::model::presets::{
    is_data, layout, PresetsModel, Row, FAMILIES, FILL_ROLES, ORM_EXAMPLE, SWIZZLE_SOURCES,
};
use crate::model::AppModel;
use eframe::egui;
use engine::config::{Backend, MipFilter, Profile, Subtype, Target};
use serde_json::{json, Value};

/// The window's id (its rectangle is kept in `gui.toml`, AM11).
pub fn window_id() -> egui::Id {
    egui::Id::new("presets-window")
}

#[derive(Clone)]
enum Act {
    /// A Select, New or Duplicate the user confirmed over unsaved edits (AM15).
    Discard(Box<Act>),
    OpenFolder,
    Select(String),
    Duplicate,
    New,
    Id(String),
    Base(String),
    Target(Option<Target>),
    /// A field of a sub-type (`None`) or of one of its suffix branches (Note AP).
    Set(Subtype, Option<String>, &'static str, Value),
    Clear(Subtype, Option<String>, &'static str),
    AddAlias(Subtype, String, String),
    RemoveAlias(String),
    Save,
    /// The answers to a Save refused because the file changed on disk (audit item 4).
    Overwrite,
    Reload,
    KeepEditing,
    Revert,
    Delete,
    ResetAll,
}

/// A9: the saved rect moved (then shrunk) to fit inside `area`, the window below the header; with none saved,
/// a rect centred on `centre` at 90 % of its size, at least 640x480, shrunk to `area`.
pub fn place(saved: Option<egui::Rect>, area: egui::Rect, centre: egui::Rect) -> egui::Rect {
    let (size, want) = match saved {
        Some(r) => (r.size(), r.min),
        None => {
            let s = (centre.size() * 0.9).max(egui::vec2(640.0, 480.0));
            (s, centre.center() - s / 2.0)
        }
    };
    let size = size.min(area.size());
    let min = want.max(area.min).min(area.max - size);
    egui::Rect::from_min_size(min, size)
}

pub fn window(ctx: &egui::Context, model: &mut AppModel) {
    if !model.presets_open {
        return;
    }
    let target = model.target;
    let area = super::below_header(ctx).unwrap_or_else(|| ctx.content_rect());
    let saved = model
        .settings
        .layout
        .presets_window
        .map(|[x, y, w, h]| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)));
    let rect = place(saved, area, super::centre_rect(ctx).unwrap_or(area));
    let pm = model.presets_model();
    let mut acts = vec![];
    let mut open = true;
    egui::Window::new("Presets")
        .id(window_id())
        .open(&mut open)
        .resizable(true)
        .default_rect(rect)
        .constrain_to(area)
        // K6: constrain_to alone lets a window taller than `area` rise over the header.
        .max_size(area.size())
        .show(ctx, |ui| {
            top_bar(ui, pm, &mut acts);
            ui.separator();
            if pm.draft.is_none() {
                ui.weak("Pick a preset, or New.");
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| body(ui, pm, &mut acts));
        });
    discard_prompt(ctx, model, &mut acts);
    let mut saved = false;
    for a in acts {
        let pm = model.presets_model();
        let a = match a {
            Act::Discard(inner) => *inner,
            a @ (Act::Select(_) | Act::New | Act::Duplicate) if pm.dirty() => {
                ctx.data_mut(|d| d.insert_temp(discard_id(), a));
                continue;
            }
            a => a,
        };
        let err = match a {
            Act::Discard(_) => None,
            Act::OpenFolder => {
                if let Some(dir) = pm.dir().map(std::path::Path::to_path_buf) {
                    model
                        .shell
                        .push(crate::model::shell::ShellRequest::Open(dir));
                }
                None
            }
            Act::Select(id) => {
                pm.select(&id);
                None
            }
            Act::Duplicate => {
                pm.duplicate();
                None
            }
            Act::New => {
                pm.new_preset(target);
                None
            }
            Act::Id(s) => {
                pm.set_id(&s);
                None
            }
            Act::Base(s) => {
                pm.set_base(&s);
                None
            }
            Act::Target(t) => {
                pm.set_target(t);
                None
            }
            Act::Set(st, scope, f, v) => {
                match scope {
                    None => pm.set_field(st, f, v),
                    Some(s) => pm.set_branch_field(st, &s, f, v),
                }
                None
            }
            Act::Clear(st, scope, f) => {
                match scope {
                    None => pm.clear_field(st, f),
                    Some(s) => pm.clear_branch_field(st, &s, f),
                }
                None
            }
            Act::AddAlias(st, a, c) => match pm.add_alias(&a, &c) {
                Ok(()) => {
                    ctx.data_mut(|d| d.insert_temp(open_branch_id(st, &a), true));
                    None
                }
                Err(e) => Some((st, e)),
            },
            Act::RemoveAlias(a) => {
                pm.remove_alias(&a);
                None
            }
            Act::Save => {
                match pm.save() {
                    Ok(_) => saved = true,
                    Err(e) => pm.error = Some(e),
                }
                None
            }
            Act::Overwrite => {
                match pm.save_overwrite() {
                    Ok(_) => saved = true,
                    Err(e) => pm.error = Some(e),
                }
                None
            }
            Act::Reload => {
                pm.reload_from_disk();
                None
            }
            Act::KeepEditing => {
                pm.changed_on_disk = false;
                pm.error = None;
                None
            }
            Act::Revert => {
                pm.revert();
                None
            }
            Act::Delete => {
                if let Err(e) = pm.delete() {
                    pm.error = Some(e);
                } else {
                    saved = true;
                }
                None
            }
            Act::ResetAll => {
                pm.reset_all();
                None
            }
        };
        if let Some((st, e)) = err {
            ctx.data_mut(|d| d.insert_temp(add_error_id(st), e));
        }
    }
    if saved {
        // The presets folder changed: the next plan loads it (and the header's preset list already shows it).
        model.replan();
    }
    if !open {
        model.presets_open = false;
    }
}

fn add_error_id(st: Subtype) -> egui::Id {
    egui::Id::new(("preset-add-error", st))
}

fn top_bar(ui: &mut egui::Ui, pm: &PresetsModel, acts: &mut Vec<Act>) {
    // AM16: files of the presets folder that did not load, with why.
    if !pm.skipped.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("{} preset files did not load:", pm.skipped.len()),
            );
            if pm.dir().is_some() && ui.button("Open folder").clicked() {
                acts.push(Act::OpenFolder);
            }
        });
        for (file, reason) in &pm.skipped {
            let name = std::path::Path::new(file)
                .file_name()
                .map_or(file.clone(), |n| n.to_string_lossy().into_owned());
            ui.label(egui::RichText::new(format!("{name}: {reason}")).small())
                .on_hover_text(file);
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Preset");
        let shown = pm
            .draft
            .as_ref()
            .map(|_| pm.id())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "(none)".into());
        egui::ComboBox::from_id_salt("presets-pick")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (id, builtin) in pm.list() {
                    let label = if builtin {
                        format!("{id} (built-in)")
                    } else {
                        id.clone()
                    };
                    if ui.selectable_label(false, label).clicked() {
                        acts.push(Act::Select(id));
                    }
                }
            });
        if ui
            .add_enabled(pm.draft.is_some(), egui::Button::new("Duplicate"))
            .on_hover_text("An editable copy under a new name (built-ins are read-only)")
            .clicked()
        {
            acts.push(Act::Duplicate);
        }
        if ui.button("New").clicked() {
            acts.push(Act::New);
        }
    });
    let Some(d) = &pm.draft else { return };
    let ro = d.read_only;
    if ro {
        ui.weak("Built-in presets are read-only: Duplicate makes an editable copy.");
    }
    ui.add_enabled_ui(!ro, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Name");
            let mut id = pm.id();
            if ui
                .add(egui::TextEdit::singleline(&mut id).desired_width(180.0))
                .on_hover_text("The preset's id and file name: lower-case letters, digits and '-'")
                .changed()
            {
                acts.push(Act::Id(id));
            }
            ui.label("Base");
            let base = pm.base();
            egui::ComboBox::from_id_salt("presets-base")
                .selected_text(&base)
                .show_ui(ui, |ui| {
                    for (b, _) in pm.list() {
                        if b != pm.id() && ui.selectable_label(b == base, &b).clicked() {
                            acts.push(Act::Base(b));
                        }
                    }
                });
            ui.label("Target");
            let own = pm.own_target();
            let name = |t: Option<Target>| match t {
                Some(Target::Cs) => "cs".to_string(),
                Some(Target::Vanilla) => "vanilla".to_string(),
                None => "from the base".to_string(),
            };
            egui::ComboBox::from_id_salt("presets-target")
                .selected_text(name(own))
                .show_ui(ui, |ui| {
                    for t in [None, Some(Target::Cs), Some(Target::Vanilla)] {
                        if ui.selectable_label(t == own, name(t)).clicked() && t != own {
                            acts.push(Act::Target(t));
                        }
                    }
                });
        });
    });
    ui.horizontal_wrapped(|ui| {
        let ok = pm.status().is_ok();
        if ui
            .add_enabled(!ro && ok && pm.dirty(), egui::Button::new("Save preset"))
            .clicked()
        {
            acts.push(Act::Save);
        }
        if ui
            .add_enabled(!ro && pm.dirty(), egui::Button::new("Revert"))
            .clicked()
        {
            acts.push(Act::Revert);
        }
        if ui
            .add_enabled(!ro, egui::Button::new("Reset preset"))
            .on_hover_text("Drop every sub-type override, so the base shows through")
            .clicked()
        {
            acts.push(Act::ResetAll);
        }
        if pm.deletable() {
            let id = ui.make_persistent_id("presets-delete-ask");
            let asking = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
            if asking {
                let file = pm.source_file().unwrap_or_else(|| pm.id());
                ui.colored_label(RED, format!("Delete {file}?"));
                if ui.button("Delete the file").clicked() {
                    acts.push(Act::Delete);
                    ui.data_mut(|d| d.remove::<bool>(id));
                }
                if ui.button("Keep").clicked() {
                    ui.data_mut(|d| d.remove::<bool>(id));
                }
            } else if ui.button("Delete…").clicked() {
                ui.data_mut(|d| d.insert_temp(id, true));
            }
        }
        match pm.status() {
            Ok(()) => {
                ui.weak(format!("Loads as a {} preset", target_text(pm.target)));
            }
            Err(e) => {
                ui.colored_label(RED, e);
            }
        }
    });
    if let Some(e) = &pm.error {
        ui.colored_label(RED, e);
    }
    // Audit item 4: the file was edited by hand since it was picked.
    if pm.changed_on_disk {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Overwrite")
                .on_hover_text("Write your version over the file")
                .clicked()
            {
                acts.push(Act::Overwrite);
            }
            if ui
                .button("Reload")
                .on_hover_text("Drop your changes and load the file")
                .clicked()
            {
                acts.push(Act::Reload);
            }
            if ui.button("Cancel").clicked() {
                acts.push(Act::KeepEditing);
            }
        });
    }
    egui::CollapsingHeader::new("TOML preview")
        .id_salt("presets-toml")
        .show(ui, |ui| {
            let mut text = pm.toml_text();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .code_editor()
                    .interactive(false)
                    .desired_width(f32::INFINITY),
            );
        });
}

fn target_text(t: Target) -> &'static str {
    match t {
        Target::Cs => "cs",
        Target::Vanilla => "vanilla",
    }
}

fn body(ui: &mut egui::Ui, pm: &PresetsModel, acts: &mut Vec<Act>) {
    let ro = pm.draft.as_ref().is_some_and(|d| d.read_only);
    // AP4: rules the creator does not own are kept as they are and shown read-only.
    let hand = pm.hand_rules();
    if !hand.is_empty() {
        egui::CollapsingHeader::new(format!("Other rules ({}, read-only)", hand.len()))
            .id_salt("preset-hand-rules")
            .show(ui, |ui| {
                ui.weak("Kept as written. Every matching rule applies in file order; the suffix branches come after these, so they win on the fields they set.");
                for r in hand {
                    ui.label(egui::RichText::new(r).monospace());
                    ui.separator();
                }
            });
    }
    // AP9, Note AZ: PBR / CS, then Vanilla; inside each, the Colour, Normal, Data and Special groups it has.
    let layout = layout();
    for (family, hint) in FAMILIES {
        egui::CollapsingHeader::new(egui::RichText::new(family).heading())
            .id_salt(("preset-family", family))
            .default_open(true)
            .show(ui, |ui| {
                ui.weak(format!("{family} — {hint}"));
                for (_, g, sts) in layout.iter().filter(|(f, _, _)| *f == family) {
                    ui.strong(*g);
                    for row in pm.rows().iter().filter(|r| sts.contains(&r.subtype)) {
                        row_header(ui, pm, row, ro, acts);
                    }
                }
            });
    }
}

fn row_header(ui: &mut egui::Ui, pm: &PresetsModel, row: &Row, ro: bool, acts: &mut Vec<Act>) {
    let st = row.subtype;
    let fmt = match &row.profile {
        Ok(p) => format_label(p.format, p.color.tag),
        Err(_) => "does not load".into(),
    };
    egui::CollapsingHeader::new(format!("{} — {fmt}", subtype_label(st)))
        .id_salt(("preset-row", st))
        .show(ui, |ui| match &row.profile {
            Ok(p) => {
                ui.add_enabled_ui(!ro, |ui| subtype_body(ui, pm, row, p, acts))
                    .inner
            }
            Err(e) => {
                ui.colored_label(RED, e);
            }
        });
}

/// Where a field edit lands (Note AP): the sub-type's own table (`None`, the "no suffix" default) or a suffix branch.
type Scope<'a> = Option<&'a str>;

fn sub(ui: &mut egui::Ui, st: Subtype, scope: Scope, name: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(name)
        .id_salt(("preset-sub", st, scope, name))
        .default_open(true)
        .show(ui, body);
}

fn subtype_body(ui: &mut egui::Ui, pm: &PresetsModel, row: &Row, p: &Profile, acts: &mut Vec<Act>) {
    sub(ui, row.subtype, None, "Suffixes", |ui| {
        suffixes(ui, pm, row, acts)
    });
    tree(ui, pm, row, p, None, acts);
}

/// The settings tree of the sub-type (`scope` `None`) or of one of its suffix branches: the same subheaders, with
/// the base (or, in a branch, the sub-type) as the hint.
fn tree(
    ui: &mut egui::Ui,
    pm: &PresetsModel,
    row: &Row,
    p: &Profile,
    scope: Scope,
    acts: &mut Vec<Act>,
) {
    let st = row.subtype;
    sub(ui, st, scope, "Compression", |ui| {
        grid(ui, st, scope, "c", |ui| {
            for (f, l) in [
                ("format", "Format"),
                ("encoder.backend", "Encoder"),
                ("encoder.effort", "Effort"),
            ] {
                field_row(ui, pm, row, p, scope, f, l, acts);
            }
        });
    });
    sub(ui, st, scope, "Resize & mips", |ui| {
        grid(ui, st, scope, "m", |ui| {
            for (f, l) in [
                ("resize", "Resize"),
                ("mips.levels", "Mip levels"),
                ("mips.filter", "Mip filter"),
            ] {
                field_row(ui, pm, row, p, scope, f, l, acts);
            }
            if is_cutout(p) {
                field_row(ui, pm, row, p, scope, "mips.coverage", "Coverage", acts);
            }
        });
    });
    if uses_alpha(row) {
        sub(ui, st, scope, "Alpha", |ui| {
            grid(ui, st, scope, "a", |ui| {
                field_row(ui, pm, row, p, scope, "alpha.policy", "Policy", acts);
                if is_cutout(p) {
                    threshold_row(ui, pm, row, p, scope, acts);
                }
            });
        });
    }
    if is_data(st) {
        sub(ui, st, scope, "Channel remap", |ui| {
            swizzle_row(ui, pm, row, p, scope, acts)
        });
        sub(ui, st, scope, "Packing fills", |ui| {
            fills(ui, pm, row, p, scope, acts)
        });
    }
}

/// AM5: the Alpha subheader shows where the base's format keeps an alpha channel.
fn uses_alpha(row: &Row) -> bool {
    row.base
        .as_ref()
        .or(row.profile.as_ref().ok())
        .is_some_and(|b| b.format.has_alpha())
}

fn grid(
    ui: &mut egui::Ui,
    st: Subtype,
    scope: Scope,
    salt: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    egui::Grid::new(("preset-grid", st, scope, salt))
        .num_columns(4)
        .show(ui, body);
}

/// What a field's hint shows: the base's profile for the sub-type; for a branch, what its files get without it (AP11).
fn hint_profile<'a>(row: &'a Row, scope: Scope) -> (Option<&'a Profile>, &'static str) {
    match scope {
        None => (row.base.as_ref(), "base"),
        Some(s) => (
            row.branches
                .iter()
                .find(|b| b.suffix == s)
                .and_then(|b| b.inherited.as_ref().ok()),
            "inherits",
        ),
    }
}

fn own_value(pm: &PresetsModel, st: Subtype, scope: Scope, field: &str) -> bool {
    match scope {
        None => pm.field(st, field),
        Some(s) => pm.branch_field(st, s, field),
    }
    .is_some()
}

fn set(st: Subtype, scope: Scope, field: &'static str, v: Value) -> Act {
    Act::Set(st, scope.map(str::to_string), field, v)
}

fn clear(st: Subtype, scope: Scope, field: &'static str) -> Act {
    Act::Clear(st, scope.map(str::to_string), field)
}

/// A field's value in `p`, as the options name it (the format as `[format, tag]`).
fn value_of(p: &Profile, field: &str) -> Value {
    if field == "format" {
        return json!([p.format, p.color.tag]);
    }
    let v = serde_json::to_value(p).unwrap_or_default();
    v.pointer(&format!("/{}", field.replace('.', "/")))
        .cloned()
        .unwrap_or_else(|| match field {
            "resize" => json!("none"),
            _ => Value::Null,
        })
}

/// The options a field offers for `p`: only what the engine accepts (owner design §2).
pub fn field_options(field: &str, t: Target, p: &Profile) -> Options {
    match field {
        "format" => encoding_options(t, p)
            .into_iter()
            .map(|((f, tag), label)| (json!([f, tag]), label))
            .collect(),
        "mips.filter" => options_of(field, t, p)
            .unwrap_or_default()
            .into_iter()
            .filter(|(v, _)| {
                serde_json::from_value::<MipFilter>(v.clone())
                    .is_ok_and(|f| f == p.mips.filter || filter_option_lock(p, f).is_none())
            })
            .collect(),
        "encoder.backend" => options_of(field, t, p)
            .unwrap_or_default()
            .into_iter()
            .filter(|(v, _)| {
                serde_json::from_value::<Backend>(v.clone())
                    .is_ok_and(|b| engine::policy::backend_fits(b, p.format))
            })
            .collect(),
        _ => options_of(field, t, p).unwrap_or_default(),
    }
}

fn label_of(opts: &Options, v: &Value) -> String {
    opts.iter().find(|(o, _)| o == v).map_or_else(
        || v.to_string().trim_matches('"').to_string(),
        |(_, l)| l.clone(),
    )
}

/// A field row: label, the value's combo, the base's (or, in a branch, the sub-type's) value as a hint, ↺ when the
/// scope sets it.
#[allow(clippy::too_many_arguments)]
fn field_row(
    ui: &mut egui::Ui,
    pm: &PresetsModel,
    row: &Row,
    p: &Profile,
    scope: Scope,
    field: &'static str,
    label: &str,
    acts: &mut Vec<Act>,
) {
    let st = row.subtype;
    let t = pm.target;
    ui.label(label);
    let opts = field_options(field, t, p);
    let cur = value_of(p, field);
    let lock = field_lock(t, p, field);
    let r = ui.add_enabled_ui(lock.is_none(), |ui| {
        combo(
            ui,
            egui::Id::new(("preset-field", st, scope, field)),
            &cur,
            &opts,
            false,
        )
    });
    if let Some(why) = lock {
        r.response.on_disabled_hover_text(why);
    }
    if let Some(v) = r.inner.0 {
        acts.push(set(st, scope, field, v));
    }
    let (h, name) = hint_profile(row, scope);
    hint(ui, name, h.map(|b| label_of(&opts, &value_of(b, field))));
    reset(ui, own_value(pm, st, scope, field), || {
        acts.push(clear(st, scope, field))
    });
    ui.end_row();
}

fn hint(ui: &mut egui::Ui, name: &str, base: Option<String>) {
    match base {
        Some(b) => ui.weak(format!("{name}: {b}")),
        None => ui.weak(""),
    };
}

fn reset(ui: &mut egui::Ui, edited: bool, clear: impl FnOnce()) {
    if edited {
        if ui
            .small_button("↺")
            .on_hover_text("Drop this override: the base's value shows through")
            .clicked()
        {
            clear();
        }
    } else {
        ui.label("");
    }
}

fn threshold_row(
    ui: &mut egui::Ui,
    pm: &PresetsModel,
    row: &Row,
    p: &Profile,
    scope: Scope,
    acts: &mut Vec<Act>,
) {
    let st = row.subtype;
    ui.label("Threshold");
    let mut v = p.alpha.threshold;
    if ui
        .add(egui::DragValue::new(&mut v).range(engine::config::ALPHA_THRESHOLD))
        .changed()
    {
        acts.push(set(st, scope, "alpha.threshold", json!(v)));
    }
    let (h, name) = hint_profile(row, scope);
    hint(ui, name, h.map(|b| b.alpha.threshold.to_string()));
    reset(ui, own_value(pm, st, scope, "alpha.threshold"), || {
        acts.push(clear(st, scope, "alpha.threshold"))
    });
    ui.end_row();
}

fn fills(
    ui: &mut egui::Ui,
    pm: &PresetsModel,
    row: &Row,
    p: &Profile,
    scope: Scope,
    acts: &mut Vec<Act>,
) {
    let st = row.subtype;
    let get = |pr: &Profile, role: &str| {
        serde_json::to_value(&pr.packing.fill)
            .ok()
            .and_then(|v| v.get(role).and_then(Value::as_u64))
    };
    grid(ui, st, scope, "f", |ui| {
        for role in FILL_ROLES {
            let field: &'static str = match role {
                "roughness" => "packing.fill.roughness",
                "metallic" => "packing.fill.metallic",
                "ao" => "packing.fill.ao",
                "specular" => "packing.fill.specular",
                _ => "packing.fill.height",
            };
            ui.label(role);
            let cur = get(p, role);
            let mut v = cur.unwrap_or(0) as u8;
            let r = ui.add(egui::DragValue::new(&mut v).range(0..=255));
            if cur.is_none() {
                r.clone()
                    .on_hover_text("Not set: the neutral default is used");
            }
            if r.changed() {
                acts.push(set(st, scope, field, json!(v)));
            }
            let (h, name) = hint_profile(row, scope);
            hint(
                ui,
                name,
                Some(
                    h.and_then(|b| get(b, role))
                        .map_or("neutral default".into(), |v| v.to_string()),
                ),
            );
            reset(ui, own_value(pm, st, scope, field), || {
                acts.push(clear(st, scope, field))
            });
            ui.end_row();
        }
    });
}

/// The text box state of an add row, kept in egui memory.
fn text_state(ui: &egui::Ui, id: egui::Id) -> String {
    ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default()
}

/// The flag that opens a new alias's branch on the next frame (AP: Add alias opens its branch).
fn open_branch_id(st: Subtype, alias: &str) -> egui::Id {
    egui::Id::new(("preset-branch-open", st, alias))
}

/// Suffixes: one branch per suffix that names the sub-type (built in, then the aliases), the ORM example under
/// rmaos, and the New alias row.
fn suffixes(ui: &mut egui::Ui, pm: &PresetsModel, row: &Row, acts: &mut Vec<Act>) {
    let st = row.subtype;
    let own = pm.suffixes(st);
    if own.is_empty() {
        ui.weak("No canonical suffix names this sub-type, so it takes no alias.");
        return;
    }
    ui.weak(
        "No suffix: the sub-type's settings below. A suffix's branch overrides them for its files.",
    );
    let canon: std::collections::BTreeMap<String, String> =
        pm.aliases_for(st).into_iter().collect();
    ui.indent(("preset-branches", st), |ui| {
        for b in &row.branches {
            let fmt = match &b.profile {
                Ok(p) => format_label(p.format, p.color.tag),
                Err(_) => "does not load".into(),
            };
            let name = match canon.get(&b.suffix) {
                Some(c) => format!("_{} → _{c}", b.suffix),
                None => format!("_{}", b.suffix),
            };
            let count = match b.overrides {
                0 => String::new(),
                n => format!(" (overrides {n})"),
            };
            let alpha = if b.by_alpha {
                " (+ rules by alpha)"
            } else {
                ""
            };
            let open = ui.data_mut(|d| d.remove_temp::<bool>(open_branch_id(st, &b.suffix)));
            egui::CollapsingHeader::new(format!("{name} — {fmt}{count}{alpha}"))
                .id_salt(("preset-branch", st, &b.suffix))
                .open(open)
                .show(ui, |ui| {
                    if b.alias && ui.small_button("Remove alias").clicked() {
                        acts.push(Act::RemoveAlias(b.suffix.clone()));
                    }
                    match &b.profile {
                        Ok(p) => tree(ui, pm, row, p, Some(&b.suffix), acts),
                        Err(e) => {
                            ui.colored_label(RED, e);
                        }
                    }
                });
        }
        if st == Subtype::Rmaos {
            egui::CollapsingHeader::new("_orm → _rmaos — ORM → RMAOS (built in, read-only)")
                .id_salt(("preset-branch", st, "orm"))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(ORM_EXAMPLE).italics())
                        .on_hover_text(
                        "Engine behaviour (Note AL), read-only: nothing is written to the preset",
                    );
                });
        }
    });
    let id = egui::Id::new(("preset-alias", st));
    let cid = egui::Id::new(("preset-alias-canon", st));
    let mut text = text_state(ui, id);
    let mut canon = ui
        .data(|d| d.get_temp::<String>(cid))
        .filter(|c| own.contains(&c.as_str()))
        .unwrap_or_else(|| own[0].to_string());
    ui.horizontal(|ui| {
        let l = ui.label("New alias _");
        ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0))
            .labelled_by(l.id);
        canon_combo(ui, st, "alias", &own, &mut canon);
        let check = (!text.is_empty()).then(|| pm.check_alias(&text, &canon));
        if ui
            .add_enabled(
                matches!(check, Some(Ok(()))),
                egui::Button::new("Add alias"),
            )
            .clicked()
        {
            acts.push(Act::AddAlias(st, text.clone(), canon.clone()));
            text.clear();
        }
        if let Some(Err(e)) = check {
            ui.colored_label(RED, e);
        }
    });
    ui.data_mut(|d| {
        d.insert_temp(id, text);
        d.insert_temp(cid, canon);
    });
    if let Some(e) = ui.data(|d| d.get_temp::<String>(add_error_id(st))) {
        ui.colored_label(RED, e);
    }
}

fn canon_combo(ui: &mut egui::Ui, st: Subtype, salt: &str, own: &[&str], canon: &mut String) {
    if own.len() < 2 {
        return;
    }
    ui.label("→");
    egui::ComboBox::from_id_salt(("preset-canon", st, salt))
        .selected_text(format!("_{canon}"))
        .show_ui(ui, |ui| {
            for s in own {
                ui.selectable_value(canon, s.to_string(), format!("_{s}"));
            }
        });
}

/// Channel remap (AM10, AP6): `channels.swizzle` of the scope, one source per output channel.
fn swizzle_row(
    ui: &mut egui::Ui,
    pm: &PresetsModel,
    row: &Row,
    p: &Profile,
    scope: Scope,
    acts: &mut Vec<Act>,
) {
    let st = row.subtype;
    let field = "channels.swizzle";
    let cur = p.channels.swizzle.clone().unwrap_or_else(|| "rgba".into());
    let mut swz: Vec<char> = cur.chars().collect();
    grid(ui, st, scope, "s", |ui| {
        ui.label("Channels");
        ui.horizontal(|ui| {
            for (i, out) in ["R", "G", "B", "A"].into_iter().enumerate() {
                ui.label(format!("{out} ="));
                egui::ComboBox::from_id_salt(("preset-swz", st, scope, i))
                    .width(40.0)
                    .selected_text(swz.get(i).map_or(String::new(), char::to_string))
                    .show_ui(ui, |ui| {
                        for c in SWIZZLE_SOURCES {
                            if ui
                                .selectable_label(swz.get(i) == Some(&c), c.to_string())
                                .clicked()
                            {
                                if let Some(s) = swz.get_mut(i) {
                                    *s = c;
                                }
                            }
                        }
                    });
            }
        });
        let next: String = swz.iter().collect();
        if next != cur {
            acts.push(set(st, scope, field, json!(next)));
        }
        let (h, name) = hint_profile(row, scope);
        hint(
            ui,
            name,
            h.map(|b| b.channels.swizzle.clone().unwrap_or_else(|| "rgba".into())),
        );
        reset(ui, own_value(pm, st, scope, field), || {
            acts.push(clear(st, scope, field))
        });
        ui.end_row();
    });
}

fn discard_id() -> egui::Id {
    egui::Id::new("presets-discard")
}

/// AM15: Select, New or Duplicate over unsaved edits asks first. Discard goes on; Cancel, Esc or a click outside
/// keeps the edits.
fn discard_prompt(ctx: &egui::Context, model: &mut AppModel, acts: &mut Vec<Act>) {
    let Some(pending) = ctx.data(|d| d.get_temp::<Act>(discard_id())) else {
        return;
    };
    let id = model.presets_model().id();
    let name = if id.is_empty() {
        "the new preset".to_string()
    } else {
        id
    };
    use super::dialogs::Role;
    // Enter does nothing here (AM15), so no button is the Default.
    let buttons = [
        ("Cancel", Role::Plain, true),
        ("Discard", Role::Destructive, true),
    ];
    let (keys, clicked) = super::dialogs::dialog(
        ctx,
        "presets-discard",
        true,
        420.0,
        "Discard changes?",
        &buttons,
        |ui| {
            ui.label(format!("Discard unsaved changes to {name}?"));
        },
    );
    if clicked == Some(1) {
        acts.push(Act::Discard(Box::new(pending)));
    }
    if clicked.is_some() || keys.cancel {
        ctx.data_mut(|d| d.remove::<Act>(discard_id()));
    }
}
