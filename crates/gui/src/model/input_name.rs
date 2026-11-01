//! GUI tweaks G1: a set's input name, picked from a known texture path (or typed). The set's members are read under
//! the new name through `[inputs]` aliases (F3b AA1: files on disk are never renamed), each keeping its own map
//! suffix and extension. The set's `sdc-project.toml` entry follows the new automatic set id, so its TruePBR
//! settings, locks and borrows stay with it. One undo step and one plan.
use super::items::ItemKind;
use super::rename::{
    input_output, lower, plan_rename, split_ext, split_path, RenameDialog, RenameMode, RenameRow,
};
use super::sets::{set_views, views, SlotFill, SlotKind};
use super::AppModel;
use engine::group::base_of;
use engine::sets::{fold_id, same_id};
use std::ops::Range;

/// What picking a name does: the rows that change, and the set entry's (old, new) automatic id when it moves.
#[derive(Debug, Clone, PartialEq)]
pub struct InputNamePlan {
    pub rows: Vec<RenameRow>,
    pub retarget: Option<(String, String)>,
}

/// `p` without the extension of its last component.
pub(super) fn no_ext(p: &str) -> &str {
    let (dir, name) = split_path(p);
    &p[..dir.len() + split_ext(name).0.len()]
}

/// Note DO: `list` (the scan's `textures/...` paths) in the form of input name `current`. A project with no
/// `textures` folder in its path names its sets without it (`discover::game_path`), so the paths drop it too.
pub fn in_form_of(list: Vec<String>, current: &str) -> Vec<String> {
    if !bare_name(current) {
        return list;
    }
    list.into_iter()
        .map(|p| if bare_name(&p) { p } else { p[9..].to_string() })
        .collect()
}

/// Note FC12 (DO6): the `gui.toml` key of set `set_id`'s Match selector in the project at `root`: the root with `/`
/// and lower case (Windows paths ignore case), then the set id folded as set ids are (Note BF).
pub fn match_key(root: &std::path::Path, set_id: &str) -> String {
    let root = root.to_string_lossy().replace('\\', "/").to_lowercase();
    format!("{root}|{}", fold_id(set_id))
}

/// An input name without the leading `textures/`.
pub fn bare_name(p: &str) -> bool {
    !p.get(..9)
        .is_some_and(|h| h.replace('\\', "/").eq_ignore_ascii_case("textures/"))
}

impl AppModel {
    /// Note FC12 (supersedes DO4's "Match 1" default and DO6): where set `v`'s Match selector starts: the match the user
    /// chose for it (`gui.toml`), else the set's lowest unused match (1 when all eight are used).
    pub fn match_slot_start(&self, v: &super::sets::SetView) -> u8 {
        let stored = self
            .root
            .as_ref()
            .and_then(|r| self.settings.match_slots.get(&match_key(r, &v.id)))
            .copied()
            .filter(|n| (1..=8).contains(n));
        stored.unwrap_or_else(|| {
            let derived = v.derived_match.as_deref().unwrap_or("");
            let used = crate::pgp::prefix::active_matches(&v.settings, derived);
            (1..=8u8)
                .find(|n| used.iter().all(|(u, _)| u != n))
                .unwrap_or(1)
        })
    }

    /// Note FC12 (DO6): keep the Match selector the user chose for set `set_id` in `gui.toml`.
    pub fn set_match_slot(&mut self, set_id: &str, n: u8) {
        let Some(root) = self.root.as_ref() else {
            return;
        };
        let key = match_key(root, set_id);
        if self.settings.match_slots.insert(key, n) != Some(n) {
            self.save_settings();
        }
    }

    /// Note AO: what the input name box offers (its ▾ rows and gray completion): the known diffuse paths, without
    /// those under `textures\pbr\`, which are PGPatcher's outputs, not input names (AO8).
    pub fn input_name_candidates(&self) -> Vec<String> {
        let mut list = self.known_textures(Some(0));
        list.retain(|p| {
            !p.replace('\\', "/")
                .get(..13)
                .is_some_and(|h| h.eq_ignore_ascii_case("textures/pbr/"))
        });
        list
    }

