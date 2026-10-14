//! Slot matches and the output prefix PGPatcher derives (M3b spec §5.5; M6 F1 spec §3, DECISIONS V1–V4).
//! Quirks are kept on purpose: the files must land where PGPatcher looks.
use super::settings::Settings;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixError(pub String);

pub fn slot_suffixes(n: u8) -> &'static [&'static str] {
    match n {
        1 => &["_d", "mask"],
        2 => &["_msn", "_n"],
        3 => &["_g", "_sk"],
        4 => &["_p"],
        5 => &["_e"],
        6 => &["_em", "_envmask", "_m", "_rmaos"],
        7 => &["_cnr", "_f", "_i", "_s"],
        8 => &["_b", "_bl", "_flow"],
        _ => &[],
    }
}

fn backslash(p: &str) -> String {
    p.replace('/', "\\")
}

fn strip_ci<'a>(s: &'a str, prefix: &str) -> &'a str {
    if s.len() >= prefix.len()
        && s.is_char_boundary(prefix.len())
        && s[..prefix.len()].eq_ignore_ascii_case(prefix)
    {
        &s[prefix.len()..]
    } else {
        s
    }
}

fn strip_ext(s: &str) -> &str {
    match s.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('\\') && !stem.is_empty() => stem,
        _ => s,
    }
}

/// F3b: a single file or folder name Windows can create: `check_rel_path`'s rules, no separator, no reserved device
/// name (`CON`, `NUL`, `COM1`… with any extension) and no trailing dot or space.
pub fn check_file_name(name: &str, field: &str) -> Result<(), PrefixError> {
    let err = |why: &str| Err(PrefixError(format!("{field}: {why} ({name:?})")));
    if name.contains(['\\', '/']) {
        return err("a name only, no folders");
    }
    check_rel_path(name, field)?;
    if name.ends_with(['.', ' ']) {
        return err("must not end in a dot or a space");
    }
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let numbered = |p: &str| {
        device
            .strip_prefix(p)
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
    };
    if ["CON", "PRN", "AUX", "NUL"].contains(&device.as_str()) || numbered("COM") || numbered("LPT")
    {
        return err("is a name Windows reserves");
    }
    Ok(())
}

/// A typed path without its leading `\` or `/`: every path here is relative already, so the separator means nothing
/// and is dropped rather than blocking export (owner, 2026-10-14; DECISIONS P13). A stored value is not rewritten.
pub fn strip_lead(p: &str) -> &str {
    p.trim().trim_start_matches(['\\', '/'])
}

/// The set keys that hold a path: a typed leading separator is dropped before they are stored.
pub fn is_path_key(key: &str) -> bool {
    key == "rename"
        || key
            .strip_prefix("match")
            .is_some_and(|n| n.len() == 1 && (b'1'..=b'8').contains(&n.as_bytes()[0]))
}

/// Reject what could escape the export folder or that Windows cannot name (DECISIONS P13).
pub fn check_rel_path(p: &str, field: &str) -> Result<(), PrefixError> {
    let err = |why: &str| Err(PrefixError(format!("{field}: {why} ({p:?})")));
    let b = backslash(p.trim());
    if b.is_empty() {
        return err("empty");
    }
    if b.starts_with('\\') {
        return err("must be relative, not start with a backslash");
    }
    if b.as_bytes().get(1) == Some(&b':') {
        return err("must be relative, not name a drive");
    }
    for c in b.split('\\') {
        if c == ".." {
            return err("must not contain '..'");
        }
        if c.chars()
            .any(|ch| "<>:\"|?*".contains(ch) || ch.is_control())
        {
            return err("contains a character Windows does not allow in names");
        }
    }
    Ok(())
}

/// A path value written into the PGPatcher JSON as typed: PGPatcher never converts '/', so it must use '\'
/// (DECISIONS P26).
pub fn check_json_path(p: &str, field: &str) -> Result<(), PrefixError> {
    if p.contains('/') {
        return Err(PrefixError(format!(
            "{field}: use \\ not /, PGPatcher does not convert / ({p:?})"
        )));
    }
    check_rel_path(p, field)
}

