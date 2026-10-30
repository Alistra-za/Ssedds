//! Per-output packing fills and the `_rmaos`/`_rfaos` choice (packing-fills Task 2, DECISIONS Note R-fills-1).
//!
//! A fill and a recipe are fields of the packed output's own `[files."<folder>/<base>_<suffix>"]` entry. The
//! engine reads `packing.recipe` from either packed key (`_rfaos` wins), so the GUI writes it on the key of the
//! recipe it picks and removes it from the other: the two never disagree, and the key that holds the choice is
//! the one the output has after the re-plan (a recipe left on the other key would show up as an unmatched entry).
//!
//! Picking `_rfaos` also moves the set to Advanced skin, but only through the engine: with the recipe resolved to
//! `rfaos` it suggests `skin_albedo` and `skin_sk` for the members whose sub-type is inferred, so nothing else is
//! written and a member the user set stays put.
use super::AppModel;
use crate::project::ProjectDoc;
use engine::config::{PackRecipe, RawMap, Target};
use engine::helper::{FillTarget, HelperRole, FILLS_FIELD};
use engine::pack::{recipes_for_with, ChannelSource, RawInput, RecipeKind};
use serde_json::json;

/// The role each channel of a packed output holds, R to A, where a fill can stand in for a missing source.
/// `ChannelSource::Default` carries no role, so this table names it. The env-mask `_m` and the parallax map have
/// nothing to fill; the complex `_m` has no fuzz or AO channel and its R (the env mask) takes the metallic fill.
pub fn channel_roles(kind: RecipeKind) -> [Option<RawMap>; 4] {
    use RawMap::*;
    match kind {
        RecipeKind::Rmaos => [Some(Roughness), Some(Metallic), Some(Ao), Some(Specular)],
        RecipeKind::Rfaos => [Some(Roughness), Some(Fuzz), Some(Ao), Some(Specular)],
        RecipeKind::CpmM => [None, Some(Glossiness), Some(Metallic), Some(Height)],
        RecipeKind::Parallax | RecipeKind::VanillaM => [None; 4],
    }
}

/// The engine's reason (`PACK-RECIPE-UNBUILDABLE`) shown when RFAOS cannot be built.
pub const RFAOS_UNBUILDABLE: &str = "RFAOS needs a roughness, glossiness or AO source";

/// The roles both `_rmaos` and `_rfaos` have, which follow the output across a recipe switch.
const SHARED_FILLS: [RawMap; 3] = [RawMap::Roughness, RawMap::Ao, RawMap::Specular];

fn role_name(role: RawMap) -> String {
    role.id()
}

fn fill_field(role: RawMap) -> String {
    format!("packing.fill.{}", role.id())
}

/// `(<folder>/<base>, the recipe the key names)` of a cs packed key.
fn split_packed_key(key: &str) -> Option<(&str, RecipeKind)> {
    if let Some(base) = key.strip_suffix("_rmaos") {
        Some((base, RecipeKind::Rmaos))
    } else {
        key.strip_suffix("_rfaos").map(|b| (b, RecipeKind::Rfaos))
    }
}

fn kind_of(choice: PackRecipe) -> Option<RecipeKind> {
    match choice {
        PackRecipe::Rmaos => Some(RecipeKind::Rmaos),
        PackRecipe::Rfaos => Some(RecipeKind::Rfaos),
        PackRecipe::Auto => None,
    }
}

impl AppModel {
    /// The recipe a packed output was explicitly given: the `_rfaos` key wins, then the `_rmaos` key, then the
    /// output's own resolved `packing.recipe` (a project or preset value). `Auto` when nobody chose.
    pub fn explicit_recipe(&self, packed_key: &str) -> PackRecipe {
        let (Some(p), Some((base, _))) = (self.project.as_ref(), split_packed_key(packed_key))
        else {
            return PackRecipe::Auto;
        };
        let read = |suffix: &str| -> PackRecipe {
            p.file_value(&format!("{base}_{suffix}"), "packing.recipe")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or(PackRecipe::Auto)
        };
        let (on_rfaos, on_rmaos) = (read("rfaos"), read("rmaos"));
        if on_rfaos != PackRecipe::Auto {
            return on_rfaos;
        }
        if on_rmaos != PackRecipe::Auto {
            return on_rmaos;
        }
        self.item(packed_key)
            .and_then(|i| i.job.as_ref())
            .map_or(PackRecipe::Auto, |j| j.profile.packing.recipe)
    }

    /// Fill (`Some`) or reset (`None`) one channel of the packed output `packed_key`, as one undo step. It edits
    /// `sdc.toml` only, so it works while `sdc-project.toml` is read-only.
    pub fn set_pack_fill(&mut self, packed_key: &str, role: RawMap, value: Option<u8>) {
        if self.project.as_ref().is_none_or(ProjectDoc::read_only) {
            return;
        }
        let (k, field) = (packed_key.to_string(), fill_field(role));
        self.dropped_fills.remove(packed_key);
        self.edit_docs("Fill value", None, true, move |p, _| match value {
            Some(v) => p.set_file_value(&k, &field, &json!(v)),
            None => p.clear_file_value(&k, &field),
        });
    }