    /// Note DO: the lighting slots (0-based bits) the scan saw texture `path` in, with or without `textures/` and
    /// `.dds`; 0 when the scan does not know it.
    pub fn known_slot_bits(&self, path: &str) -> u16 {
        let p = lower(super::known_paths::strip_dds(path.trim()));
        let p = if bare_name(&p) {
            format!("textures/{p}")
        } else {
            p
        };
        self.known
            .textures
            .get(&format!("{p}.dds"))
            .copied()
            .unwrap_or(0)
    }

    /// The current input name of set `set_id`: its own diffuse's game path without extension.
    pub fn input_name_of(&self, set_id: &str) -> Option<String> {
        let views = views(self);
        let v = views.iter().find(|v| same_id(&v.id, set_id))?;
        let Some(SlotFill::Own { item }) = v.slots.get(&SlotKind::Diffuse) else {
            return None;
        };
        let j = self.item(item)?.job.as_ref()?;
        Some(no_ext(&j.game_path).to_string())
    }

    /// The plan for giving set `set_id` the input name `chosen` (a game path such as `textures/landscape/rock02`,
    /// with or without `.dds`), or why it cannot have it.
    pub fn input_name_plan(&self, set_id: &str, chosen: &str) -> Result<InputNamePlan, String> {
        let v = set_views(self)
            .into_iter()
            .find(|v| same_id(&v.id, set_id))
            .ok_or("the set is no longer in the plan")?;
        let Some(SlotFill::Own { item: diffuse }) = v.slots.get(&SlotKind::Diffuse) else {
            return Err("the set has no diffuse of its own to name it by".into());
        };
        let current = |k: &str| {
            self.read_as_of(k)
                .map_or_else(|| k.to_string(), |a| a.replace('\\', "/"))
        };
        let d_item = self.item(diffuse).ok_or("the diffuse is gone")?;
        let game = lower(no_ext(
            &d_item
                .job
                .as_ref()
                .ok_or("the diffuse has no job")?
                .game_path,
        ));
        let d_cur = current(diffuse);
        let d_cur_l = lower(no_ext(&d_cur));
        // The part of the game path above the folder holding sdc.toml (`textures/` when the project is a mod's
        // `textures` folder's parent… or more).
        let prefix = game
            .strip_suffix(&d_cur_l)
            .ok_or("the diffuse's game path does not end with its file path")?
            .to_string();
        let chosen = lower(chosen.trim());
        let chosen = super::known_paths::strip_dds(&chosen).trim_matches('/');
        if chosen.is_empty() {
            return Err("type or pick a path".into());
        }
        let Some(new_base) = chosen.strip_prefix(prefix.as_str()) else {
            return Err(format!(
                "pick a path under {prefix}: the project folder is read from there"
            ));
        };
        let mut d = RenameDialog {
            mode: RenameMode::Inputs,
            ..RenameDialog::default()
        };
        for k in &v.members {
            let Some(it) = self.item(k) else { continue };
            if it.kind == ItemKind::Packed {
                continue;
            }
            let cur = current(k);
            let (_, name) = split_path(&cur);
            let (stem, ext) = split_ext(name);
            let tail = &stem[base_of(stem).len()..];
            d.keys.push(k.clone());
            d.typed.insert(k.clone(), format!("{new_base}{tail}.{ext}"));
        }
        let plan = plan_rename(self, &d);
        if let Some(r) = plan.rows.iter().find(|r| r.skipped.is_some()) {
            return Err(format!(
                "{}: {}",
                r.key,
                r.skipped.clone().unwrap_or_default()
            ));
        }
        if let Some(r) = plan.rows.iter().find(|r| r.problem.is_some()) {
            return Err(format!(
                "{}: {}",
                r.key,
                r.problem.clone().unwrap_or_default()
            ));
        }
        let rows: Vec<RenameRow> = plan.rows.into_iter().filter(RenameRow::writes).collect();
        if rows.is_empty() {
            return Err("the set already has this name".into());
        }
        let retarget = rows
            .iter()
            .find(|r| r.key == *diffuse)
            .and_then(|r| self.auto_retarget(&v.key, diffuse, &r.new));
        Ok(InputNamePlan { rows, retarget })
    }

