//! PGPatcher's path-string helpers (01 §1.3) on ASCII-lower-cased strings.

/// `PGNIFUtil::getTexBase(path, UNKNOWN)` [Src 1.2.0 NU getTexBase]: `path.parent_path() / path.stem()`,
/// lower-cased. MSVC `std::filesystem` treats `\` and `/` as separators and joins with `\`, so the last
/// separator comes back as `\` (a leading root separator is kept). The extension goes only when its dot is
/// inside the file name and is not its first character (`stem()` of `.dds` is `.dds`).
pub(crate) fn tex_base(path: &str) -> String {
    let p = path.to_ascii_lowercase();
    let sep = p.rfind(['\\', '/']);
    let file = match sep {
        Some(i) => &p[i + 1..],
        None => p.as_str(),
    };
    let stem = match file.rfind('.') {
        Some(d) if d > 0 && file != ".." => &file[..d],
        _ => file,
    };
    match sep {
        None => stem.to_string(),
        Some(0) => format!("{}{stem}", &p[..1]),
        Some(i) => format!("{}\\{stem}", &p[..i]),
    }
}

/// `getTexSuffixMap` in `std::map` (sorted key) order: suffix → 0-based slot [Src 1.2.0 NU:40-66].
const SUFFIXES: [(&str, usize); 19] = [
    ("_b", 7),
    ("_bl", 7),
    ("_cnr", 6),
    ("_d", 0),
    ("_e", 4),
    ("_em", 5),
    ("_envmask", 5),
    ("_f", 6),
    ("_flow", 7),
    ("_g", 2),
    ("_i", 6),
    ("_m", 5),
    ("_msn", 1),
    ("_n", 1),
    ("_p", 3),
    ("_rmaos", 5),
    ("_s", 6),
    ("_sk", 2),
    ("mask", 0),
];

/// `getTexBase(path, slot)`: `tex_base`, then the first suffix of that slot (map order) is cut.
pub(crate) fn tex_base_slot(path: &str, slot: usize) -> String {
    let b = tex_base(path);
    for (suf, s) in SUFFIXES {
        if s == slot && b.ends_with(suf) {
            return b[..b.len() - suf.len()].to_string();
        }
    }
    b
}

/// A leading `textures\pbr\` becomes `textures\` [Src 1.2.0 TP:286-290].
pub(crate) fn strip_pbr(p: String) -> String {
    match p.strip_prefix("textures\\pbr\\") {
        Some(rest) => format!("textures\\{rest}"),
        None => p,
    }
}

/// `boost::icontains` on ASCII.
pub(crate) fn icontains(hay: &str, needle: &str) -> bool {
    hay.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

/// A leading `textures\` that is not `textures\pbr\` becomes `textures\pbr\` [Src 1.2.0 TP:551-555].
pub(crate) fn add_pbr(p: &str) -> String {
    match p.strip_prefix("textures\\") {
        Some(rest) if !rest.starts_with("pbr\\") => format!("textures\\pbr\\{rest}"),
        _ => p.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tex_base_follows_std_filesystem() {
        assert_eq!(tex_base("Textures\\A\\B_D.DDS"), "textures\\a\\b_d");
        assert_eq!(tex_base("\\rock01_d"), "\\rock01_d");
        assert_eq!(tex_base("a/b/c.dds"), "a/b\\c");
        assert_eq!(tex_base("rock.v2.dds"), "rock.v2");
        assert_eq!(tex_base("textures\\.dds"), "textures\\.dds");
        assert_eq!(tex_base(""), "");
    }

    #[test]
    fn tex_base_slot_cuts_the_slot_suffix_in_map_order() {
        assert_eq!(tex_base_slot("x_msn.dds", 1), "x");
        assert_eq!(tex_base_slot("x_n.dds", 1), "x");
        assert_eq!(tex_base_slot("x_d.dds", 1), "x_d");
        assert_eq!(tex_base_slot("x_rmaos.dds", 5), "x");
        assert_eq!(tex_base_slot("xmask.dds", 0), "x");
        assert_eq!(tex_base_slot("x_bl.dds", 7), "x");
    }

    #[test]
    fn strip_pbr_and_icontains() {
        assert_eq!(strip_pbr("textures\\pbr\\a\\b".into()), "textures\\a\\b");
        assert_eq!(strip_pbr("textures\\a\\b".into()), "textures\\a\\b");
        assert!(icontains("textures\\landscape", "LandScape"));
        assert!(!icontains("textures", "pbr"));
    }
}
