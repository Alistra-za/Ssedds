//! A set's PBR members follow its diffuse (M3b polish, owner): deciding the Diffuse-slot member as `pbr_albedo` (or
//! `skin_albedo`, final review C1) switches the other members whose sub-type is only inferred to their PBR
//! counterparts, and taking the diffuse back to a vanilla sub-type reverts exactly the members that were switched.
//! Everything is one undo step.
//!
//! The switched members are recorded in `sdc-project.toml` (`[project] auto_pbr`), because `sdc.toml`'s
//! `[files]` entries reject unknown keys. While `sdc-project.toml` is read-only (DECISIONS P8) the diffuse still
//! changes but nothing propagates: there would be no way to find the members again to revert them.
use super::items::Decision;
use super::sets::{is_pbr_diffuse, locked_items, set_views, SlotFill, SlotKind};
use super::AppModel;
use engine::config::{PackRecipe, Subtype};
use engine::presets::Provenance;
use serde_json::json;

/// The PBR counterpart of an inferred member sub-type; `None` leaves the member alone (`height` stays `height`).
fn pbr_counterpart(st: Subtype) -> Option<Subtype> {
    match st {
        Subtype::Normal | Subtype::Msn => Some(Subtype::PbrNormal),
        Subtype::Glow => Some(Subtype::PbrEmissive),
        _ => None,
    }
}

/// A sub-type that keeps a diffuse in PBR: a PBR colour type, or the Advanced skin `skin_albedo` (a PBR set too,
/// final review C1), so moving between `pbr_albedo` and `skin_albedo` reverts nothing.
fn pbr_like(st: Subtype) -> bool {
    st.is_pbr_color() || is_pbr_diffuse(st)
}

/// What a sub-type edit on the Diffuse-slot member does to the rest of its set.
enum Follow {
    Nothing,
    /// Members to switch, with their new sub-type.
    Switch(Vec<(String, Subtype)>),
    /// The set's members; those listed in `auto_pbr` that still hold their auto value go back to inferred.
    Revert(Vec<String>),
}

impl AppModel {
    /// Change (`Some`) or reset (`None`) `key`'s sub-type, with the PBR follow-on for its set, as one step.
    pub(super) fn edit_subtype(&mut self, label: &str, key: &str, new: Option<Subtype>) {
        // Owner 2026-10-27 (Note DQ): a diffuse made Advanced skin takes its set's packed `_rmaos` to `_rfaos`, and one
        // taken off it goes back, in the same step (`set_packing_recipe` keeps `_rmaos` when `_rfaos` cannot be built).
        if let Some((packed, recipe)) = self.packed_recipe_follow(key, new) {
            self.batch_edit(label, |m| {
                m.edit_subtype_alone(label, key, new);
                m.set_packing_recipe(&packed, recipe);
            });
            return;
        }
        self.edit_subtype_alone(label, key, new);
    }

    /// The set's packed output key and its new recipe when its Diffuse-slot member `key` moves onto `skin_albedo`
    /// (`_rmaos` to RFAOS) or off it to another sub-type (`_rfaos` to RMAOS). A reset (`None`) changes nothing: it
    /// falls back to the engine's suggestion, which is `skin_albedo` again while the set packs `_rfaos`.
    fn packed_recipe_follow(
        &self,
        key: &str,
        new: Option<Subtype>,
    ) -> Option<(String, PackRecipe)> {
        let (was, new) = (self.item(key)?.job.as_ref()?.profile.subtype, new?);
        let (from, recipe) = match (was == Subtype::SkinAlbedo, new == Subtype::SkinAlbedo) {
            (false, true) => ("_rmaos", PackRecipe::Rfaos),
            (true, false) => ("_rfaos", PackRecipe::Rmaos),
            _ => return None,
        };
        let views = set_views(self);
        let v = views.iter().find(|v| {
            matches!(v.slots.get(&SlotKind::Diffuse), Some(SlotFill::Own { item }) if item == key)
        })?;
        match v.slots.get(&SlotKind::Rmaos) {
            Some(SlotFill::Own { item }) if item.ends_with(from) => Some((item.clone(), recipe)),
            _ => None,
        }
    }