    /// Review M1: tell the user about each retarget `retarget_auto` will not do because the new id already has an
    /// entry (the old entry stays where it is).
    // shortcut: checked against the entries before the batch, so a chain (x→y while y→z) in one Apply is not
    // reported or ordered; upgrade if recipes that map one set onto another's name turn up.
    pub(crate) fn note_kept_entries(&mut self, retargets: &[(String, String)]) {
        let Some(d) = self.sets_doc.as_ref() else {
            return;
        };
        let e = d.entries();
        let kept: Vec<String> = retargets
            .iter()
            .filter(|(from, to)| {
                e.iter()
                    .any(|x| same_id(&x.id, from) && !x.created && x.renamed_from.is_none())
                    && e.iter().any(|x| same_id(&x.id, to))
            })
            .map(|(from, to)| {
                format!(
                    "{from}: its sdc-project.toml entry was kept under the old id, because {to} already has one"
                )
            })
            .collect();
        for k in kept {
            self.messages.notify(super::messages::Severity::Warning, k);
        }
    }

    /// The automatic set id follows the diffuse's output (when the id is that output under the export folder): the
    /// (old, new) id of set `set_key` when its own diffuse `diffuse` is read as `new` instead of its current name.
    pub(crate) fn auto_retarget(
        &self,
        set_key: &str,
        diffuse: &str,
        new: &str,
    ) -> Option<(String, String)> {
        let cur = self
            .read_as_of(diffuse)
            .map_or_else(|| diffuse.to_string(), |a| a.replace('\\', "/"));
        let old_out = lower(no_ext(&input_output(&cur)));
        let new_out = lower(no_ext(&input_output(new)));
        let old_auto = fold_id(&engine::sets::set_id(&self.sets_root(), set_key));
        let head = old_auto.strip_suffix(&old_out)?;
        let new_auto = format!("{head}{new_out}");
        (new_auto != old_auto).then_some((old_auto, new_auto))
    }

    /// Give set `set_id` the input name `chosen` (see `input_name_plan`): one undo step and one plan.
    pub fn set_input_name(&mut self, set_id: &str, chosen: &str) -> Result<(), String> {
        if self.project.as_ref().is_some_and(|p| p.read_only()) {
            return Err("sdc.toml is read-only".into());
        }
        // #33: the set entry must follow the aliases (AB1), so a read-only sdc-project.toml refuses too.
        if self.sets_doc.as_ref().is_some_and(|d| d.read_only()) {
            return Err("sdc-project.toml is read-only".into());
        }
        let plan = self.input_name_plan(set_id, chosen)?;
        let label = format!("Input name of {set_id}");
        self.note_kept_entries(plan.retarget.as_slice());
        self.batch_edit(&label, |m| {
            for r in &plan.rows {
                let alias = (r.new != r.key).then_some(r.new.as_str());
                m.edit_docs(&label, None, true, |p, _| p.set_read_as(&r.key, alias));
            }
            if let Some((from, to)) = &plan.retarget {
                m.edit_docs(&label, None, true, |_, d| d.retarget_auto(from, to));
            }
        });
        Ok(())
    }
}

/// Note AO: the paths of `paths` that start with `typed` (case-insensitive, `\` read as `/`), in list order.
fn starting_with<'a>(paths: &'a [String], typed: &str) -> impl Iterator<Item = &'a str> {
    let t = typed.replace('\\', "/");
    paths
        .iter()
        .map(String::as_str)
        .filter(move |p| p.get(..t.len()).is_some_and(|h| h.eq_ignore_ascii_case(&t)))
}

