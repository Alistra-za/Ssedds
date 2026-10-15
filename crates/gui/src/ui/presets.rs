//! The preset creator window (Note AM, owner design §2). Drawing only: every edit is collected and applied to the
//! `PresetsModel` after the window is drawn.
use super::columns::RED;
use super::settings_panel::{combo, options_of, subtype_label, Options};
use crate::model::fields::{
    encoding_options, field_lock, filter_option_lock, format_label, is_cutout,
};
use crate::model::presets::{
    group_of, is_data, suffixes_of, PresetsModel, Row, FILL_ROLES, GROUPS, ORM_EXAMPLE,
    SWIZZLE_SOURCES,
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
    Set(Subtype, &'static str, Value),
    Clear(Subtype, &'static str),
    AddAlias(Subtype, String, String),
    RemoveAlias(String),
    AddRemap(Subtype, String, String, String),
    Save,
    Revert,
    Delete,
    ResetAll,
}

pub fn window(ctx: &egui::Context, model: &mut AppModel) {
    if !model.presets_open {
        return;
    }
    let target = model.target;
    let rect = model.settings.layout.presets_window.map_or(
        egui::Rect::from_min_size(egui::pos2(80.0, 80.0), egui::vec2(860.0, 680.0)),
        |[x, y, w, h]| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)),
    );
    let pm = model.presets_model();
    let mut acts = vec![];
    let mut open = true;
    egui::Window::new("Presets")
        .id(window_id())
        .open(&mut open)
        .resizable(true)
        .default_rect(rect)
        .constrain(true)
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
            Act::Set(st, f, v) => {
                pm.set_field(st, f, v);
                None
            }
            Act::Clear(st, f) => {
                pm.clear_field(st, f);
                None
            }
            Act::AddAlias(st, a, c) => pm.add_alias(&a, &c).err().map(|e| (st, e)),
            Act::RemoveAlias(a) => {
                pm.remove_alias(&a);
                None
            }
            Act::AddRemap(st, a, c, s) => pm.add_remap(&a, &c, &s).err().map(|e| (st, e)),
            Act::Save => {
                match pm.save() {
                    Ok(_) => saved = true,
                    Err(e) => pm.error = Some(e),
                }
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
                Some(Target::Cpm) => "cpm".to_string(),
                Some(Target::Vanilla) => "vanilla".to_string(),
                None => "from the base".to_string(),
            };
            egui::ComboBox::from_id_salt("presets-target")
                .selected_text(name(own))
                .show_ui(ui, |ui| {
                    for t in [
                        None,
                        Some(Target::Cs),
                        Some(Target::Vanilla),
                        Some(Target::Cpm),
                    ] {
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
        Target::Cpm => "cpm",
        Target::Vanilla => "vanilla",
    }
}

fn body(ui: &mut egui::Ui, pm: &PresetsModel, acts: &mut Vec<Act>) {
    let ro = pm.draft.as_ref().is_some_and(|d| d.read_only);
    for g in GROUPS {
        ui.heading(g);
        for row in pm.rows().iter().filter(|r| group_of(r.subtype) == g) {
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
    }
}

fn sub(ui: &mut egui::Ui, st: Subtype, name: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(name)
        .id_salt(("preset-sub", st, name))
        .default_open(true)
        .show(ui, body);
}

fn subtype_body(ui: &mut egui::Ui, pm: &PresetsModel, row: &Row, p: &Profile, acts: &mut Vec<Act>) {
    let st = row.subtype;
    let t = pm.target;
    sub(ui, st, "Suffixes", |ui| suffixes(ui, pm, st, acts));
    sub(ui, st, "Compression", |ui| {
        grid(ui, st, "c", |ui| {
            for (f, l) in [
                ("format", "Format"),
                ("encoder.backend", "Encoder"),
                ("encoder.effort", "Effort"),
            ] {
                field_row(ui, pm, row, p, t, f, l, acts);
            }
        });
    });
    sub(ui, st, "Resize & mips", |ui| {
        grid(ui, st, "m", |ui| {
            for (f, l) in [
                ("resize", "Resize"),
                ("mips.levels", "Mip levels"),
                ("mips.filter", "Mip filter"),
            ] {
                field_row(ui, pm, row, p, t, f, l, acts);
            }
            if is_cutout(p) {
                field_row(ui, pm, row, p, t, "mips.coverage", "Coverage", acts);
            }
        });
    });
    if uses_alpha(row) {
        sub(ui, st, "Alpha", |ui| {
            grid(ui, st, "a", |ui| {
                field_row(ui, pm, row, p, t, "alpha.policy", "Policy", acts);
                if is_cutout(p) {
                    threshold_row(ui, pm, row, p, acts);
                }
            });
        });
    }
    if is_data(st) {
        sub(ui, st, "Channel remap", |ui| remaps(ui, pm, st, acts));
        sub(ui, st, "Packing fills", |ui| fills(ui, pm, row, p, acts));
    }
}

/// AM5: the Alpha subheader shows where the base's format keeps an alpha channel.
fn uses_alpha(row: &Row) -> bool {
    row.base
        .as_ref()
        .or(row.profile.as_ref().ok())
        .is_some_and(|b| b.format.has_alpha())
}

fn grid(ui: &mut egui::Ui, st: Subtype, salt: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(("preset-grid", st, salt))
        .num_columns(4)
        .show(ui, body);
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

/// A field row: label, the value's combo, the base's value as a hint, ↺ when the preset sets it.
#[allow(clippy::too_many_arguments)]
fn field_row(
    ui: &mut egui::Ui,
    pm: &PresetsModel,
    row: &Row,
    p: &Profile,
    t: Target,
    field: &'static str,
    label: &str,
    acts: &mut Vec<Act>,
) {
    let st = row.subtype;
    ui.label(label);
    let opts = field_options(field, t, p);
    let cur = value_of(p, field);
    let lock = field_lock(t, p, field);
    let r = ui.add_enabled_ui(lock.is_none(), |ui| {
        combo(
            ui,
            egui::Id::new(("preset-field", st, field)),
            &cur,
            &opts,
            false,
        )
    });
    if let Some(why) = lock {
        r.response.on_disabled_hover_text(why);
    }
    if let Some(v) = r.inner.0 {
        acts.push(Act::Set(st, field, v));
    }
    hint(
        ui,
        row.base
            .as_ref()
            .map(|b| label_of(&opts, &value_of(b, field))),
    );
    reset(ui, pm.field(st, field).is_some(), || {
        acts.push(Act::Clear(st, field))
    });
    ui.end_row();
}

fn hint(ui: &mut egui::Ui, base: Option<String>) {
    match base {
        Some(b) => ui.weak(format!("base: {b}")),
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
    acts: &mut Vec<Act>,
) {
    let st = row.subtype;
    ui.label("Threshold");
    let mut v = p.alpha.threshold;
    if ui
        .add(egui::DragValue::new(&mut v).range(1..=254))
        .changed()
    {
        acts.push(Act::Set(st, "alpha.threshold", json!(v)));
    }
    hint(ui, row.base.as_ref().map(|b| b.alpha.threshold.to_string()));
    reset(ui, pm.field(st, "alpha.threshold").is_some(), || {
        acts.push(Act::Clear(st, "alpha.threshold"))
    });
    ui.end_row();
}

fn fills(ui: &mut egui::Ui, pm: &PresetsModel, row: &Row, p: &Profile, acts: &mut Vec<Act>) {
    let st = row.subtype;
    let get = |pr: &Profile, role: &str| {
        serde_json::to_value(&pr.packing.fill)
            .ok()
            .and_then(|v| v.get(role).and_then(Value::as_u64))
    };
    grid(ui, st, "f", |ui| {
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
                acts.push(Act::Set(st, field, json!(v)));
            }
            hint(
                ui,
                Some(
                    row.base
                        .as_ref()
                        .and_then(|b| get(b, role))
                        .map_or("neutral default".into(), |v| v.to_string()),
                ),
            );
            reset(ui, pm.field(st, field).is_some(), || {
                acts.push(Act::Clear(st, field))
            });
            ui.end_row();
        }
    });
}

/// The text box state of an add row, kept in egui memory.
fn text_state(ui: &egui::Ui, id: egui::Id) -> String {
    ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default()
}

fn suffixes(ui: &mut egui::Ui, pm: &PresetsModel, st: Subtype, acts: &mut Vec<Act>) {
    let own = suffixes_of(st, pm.target);
    if own.is_empty() {
        ui.weak("No canonical suffix names this sub-type, so it takes no alias.");
        return;
    }
    let list: Vec<String> = own.iter().map(|s| format!("_{s}")).collect();
    ui.label(format!("Built in: {}", list.join(", ")));
    for (a, c) in pm.aliases_for(st) {
        ui.horizontal(|ui| {
            ui.label(format!("_{a} → _{c}"));
            if ui.small_button("Remove").clicked() {
                acts.push(Act::RemoveAlias(a.clone()));
            }
        });
    }
    let id = egui::Id::new(("preset-alias", st));
    let cid = egui::Id::new(("preset-alias-canon", st));
    let mut text = text_state(ui, id);
    let mut canon = ui
        .data(|d| d.get_temp::<String>(cid))
        .filter(|c| own.contains(&c.as_str()))
        .unwrap_or_else(|| own[0].to_string());
    ui.horizontal(|ui| {
        ui.label("New alias _");
        ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
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

fn remaps(ui: &mut egui::Ui, pm: &PresetsModel, st: Subtype, acts: &mut Vec<Act>) {
    if st == Subtype::Rmaos {
        ui.label(egui::RichText::new(ORM_EXAMPLE).italics())
            .on_hover_text(
                "Engine behaviour (Note AL), read-only: nothing is written to the preset",
            );
    }
    let own = suffixes_of(st, pm.target);
    for (a, c, s) in pm.remaps() {
        if !own.contains(&c.as_str()) {
            continue;
        }
        ui.horizontal(|ui| {
            ui.label(format!("_{a} → _{c}, channels {s}"));
            if ui.small_button("Remove").clicked() {
                acts.push(Act::RemoveAlias(a.clone()));
            }
        });
    }
    if own.is_empty() {
        return;
    }
    let id = egui::Id::new(("preset-remap", st));
    let sid = egui::Id::new(("preset-remap-swz", st));
    let cid = egui::Id::new(("preset-remap-canon", st));
    let mut text = text_state(ui, id);
    let mut swz: [char; 4] = ui
        .data(|d| d.get_temp::<[char; 4]>(sid))
        .unwrap_or(['r', 'g', 'b', 'a']);
    let mut canon = ui
        .data(|d| d.get_temp::<String>(cid))
        .filter(|c| own.contains(&c.as_str()))
        .unwrap_or_else(|| own[0].to_string());
    ui.horizontal_wrapped(|ui| {
        ui.label("New remap _");
        ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0));
        canon_combo(ui, st, "remap", &own, &mut canon);
        for (i, out) in ["R", "G", "B", "A"].into_iter().enumerate() {
            ui.label(format!("{out} ="));
            egui::ComboBox::from_id_salt(("preset-swz", st, i))
                .width(40.0)
                .selected_text(swz[i].to_string())
                .show_ui(ui, |ui| {
                    for c in SWIZZLE_SOURCES {
                        ui.selectable_value(&mut swz[i], c, c.to_string());
                    }
                });
        }
        let check = (!text.is_empty()).then(|| pm.check_alias(&text, &canon));
        if ui
            .add_enabled(
                matches!(check, Some(Ok(()))),
                egui::Button::new("Add remap"),
            )
            .clicked()
        {
            acts.push(Act::AddRemap(
                st,
                text.clone(),
                canon.clone(),
                swz.iter().collect(),
            ));
            text.clear();
        }
        if let Some(Err(e)) = check {
            ui.colored_label(RED, e);
        }
    });
    ui.data_mut(|d| {
        d.insert_temp(id, text);
        d.insert_temp(sid, swz);
        d.insert_temp(cid, canon);
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
    let (mut discard, mut cancel) = (false, false);
    let keys = super::dialogs::modal_keys(ctx, "presets-discard", true, |ui| {
        ui.label(format!("Discard unsaved changes to {name}?"));
        ui.horizontal(|ui| {
            discard = ui.button("Discard").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if discard {
        acts.push(Act::Discard(Box::new(pending)));
    }
    if discard || cancel || keys.cancel {
        ctx.data_mut(|d| d.remove::<Act>(discard_id()));
    }
}