    /// Why the set of the cs packed output `packed_key` cannot build `_rfaos`, if it cannot (final review I2): the
    /// engine's rule (`recipes_for_with` with `rfaos`) on the sources the output is planned from. `_rfaos` has no
    /// metallic channel, so a metallic-only set gets `PACK-RECIPE-UNBUILDABLE` and keeps `_rmaos`. `None` when
    /// there is nothing to judge (no planned packed job).
    pub fn rfaos_unbuildable(&self, packed_key: &str) -> Option<&'static str> {
        let pk = self.item(packed_key)?.job.as_ref()?.pack.as_ref()?;
        if pk.kind == RecipeKind::Rfaos {
            return None;
        }
        let raw: Vec<RawInput> = pk
            .channels
            .iter()
            .filter_map(|c| match c {
                ChannelSource::Source { path, role, .. } => Some(RawInput {
                    path: path.clone(),
                    role: *role,
                }),
                _ => None,
            })
            .collect();
        let built = recipes_for_with(Target::Cs, &raw, PackRecipe::Rfaos, false)
            .0
            .iter()
            .any(|r| r.kind == RecipeKind::Rfaos);
        (!built).then_some(RFAOS_UNBUILDABLE)
    }

    /// Choose `_rmaos` or `_rfaos` for the set that owns the packed output `packed_key`, as one undo step.
    /// `Auto` is not a choice the card offers. RFAOS on a set that cannot build it changes nothing and returns
    /// the reason: the fills never move to a key whose output is not planned (final review I2), so the key a note
    /// goes on (`to`) is always the key of the card shown after the re-plan.
    pub fn set_packing_recipe(&mut self, packed_key: &str, choice: PackRecipe) -> Option<String> {
        let (Some(new), Some((base, current))) = (kind_of(choice), split_packed_key(packed_key))
        else {
            return None;
        };
        if self.target != Target::Cs || self.project.as_ref().is_none_or(ProjectDoc::read_only) {
            return None;
        }
        if new == RecipeKind::Rfaos {
            if let Some(why) = self.rfaos_unbuildable(packed_key) {
                return Some(why.to_string());
            }
        }
        let key = |kind: RecipeKind| format!("{base}_{}", kind.suffix());
        let (from, to) = (packed_key.to_string(), key(new));
        let note_key = to.clone();
        let other = key(if new == RecipeKind::Rfaos {
            RecipeKind::Rmaos
        } else {
            RecipeKind::Rfaos
        });
        let moves = current != new;
        // A fill with no channel in the new recipe is removed; the card says so.
        let mut notes: Vec<String> = vec![];
        if let (true, Some(p)) = (moves, self.project.as_ref()) {
            let dropped: Vec<String> = RawMap::ALL
                .into_iter()
                .filter(|r| !SHARED_FILLS.contains(r))
                .filter(|r| p.file_value(&from, &fill_field(*r)).is_some())
                .map(role_name)
                .collect();
            if !dropped.is_empty() {
                let why = if new == RecipeKind::Rfaos {
                    "RFAOS has no metallic channel"
                } else {
                    "RMAOS has no fuzz channel"
                };
                notes.push(format!("{} fill removed ({why})", dropped.join(", ")));
            }
            for r in SHARED_FILLS {
                let f = fill_field(r);
                if let (Some(a), Some(b)) = (p.file_value(&from, &f), p.file_value(&to, &f)) {
                    if a != b {
                        notes.push(format!(
                            "{} fill {a} not moved: the _{} entry already had {b}",
                            role_name(r),
                            new.suffix()
                        ));
                    }
                }
            }
        }
        self.edit_docs("Packing recipe", None, true, move |p, _| {
            if moves {
                // The fills follow the output to its new key; one with no channel there is dropped.
                for role in RawMap::ALL {
                    let field = fill_field(role);
                    let held = p.file_value(&from, &field);
                    if SHARED_FILLS.contains(&role) && p.file_value(&to, &field).is_none() {
                        if let Some(v) = &held {
                            p.set_file_value(&to, &field, v);
                        }
                    }
                    p.clear_file_value(&from, &field);
                }
            }
            p.set_file_value(&to, "packing.recipe", &json!(choice));
            p.clear_file_value(&other, "packing.recipe");
        });
        // Only the latest switch's notes show.
        self.dropped_fills.clear();
        if !notes.is_empty() {
            self.dropped_fills.insert(note_key, notes.join("; "));
            self.dropped_fills_step = self.history.pushes;
        }
        None
    }

    /// The project-wide `defaults.packing.fill.*` values still in `sdc.toml` (the retired global panel's keys).
    pub fn legacy_fills(&self) -> Vec<(RawMap, u8)> {
        let Some(p) = self.project.as_ref() else {
            return vec![];
        };
        RawMap::ALL
            .into_iter()
            .filter_map(|r| p.fill(r).map(|v| (r, v)))
            .collect()
    }

    /// Remove every project-wide fill, as one undo step.
    pub fn clear_legacy_fills(&mut self) {
        if self.project.as_ref().is_none_or(ProjectDoc::read_only) {
            return;
        }
        self.edit_docs("Clear project-wide fills", None, true, |p, _| {
            for r in RawMap::ALL {
                p.set_fill(r, None);
            }
        });
    }
}