/// Note AO: the gray ghost after `typed`: the rest of the first known path that starts with it. `None` when no path
/// does, or when `typed` already is the first one.
pub fn completion(paths: &[String], typed: &str) -> Option<String> {
    let first = starting_with(paths, typed).next()?;
    let rest = &first[typed.len()..];
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Note AO: what Tab accepts of the ghost: up to and including its next separator, or all of it when only one path
/// is left.
pub fn tab_part(paths: &[String], typed: &str) -> Option<String> {
    let ghost = completion(paths, typed)?;
    if starting_with(paths, typed).nth(1).is_none() {
        return Some(ghost);
    }
    Some(match ghost.find(['/', '\\']) {
        Some(i) => ghost[..=i].to_string(),
        None => ghost,
    })
}

/// Note AO4: the live list under the box: the first `limit` paths that start with `text` (the top one is the ghost);
/// with none, the paths under the folder `text[..cut]` (`segment_start`, Note FI17) that contain every word after it.
pub fn live_rows(paths: &[String], text: &str, cut: usize, limit: usize) -> Vec<String> {
    let rows: Vec<String> = starting_with(paths, text)
        .take(limit)
        .map(str::to_string)
        .collect();
    if !rows.is_empty() {
        return rows;
    }
    let q = text[cut..].trim().replace('\\', "/").to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    starting_with(paths, &text[..cut])
        .filter(|p| {
            let l = p.to_lowercase();
            words.iter().all(|w| l.contains(w))
        })
        .take(limit)
        .map(str::to_string)
        .collect()
}

/// Note FI17: a path separator (`\` is read as `/` everywhere in the box).
pub fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Note FI17: the path segment at char index `i` of `s` (char indices, separators excluded): the run between the
/// separator before `i` and the one at or after it, so a `/` at `i` belongs to the segment before it.
pub fn segment_at(s: &str, i: usize) -> Range<usize> {
    let chars: Vec<char> = s.chars().collect();
    let i = i.min(chars.len());
    let start = chars[..i]
        .iter()
        .rposition(|c| is_sep(*c))
        .map_or(0, |p| p + 1);
    let end = chars[i..]
        .iter()
        .position(|c| is_sep(*c))
        .map_or(chars.len(), |p| i + p);
    start..end
}

/// Note FI17: a drag from `anchor` to `head` (char indices) that crosses a separator covers whole segments: from the
/// anchor's segment start to the head's segment end, or reversed when dragging left. Returns (anchor, head); unchanged
/// inside one segment.
pub fn snap_to_segments(s: &str, anchor: usize, head: usize) -> (usize, usize) {
    let (a, h) = (segment_at(s, anchor), segment_at(s, head));
    if a == h {
        (anchor, head)
    } else if head >= anchor {
        (a.start, h.end)
    } else {
        (a.end, h.start)
    }
}

/// Note FI17: the byte index after the last separator of `s` (0 with none): `s[..segment_start(s)]` is the folder
/// before the segment being edited, which the live list, the ▾ list and Tab completion filter by.
pub fn segment_start(s: &str) -> usize {
    s.rfind(['/', '\\']).map_or(0, |i| i + 1)
}

/// Note FI17: the 0-based index of the segment at char index `i` (the separators before it).
pub fn segment_index(s: &str, i: usize) -> usize {
    s.chars()
        .take(segment_at(s, i).start)
        .filter(|c| is_sep(*c))
        .count()
}

/// Note FI17: `s` with segment `k` replaced by `value`, and the new segment's char range.
pub fn replace_segment(s: &str, k: usize, value: &str) -> (String, Range<usize>) {
    let mut parts: Vec<&str> = s.split(['/', '\\']).collect();
    if k < parts.len() {
        parts[k] = value;
    }
    let start: usize = parts[..k.min(parts.len())]
        .iter()
        .map(|p| p.chars().count() + 1)
        .sum();
    (parts.join("/"), start..start + value.chars().count())
}

/// Note FI17: one row of a segment list (Ctrl+Up/Down in the input name box).
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The row's text: a folder's name, or (the last segment) the whole path, as the ▾ list shows it.
    pub label: String,
    /// What replaces the segment.
    pub value: String,
    /// A folder that also holds the rest of the path after this segment (` ✓`).
    pub keeps_tail: bool,
}

