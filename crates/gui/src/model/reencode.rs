//! Note FI3: re-encoding a DDS input from the texture or bulk panel. Each edit writes the S6 `reencode` selector
//! of the files' own `[files]` entries in `sdc.toml` as one undo step, then plans again.
use super::items::TextureItem;
use super::AppModel;
use engine::overrides::REENCODE_FIELD;
use serde_json::json;
use std::collections::HashMap;

impl AppModel {
    /// `on` writes `reencode = true`; off clears it (false is the default, never stored). One undo step; refused
    /// (no step, the reason on the status line) while `sdc.toml` is read-only.
    pub fn set_reencode(&mut self, keys: &[String], on: bool) {
        if let Some(why) = self.toml_read_only_reason() {
            self.status_line = why;
            return;
        }
        let n = keys.len();
        let s = if n == 1 { "" } else { "s" };
        let label = if on {
            format!("Re-encode {n} DDS file{s}")
        } else {
            format!("Stop re-encoding {n} DDS file{s}")
        };
        if self.project.as_ref().is_none_or(|p| p.read_only()) {
            return;
        }
        let keys = keys.to_vec();
        // The settled plan's FC1 recording (the new jobs' sub-types) joins this step, as FC8's weighting follow
        // does: one Undo takes back the re-encode and what it caused.
        let merge = format!("reencode-{}", self.history.pushes);
        let pushes = self.history.pushes;
        self.edit_docs(&label, Some(merge.clone()), true, move |p, _| {
            for k in &keys {
                if on {
                    p.set_file_value(k, REENCODE_FIELD, &json!(true));
                } else {
                    p.clear_file_value(k, REENCODE_FIELD);
                }
            }
        });
        self.reencode_follow = (on && self.history.pushes != pushes).then_some(merge);
    }

    /// Whether `sdc.toml` sets `reencode = true` for `key`.
    pub fn reencode_on(&self, key: &str) -> bool {
        self.project
            .as_ref()
            .and_then(|p| p.file_value(key, REENCODE_FIELD))
            .is_some_and(|v| v == json!(true))
    }

    /// Why DDS `key` cannot be re-encoded: another input the plan keeps already writes its output, so both would be
    /// IN-DUPLICATE (Blocking). Kept inputs are the ones with a job: GEN-REPLACED and IN-DUPLICATE inputs have none.
    /// `job_key` is the case-folded output path, as the engine's duplicate check compares outputs.
    pub fn reencode_blocker(&self, key: &str) -> Option<String> {
        // One item per frame: a linear scan, not the writers map `reencode_split` builds for many.
        let me = self.item(key)?;
        self.items
            .iter()
            .find(|i| {
                i.job.is_some()
                    && !i.job_key.is_empty()
                    && i.job_key == me.job_key
                    && i.key != me.key
            })
            .map(|i| format!("{} already writes this file", i.name))
    }

    /// The bulk panel's split of `keys`: the skipped DDS files it can re-encode, and the names of the blocked ones.
    pub fn reencode_split(&self, keys: &[String]) -> (Vec<String>, Vec<String>) {
        let writers = self.writers();
        let (mut ok, mut blocked) = (vec![], vec![]);
        for i in keys.iter().filter_map(|k| self.item(k)) {
            if i.kind != super::items::ItemKind::Dds || i.job.is_some() {
                continue;
            }
            match blocker(&writers, i) {
                Some(_) => blocked.push(i.name.clone()),
                None => ok.push(i.key.clone()),
            }
        }
        (ok, blocked)
    }

    /// Output key → (item key, name) of every input with a job.
    fn writers(&self) -> HashMap<&str, (&str, &str)> {
        self.items
            .iter()
            .filter(|i| i.job.is_some() && !i.job_key.is_empty())
            .map(|i| (i.job_key.as_str(), (i.key.as_str(), i.name.as_str())))
            .collect()
    }
}

fn blocker(writers: &HashMap<&str, (&str, &str)>, me: &TextureItem) -> Option<String> {
    writers
        .get(me.job_key.as_str())
        .filter(|(k, _)| *k != me.key)
        .map(|(_, name)| format!("{name} already writes this file"))
}
