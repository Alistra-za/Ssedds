//! Note FC1 (amends U3/U5): what the settled plan decided on its own is written into `sdc.toml`, so Save keeps it and
//! `sdc convert` plans the same. An accepted High suggestion (mesh-vote and GRAS sub-types are High suggestions too)
//! writes `subtype`; a mesh-alpha verdict writes `alpha.policy` and its threshold. A written value is a plain project
//! value, as if the user had accepted it: later evidence never rewrites it.
//!
//! R5 (amends R1): a user action that re-types (a set-shader pick, PBR follow, a preset alias change) treats an
//! untouched recorded sub-type, one equal to what auto-accept picks from the plan, as not set, as before FC1. Scan
//! evidence still never rewrites one.
use super::items::{ItemKind, ItemStatus};
use super::{fields, AppModel};
use engine::config::{AlphaPolicy, Subtype};
use engine::policy::{AcceptSuggestions, Provenance};
use engine::shader::{member_subtype, Shader};
use engine::suggest::Confidence;
use serde_json::json;

/// The last recording, for its Problems row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutoNote {
    /// The row's text, `{n} types set automatically (High confidence)`; its count is the step label's.
    pub text: String,
    /// `key: value`, one per written value (a threshold rides on its policy's line), for the row's hover.
    pub lines: Vec<String>,
    /// The `history.pushes` the step left: the row offers Undo only while it is the top step.
    pub step: u64,
    /// Note F?4: nothing prompted to be saved just before it, so while it is the top step nothing does.
    pub clean_before: bool,
}

/// A written value as the row lists it: `normal`, `cutout`.
fn show(v: &serde_json::Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}

impl AppModel {
    /// What auto-accept picks for `key` from the plan: its top name-based suggestion when High (a `[files]` sub-type
    /// leaves `job.suggestions` as they would be without it), re-typed by `shader` as the engine does (S-shader-4).
    pub(crate) fn auto_pick(&self, key: &str, shader: Option<Shader>) -> Option<Subtype> {
        if self.accept != AcceptSuggestions::High {
            return None;
        }
        let job = self.item(key)?.job.as_ref()?;
        let s = job
            .suggestions
            .first()
            .filter(|s| s.confidence == Confidence::High)?;
        Some(
            shader
                .and_then(|sh| member_subtype(sh, s.subtype))
                .map_or(s.subtype, |(st, _)| st),
        )
    }

    /// R5: `key`'s `[files]` sub-type is untouched: it is what auto-accept picks from the plan, with or without the
    /// set's current `shader`, so a re-typing user action may replace it. A different value is the user's.
    pub(crate) fn untouched_subtype(&self, key: &str, shader: Option<Shader>) -> bool {
        let Some(v) = self
            .project
            .as_ref()
            .and_then(|p| p.file_value(key, "subtype"))
            .and_then(|v| serde_json::from_value::<Subtype>(v).ok())
        else {
            return false;
        };
        self.auto_pick(key, None) == Some(v)
            || (shader.is_some() && self.auto_pick(key, shader) == Some(v))
    }

    /// Each set member's current shader (chosen or automatic).
    fn shaders_by_member(&self) -> std::collections::HashMap<String, Shader> {
        let mut out = std::collections::HashMap::new();
        for v in super::sets::views(self).iter() {
            if let Some(p) = &v.shader {
                for m in &v.members {
                    out.insert(m.clone(), p.shader);
                }
            }
        }
        out
    }

    /// The presets folder was saved (an alias may re-map a suffix): remember the untouched recorded sub-types, and
    /// plan again; the settled plan re-types those whose pick changed (R5).
    pub fn presets_saved(&mut self) {
        let shaders = self.shaders_by_member();
        let mut keep = vec![];
        for i in &self.items {
            if self.untouched_subtype(&i.key, shaders.get(&i.key).copied()) {
                if let Some(v) = self
                    .project
                    .as_ref()
                    .and_then(|p| p.file_value(&i.key, "subtype"))
                {
                    keep.push((i.key.clone(), v));
                }
            }
        }
        self.history.retype_untouched = keep;
        self.replan();
    }