/// Note FI17: the known names for segment `seg` of `text`, given the folder before it (from the ▾ list's cached
/// `paths`). A middle segment lists the sibling folders: those that also hold the rest of the path first (`keeps_tail`),
/// then the rest, alphabetically. The last segment lists the paths of the textures in that folder, in list order.
pub fn segment_choices(paths: &[String], text: &str, seg: usize) -> Vec<Choice> {
    let norm = text.replace('\\', "/");
    let parts: Vec<&str> = norm.split('/').collect();
    if seg >= parts.len() {
        return vec![];
    }
    let folder: String = parts[..seg].iter().map(|p| format!("{p}/")).collect();
    let last = seg + 1 == parts.len();
    let fold = |s: &str| super::known_paths::strip_dds(&s.to_lowercase()).to_string();
    let tail = fold(&parts[seg + 1..].join("/"));
    let mut folders: std::collections::BTreeMap<String, Choice> = Default::default();
    let mut out = vec![];
    for p in starting_with(paths, &folder) {
        let rest = &p[folder.len()..];
        match rest.split_once(['/', '\\']) {
            None if last => out.push(Choice {
                label: p.to_string(),
                value: rest.to_string(),
                keeps_tail: false,
            }),
            Some((name, after)) if !last && !name.is_empty() => {
                let c = folders
                    .entry(name.to_lowercase())
                    .or_insert_with(|| Choice {
                        label: name.to_string(),
                        value: name.to_string(),
                        keeps_tail: false,
                    });
                c.keeps_tail |= fold(&after.replace('\\', "/")) == tail;
            }
            _ => {}
        }
    }
    if last {
        return out;
    }
    let mut out: Vec<Choice> = folders.into_values().collect();
    // Stable: the BTreeMap's alphabetical order stays within each group.
    out.sort_by_key(|c| !c.keeps_tail);
    out
}

