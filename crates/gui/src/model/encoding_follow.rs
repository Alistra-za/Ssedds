//! Task 9 (owner, 2026-10-13; DECISIONS Z11): encoding follows type. When a plan changes a texture's sub-type, its
//! explicit format and tag that the new sub-type does not allow are cleared, so the new type's default applies.
use super::fields::{field_view, format_label};
use super::sets::locked_items;
use super::{history, AppModel};
use engine::config::Subtype;
use engine::policy::allowed_encodings;
use std::collections::HashMap;

/// What a cleanup did, waiting for the plan that shows the new defaults: the cleared textures as (key, "name: old →
/// new"), and the lines of the ones kept because a lock holds them.
#[derive(Debug, Default)]
pub(super) struct Pending {
    cleared: Vec<(String, String)>,
    kept: Vec<String>,
}

fn type_name(st: Subtype) -> String {
    st.id().replace('_', " ")
}

impl AppModel {
    /// Each planned texture's sub-type, by item key.
    pub(super) fn subtypes(&self) -> HashMap<String, Subtype> {
        self.items
            .iter()
            .filter_map(|i| Some((i.key.clone(), i.job.as_ref()?.profile.subtype)))
            .collect()
    }

    /// Run from `apply_plan` once the new items are in, with the sub-types of the plan before. Finishes the note of
    /// a cleanup this plan shows, then clears the encodings a sub-type change left invalid: one edit, folded into
    /// the step that caused it, and one replan. True when it re-planned. Plans of an undo, redo or reload never
    /// clean up (they restore a state as it was), and `auto_replans` bounds the chain.
    pub(super) fn follow_types(&mut self, before: &HashMap<String, Subtype>) -> bool {
        if let Some(p) = self.type_pending.take() {
            self.set_type_note(p);
        }
        if self.follow_off
            || self.auto_replans >= 2
            || self.project.as_ref().is_none_or(|p| p.read_only())
        {
            return false;
        }
        let Some(doc) = self.project.as_ref() else {
            return false;
        };
        let locked = locked_items(self);
        let mut pending = Pending::default();
        for i in &self.items {
            let (Some(j), Some(&old)) = (&i.job, before.get(&i.key)) else {
                continue;
            };
            let (st, e) = (j.profile.subtype, (j.profile.format, j.profile.color.tag));
            let explicit = ["format", "color.tag"]
                .iter()
                .any(|f| doc.file_value(&i.key, f).is_some());
            if st == old || !explicit || allowed_encodings(st, self.target).contains(&e) {
                continue;
            }
            let head = format!("{}: {} → {}", i.name, type_name(old), type_name(st));
            let lock = locked.get(&i.key).cloned().or_else(|| {
                field_view(self.target, j, "format")
                    .and_then(|v| v.lock)
                    .map(str::to_string)
            });
            match lock {
                Some(why) => pending
                    .kept
                    .push(format!("{head}; {} kept ({why})", format_label(e.0, e.1))),
                None => pending.cleared.push((i.key.clone(), head)),
            }
        }
        if pending.cleared.is_empty() {
            if !pending.kept.is_empty() {
                self.set_type_note(pending);
            }
            return false;
        }
        let before_docs = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        self.batch = true;
        for (k, _) in &pending.cleared {
            // Clearing the format clears the explicit tag too (`clear_field`).
            self.clear_field(k, "format");
        }
        self.batch = false;
        let after = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        if after == before_docs {
            return false;
        }
        match self.history.undo.last_mut() {
            // The step that left the documents as they were is the edit that caused the change: one undo for both.
            Some(last) if last.after == before_docs => last.after = after,
            // shortcut: a change no step caused (scan votes) gets its own step; an unrelated later step cannot fold.
            _ => self.history.push(history::Command {
                label: "Encoding follows type".into(),
                before: before_docs,
                after,
                merge: None,
                created_file: None,
            }),
        }
        self.type_pending = Some(pending);
        self.auto_replans += 1;
        self.replan();
        true
    }

    /// The status-line note: one texture's line, or a count with every line on hover.
    fn set_type_note(&mut self, p: Pending) {
        let mut lines: Vec<String> = p
            .cleared
            .iter()
            .map(|(k, head)| {
                let default = self
                    .item(k)
                    .and_then(|i| i.job.as_ref())
                    .map(|j| format_label(j.profile.format, j.profile.color.tag))
                    .unwrap_or_else(|| "the new type's".into());
                format!("{head}; format reset to the default ({default}).")
            })
            .collect();
        lines.extend(p.kept.iter().cloned());
        let line = match lines.as_slice() {
            [one] => one.clone(),
            _ => {
                let mut s = format!(
                    "Encoding follows type: {} reset to the new type's default format",
                    textures(p.cleared.len())
                );
                if !p.kept.is_empty() {
                    s.push_str(&format!("; {} kept (locked)", textures(p.kept.len())));
                }
                s
            }
        };
        self.encoding_note = Some((line, lines.join("\n")));
    }
}

fn textures(n: usize) -> String {
    format!("{n} texture{}", if n == 1 { "" } else { "s" })
}
