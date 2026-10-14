//! PGPatcher settings controls (M3b spec §4.3–4.5): the main groups for a set, and the advanced keys for the
//! Advanced drawer. Drawing only; edits go through `AppModel::set_json_value` after drawing.
use super::columns::AMBER;
use crate::model::sets::{set_views, slot_matches, SetView, SlotFill, SlotKind, SlotMatch};
use crate::model::AppModel;
use crate::pgp::prefix::{
    match_toggle, match_value, matches_nothing, stored_match, strip_lead, EMPTY_ROW, NO_MATCH,
};
use crate::pgp::schema::{Group, KeyKind, KeySpec, KEYS};
use crate::pgp::settings::{
    blocked_by, feature_key, feature_on, Feature, Settings, FEATURE_KEYS, GLINT_DEFAULTS,
};
use crate::pgp::writer::FIXED_NUMBERS;
use eframe::egui;
use engine::sets::same_id;
use serde_json::{json, Value};

type Edit = (String, Option<Value>);

const READ_ONLY: &str = "sdc-project.toml is read-only";
/// Fuzz and a coat normal cannot be in the same set (owner, 2026-10-10).
const FUZZ_BLOCKED_BY_COAT: &str = "off: the set has a coat normal (_cnr)";
const COAT_NORMAL_BLOCKED_BY_FUZZ: &str = "off: the set has a fuzz texture (_f)";

/// `glint` and `fuzz` (the Object rows) are switched by `glint_on` / `fuzz_on`; every other key is its own switch.
fn edit_key(key: &str) -> &str {
    match key {
        "glint" => "glint_on",
        "fuzz" => "fuzz_on",
        k => k,
    }
}

fn feature_of(key: &str) -> Option<Feature> {
    let key = edit_key(key);
    FEATURE_KEYS
        .iter()
        .find(|(_, k)| *k == key)
        .map(|(f, _)| *f)
}

/// The feature's display name: its switch key without the GUI-only `_on`.
fn feature_name(f: Feature) -> &'static str {
    feature_key(f).trim_end_matches("_on")
}

fn group_open(g: Group, own: &Settings) -> bool {
    let f = match g {
        Group::Emissive => Feature::Emissive,
        Group::Parallax => Feature::Parallax,
        Group::Subsurface => Feature::Subsurface,
        Group::Multilayer => Feature::Multilayer,
        Group::Glint => Feature::Glint,
        Group::Fuzz => Feature::Fuzz,
        _ => return true,
    };
    feature_on(own, f) && blocked_by(own, f).is_none()
}

fn group_title(g: Group) -> &'static str {
    match g {
        Group::Match => "Match",
        Group::Material => "Material",
        Group::Features => "Features",
        Group::Emissive => "Emissive settings",
        Group::Parallax => "Parallax settings",
        Group::Subsurface => "Subsurface settings",
        Group::Multilayer => "Coat settings",
        Group::Glint => "Glint settings",
        Group::Fuzz => "Fuzz settings",
        Group::Vertex => "Vertex colours and depth",
        Group::Locks => "Texture locks",
        Group::Uv => "UV and cubemap",
        Group::Slots => "Slots",
    }
}

/// The visual settings (F1 spec §4 header 1). Matching and the advanced keys have their own headers.
const MAIN: [Group; 8] = [
    Group::Material,
    Group::Features,
    Group::Emissive,
    Group::Parallax,
    Group::Subsurface,
    Group::Multilayer,
    Group::Glint,
    Group::Fuzz,
];
const ADVANCED: [Group; 5] = [
    Group::Match,
    Group::Uv,
    Group::Vertex,
    Group::Locks,
    Group::Multilayer,
];

struct Ctx {
    /// The set's own settings (there are no inherited ones: the `default` block is fixed).
    own: Settings,
    set: String,
    /// Why every control is disabled, when the sets document is read-only.
    read_only: Option<String>,
    coat_filled: bool,
    fuzz_filled: bool,
    /// The Matching header (F1 spec §3.2): the active slot matches, the + menu, and whether nothing matches.
    matches: Vec<SlotMatch>,
    addable: Vec<SlotMatch>,
    no_match: bool,
}