/// `rel_dir` (the diffuse's folder under the opened folder, '/'-separated) and its output stem → `match_diffuse`:
/// without a leading `textures/` and `pbr/`, with backslashes, case kept.
pub fn derive_match_diffuse(rel_dir: &str, stem: &str) -> String {
    let dir = rel_dir.replace('\\', "/");
    let dir = strip_ci(&dir, "textures/");
    let dir = if dir.eq_ignore_ascii_case("textures") {
        ""
    } else {
        dir
    };
    let dir = strip_ci(dir, "pbr/");
    let dir = if dir.eq_ignore_ascii_case("pbr") {
        ""
    } else {
        dir
    };
    let dir = dir.trim_matches('/');
    if dir.is_empty() {
        stem.to_string()
    } else {
        backslash(&format!("{dir}/{stem}"))
    }
}

/// `textures\pbr\` + `rest` (a relative path that may already start with `textures\` and/or `pbr\`).
fn under_pbr(rest: &str) -> String {
    let r = strip_ci(rest, "textures\\");
    let r = strip_ci(r, "pbr\\");
    format!("textures\\pbr\\{r}")
}

fn text<'a>(s: &'a Settings, k: &str) -> Option<&'a str> {
    s.get(k)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
}

fn strip_slot_suffix(v: &str, n: u8) -> String {
    // ASCII-only comparison on bytes: `to_lowercase` can change a string's length (the Kelvin sign).
    for suf in slot_suffixes(n) {
        let cut = v.len().checked_sub(suf.len());
        if let Some(c) = cut.filter(|&c| v.is_char_boundary(c) && v[c..].eq_ignore_ascii_case(suf))
        {
            return v[..c].to_string();
        }
    }
    v.to_string()
}

/// `matchN` as the JSON writes it (DECISIONS V2): no leading `textures\` (PGPatcher prepends one only when the value
/// does not start with exactly `textures\`), and `.dds` exactly once (F1 Review Focus 2).
pub fn match_value(v: &str) -> String {
    let v = strip_ci(v.trim(), "textures\\");
    if has_dds(v) {
        v.to_string()
    } else {
        format!("{v}.dds")
    }
}

fn has_dds(v: &str) -> bool {
    v.len() >= 4
        && v.get(v.len() - 4..)
            .is_some_and(|e| e.eq_ignore_ascii_case(".dds"))
}

/// The active slot matches of a set, stored form (no `.dds`), N ascending (F1 spec §3.1, DECISIONS V2). `match1` is
/// the derived diffuse path while the set has no `match1` key; `match1 = ""` is the diffuse toggle turned off.
/// `match_diffuse` and `match_normal` are ignored (spec §3.3).
pub fn active_matches(s: &Settings, derived: &str) -> Vec<(u8, String)> {
    (1..=8u8)
        .filter_map(|n| {
            let v = match s.get(&format!("match{n}")) {
                None if n == 1 => derived,
                Some(Value::String(v)) => v.as_str(),
                _ => return None,
            };
            let v = v.trim();
            (!v.is_empty()).then(|| (n, v.to_string()))
        })
        .collect()
}

/// What a typed `matchN` value stores: trimmed, one trailing `.dds` cut. Empty turns the match off: `match1 = ""`
/// (removing the key would bring the derived default back), any other slot loses its key.
pub fn stored_match(n: u8, typed: &str) -> Option<Value> {
    let t = strip_lead(typed);
    let t = if has_dds(t) { &t[..t.len() - 4] } else { t };
    match (n, t.is_empty()) {
        (1, true) => Some(json!("")),
        (_, true) => None,
        _ => Some(json!(t)),
    }
}

/// The edit that turns slot match `n` on or off, as (key, value for `set_json_value`) (F1 spec §3.2). Slot 1 on
/// removes the key (the derived default applies) and off stores `""`; other slots store `path` when on and lose the
/// key when off.
pub fn match_toggle(n: u8, on: bool, path: &str) -> (String, Option<Value>) {
    let v = match (n == 1, on) {
        (true, true) | (false, false) => None,
        (true, false) => Some(json!("")),
        (false, true) => Some(json!(path)),
    };
    (format!("match{n}"), v)
}