    fn edit_subtype_alone(&mut self, label: &str, key: &str, new: Option<Subtype>) {
        if self.project.as_ref().is_none_or(|p| p.read_only()) {
            return;
        }
        let follow = self.follow_of(key, new);
        let live: Vec<String> = self.items.iter().map(|i| i.key.clone()).collect();
        let k = key.to_string();
        self.edit_docs(label, None, true, move |p, s| {
            match new {
                Some(st) => p.set_file_value(&k, "subtype", &json!(st)),
                None => p.clear_file_value(&k, "subtype"),
            }
            if s.read_only() {
                return;
            }
            // Note DU8: a sub-type the user chose is theirs: leaving Advanced Skin no longer reverts it.
            let mut skin = s.auto_skin();
            if skin.remove(&k).is_some() {
                s.set_auto_skin(&skin);
            }
            let before = s.auto_pbr();
            // A sub-type the user just chose is no longer automatic.
            let mut auto: Vec<String> = before.iter().filter(|a| **a != k).cloned().collect();
            match follow {
                Follow::Nothing => {}
                Follow::Switch(members) => {
                    for (mk, st) in members {
                        p.set_file_value(&mk, "subtype", &json!(st));
                        if !auto.contains(&mk) {
                            auto.push(mk);
                        }
                    }
                }
                Follow::Revert(members) => {
                    for mk in members {
                        let held = p.file_value(&mk, "subtype");
                        let is_auto = held == Some(json!(Subtype::PbrNormal))
                            || held == Some(json!(Subtype::PbrEmissive));
                        if auto.contains(&mk) && is_auto {
                            p.clear_file_value(&mk, "subtype");
                            auto.retain(|a| *a != mk);
                        }
                    }
                }
            }
            // Files that no longer exist drop off the list whenever it is written.
            auto.retain(|a| live.contains(a));
            if auto != before {
                s.set_auto_pbr(&auto);
            }
        });
    }

    /// Whether the diffuse `key` ends up with a non-PBR-colour sub-type after setting `new` (`Some`) or clearing
    /// its own entry (`None`). A clear falls back to another `[files]` key that matches the file (the extension-less
    /// spelling) or the engine's suggestion, so it leaves PBR only when neither names a PBR colour type.
    fn leaves_pbr(&self, key: &str, new: Option<Subtype>) -> bool {
        if let Some(st) = new {
            return !pbr_like(st);
        }
        let (Some(p), Some(job)) = (
            self.project.as_ref(),
            self.item(key).and_then(|i| i.job.as_ref()),
        ) else {
            return false;
        };
        let suggested = job.suggestions.first().map(|s| s.subtype);
        let other = p.file_keys().into_iter().any(|k| {
            k != key
                && engine::overrides::path_key_matches(&k, key)
                && p.file_value(&k, "subtype")
                    .and_then(|v| serde_json::from_value::<Subtype>(v).ok())
                    .is_some_and(is_pbr_diffuse)
        });
        !other && suggested.is_none_or(|s| !pbr_like(s))
    }

    fn follow_of(&self, key: &str, new: Option<Subtype>) -> Follow {
        let (Some(p), Some(item)) = (self.project.as_ref(), self.item(key)) else {
            return Follow::Nothing;
        };
        let was_pbr = item
            .job
            .as_ref()
            .is_some_and(|j| is_pbr_diffuse(j.profile.subtype));
        let enters = !was_pbr && new.is_some_and(is_pbr_diffuse);
        let leaves = was_pbr && self.leaves_pbr(key, new);
        if !enters && !leaves {
            return Follow::Nothing;
        }
        let views = set_views(self);
        let Some(v) = views.iter().find(|v| v.members.iter().any(|m| m == key)) else {
            return Follow::Nothing;
        };
        // Only the set's own Diffuse-slot member leads; borrowed slots do not.
        let leads =
            matches!(v.slots.get(&SlotKind::Diffuse), Some(SlotFill::Own { item }) if item == key);
        if !leads {
            return Follow::Nothing;
        }
        // F3a Task 8: a member filling a locked slot, here or in a set borrowing it, keeps its sub-type (the bulk rule).
        let locked = locked_items(self);
        let free = |mk: &&String| !locked.contains_key(*mk);
        if leaves {
            return Follow::Revert(v.members.iter().filter(free).cloned().collect());
        }
        let switch = v
            .members
            .iter()
            .filter(free)
            .filter(|m| *m != key && p.file_value(m, "subtype").is_none())
            .filter_map(|m| {
                let it = self.item(m)?;
                let job = it.job.as_ref()?;
                // Inferred means nothing the user wrote decided it: no `[files]` entry (under either spelling of the
                // key) and no rule. `[files]` keys are paths, never globs; only the CLI's `--set` takes globs. Anything
                // the user wrote resolves with a provenance other than the engine's own suggestion.
                if job.provenance.get("subtype") != Some(&Provenance::Suggestion) {
                    return None;
                }
                let suggested = it.decisions.iter().find_map(|d| match d {
                    Decision::Subtype { suggested } => *suggested,
                    _ => None,
                });
                let st = pbr_counterpart(suggested.unwrap_or(job.profile.subtype))?;
                Some((m.clone(), st))
            })
            .collect();
        Follow::Switch(switch)
    }
}
