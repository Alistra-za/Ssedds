//! Task 9 (owner, 2026-10-13; DECISIONS Z11): encoding follows type. When a texture's sub-type changes, its explicit
//! format and tag that the new sub-type does not allow are cleared, so the new type's default applies.
use super::fields::{field_view, format_label};
use super::sets::locked_items;
use super::{history, AppModel};
use engine::config::{ColorTag, Format, Subtype, Target};
use engine::policy::allowed_encodings;
use std::collections::HashMap;

/// What a cleanup did, waiting for the plan that shows the new defaults: the cleared textures as (key, "name: old →
/// new"), and the lines of the ones kept because a lock holds them.
#[derive(Debug, Default)]
pub(super) struct Pending {
    cleared: Vec<(String, String)>,
    kept: Vec<String>,
}

/// What the cleanup does with one re-typed texture.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing explicit, or the new sub-type allows the pair.
    Keep,
    /// The explicit pair is not allowed: clear it.
    Clear,
    /// Not allowed, but a lock holds it: report it (the reason).
    Kept(String),
}

/// The rule for a texture now of sub-type `st` resolving to `pair`: `explicit` when its `[files]` entry sets the
/// format or tag; `lock` is a slot lock or a Format field lock.
pub fn verdict(
    target: Target,
    st: Subtype,
    pair: (Format, ColorTag),
    explicit: bool,
    lock: Option<&str>,
) -> Verdict {
    if !explicit || allowed_encodings(st, target).contains(&pair) {
        return Verdict::Keep;
    }
    match lock {
        Some(why) => Verdict::Kept(why.into()),
        None => Verdict::Clear,
    }
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

    /// Forget the cleanup state: an undo, redo, open or reload restores a state as it was (its plans never clean
    /// up), and a note or pending cleanup of before no longer applies.
    pub(super) fn reset_type_follow(&mut self) {
        self.follow_off = true;
        self.encoding_note = None;
        self.type_pending = None;
        self.type_base = None;
        self.type_cause = false;
        self.type_cleaned = false;
    }

    /// A user edit recorded a step and re-plans: a cleanup on its settled plan folds into that step.
    pub(super) fn type_edit_recorded(&mut self) {
        self.encoding_note = None;
        self.type_pending = None;
        self.type_cause = true;
        self.type_cleaned = false;
    }

    /// Run from `finish_plan` when a plan chain has settled (the final pass: output paths agree with sub-types).
    /// Writes the note of a cleanup this plan shows, then compares the sub-types with those before the chain and
    /// clears the encodings a change left invalid: one edit and one replan. The edit is folded into the user step
    /// that started the chain, else it is its own step. True when it re-planned. A one-shot guard
    /// (`type_cleaned`) keeps the cleanup's own plan from cleaning again; it does not use `auto_replans`.
    pub(super) fn settle_types(&mut self) -> bool {
        let base = self.type_base.take();
        if let Some(p) = self.type_pending.take() {
            self.set_type_note(p);
        }
        let cause = std::mem::take(&mut self.type_cause);
        let off = std::mem::take(&mut self.follow_off);
        let cleaned = std::mem::take(&mut self.type_cleaned);
        let (Some(base), false, false) = (base, off, cleaned) else {
            return false;
        };
        let Some(doc) = self.project.as_ref().filter(|p| !p.read_only()) else {
            return false;
        };
        let locked = locked_items(self);
        let mut pending = Pending::default();
        for i in &self.items {
            let (Some(j), Some(&old)) = (&i.job, base.get(&i.key)) else {
                continue;
            };
            let st = j.profile.subtype;
            if st == old {
                continue;
            }
            let e = (j.profile.format, j.profile.color.tag);
            let explicit = ["format", "color.tag"]
                .iter()
                .any(|f| doc.file_value(&i.key, f).is_some());
            let lock = locked.get(&i.key).cloned().or_else(|| {
                field_view(self.target, j, "format")
                    .and_then(|v| v.lock)
                    .map(str::to_string)
            });
            let head = format!("{}: {} → {}", i.name, type_name(old), type_name(st));
            match verdict(self.target, st, e, explicit, lock.as_deref()) {
                Verdict::Keep => {}
                Verdict::Clear => pending.cleared.push((i.key.clone(), head)),
                Verdict::Kept(why) => pending
                    .kept
                    .push(format!("{head}; {} kept ({why})", format_label(e.0, e.1))),
            }
        }
        if pending.cleared.is_empty() {
            if !pending.kept.is_empty() {
                self.set_type_note(pending);
            }
            return false;
        }
        let before = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        self.batch = true;
        for (k, _) in &pending.cleared {
            // Clearing the format clears the explicit tag too (`clear_field`).
            self.clear_field(k, "format");
        }
        self.batch = false;
        let after = Self::snapshot_of(self.project.as_ref(), self.sets_doc.as_ref());
        if after == before {
            return false;
        }
        match self.history.undo.last_mut() {
            // The user step that started this chain: one undo restores the edit and the cleanup.
            Some(last) if cause && last.after == before => last.after = after,
            // A change no step caused (a scan vote, a reload) is its own step.
            _ => self.history.push(history::Command {
                label: "Encoding follows type".into(),
                before,
                after,
                merge: None,
                created_file: None,
            }),
        }
        self.type_pending = Some(pending);
        self.type_cleaned = true;
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
