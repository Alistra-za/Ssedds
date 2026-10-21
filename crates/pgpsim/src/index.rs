//! A pre-filter over one file's prepared entries (v2 B8 fix 2, B-P38): `shape_matches` on the entries `candidates`
//! returns gives exactly the matches of a scan over every entry, without the scan. It never decides a match.
use crate::matching::{pg_slots, str_of};
use crate::text::{strip_pbr, tex_base, tex_base_slot};
use nif::path::TexPath;
use serde_json::Value;
use std::collections::HashMap;

/// The last path component of `p` after its last `\` or `/`; None when there is no separator or nothing follows it.
fn last(p: &str) -> Option<&str> {
    p.rfind(['\\', '/'])
        .map(|i| &p[i + 1..])
        .filter(|s| !s.is_empty())
}

/// The two names a shape is looked up by: the last component of PGPatcher's prepared slot 0 and slot 1 bases, the
/// strings `match_diffuse` and `match_normal` are tested against (`matching::tex_name`).
pub struct ShapeKeys([String; 2]);

impl ShapeKeys {
    pub fn of(slots: &[TexPath]) -> ShapeKeys {
        let s = pg_slots(slots);
        let pre0 = strip_pbr(tex_base(&s[0]));
        let pre1 = strip_pbr(tex_base_slot(&s[1], 1));
        let name = |p: &str| last(p).unwrap_or(p).to_string();
        ShapeKeys([name(&pre0), name(&pre1)])
    }
}

/// Entry indices by the name their `match_diffuse` / `match_normal` can match, plus the entries that must always be
/// tested (`path_contains`, `match1`..`match8`, or a match value without a separator, which `ends_with` can hit
/// mid-name). Entries with no string match key can never match and are left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchIndex {
    by_name: HashMap<String, Vec<usize>>,
    always: Vec<usize>,
}

impl MatchIndex {
    /// Over `prepared` entries (`prepare(entry, default)`), as `shape_matches` takes them.
    pub fn new(prepared: &[Value]) -> MatchIndex {
        let mut ix = MatchIndex::default();
        for (i, p) in prepared.iter().enumerate() {
            let Some(e) = p.as_object() else { continue };
            let other = str_of(e, "path_contains").is_some()
                || (1..=8).any(|n| str_of(e, &format!("match{n}")).is_some());
            let mut names = vec![];
            let mut always = other;
            for k in ["match_diffuse", "match_normal"] {
                if let Some(v) = str_of(e, k) {
                    // pre.ends_with("…\name") holds only when pre's last component is `name`
                    match last(&tex_base(v)) {
                        Some(n) => names.push(n.to_string()),
                        None => always = true,
                    }
                }
            }
            if always {
                ix.always.push(i);
            } else {
                names.dedup();
                for n in names {
                    ix.by_name.entry(n).or_default().push(i);
                }
            }
        }
        ix
    }

    /// The entries that may match a shape with these keys, ascending. A superset of the matching entries.
    pub fn candidates(&self, k: &ShapeKeys) -> Vec<usize> {
        let mut out: Vec<usize> = self.always.clone();
        for n in &k.0 {
            out.extend(self.by_name.get(n).into_iter().flatten());
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}
