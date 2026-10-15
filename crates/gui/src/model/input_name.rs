//! GUI tweaks G1: a set's input name, picked from a known texture path (or typed). The set's members are read under
//! the new name through `[inputs]` aliases (F3b AA1: files on disk are never renamed), each keeping its own map
//! suffix and extension. The set's `sdc-project.toml` entry follows the new automatic set id, so its TruePBR
//! settings, locks and borrows stay with it. One undo step and one plan.
use super::items::ItemKind;
use super::rename::{
    base_of, input_output, plan_rename, split_ext, split_path, RenameDialog, RenameMode, RenameRow,
};
use super::sets::{set_views, SlotFill, SlotKind};
use super::AppModel;
use engine::sets::{fold_id, same_id};

/// What picking a name does: the rows that change, and the set entry's (old, new) automatic id when it moves.
#[derive(Debug, Clone, PartialEq)]
pub struct InputNamePlan {
    pub rows: Vec<RenameRow>,
    pub retarget: Option<(String, String)>,
}

fn lower(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

/// `p` without the extension of its last component.
fn no_ext(p: &str) -> &str {
    let (dir, name) = split_path(p);
    &p[..dir.len() + split_ext(name).0.len()]
}

impl AppModel {
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

    /// The current input name of set `set_id`: its own diffuse's game path without extension.
    pub fn input_name_of(&self, set_id: &str) -> Option<String> {
        let v = set_views(self)
            .into_iter()
            .find(|v| same_id(&v.id, set_id))?;
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
        // The automatic set id follows the diffuse's output (when the id is that output under the export folder).
        let retarget = rows.iter().find(|r| r.key == *diffuse).and_then(|r| {
            let old_out = lower(no_ext(&input_output(&d_cur)));
            let new_out = lower(no_ext(&input_output(&r.new)));
            let old_auto = fold_id(&engine::sets::set_id(&self.export_root(), &v.key));
            let head = old_auto.strip_suffix(&old_out)?;
            let new_auto = format!("{head}{new_out}");
            (new_auto != old_auto).then_some((old_auto, new_auto))
        });
        Ok(InputNamePlan { rows, retarget })
    }

    /// Give set `set_id` the input name `chosen` (see `input_name_plan`): one undo step and one plan.
    pub fn set_input_name(&mut self, set_id: &str, chosen: &str) -> Result<(), String> {
        if self.project.as_ref().is_some_and(|p| p.read_only()) {
            return Err("sdc.toml is read-only".into());
        }
        let plan = self.input_name_plan(set_id, chosen)?;
        let label = format!("Input name of {set_id}");
        self.batch_edit(&label, |m| {
            for r in &plan.rows {
                let alias = (lower(&r.new) != lower(&r.key)).then_some(r.new.as_str());
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

/// Note AO: the path lock's stops: the byte index of each `/` or `\` in `s`.
pub fn separator_stops(s: &str) -> Vec<usize> {
    s.match_indices(['/', '\\']).map(|(i, _)| i).collect()
}

/// Note AO: where lock `n` cuts `s`: just past its `n`-th separator (0 keeps nothing; past the last keeps the folder).
pub fn lock_cut(s: &str, n: usize) -> usize {
    n.checked_sub(1)
        .and_then(|i| {
            separator_stops(s)
                .get(i)
                .or(separator_stops(s).last())
                .copied()
        })
        .map_or(0, |i| i + 1)
}
