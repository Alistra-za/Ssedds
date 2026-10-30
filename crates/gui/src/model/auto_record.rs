//! Note FC1 (amends U3/U5): what the settled plan decided on its own is written into `sdc.toml`, so Save keeps it and
//! `sdc convert` plans the same. An accepted High suggestion (mesh-vote and GRAS sub-types are High suggestions too)
//! writes `subtype`; a mesh-alpha verdict writes `alpha.policy` and its threshold. A written value is a plain project
//! value, as if the user had accepted it: later evidence never rewrites it.
use super::items::{ItemKind, ItemStatus};
use super::{fields, AppModel};
use engine::config::AlphaPolicy;
use engine::policy::Provenance;
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
}

/// A written value as the row lists it: `normal`, `cutout`.
fn show(v: &serde_json::Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}

impl AppModel {
    /// Write the settled plan's own decisions for the fields no entry sets yet, as one undo step of its own (`Set
    /// {n} types automatically`) and one Problems row. Each `(key, field)` is written once per history
    /// (`History::auto_written`), so an Undo of the step stays undone. True when it wrote (and re-planned).
    pub(super) fn record_auto_decisions(&mut self) -> bool {
        // Never over a running export, nor over the crash restore its toast can still undo (Note FB5).
        if self.export.running.is_some() || self.crash_restore_on_top() {
            return false;
        }
        let Some(doc) = self.project.as_ref().filter(|p| !p.read_only()) else {
            return false;
        };
        let mut writes: Vec<(String, &'static str, serde_json::Value)> = vec![];
        let mut lines = vec![];
        let mut only_types = true;
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
            });
            self.invalidate_views();
        }
        wrote
    }
}
