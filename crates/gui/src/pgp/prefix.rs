//! Match modes and the output prefix PGPatcher derives (M3b spec §5.5, gap review "How match1–match8 build the
//! output prefix"). Quirks are kept on purpose: the files must land where PGPatcher looks.
use super::settings::Settings;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Diffuse,
    Normal,
    PathContains,
    Slot(u8),
}

/// The first alternative match key that is set decides the mode; none set means `match_diffuse` (spec §4.4).
pub fn match_mode(s: &Settings) -> MatchMode {
    let set = |k: &str| {
        s.get(k)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
    };
    if set("match_normal") {
        return MatchMode::Normal;
    }
    if set("path_contains") {
        return MatchMode::PathContains;
    }
    (1..=8u8)
        .find(|n| set(&format!("match{n}")))
        .map_or(MatchMode::Diffuse, MatchMode::Slot)
}

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

/// The output prefix (backslashes, no extension) of a set with effective settings `s`; `derived` is the derived
/// `match_diffuse`.
pub fn prefix_of(s: &Settings, derived: &str) -> Result<String, PrefixError> {
    let rename = text(s, "rename").map(backslash);
    if let Some(r) = &rename {
        check_rel_path(r, "rename")?;
    }
    let mode = match_mode(s);
    match mode {
        MatchMode::Diffuse | MatchMode::PathContains | MatchMode::Normal => {
            let (field, value) = match mode {
                MatchMode::Normal => (
                    "match_normal",
                    text(s, "match_normal").unwrap_or_default().to_string(),
                ),
                MatchMode::Diffuse => (
                    "match_diffuse",
                    text(s, "match_diffuse").map_or_else(|| derived.to_string(), str::to_string),
                ),
                _ => ("match_diffuse", derived.to_string()),
            };
            let value = backslash(value.trim());
            check_rel_path(&value, field)?;
            if let Some(r) = rename {
                return Ok(under_pbr(&r));
            }
            let mut v = strip_ext(&value).to_string();
            if mode == MatchMode::Normal {
                v = strip_slot_suffix(&v, 2);
            }
            Ok(under_pbr(&v))
        }
        MatchMode::Slot(n) => {
            let field = format!("match{n}");
            let value = backslash(text(s, &field).unwrap_or_default().trim());
            check_rel_path(&value, &field)?;
            if let Some(r) = rename {
                return Ok(r);
            }
            Ok(under_pbr(&strip_slot_suffix(strip_ext(&value), n)))
        }
    }
}

/// Warnings that are never "corrected" (owner, 2026-10-09).
pub fn pitfalls(s: &Settings) -> Vec<String> {
    let mut out = vec![];
    if let Some(v) = text(s, "match8") {
        if strip_ext(&v.to_lowercase()).ends_with("_s") {
            out.push(
                "match8 does not strip _s (its list is the old backlight one); use match7 for a subsurface map"
                    .into(),
            );
        }
    }
    if matches!(match_mode(s), MatchMode::Slot(_)) {
        if let Some(r) = text(s, "rename") {
            let starts_with_textures = backslash(r).to_lowercase().starts_with("textures\\");
            if !starts_with_textures {
                out.push(
                    "rename on a matchN entry replaces the whole prefix, so it must be a full path such as textures\\pbr\\…"
                        .into(),
                );
            }
        }
    }
    out
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
