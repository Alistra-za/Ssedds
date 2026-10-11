//! Per-output packing fills and the `_rmaos`/`_rfaos` choice (packing-fills Task 2, DECISIONS Note R-fills-1).
//!
//! A fill and a recipe are fields of the packed output's own `[files."<folder>/<base>_<suffix>"]` entry. The
//! engine reads `packing.recipe` from either packed key (`_rfaos` wins), so the GUI writes it on the key of the
//! recipe it picks and removes it from the other: the two never disagree, and the key that holds the choice is
//! the one the output has after the re-plan (a recipe left on the other key would show up as an unmatched entry).
//!
//! Picking `_rfaos` also switches the set to Advanced skin (`skin_albedo`, `skin_sk`), and picking `_rmaos`
//! switches back, as one undo step. Only members whose sub-type is inferred change, and they are tracked in
//! `sdc-project.toml` (`[project] auto_skin`) like polish Task 2's `auto_pbr`.
use super::items::TextureItem;
use super::sets::{set_views, SlotFill, SlotKind};
use super::AppModel;
use crate::project::ProjectDoc;
use engine::config::{PackRecipe, RawMap, Subtype, Target};
use engine::pack::{ChannelSource, RecipeKind};
use serde_json::json;

/// The role each channel of a packed output holds, R to A, where a fill can stand in for a missing source.
/// `ChannelSource::Default` carries no role, so this table names it. Vanilla `_m` and the parallax map have
/// nothing to fill; the cpm `_m` has no fuzz or AO channel and its R (the env mask) takes the metallic fill.
pub fn channel_roles(kind: RecipeKind) -> [Option<RawMap>; 4] {
    use RawMap::*;
    match kind {
        RecipeKind::Rmaos => [Some(Roughness), Some(Metallic), Some(Ao), Some(Specular)],
        RecipeKind::Rfaos => [Some(Roughness), Some(Fuzz), Some(Ao), Some(Specular)],
        RecipeKind::CpmM => [None, Some(Glossiness), Some(Metallic), Some(Height)],
        RecipeKind::Parallax | RecipeKind::VanillaM => [None; 4],
    }
}

/// The roles both `_rmaos` and `_rfaos` have, which follow the output across a recipe switch.
const SHARED_FILLS: [RawMap; 3] = [RawMap::Roughness, RawMap::Ao, RawMap::Specular];

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

/// What switching a set to or from Advanced skin does to its members.
enum SkinFollow {
    Nothing,
    /// Members to switch, with their new sub-type.
    Switch(Vec<(String, Subtype)>),
    /// The set's members; those listed in `auto_skin` that still hold their auto value go back to inferred.
    Revert(Vec<String>),
}

fn skin_counterpart(st: Subtype) -> Option<Subtype> {
    match st {
        Subtype::Diffuse | Subtype::PbrAlbedo => Some(Subtype::SkinAlbedo),
        Subtype::BacklightTint => Some(Subtype::SkinSk),
        _ => None,
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
        self.edit_docs("Fill value", None, true, move |p, _| match value {
            Some(v) => p.set_file_value(&k, &field, &json!(v)),
            None => p.clear_file_value(&k, &field),
        });
    }

    /// Choose `_rmaos` or `_rfaos` for the set that owns the packed output `packed_key`, as one undo step.
    /// `Auto` is not a choice the card offers.
    pub fn set_packing_recipe(&mut self, packed_key: &str, choice: PackRecipe) {
        let (Some(new), Some((base, current))) = (kind_of(choice), split_packed_key(packed_key))
        else {
            return;
        };
        if self.target != Target::Cs || self.project.as_ref().is_none_or(ProjectDoc::read_only) {
            return;
        }
        let key = |kind: RecipeKind| format!("{base}_{}", kind.suffix());
        let (from, to) = (packed_key.to_string(), key(new));
        let other = key(if new == RecipeKind::Rfaos {
            RecipeKind::Rmaos
        } else {
            RecipeKind::Rfaos
        });
        let follow = self.skin_follow_of(packed_key, new);
        let live: Vec<String> = self.items.iter().map(|i| i.key.clone()).collect();
        let moves = current != new;
        self.edit_docs("Packing recipe", None, true, move |p, s| {
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
            // P8: with `sdc-project.toml` read-only only the recipe changes (the engine still re-suggests the
            // inferred members); the switched members could not be recorded, so they are not switched.
            if s.read_only() {
                return;
            }
            let before = s.auto_skin();
            let mut auto = before.clone();
            match follow {
                SkinFollow::Nothing => {}
                SkinFollow::Switch(members) => {
                    for (m, st) in members {
                        p.set_file_value(&m, "subtype", &json!(st));
                        if !auto.contains(&m) {
                            auto.push(m);
                        }
                    }
                }
                SkinFollow::Revert(members) => {
                    for m in members {
                        let held = p.file_value(&m, "subtype");
                        let is_auto = held == Some(json!(Subtype::SkinAlbedo))
                            || held == Some(json!(Subtype::SkinSk));
                        if auto.contains(&m) && is_auto {
                            p.clear_file_value(&m, "subtype");
                            auto.retain(|a| *a != m);
                        }
                    }
                }
            }
            auto.retain(|a| live.contains(a));
            if auto != before {
                s.set_auto_skin(&auto);
            }
        });
    }

    /// The members that move with the choice, decided from the plan as it is now: once the recipe is written the
    /// engine's own suggestions change, so "inferred" must be read first.
    fn skin_follow_of(&self, packed_key: &str, new: RecipeKind) -> SkinFollow {
        let views = set_views(self);
        let Some(v) = views
            .iter()
            .find(|v| v.members.iter().any(|m| m == packed_key))
        else {
            return SkinFollow::Nothing;
        };
        if new != RecipeKind::Rfaos {
            return SkinFollow::Revert(v.members.clone());
        }
        let diffuse = match v.slots.get(&SlotKind::Diffuse) {
            Some(SlotFill::Own { item }) => Some(item.as_str()),
            _ => None,
        };
        let switch = v
            .members
            .iter()
            .filter_map(|m| {
                let it = self.item(m)?;
                let job = it.job.as_ref()?;
                if job.pack.is_some() || !self.is_inferred_subtype(it) {
                    return None;
                }
                let st = skin_counterpart(job.profile.subtype)?;
                // A diffuse sub-type counts only on the set's own Diffuse-slot member.
                (st != Subtype::SkinAlbedo || diffuse == Some(m.as_str())).then(|| (m.clone(), st))
            })
            .collect();
        SkinFollow::Switch(switch)
    }

    /// Whether nothing the user wrote decided `item`'s sub-type: no `[files]` value, and the engine's own
    /// suggestion applied (the rule polish Task 2's PBR follow uses).
    pub(super) fn is_inferred_subtype(&self, item: &TextureItem) -> bool {
        use engine::presets::Provenance;
        let (Some(p), Some(job)) = (self.project.as_ref(), item.job.as_ref()) else {
            return false;
        };
        p.file_value(&item.key, "subtype").is_none()
            && job.provenance.get("subtype") == Some(&Provenance::Suggestion)
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

/// What the card shows for one channel of a packed output.
#[derive(Debug, Clone, PartialEq)]
pub enum FillRow {
    /// The channel has a source file: read-only text.
    FromFile { role: RawMap, file: String },
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
                ChannelSource::Source { path, .. } => FillRow::FromFile {
                    role,
                    file: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                },
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
