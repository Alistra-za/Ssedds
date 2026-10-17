//! PGPatcher settings controls (M3b spec §4.3–4.5): the main groups for a set, and the advanced keys for the
//! Advanced drawer. Drawing only; edits go through `AppModel::set_json_value` after drawing.
use super::columns::AMBER;
use crate::model::auto_flags::{auto_flags_for, hint, resolved_for};
use crate::model::sets::{
    is_pbr_grass, set_views, slot_matches, SetView, SlotFill, SlotKind, SlotMatch,
};
use crate::model::AppModel;
use crate::pgp::alpha::{AlphaFlags, BLEND_MODES, DEFAULT_FLAGS, DEFAULT_THRESH, TEST_FUNCS};
use crate::pgp::prefix::{
    json_rename, match_toggle, match_value, matches_nothing, stored_set_match, strip_lead,
    subsurface_pitfalls, EMPTY_ROW, NOT_WRITTEN, NO_MATCH, PATH_CONTAINS_NOT_WRITTEN,
    RENAMED_MATCH_NOTE, RENAMED_ONE_MATCH,
};
use crate::pgp::schema::{grass_ignores, spec, Group, KeyKind, KeySpec, NumRange, KEYS};
use crate::pgp::settings::{
    blocked_by, feature_key, feature_on, number_default, rgb_default, rgb_of, sub_edits, sub_mode,
    subsurface_group_open, Feature, Settings, SubMode, FEATURE_KEYS, FOLIAGE_KEY, RGB_MAX,
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
pub(crate) fn edit_key(key: &str) -> &str {
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
    if g == Group::Subsurface {
        return subsurface_group_open(own);
    }
    let f = match g {
        Group::Emissive => Feature::Emissive,
        Group::Parallax => Feature::Parallax,
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
        Group::Alpha => "NiAlphaProperty",
        Group::Locks => "Texture locks",
        Group::Uv => "UV and cubemap",
        Group::Slots => "Slots",
    }
}

/// The visual settings (F1 spec §4 header 1). Matching and the advanced keys have their own headers.
const MAIN: [Group; 8] = [
    Group::Material,
    Group::Features,
    Group::Multilayer,
    Group::Emissive,
    Group::Parallax,
    Group::Subsurface,
    Group::Glint,
    Group::Fuzz,
];
const ADVANCED: [Group; 4] = [Group::Match, Group::Uv, Group::Vertex, Group::Locks];

pub(crate) struct Ctx {
    /// The set's own settings (there are no inherited ones: the `default` block is fixed).
    pub(crate) own: Settings,
    /// `own` with the keys the members switch on (DECISIONS AJ7); what the card shows and the writer writes.
    pub(crate) eff: Settings,
    /// The auto keys with why: a key here that `own` leaves unset shows "(auto)".
    auto: Vec<(&'static str, &'static str)>,
    set: String,
    /// Why every control is disabled, when the sets document is read-only.
    pub(crate) read_only: Option<String>,
    coat_filled: bool,
    sub_filled: bool,
    tree_anim: Option<bool>,
    fuzz_filled: bool,
    /// PBR grass design §4.3: the set is `pbr_grass`, so the keys grass ignores are not drawn.
    pub(crate) grass: bool,
    /// The Matching header (F1 spec §3.2): the active slot matches, the + menu, and whether nothing matches.
    matches: Vec<SlotMatch>,
    addable: Vec<SlotMatch>,
    no_match: bool,
    /// A renamed set's `match_diffuse` (AN1): its one match.
    renamed: Option<String>,
    /// The derived diffuse path (`match1` when the set has no key).
    derived: String,
    /// The set's locked slots: a locked slot command shows no remove button (DECISIONS AF6).
    pub(crate) locks: std::collections::BTreeSet<u8>,
}

pub(crate) fn context(model: &AppModel, set: &str) -> Ctx {
    let v = set_views(model).into_iter().find(|v| same_id(&v.id, set));
    let filled =
        |v: &SetView, s: SlotKind| !matches!(v.slots.get(&s), None | Some(SlotFill::Empty));
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let own: Settings = v.as_ref().map(|v| v.settings.clone()).unwrap_or_default();
    let derived = v.as_ref().and_then(|v| v.derived_match.clone());
    let ss = v.as_ref().map(|v| hint(model, v)).unwrap_or_default();
    let auto = v
        .as_ref()
        .map(|v| auto_flags_for(v.pbr, &v.slots, &own, &ss))
        .unwrap_or_default();
    let eff = v
        .as_ref()
        .map(|v| resolved_for(v.pbr, &v.slots, &own, &ss))
        .unwrap_or_else(|| own.clone());
    let (matches, addable) = v.as_ref().map(slot_matches).unwrap_or_default();
    Ctx {
        locks: v.as_ref().map(|v| v.locks.clone()).unwrap_or_default(),
        no_match: matches_nothing(&own, derived.as_deref().unwrap_or("")),
        renamed: json_rename(&own, derived.as_deref().unwrap_or("")).map(|(d, _)| d),
        derived: derived.clone().unwrap_or_default(),
        coat_filled: v.as_ref().is_some_and(|v| filled(v, SlotKind::Coat)),
        fuzz_filled: v.as_ref().is_some_and(|v| filled(v, SlotKind::Fuzz)),
        grass: v.as_ref().is_some_and(is_pbr_grass),
        sub_filled: v.as_ref().is_some_and(|v| filled(v, SlotKind::Subsurface)),
        tree_anim: ss.tree_anim,
        own,
        eff,
        auto,
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
    // G3: the whole file, every set and the delete sections, in its own window; both edit the same settings.
    if ui
        .small_button("Open the TruePBR JSON editor…")
        .on_hover_text(
            "Every set's entry and the delete sections of the PGPatcher JSON, in one window",
        )
        .clicked()
    {
        model.json_editor_open = true;
    }
    for g in MAIN {
        if !group_open(g, &c.eff) {
            continue;
        }
        let keys: Vec<&KeySpec> = KEYS
            .iter()
            .filter(|k| {
                k.group == g && !k.advanced && k.kind != KeyKind::Slot && !is_sub_key(k.key)
            })
            .filter(|k| !(c.grass && grass_ignores(k.key)))
            .collect();
        group(ui, &c, "pgp", g, &keys, &mut edits);
        if g == Group::Features {
            subsurface_selector(ui, &c, &mut edits);
            coat_kept_note(ui, &c);
        }
    }
    alpha_section(ui, &c, &mut edits);
    matching(ui, &c, &mut edits);
    egui::CollapsingHeader::new(egui::RichText::new("Advanced TruePBR settings").strong())
        .id_salt(("pgp-advanced", &c.set))
        .default_open(false)
        .show(ui, |ui| {
            for g in ADVANCED {
                if !group_open(g, &c.eff) {
                    continue;
                }
                let keys: Vec<&KeySpec> = KEYS
                    .iter()
                    .filter(|k| k.group == g && k.advanced)
                    .filter(|k| !(c.grass && grass_ignores(k.key)))
                    .collect();
                group(ui, &c, "pgp-adv", g, &keys, &mut edits);
            }
        });
    model.set_json_values(set, edits);
}

/// Subsurface keys the selector owns instead of the generic rows.
fn is_sub_key(k: &str) -> bool {
    k == "subsurface" || k == FOLIAGE_KEY
}

/// "Subsurface: Off / Standard / Foliage" (owner, 2026-10-14; DECISIONS AG101): one choice, one key. Auto (AG102) and
/// the pitfalls (AG104) show beside it.
fn subsurface_selector(ui: &mut egui::Ui, c: &Ctx, edits: &mut Vec<Edit>) {
    let mode = sub_mode(&c.eff);
    let set_by_hand = c.own.contains_key("subsurface") || c.own.contains_key(FOLIAGE_KEY);
    let auto = c
        .auto
        .iter()
        .find(|(k, _)| is_sub_key(k))
        .map(|(_, why)| *why)
        .filter(|_| !set_by_hand);
    let reason = c.read_only.clone().or_else(|| {
        blocked_by(&c.eff, Feature::Subsurface)
            .filter(|_| mode == SubMode::Off)
            .map(|b| format!("off: {} is on", feature_name(b)))
    });
    ui.horizontal(|ui| {
        ui.label("Subsurface");
        let r = ui.add_enabled_ui(reason.is_none(), |ui| {
            let text = match mode {
                SubMode::Off => "Off",
                SubMode::Standard => "Standard",
                SubMode::Foliage => "Foliage",
                SubMode::Conflict => "Standard + Foliage (conflict)",
            };
            egui::ComboBox::from_id_salt(("pgp-subsurface", &c.set))
                .selected_text(text)
                .show_ui(ui, |ui| {
                    for (m, l) in [
                        (SubMode::Off, "Off"),
                        (SubMode::Standard, "Standard"),
                        (SubMode::Foliage, "Foliage"),
                    ] {
                        // An auto value can be pinned: choosing it stores it (AG110).
                        if ui.selectable_label(mode == m, l).clicked()
                            && (mode != m || !set_by_hand)
                        {
                            for (k, v) in sub_edits(m, c.sub_filled) {
                                edits.push((k.to_string(), v));
                            }
                        }
                    }
                });
        });
        if let Some(why) = reason {
            r.response.on_disabled_hover_text(why);
        }
        if let Some(why) = auto {
            ui.label(egui::RichText::new("(auto)").small().weak())
                .on_hover_text(why);
        } else if set_by_hand
            && c.read_only.is_none()
            && ui
                .small_button("reset")
                .on_hover_text("Remove this setting (the default applies)")
                .clicked()
        {
            edits.push(("subsurface".into(), None));
            edits.push((FOLIAGE_KEY.into(), None));
        }
    });
    for w in subsurface_pitfalls(&c.eff, c.tree_anim) {
        ui.colored_label(AMBER, w);
    }
}

/// Coat values stay in the set while Multilayer is off but are not written (DECISIONS AJ4); say so.
fn coat_kept_note(ui: &mut egui::Ui, c: &Ctx) {
    let n = KEYS
        .iter()
        .filter(|k| k.group == Group::Multilayer && c.own.contains_key(k.key))
        .count();
    if n > 0 && !group_open(Group::Multilayer, &c.eff) {
        let s = if n == 1 { "" } else { "s" };
        ui.label(
            egui::RichText::new(format!(
                "{n} coat value{s} kept, unused while Multilayer is off"
            ))
            .small()
            .weak(),
        );
    }
}

/// Hover text for a label that needs more than its name.
fn label_hover(key: &str) -> Option<&'static str> {
    match key {
        "coat_color" => Some("Flat coat tint; multiplied by the _s coat diffuse map when Coat diffuse is on"),
        "inner_uv_scale" => Some("UV scale of the inner layer (multilayer parallax). Visible only where the diffuse alpha is below 1 and an inner-layer texture is set"),
        _ => None,
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
    // AN1: a renamed entry matches by its diffuse alone.
    if g == Group::Match && c.renamed.is_some() && c.own.contains_key("path_contains") {
        ui.colored_label(AMBER, PATH_CONTAINS_NOT_WRITTEN);
    }
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
    if let Some(d) = &c.renamed {
        ui.label(egui::RichText::new(RENAMED_MATCH_NOTE).small())
            .on_hover_text(format!("Written as match_diffuse = {d}"));
    }
    let r = ui.add_enabled_ui(c.read_only.is_none(), |ui| {
        egui::Grid::new(("pgp-matching", &c.set))
            .num_columns(3)
            .show(ui, |ui| {
                let first = c.matches.iter().find(|m| !m.path.is_empty()).map(|m| m.n);
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
                        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if !esc && match_value(strip_lead(&text)) != match_value(&m.path) {
                            edits.push((
                                format!("match{}", m.n),
                                stored_set_match(m.n, &text, &c.derived),
                            ));
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
                    } else if Some(m.n) != first && c.renamed.is_some() {
                        ui.label("");
                        ui.colored_label(AMBER, NOT_WRITTEN);
                        ui.end_row();
                    }
                }
            });
        let refused = (c.renamed.is_some() && !c.matches.is_empty()).then_some(RENAMED_ONE_MATCH);
        if !c.addable.is_empty() {
            ui.add_enabled_ui(refused.is_none(), |ui| {
                ui.menu_button("+", |ui| {
                    for a in &c.addable {
                        if ui.button(format!("{} · {}", a.n, a.slot)).clicked() {
                            edits.push(match_toggle(a.n, true, &a.path));
                            ui.close();
                        }
                    }
                });
            })
            .response
            .on_disabled_hover_text(refused.unwrap_or_default());
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
pub(crate) fn disabled_reason(k: &KeySpec, c: &Ctx) -> Option<String> {
    if let Some(why) = &c.read_only {
        return Some(why.clone());
    }
    // AN2: a renamed set takes no path_contains (one already there stays editable).
    if k.key == "path_contains" && c.renamed.is_some() && !c.own.contains_key(k.key) {
        return Some(RENAMED_ONE_MATCH.into());
    }
    let on = c.eff.get(edit_key(k.key)).and_then(Value::as_bool) == Some(true);
    if on {
        return None;
    }
    // The member reasons come first: a coat normal turns multilayer on, which would otherwise be the stated reason.
    match k.key {
        "fuzz" if c.coat_filled => return Some(FUZZ_BLOCKED_BY_COAT.into()),
        "coat_normal" if c.fuzz_filled => return Some(COAT_NORMAL_BLOCKED_BY_FUZZ.into()),
        _ => {}
    }
    feature_of(k.key)
        .and_then(|f| blocked_by(&c.eff, f))
        .map(|b| format!("off: {} is on", feature_name(b)))
}

/// One row: label, control, and reset. A disabled control says why on hover.
fn row(ui: &mut egui::Ui, k: &KeySpec, c: &Ctx, edits: &mut Vec<Edit>) {
    let key = edit_key(k.key);
    let auto = c.auto.iter().find(|(a, _)| *a == key).map(|(_, why)| *why);
    let auto_true = json!(true);
    let own = c.own.get(key).or(auto.map(|_| &auto_true));
    if matches!(k.kind, KeyKind::OnlyTrue | KeyKind::Object) {
        // The checkbox carries the label, so the label is the clickable part.
        ui.label("");
    } else {
        let r = ui.label(k.label);
        if let Some(t) = label_hover(k.key) {
            r.on_hover_text(t);
        }
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
    if let (None, Some(why)) = (c.own.get(key), auto) {
        ui.label(egui::RichText::new("(auto)").small().weak())
            .on_hover_text(why);
    } else if own.is_some() && c.read_only.is_none() {
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

pub(crate) fn control(
    ui: &mut egui::Ui,
    k: &KeySpec,
    value: Option<Value>,
    c: &Ctx,
    edits: &mut Vec<(String, Option<Value>)>,
) {
    let key = edit_key(k.key).to_string();
    match k.kind {
        KeyKind::OnlyTrue | KeyKind::Object => {
            let mut on = value.and_then(|v| v.as_bool()).unwrap_or(false);
            if ui.checkbox(&mut on, k.label).changed() {
                // Unticking an auto-on key stores an explicit off, which beats the auto value (AJ8).
                let auto = c.auto.iter().any(|(a, _)| *a == key);
                edits.push((key, (on || auto).then_some(json!(on))));
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
        KeyKind::Number(NumRange::Int(max)) => {
            // A whole number inside its range; DragValue clamps what is typed (AG27).
            // Read as a float so a hand-edited `4844.0` is 4844; a negative or fractional value is invalid, not unset.
            let stored = value.as_ref().and_then(|v| NumRange::whole(v, max));
            let invalid = value.as_ref().filter(|_| stored.is_none());
            let mut x = stored.map_or_else(|| alpha_default(k.key), i64::from);
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::DragValue::new(&mut x)
                        .range(0..=i64::from(max))
                        .speed(k.drag.speed),
                );
                if r.changed() && stored.map(i64::from) != Some(x) {
                    edits.push((key, Some(json!(x))));
                }
                if let Some(v) = invalid {
                    ui.label(
                        egui::RichText::new(format!(
                            "stored {v} is not valid; {}",
                            NumRange::Int(max).note()
                        ))
                        .small()
                        .color(AMBER),
                    );
                }
            });
        }
        KeyKind::Number(range) => {
            let default = number_default(k.key).or_else(|| {
                FIXED_NUMBERS
                    .iter()
                    .find(|(d, _)| *d == k.key)
                    .map(|(_, x)| *x)
            });
            let unset_unknown = value.is_none() && default.is_none();
            let stored = value.as_ref().and_then(Value::as_f64);
            let mut x = stored.or(default).unwrap_or(0.0);
            let (lo, hi) = range.bounds();
            // The parser is the only place a typed number is seen before DragValue clamps it.
            let typed_outside = std::cell::Cell::new(false);
            let speed = k.drag.speed_at(x);
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
                .speed(speed)
                .max_decimals(k.drag.decimals)
                .custom_formatter(|n, _| k.drag.format(n));
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
                // Esc cancels (AG10/AG11, trace-debug #57).
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    edits.push((key, (!t.is_empty()).then(|| json!(t))));
                }
            }
        }
        KeyKind::Rgb | KeyKind::Rgba => {
            // AG133: a float per channel, 0..=RGB_MAX; an old 0-255 colour reads as value/255.
            let mut rgb = value
                .as_ref()
                .and_then(rgb_of)
                .or(rgb_default(k.key))
                .unwrap_or([0.0; 3]);
            let mut changed = false;
            ui.horizontal(|ui| {
                // Swatch + hex (AG2) show the colour clamped to 1.0; only a change made through them writes, as
                // 0-1 values, so a stored 1.25 survives opening the picker.
                let shown = rgb.map(|x| super::color::unit_to_u8(x as f32));
                let id = egui::Id::new(("pgp-rgb", k.key, &c.set));
                if let Some(n) = super::color::color_field(ui, id, shown) {
                    rgb = n.map(|b| f64::from(b) / 255.0);
                    changed = true;
                }
                for (i, l) in ["R", "G", "B"].iter().enumerate() {
                    ui.label(*l);
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut rgb[i])
                                .range(0.0..=RGB_MAX)
                                .speed(0.005)
                                .fixed_decimals(3),
                        )
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

/// What an `alphaprop_*` number starts from when the key is unset or first switched on (DECISIONS AG28).
pub(crate) fn alpha_default(key: &str) -> i64 {
    if key == "alphaprop_thresh" {
        i64::from(DEFAULT_THRESH)
    } else {
        i64::from(DEFAULT_FLAGS)
    }
}

const DELETE_WINS: &str = "Delete takes priority over the other alpha keys of this entry";
/// AG131: NiAlphaProperty bit 13. A capture (frame15011) showed Skyrim SE draws a blended shape in its stored triangle
/// order with the bit on or off.
pub const NO_SORTER_LABEL: &str = "No sorter (no effect in Skyrim SE)";
const NO_SORTER_HOVER: &str = "Bit 13. Skyrim SE does not sort the triangles of an alpha-blended shape, so this \
     bit changes nothing: a RenderDoc capture drew the same hair with it off and on from byte-identical index lists, \
     in the order stored in the NIF. Kept so a stored value round-trips; Tessalith never sets it.";
const ALPHA_KEYS: [&str; 4] = [
    "alphaprop_flags",
    "alphaprop_thresh",
    "alphaprop_create",
    "alphaprop_delete",
];

/// A small reset button beside an alpha key that is set: removes the key, as the other rows' reset does.
fn alpha_reset(ui: &mut egui::Ui, c: &Ctx, key: &str, edits: &mut Vec<Edit>) {
    if c.own.contains_key(key)
        && c.read_only.is_none()
        && ui
            .small_button("reset")
            .on_hover_text("Remove this setting (the default applies)")
            .clicked()
    {
        edits.push((key.to_string(), None));
    }
}

/// A key whose stored value is not a whole number in range says so beside its control.
fn alpha_invalid_note(ui: &mut egui::Ui, c: &Ctx, key: &str, max: u32, valid: bool) {
    if let Some(v) = c.own.get(key).filter(|_| !valid) {
        ui.label(
            egui::RichText::new(format!(
                "stored {v} is not valid; {}",
                NumRange::Int(max).note()
            ))
            .small()
            .color(AMBER),
        );
    }
}

/// The NiAlphaProperty section (Task 4b, DECISIONS AG27-AG31): NifSkope-style flag controls composed into
/// `alphaprop_flags`, the test threshold, and the create and delete switches. Every change is one edit of the key.
fn alpha_section(ui: &mut egui::Ui, c: &Ctx, edits: &mut Vec<Edit>) {
    let open = ALPHA_KEYS.iter().any(|k| c.own.contains_key(*k));
    egui::CollapsingHeader::new(egui::RichText::new("NiAlphaProperty").strong())
        .id_salt(("pgp-alpha", &c.set))
        .default_open(open)
        .show(ui, |ui| {
            let flag = |k: &str| c.own.get(k).and_then(Value::as_bool) == Some(true);
            let (deleting, editable) = (flag("alphaprop_delete"), c.read_only.is_none());
            // A float read: a hand-edited 4844.0 is 4844. A key that is set but not a whole number in range is
            // "set and invalid" (its box is ticked and a note says why), never "not set".
            let flags_set = c.own.contains_key("alphaprop_flags");
            let cur = c
                .own
                .get("alphaprop_flags")
                .and_then(|v| NumRange::whole(v, 65535))
                .map(|v| v as u16);
            let thresh_set = c.own.contains_key("alphaprop_thresh");
            let thresh = c
                .own
                .get("alphaprop_thresh")
                .and_then(|v| NumRange::whole(v, 255))
                .map(|v| v as u8);
            let r = ui.add_enabled_ui(editable && !deleting, |ui| {
                ui.horizontal(|ui| {
                    let mut on = flags_set;
                    if ui
                        .checkbox(&mut on, "Override flags")
                        .on_hover_text(
                            "Sets the flags of the NiAlphaProperty: blending, testing and their modes",
                        )
                        .changed()
                    {
                        edits.push(("alphaprop_flags".into(), on.then_some(json!(DEFAULT_FLAGS))));
                    }
                    alpha_reset(ui, c, "alphaprop_flags", edits);
                    alpha_invalid_note(ui, c, "alphaprop_flags", 65535, cur.is_some());
                });
                ui.add_enabled_ui(flags_set, |ui| {
                    let f = AlphaFlags::from_raw(cur.unwrap_or(DEFAULT_FLAGS));
                    alpha_flag_controls(ui, c, f, edits);
                });
                ui.horizontal(|ui| {
                    let mut on = thresh_set;
                    if ui
                        .checkbox(&mut on, "Alpha test threshold")
                        .on_hover_text("Sets the alpha test threshold, 0-255")
                        .changed()
                    {
                        edits.push(("alphaprop_thresh".into(), on.then_some(json!(DEFAULT_THRESH))));
                    }
                    ui.add_enabled_ui(thresh_set, |ui| {
                        let mut x = thresh.unwrap_or(DEFAULT_THRESH);
                        let r = ui.add(
                            egui::DragValue::new(&mut x)
                                .range(0..=255)
                                .speed(spec("alphaprop_thresh").map_or(1.0, |k| k.drag.speed)),
                        );
                        if r.changed() && thresh != Some(x) {
                            edits.push(("alphaprop_thresh".into(), Some(json!(x))));
                        }
                    });
                    alpha_reset(ui, c, "alphaprop_thresh", edits);
                    alpha_invalid_note(ui, c, "alphaprop_thresh", 255, thresh.is_some());
                });
                ui.horizontal(|ui| {
                    let mut create = flag("alphaprop_create");
                    if ui
                        .checkbox(&mut create, "Create NiAlphaProperty if missing")
                        .on_hover_text(
                            "Gives a shape without one a new NiAlphaProperty (flags 4844, threshold 128 unless set \
                             here). Off: only an existing one is changed",
                        )
                        .changed()
                    {
                        edits.push(("alphaprop_create".into(), create.then_some(json!(true))));
                    }
                    alpha_reset(ui, c, "alphaprop_create", edits);
                });
            });
            if !deleting {
                if let Some(why) = &c.read_only {
                    r.response.on_disabled_hover_text(why);
                }
            }
            ui.add_enabled_ui(editable, |ui| {
                ui.horizontal(|ui| {
                    let mut del = deleting;
                    // The hover is on the checkbox itself: a disabled group does not reliably report hover.
                    let mut hover =
                        "Removes the NiAlphaProperty and its controller. Other shapes sharing it keep it"
                            .to_string();
                    if deleting {
                        hover = format!("{DELETE_WINS}. {hover}");
                    }
                    if ui
                        .checkbox(&mut del, "Delete NiAlphaProperty")
                        .on_hover_text(hover)
                        .changed()
                    {
                        edits.push(("alphaprop_delete".into(), del.then_some(json!(true))));
                    }
                    alpha_reset(ui, c, "alphaprop_delete", edits);
                });
            });
            ui.add_space(6.0);
        });
}

/// The decoded flag controls and the raw value. One edit of `alphaprop_flags` per change.
fn alpha_flag_controls(ui: &mut egui::Ui, c: &Ctx, f: AlphaFlags, edits: &mut Vec<Edit>) {
    let mut n = f;
    ui.checkbox(&mut n.blend, "Enable blending")
        .on_hover_text("Bit 0: blend the shape with what is behind it");
    for (label, v) in [
        ("Source blend mode", &mut n.src),
        ("Destination blend mode", &mut n.dst),
    ] {
        ui.horizontal(|ui| {
            ui.label(label);
            alpha_combo(ui, (label, &c.set), v, &BLEND_MODES);
        });
    }
    ui.checkbox(&mut n.test, "Enable testing")
        .on_hover_text("Bit 9: drop pixels that fail the alpha test");
    ui.horizontal(|ui| {
        ui.label("Test function");
        alpha_combo(ui, ("Test function", &c.set), &mut n.func, &TEST_FUNCS);
    });
    if n != f {
        edits.push(("alphaprop_flags".into(), Some(json!(n.raw()))));
    }
    // The raw value, and a box to type one (0-65535; anything else is refused, outlined red).
    ui.horizontal(|ui| {
        ui.label(format!("= {}", f.raw()));
        let id = egui::Id::new(("pgp-alpha-raw", &c.set));
        // The buffer carries the frame it was last drawn in: one left by a collapsed section is stale (as color.rs).
        let frame = ui.ctx().cumulative_frame_nr();
        let mut text = ui
            .data(|d| d.get_temp::<(String, u64)>(id))
            .filter(|(_, at)| at + 1 >= frame)
            .map_or_else(|| f.raw().to_string(), |(t, _)| t);
        let r = ui
            .add(egui::TextEdit::singleline(&mut text).desired_width(56.0))
            .on_hover_text("edit raw: a whole number from 0 to 65535");
        let ok = text.trim().parse::<u16>().ok();
        if ok.is_none() {
            ui.painter().rect_stroke(
                r.rect,
                2.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(210, 60, 60)),
                egui::StrokeKind::Outside,
            );
        }
        if r.has_focus() || r.changed() {
            ui.data_mut(|d| d.insert_temp(id, (text.clone(), frame)));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove::<(String, u64)>(id));
            let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if let Some(v) = ok.filter(|v| *v != f.raw() && !cancelled) {
                edits.push(("alphaprop_flags".into(), Some(json!(v))));
            }
        }
    });
    // AG131: last, as it does nothing in Skyrim SE; kept so a stored bit 13 round-trips, never set automatically.
    let mut ns = f;
    if ui
        .checkbox(&mut ns.no_sorter, NO_SORTER_LABEL)
        .on_hover_text(NO_SORTER_HOVER)
        .changed()
    {
        edits.push(("alphaprop_flags".into(), Some(json!(ns.raw()))));
    }
}

fn alpha_combo(ui: &mut egui::Ui, id: (&str, &String), v: &mut u8, list: &[&str]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(list.get(usize::from(*v)).copied().unwrap_or("?"))
        .show_ui(ui, |ui| {
            for (i, name) in list.iter().enumerate() {
                ui.selectable_value(v, i as u8, *name);
            }
        });
}