/// Note DY18: the greyscale helpers of the packed output's set, `(item key, file name, role)`, in the engine's order
/// (by file name); normal layers are left out.
pub fn channel_helpers(model: &AppModel, packed_key: &str) -> Vec<(String, String, HelperRole)> {
    let Some(set) = model.item(packed_key).and_then(|i| i.set_key.clone()) else {
        return vec![];
    };
    model
        .items
        .iter()
        .filter_map(|i| Some((i, i.helper.as_ref()?)))
        .filter(|(_, h)| h.set_key == set && !h.role.is_normal())
        .map(|(i, h)| (i.key.clone(), i.name.clone(), h.role))
        .collect()
}

impl AppModel {
    /// Note DY18: make `helper` (an item key; `None` = a value) the source of channel `channel` of the packed output
    /// `packed_key`, as one undo step. Every helper of the set whose current list (its hand-set `helper.fills`, else
    /// the engine's reported fills) holds the channel gets that list without it; `helper` gets its list plus the
    /// channel. A helper not touched keeps no key, so its suggestion stays live.
    pub fn set_channel_helper(&mut self, packed_key: &str, channel: u8, helper: Option<&str>) {
        let Some(kind) = self
            .item(packed_key)
            .and_then(|i| i.job.as_ref()?.pack.as_ref())
            .map(|p| p.kind)
        else {
            return;
        };
        let target = FillTarget::Packed {
            suffix: kind.suffix(),
            channel,
        };
        let mut writes: Vec<(String, Vec<FillTarget>)> = vec![];
        for (key, ..) in channel_helpers(self, packed_key) {
            let Some(h) = self.item(&key).and_then(|i| i.helper.clone()) else {
                continue;
            };
            let mut list = self.current_fills(&key, &h);
            let held = list.contains(&target);
            list.retain(|t| *t != target);
            if helper == Some(key.as_str()) {
                list.push(target);
            } else if !held {
                continue;
            }
            writes.push((key, list));
        }
        self.edit("Channel source", move |p| {
            for (k, list) in writes {
                let ids: Vec<String> = list.iter().map(FillTarget::id).collect();
                p.set_file_value(&k, FILLS_FIELD, &json!(ids));
            }
        });
    }
}

/// What the card shows for one channel of a packed output.
#[derive(Debug, Clone, PartialEq)]
pub enum FillRow {
    /// The channel has a source file. `key` is the file's item key; `hand_set` when that file is a helper whose
    /// `helper.fills` was set by hand (the card's picker writes it).
    FromFile {
        role: RawMap,
        file: String,
        key: String,
        hand_set: bool,
    },
    /// No source: a 0..=255 value (the fill, or the neutral default that stands in), with a reset when the
    /// output's own entry holds the key.
    Value {
        role: RawMap,
        shown: u8,
        own_key: bool,
    },
}

/// The fill rows of the packed item `key`, in channel order (empty for vanilla `_m` and the parallax map).
pub fn fill_rows(model: &AppModel, key: &str) -> Vec<FillRow> {
    let (Some(item), Some(p)) = (model.item(key), model.project.as_ref()) else {
        return vec![];
    };
    let Some(pk) = item.job.as_ref().and_then(|j| j.pack.as_ref()) else {
        return vec![];
    };
    channel_roles(pk.kind)
        .into_iter()
        .zip(&pk.channels)
        .filter_map(|(role, ch)| {
            let role = role?;
            Some(match ch {
                ChannelSource::Source { path, .. } => {
                    let key = model
                        .root
                        .as_deref()
                        .map(|r| super::items::key_of(r, path))
                        .unwrap_or_default();
                    FillRow::FromFile {
                        role,
                        file: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        hand_set: model
                            .item(&key)
                            .and_then(|i| i.helper.as_ref())
                            .is_some_and(|h| h.fills_hand_set),
                        key,
                    }
                }
                ChannelSource::Fill { role: r, code } => FillRow::Value {
                    role,
                    // An inverted fill (glossiness for a roughness channel) is stored as 255 - fill.
                    shown: if *r == role { *code } else { 255 - *code },
                    own_key: p.file_value(key, &fill_field(role)).is_some(),
                },
                ChannelSource::Default { value } => FillRow::Value {
                    role,
                    shown: (value * 255.0).round().clamp(0.0, 255.0) as u8,
                    own_key: false,
                },
                ChannelSource::MirrorR | ChannelSource::Opaque => return None,
            })
        })
        .collect()
}