/// The path a match on the slot with canonical `suffix` starts from (DECISIONS V1): the derived diffuse path for the
/// diffuse (`""`), else that path with a trailing `_d` cut and `_<suffix>` added. Only `_d` pairs with `_n` in vanilla
/// naming: `facemask.dds` goes with `facemask_n.dds`, so `mask` stays (unlike PGPatcher's prefix rule).
pub fn slot_match_path(derived: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return derived.to_string();
    }
    let cut = derived.len().saturating_sub(2);
    let base = match derived.get(cut..) {
        Some(d) if d.eq_ignore_ascii_case("_d") => &derived[..cut],
        _ => derived,
    };
    format!("{base}_{suffix}")
}

/// The warning of a set whose entry can never apply (F1 spec §3.2).
pub const NO_MATCH: &str = "this entry matches nothing";
/// The warning under a + row with no path yet (`matchN = ""`, owner 2026-10-12). It is not written to the JSON.
pub const EMPTY_ROW: &str = "this row matches nothing until a path is entered";

/// No active `matchN` and no `path_contains`: PGPatcher would never apply the entry (DECISIONS V4).
pub fn matches_nothing(s: &Settings, derived: &str) -> bool {
    active_matches(s, derived).is_empty() && text(s, "path_contains").is_none()
}

/// The output prefix (backslashes, no extension) of a set with settings `s` and derived diffuse path `derived`
/// (DECISIONS V3): the lowest active `matchN`, as PGPatcher's first hit gives it, else the derived path. A `rename`
/// is a new file name (owner, 2026-10-12): it replaces the last component, so the folder stays the matched one.
pub fn prefix_of(s: &Settings, derived: &str) -> Result<String, PrefixError> {
    // One `.dds` is cut, like a match path.
    let rename =
        text(s, "rename")
            .map(str::trim)
            .map(|r| if has_dds(r) { &r[..r.len() - 4] } else { r });
    if let Some(r) = rename {
        if r.contains(['\\', '/']) {
            return Err(PrefixError(format!(
                "rename: a file name only, no folders ({r:?})"
            )));
        }
        check_rel_path(r, "rename")?;
    }
    let base = matched_prefix(s, derived)?;
    Ok(match rename {
        // `base` always starts with `textures\pbr\`, so it has a folder.
        Some(r) => format!("{}\\{r}", base.rsplit_once('\\').map_or("", |(d, _)| d)),
        None => base,
    })
}

/// The prefix before any rename: the lowest active `matchN`, else the derived path (DECISIONS V3).
fn matched_prefix(s: &Settings, derived: &str) -> Result<String, PrefixError> {
    let matches = active_matches(s, derived);
    for (n, v) in &matches {
        check_json_path(v, &format!("match{n}"))?;
    }
    Ok(match matches.first() {
        Some((n, v)) => match_prefix(*n, v),
        None => {
            let d = derived.trim();
            check_json_path(d, "diffuse path")?;
            under_pbr(d)
        }
    })
}

/// The `match_diffuse` and `rename` a renamed set's entry carries, both relative to `textures\pbr\` (owner,
/// 2026-10-14; DECISIONS AA8). PGPatcher renames "the matching part (from match_diffuse or match_normal)": with
/// `match_diffuse` it keeps `textures\pbr` in front of the match and appends the rename. Without it, it substitutes
/// `\` + rename for the whole path and finds no file, so the entry patches nothing (PGPatcher 1.2.0 log). A hit
/// lands on the renamed files only when its texture is that `match_diffuse` (PGPatcher cuts the matched path by its
/// length); `pitfalls` warns otherwise. `None` without a rename or with a bad one.
pub fn json_rename(s: &Settings, derived: &str) -> Option<(String, String)> {
    text(s, "rename")?;
    let full = prefix_of(s, derived).ok()?;
    let base = matched_prefix(s, derived).ok()?;
    let rel = |p: &str| strip_ci(p, "textures\\pbr\\").to_string();
    Some((rel(&base), rel(&full)))
}