    /// Write the settled plan's own decisions for the fields no entry sets yet, as one undo step of its own (`Set
    /// {n} types automatically`) and one Problems row. Each `(key, field)` is written once per history
    /// (`History::auto_written`), so an Undo of the step stays undone. True when it wrote (and re-planned).
    pub(super) fn record_auto_decisions(&mut self) -> bool {
        // Never over a running export, nor over the crash restore its toast can still undo (Note FB5), nor over the
        // *Half size* step Review's Export may still take back (Note FC14): the plan after it records instead.
        let half_pending =
            self.review
                .as_ref()
                .and_then(|r| r.half_step)
                .is_some_and(|(depth, pushes)| {
                    self.history.undo.len() == depth && self.history.pushes == pushes
                });
        if self.export.running.is_some() || self.crash_restore_on_top() || half_pending {
            return false;
        }
        let Some(doc) = self.project.as_ref().filter(|p| !p.read_only()) else {
            return false;
        };
        // R5: after a presets save, an untouched sub-type follows the new pick (`presets_saved`). Taken only once the
        // recording can write, so a blocked recording keeps it for the next one.
        let retype = std::mem::take(&mut self.history.retype_untouched);
        let mut writes: Vec<(String, &'static str, serde_json::Value)> = vec![];
        let mut lines = vec![];
        let mut only_types = true;
        if !retype.is_empty() {
            let shaders = self.shaders_by_member();
            for (k, old) in retype {
                let new = self
                    .auto_pick(&k, shaders.get(&k).copied())
                    .map(|st| json!(st));
                if let Some(new) =
                    new.filter(|n| *n != old && doc.file_value(&k, "subtype") == Some(old.clone()))
                {
                    lines.push(format!("{k}: {} → {}", show(&old), show(&new)));
                    writes.push((k, "subtype", new));
                }
            }
        }
        for i in &self.items {
            let own = matches!(i.kind, ItemKind::Image | ItemKind::Dds)
                && !matches!(i.status, ItemStatus::PackedInto(_) | ItemStatus::Skipped(_));
            let Some(j) = i.job.as_ref().filter(|_| own) else {
                continue;
            };
            let st = fields::auto_subtype_reason(j).map(|_| json!(j.profile.subtype));
            // shortcut: a mesh `remove` stays GUI-only. Written as an explicit policy it would stop cs-compact's
            // opaque-colour rules (owner O1) and change the format; record it once the engine treats both alike.
            let mesh = |f: &str| j.provenance.get(f) == Some(&Provenance::MeshAlpha);
            let mesh_policy = mesh("alpha.policy") && j.profile.alpha.policy != AlphaPolicy::Remove;
            let alpha = mesh_policy.then(|| json!(j.profile.alpha.policy));
            let threshold =
                (mesh_policy && mesh("alpha.threshold")).then(|| json!(j.profile.alpha.threshold));
            for (f, v) in [
                ("subtype", st),
                ("alpha.policy", alpha),
                ("alpha.threshold", threshold),
            ] {
                let Some(v) = v else { continue };
                let done = self.history.auto_written.contains(&(i.key.clone(), f));
                if done || doc.file_value(&i.key, f).is_some() {
                    continue;
                }
                if f != "alpha.threshold" {
                    only_types &= f == "subtype";
                    lines.push(format!("{}: {}", i.key, show(&v)));
                }
                writes.push((i.key.clone(), f, v));
            }
        }
        // Note F?3: after a re-encode step still on top, the recording joins it (`set_reencode`).
        let join = self
            .reencode_follow
            .take()
            .filter(|k| self.history.undo.last().and_then(|c| c.merge.as_ref()) == Some(k));
        if writes.is_empty() {
            return false;
        }
        let n = lines.len();
        let noun = if only_types { "type" } else { "value" };
        let s = if n == 1 { "" } else { "s" };
        let label = format!("Set {n} {noun}{s} automatically");
        for (k, f, _) in &writes {
            self.history.auto_written.insert((k.clone(), f));
        }
        if let Some(k) = join {
            // One step with the re-encode (its label stays), so no row of its own offers a separate Undo.
            let pushes = self.history.pushes;
            self.edit_docs(&label, Some(k), true, |p, _| {
                for (k, f, v) in &writes {
                    p.set_file_value(k, f, v);
                }
            });
            return self.history.pushes != pushes;
        }
        let clean_before = !self.prompts_unsaved();
        let wrote = self.batch_edit(&label, |m| {
            m.edit("Set automatically", |p| {
                for (k, f, v) in &writes {
                    p.set_file_value(k, f, v);
                }
            })
        });
        if wrote {
            self.auto_note = Some(AutoNote {
                text: format!("{n} {noun}{s} set automatically (High confidence)"),
                lines,
                step: self.history.pushes,
                clean_before,
            });
            self.invalidate_views();
        }
        wrote
    }
}