fn context(model: &AppModel, set: &str) -> Ctx {
    let v = set_views(model).into_iter().find(|v| same_id(&v.id, set));
    let filled =
        |v: &SetView, s: SlotKind| !matches!(v.slots.get(&s), None | Some(SlotFill::Empty));
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let own: Settings = v.as_ref().map(|v| v.settings.clone()).unwrap_or_default();
    let derived = v.as_ref().and_then(|v| v.derived_match.clone());
    let (matches, addable) = v.as_ref().map(slot_matches).unwrap_or_default();
    Ctx {
        no_match: matches_nothing(&own, derived.as_deref().unwrap_or("")),
        coat_filled: v.as_ref().is_some_and(|v| filled(v, SlotKind::Coat)),
        fuzz_filled: v.as_ref().is_some_and(|v| filled(v, SlotKind::Fuzz)),
        own,
        matches,
        addable,
        set: set.to_string(),
        read_only: (!editable).then(|| {
            model
                .sets_doc
                .as_ref()
                .and_then(|d| d.error())
                .map_or_else(|| READ_ONLY.into(), str::to_string)
        }),
    }
}

/// The TruePBR box of set `set` (F1 spec §4): the visual settings, Matching, and Advanced TruePBR settings.
pub fn panel(ui: &mut egui::Ui, model: &mut AppModel, set: &str) {
    let c = context(model, set);
    let mut edits: Vec<Edit> = vec![];
    for g in MAIN {
        if !group_open(g, &c.own) {
            continue;
        }
        let keys: Vec<&KeySpec> = KEYS
            .iter()
            .filter(|k| k.group == g && !k.advanced && k.kind != KeyKind::Slot)
            .collect();
        group(ui, &c, "pgp", g, &keys, &mut edits);
    }
    matching(ui, &c, &mut edits);
    egui::CollapsingHeader::new(egui::RichText::new("Advanced TruePBR settings").strong())
        .id_salt(("pgp-advanced", &c.set))
        .default_open(false)
        .show(ui, |ui| {
            for g in ADVANCED {
                if !group_open(g, &c.own) {
                    continue;
                }
                let keys: Vec<&KeySpec> =
                    KEYS.iter().filter(|k| k.group == g && k.advanced).collect();
                group(ui, &c, "pgp-adv", g, &keys, &mut edits);
            }
        });
    for (k, v) in edits {
        model.set_json_value(set, &k, v);
    }
}

fn group(
    ui: &mut egui::Ui,
    c: &Ctx,
    salt: &str,
    g: Group,
    keys: &[&KeySpec],
    edits: &mut Vec<Edit>,
) {
    if keys.is_empty() {
        return;
    }
    ui.label(egui::RichText::new(group_title(g)).strong());
    egui::Grid::new((salt, g as u8, &c.set))
        .num_columns(3)
        .show(ui, |ui| {
            for k in keys {
                row(ui, k, c, edits);
                ui.end_row();
            }
        });
    ui.add_space(6.0);
}

