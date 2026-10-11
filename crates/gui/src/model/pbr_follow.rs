//! A set's PBR members follow its diffuse (M3b polish, owner): deciding the Diffuse-slot member as `pbr_albedo`
//! switches the other members whose sub-type is only inferred to their PBR counterparts, and taking the diffuse
//! back to a vanilla sub-type reverts exactly the members that were switched. Everything is one undo step.
//!
//! The switched members are recorded in `sdc-project.toml` (`[project] auto_pbr`), because `sdc.toml`'s
//! `[files]` entries reject unknown keys. While `sdc-project.toml` is read-only (DECISIONS P8) the diffuse still
//! changes but nothing propagates: there would be no way to find the members again to revert them.
use super::items::Decision;
use super::sets::{set_views, SlotFill, SlotKind};
use super::AppModel;
use engine::config::Subtype;
use serde_json::json;

/// The PBR counterpart of an inferred member sub-type; `None` leaves the member alone (`height` stays `height`).
fn pbr_counterpart(st: Subtype) -> Option<Subtype> {
    match st {
        Subtype::Normal | Subtype::Msn => Some(Subtype::PbrNormal),
        Subtype::Glow => Some(Subtype::PbrEmissive),
        _ => None,
    }
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
        if self.project.as_ref().is_none_or(|p| p.read_only()) {
            return;
        }
        let follow = self.follow_of(key, new);
        let k = key.to_string();
        self.edit_docs(label, None, true, move |p, s| {
            match new {
                Some(st) => p.set_file_value(&k, "subtype", &json!(st)),
                None => p.clear_file_value(&k, "subtype"),
            }
            if s.read_only() {
                return;
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
            if auto != before {
                s.set_auto_pbr(&auto);
            }
        });
    }

    fn follow_of(&self, key: &str, new: Option<Subtype>) -> Follow {
        let (Some(p), Some(item)) = (self.project.as_ref(), self.item(key)) else {
            return Follow::Nothing;
        };
        let was_pbr = item
            .job
            .as_ref()
            .is_some_and(|j| j.profile.subtype == Subtype::PbrAlbedo);
        let will_be_pbr = new == Some(Subtype::PbrAlbedo);
        if was_pbr == will_be_pbr {
            return Follow::Nothing;
        }
        let views = set_views(self);
        let Some(v) = views.iter().find(|v| v.members.iter().any(|m| m == key)) else {
            return Follow::Nothing;
        };
        // Only the set's own Diffuse-slot member leads; borrowed slots do not.
        let leads = match v.slots.get(&SlotKind::Diffuse) {
            Some(SlotFill::Own { item }) => item == key,
            None => will_be_pbr,
            Some(_) => false,
        };
        if !leads {
            return Follow::Nothing;
        }
        if !will_be_pbr {
            return Follow::Revert(v.members.clone());
        }
        let switch = v
            .members
            .iter()
            .filter(|m| *m != key && p.file_value(m, "subtype").is_none())
            .filter_map(|m| {
                let it = self.item(m)?;
                let job = it.job.as_ref()?;
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