/// Note FI17 (rule 13): `choices` with those closest to `old` first (the longest common prefix, case aside: `shield`
/// puts `shield_d` first), the rest in their order.
pub fn closest_first(mut choices: Vec<Choice>, old: &str) -> Vec<Choice> {
    let old = old.to_lowercase();
    let common = |v: &str| {
        v.to_lowercase()
            .chars()
            .zip(old.chars())
            .take_while(|(a, b)| a == b)
            .count()
    };
    choices.sort_by_key(|c| std::cmp::Reverse(common(&c.value)));
    choices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn segment_at_splits_only_on_slashes() {
        let s = "textures/armor/iron_d.v2-x";
        let i = s.find("iron").unwrap();
        let r = segment_at(s, i);
        assert_eq!(&s[r], "iron_d.v2-x");
        // A `/` at the index belongs to the segment before it.
        let r = segment_at(s, s.find("/iron").unwrap());
        assert_eq!(&s[r], "armor");
        assert_eq!(segment_at(s, 0), 0..8);
        assert_eq!(segment_at("a\\bc", 3), 2..4);
        assert_eq!(segment_at("", 0), 0..0);
    }

    #[test]
    fn snap_to_segments_covers_whole_segments() {
        let s = "textures/landscape/rock01";
        // Inside one segment: per character.
        assert_eq!(snap_to_segments(s, 11, 15), (11, 15));
        // Right across a `/`: the anchor's segment start to the head's segment end.
        assert_eq!(snap_to_segments(s, 12, 21), (9, 25));
        // Left: the anchor's segment end to the head's segment start.
        assert_eq!(snap_to_segments(s, 21, 12), (25, 9));
        assert_eq!(snap_to_segments(s, 3, 22), (0, 25));
    }

    #[test]
    fn segment_start_is_after_the_last_slash() {
        assert_eq!(segment_start("textures/landscape/rock01"), 19);
        assert_eq!(segment_start("textures\\rock01"), 9);
        assert_eq!(segment_start("rock01"), 0);
        assert_eq!(segment_start("textures/"), 9);
    }

    #[test]
    fn segment_index_and_replace_segment() {
        let s = "textures/landscape/rock01";
        assert_eq!(segment_index(s, 0), 0);
        assert_eq!(segment_index(s, 8), 0);
        assert_eq!(segment_index(s, 9), 1);
        assert_eq!(segment_index(s, 25), 2);
        assert_eq!(
            replace_segment(s, 1, "other"),
            ("textures/other/rock01".to_string(), 9..14)
        );
        assert_eq!(
            replace_segment(s, 2, "big"),
            ("textures/landscape/big".to_string(), 19..22)
        );
    }

    #[test]
    fn segment_choices_lists_siblings_and_marks_the_tail() {
        let list = paths(&[
            "armor/bronze/helmet",
            "armor/iron/shield",
            "armor/steel/shield",
        ]);
        let c = segment_choices(&list, "armor/iron/shield", 1);
        let rows: Vec<(&str, bool)> = c.iter().map(|c| (c.label.as_str(), c.keeps_tail)).collect();
        assert_eq!(
            rows,
            vec![("iron", true), ("steel", true), ("bronze", false)]
        );
        // Case and `.dds` aside.
        let c = segment_choices(&list, "Armor\\iron/SHIELD.dds", 1);
        assert!(c.iter().filter(|c| c.keeps_tail).count() == 2, "{c:?}");
        // The first segment lists the top folders.
        let c = segment_choices(&list, "armor/iron/shield", 0);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].value, "armor");
    }

    #[test]
    fn segment_choices_on_the_last_segment_lists_textures() {
        let list = paths(&[
            "armor/iron/shield",
            "armor/iron/sub/deep",
            "armor/iron/boots",
            "armor/steel/shield",
        ]);
        let c = segment_choices(&list, "armor/iron/x", 2);
        let rows: Vec<(&str, &str)> = c
            .iter()
            .map(|c| (c.label.as_str(), c.value.as_str()))
            .collect();
        // The ▾ list's rows (whole paths, list order), only the folder's own textures.
        assert_eq!(
            rows,
            vec![
                ("armor/iron/shield", "shield"),
                ("armor/iron/boots", "boots")
            ]
        );
    }

    /// Review fix: the folder match folds ASCII only (Note BF's game-path fold), so a Unicode case change whose byte
    /// length differs (`ẞ` 3 bytes, `ß` 2) is no match rather than a slice off a char boundary.
    #[test]
    fn segment_choices_survive_a_non_ascii_case_change() {
        let list = paths(&[
            "textures/ẞtraße/x/a",
            "textures/ßtraße/x/b",
            "TEXTURES/ßtraße/y/c",
        ]);
        let c = segment_choices(&list, "textures/ßtraße/x/a", 3);
        let labels: Vec<&str> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["textures/ßtraße/x/b"]);
        // `ẞtraße` and `ßtraße` are two folders: only the first one's `x` is listed.
        let c = segment_choices(&list, "textures/ẞtraße/q/a", 2);
        let labels: Vec<&str> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["x"]);
        // ASCII case still folds.
        let c = segment_choices(&list, "Textures/ßtraße/x/a", 2);
        let labels: Vec<&str> = c.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["x", "y"]);
    }

    #[test]
    fn closest_first_puts_a_suffix_change_first() {
        let list = paths(&["armor/steel/apron", "armor/steel/shield_d"]);
        let c = closest_first(segment_choices(&list, "armor/steel/shield", 2), "shield");
        assert_eq!(c[0].value, "shield_d");
        assert_eq!(c[1].value, "apron");
    }
}