/// The Matching header (F1 spec §3.2): one row per slot match (slot, exact path, ×), a + menu for filled slots 3–8,
/// and the warning when nothing matches. The slot 1 and 2 rows are the card toggles' state.
fn matching(ui: &mut egui::Ui, c: &Ctx, edits: &mut Vec<Edit>) {
    ui.label(egui::RichText::new("Matching").strong());
    ui.label(
        egui::RichText::new("The entry applies when any of these matches.")
            .small()
            .weak(),
    );
    let r = ui.add_enabled_ui(c.read_only.is_none(), |ui| {
        egui::Grid::new(("pgp-matching", &c.set))
            .num_columns(3)
            .show(ui, |ui| {
                for m in &c.matches {
                    ui.label(&m.slot);
                    let id = egui::Id::new(("pgp-match", m.n, &c.set));
                    let mut text = ui
                        .data(|d| d.get_temp::<String>(id))
                        .unwrap_or_else(|| m.path.clone());
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
                    if r.changed() {
                        ui.data_mut(|d| d.insert_temp(id, text.clone()));
                    }
                    // Committed on Enter or focus loss; an unchanged path (`.dds` and a leading `textures\` aside)
                    // is no edit (F1 Review Focus 4).
                    if r.lost_focus() {
                        ui.data_mut(|d| d.remove::<String>(id));
                        if match_value(strip_lead(&text)) != match_value(&m.path) {
                            edits.push((format!("match{}", m.n), stored_match(m.n, &text)));
                        }
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Stop matching on this slot")
                        .clicked()
                    {
                        edits.push(match_toggle(m.n, false, &m.path));
                    }
                    ui.end_row();
                    if m.path.is_empty() {
                        ui.label("");
                        ui.colored_label(AMBER, EMPTY_ROW);
                        ui.end_row();
                    }
                }
            });
        if !c.addable.is_empty() {
            ui.menu_button("+", |ui| {
                for a in &c.addable {
                    if ui.button(format!("{} · {}", a.n, a.slot)).clicked() {
                        edits.push(match_toggle(a.n, true, &a.path));
                        ui.close();
                    }
                }
            });
        }
    });
    if let Some(why) = &c.read_only {
        r.response.on_disabled_hover_text(why);
    }
    if c.no_match {
        ui.colored_label(AMBER, NO_MATCH);
    }
    ui.add_space(6.0);
}

/// Why this key cannot be changed now, if it cannot. A key that is already on can always be turned off.
fn disabled_reason(k: &KeySpec, c: &Ctx) -> Option<String> {
    if let Some(why) = &c.read_only {
        return Some(why.clone());
    }
    let on = c.own.get(edit_key(k.key)).and_then(Value::as_bool) == Some(true);
    if on {
        return None;
    }
    if let Some(b) = feature_of(k.key).and_then(|f| blocked_by(&c.own, f)) {
        return Some(format!("off: {} is on", feature_name(b)));
    }
    match k.key {
        "fuzz" if c.coat_filled => Some(FUZZ_BLOCKED_BY_COAT.into()),
        "coat_normal" if c.fuzz_filled => Some(COAT_NORMAL_BLOCKED_BY_FUZZ.into()),
        _ => None,
    }
}

/// One row: label, control, and reset. A disabled control says why on hover.
fn row(ui: &mut egui::Ui, k: &KeySpec, c: &Ctx, edits: &mut Vec<Edit>) {
    let key = edit_key(k.key);
    let own = c.own.get(key);
    if matches!(k.kind, KeyKind::OnlyTrue | KeyKind::Object) {
        // The checkbox carries the label, so the label is the clickable part.
        ui.label("");
    } else {
        ui.label(k.label);
    }
    let reason = disabled_reason(k, c);
    let r = ui.add_enabled_ui(reason.is_none(), |ui| {
        if own.is_none() && matches!(k.kind, KeyKind::Number(_) | KeyKind::Rgb | KeyKind::Rgba) {
            // An unset value shows what applies, greyed.
            ui.style_mut().visuals.override_text_color = Some(ui.visuals().weak_text_color());
        }
        control(ui, k, own.cloned(), c, edits);
    });
    if let Some(why) = reason {
        r.response.on_disabled_hover_text(why);
    }
    if own.is_some() && c.read_only.is_none() {
        if ui
            .small_button("reset")
            .on_hover_text("Remove this setting (the default applies)")
            .clicked()
        {
            edits.push((key.to_string(), None));
        }
    } else {
        ui.label("");
    }
}

fn control(ui: &mut egui::Ui, k: &KeySpec, value: Option<Value>, c: &Ctx, edits: &mut Vec<Edit>) {
    let key = edit_key(k.key).to_string();
    match k.kind {
        KeyKind::OnlyTrue | KeyKind::Object => {
            let mut on = value.and_then(|v| v.as_bool()).unwrap_or(false);
            if ui.checkbox(&mut on, k.label).changed() {
                edits.push((key, on.then_some(json!(true))));
            }
        }
        KeyKind::Tri => {
            let cur = value.and_then(|v| v.as_bool());
            let mut out = None;
            ui.horizontal(|ui| {
                for (label, v) in [("not set", None), ("on", Some(true)), ("off", Some(false))] {
                    if ui.selectable_label(cur == v, label).clicked() && cur != v {
                        out = Some(v.map(Value::Bool));
                    }
                }
            });
            if let Some(v) = out {
                edits.push((key, v));
            }
        }
        KeyKind::Number(range) => {
            let default = GLINT_DEFAULTS
                .iter()
                .chain(FIXED_NUMBERS.iter())
                .find(|(d, _)| *d == k.key)
                .map(|(_, x)| *x);
            let unset_unknown = value.is_none() && default.is_none();
            let stored = value.as_ref().and_then(Value::as_f64);
            let mut x = stored.or(default).unwrap_or(0.0);
            let (lo, hi) = range.bounds();
            // The parser is the only place a typed number is seen before DragValue clamps it.
            let typed_outside = std::cell::Cell::new(false);
            let mut drag = egui::DragValue::new(&mut x)
                .range(lo..=hi)
                // A hand-edited value outside the range shows as it is: clamping it here would edit the project just
                // by drawing it (F1 Review Focus 3). The writer clamps it on export.
                .clamp_existing_to_range(false)
                .custom_parser(|s| {
                    let n = parse_number(s)?;
                    typed_outside.set(!(lo..=hi).contains(&n));
                    Some(n)
                })
                .speed(0.01)
                .max_decimals(4)
                .custom_formatter(|n, _| format!("{:?}", (n * 1e4).round() / 1e4));
            if unset_unknown {
                // No default to show: say so rather than a 0.0 that looks like one.
                drag = drag.custom_formatter(|_, _| "not set".to_string());
            }
            let r = ui
                .horizontal(|ui| {
                    let r = ui.add(drag);
                    let stored_outside = stored.is_some_and(|s| range.clamp(s) != s);
                    // `Any` has no note and never clamps.
                    if (typed_outside.get() || stored_outside) && !range.note().is_empty() {
                        ui.label(egui::RichText::new(range.note()).small().color(AMBER));
                    }
                    r
                })
                .inner;
            // An equal value is no edit: `SetsDoc::set_value` marks the project dirty even then.
            if r.changed() && stored != Some(x) {
                edits.push((key, Some(json!(x))));
            }
        }
        KeyKind::Text => {
            let id = egui::Id::new(("pgp-text", k.key, &c.set));
            let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| {
                value
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default()
            });
            // sdc puts the name in the matched folder and writes the full prefix (DECISIONS V3).
            let hint = if k.key == "rename" {
                "new file name"
            } else {
                ""
            };
            let r = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(hint)
                    .desired_width(160.0),
            );
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(id, text.clone()));
            }
            // Committed on Enter or focus loss; an empty field removes the key.
            if r.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(id));
                let t = text.trim();
                edits.push((key, (!t.is_empty()).then(|| json!(t))));
            }
        }
        KeyKind::Rgb | KeyKind::Rgba => {
            let mut rgb: [i64; 3] = value
                .and_then(|v| v.as_array().cloned())
                .map(|a| {
                    [0, 1, 2].map(|i| {
                        a.get(i)
                            .and_then(Value::as_f64)
                            .map_or(0, |x| x.round() as i64)
                    })
                })
                .unwrap_or([0, 0, 0]);
            let mut changed = false;
            ui.horizontal(|ui| {
                for (i, l) in ["R", "G", "B"].iter().enumerate() {
                    ui.label(*l);
                    changed |= ui
                        .add(egui::DragValue::new(&mut rgb[i]).range(0..=255))
                        .changed();
                }
            });
            if changed {
                edits.push((key, Some(json!(rgb))));
            }
        }
        KeyKind::Slot => {}
    }
}

/// egui's own number parsing (whitespace ignored, U+2212 read as a minus), so the box accepts what it always did.
fn parse_number(s: &str) -> Option<f64> {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .collect::<String>()
        .parse()
        .ok()
}