/// The prefix PGPatcher derives when `matchN = v` is the match that hits.
fn match_prefix(n: u8, v: &str) -> String {
    let full = match_value(v);
    under_pbr(&strip_slot_suffix(&full[..full.len() - 4], n))
}

/// Warnings that are never "corrected" (owner, 2026-10-09; F1 DECISIONS V4).
pub fn pitfalls(s: &Settings, derived: &str) -> Vec<String> {
    let mut out = vec![];
    // PGPatcher takes the prefix from whichever match hits; without a rename, matches that disagree split the outputs.
    let mut prefixes: Vec<String> = active_matches(s, derived)
        .iter()
        .map(|(n, v)| match_prefix(*n, v).to_ascii_lowercase())
        .collect();
    prefixes.dedup();
    let renamed = text(s, "rename").is_some();
    if prefixes.len() > 1 && !renamed {
        out.push(
            "match slots point at different textures; the entry applies to whichever hits first; edit the match \
             paths so they point at the same texture"
                .into(),
        );
    }
    // A rename lands right only for meshes whose matched texture is the entry's `match_diffuse` (DECISIONS AA8).
    if renamed && (prefixes.len() > 1 || text(s, "path_contains").is_some()) {
        out.push(
            "with a rename, only meshes using the first matched texture get the renamed files; meshes hit through \
             path_contains or a match slot pointing at another texture get a wrong path"
                .into(),
        );
    }
    if matches_nothing(s, derived) {
        out.push(format!(
            "{NO_MATCH}, so it is left out of the PGPatcher JSON"
        ));
    }
    let on = |k: &str| s.get(k).is_some_and(|v| v.as_bool() == Some(true));
    if on("alphaprop_delete") {
        if ["alphaprop_flags", "alphaprop_thresh", "alphaprop_create"]
            .iter()
            .any(|k| s.get(*k).is_some_and(|v| *v != json!(false)))
        {
            out.push("Delete wins; the other alpha keys of this entry do nothing".into());
        }
    } else if (s.contains_key("alphaprop_flags") || s.contains_key("alphaprop_thresh"))
        && !on("alphaprop_create")
    {
        out.push(
            "alphaprop_flags and alphaprop_thresh: only changes shapes that already have an NiAlphaProperty; turn on \
             Create NiAlphaProperty if missing to add one to shapes without"
                .into(),
        );
    }
    if let Some(v) = text(s, "match8") {
        if strip_ext(&v.to_lowercase()).ends_with("_s") {
            out.push(
                "match8 does not strip _s (its list is the old backlight one); use match7 for a subsurface map"
                    .into(),
            );
        }
    }
    out
}

/// DECISIONS AF8: PGPatcher matches a renamed entry's `match_diffuse` by its last path components, so a short or
/// folderless one also hits a same-named texture in another folder; those meshes get the rename in their own folder,
/// where no such file exists, and PGPatcher skips them. The warning names the first such path in `known` (texture
/// paths, `/` or `\`, any case, with or without `textures/` and `.dds`), the set's own paths aside.
pub fn rename_hits_elsewhere(s: &Settings, derived: &str, known: &[String]) -> Option<String> {
    let (md, _) = json_rename(s, derived)?;
    let md = md.replace('/', "\\").to_lowercase();
    let other = known.iter().find(|k| {
        let k = k.replace('/', "\\").to_lowercase();
        let k = strip_ext(&k);
        let rel = strip_ci(strip_ci(k, "textures\\"), "pbr\\");
        rel != md && rel.ends_with(&format!("\\{md}"))
    })?;
    Some(format!(
        "PGPatcher matches the renamed entry's match_diffuse \"{md}\" by its end, so it also hits {other} in another \
         folder; meshes using that texture get the rename there, where the renamed file does not exist, and \
         PGPatcher skips them; give the set a match path with more of its folder"
    ))
}

/// The output path, relative to the export folder, of the file with canonical `suffix` (`""` for the diffuse).
pub fn output_rel(prefix: &str, suffix: &str) -> PathBuf {
    let name = if suffix.is_empty() {
        format!("{prefix}.dds")
    } else {
        format!("{prefix}_{suffix}.dds")
    };
    PathBuf::from(name.replace('\\', "/"))
}
