//! `MatchIndex` (v2 B8 fix 2): `shape_matches` over its candidates gives exactly the entries a scan over every entry
//! gives, for every entry form the match rules read (01 §1.3).
mod common;
use common::{shape, NIF};
use pgpsim::{prepare, shape_matches, MatchIndex, ShapeKeys};
use serde_json::{json, Value};

fn entries() -> Vec<Value> {
    let raw = vec![
        json!({"match_diffuse": "rock01"}),
        json!({"match_diffuse": "rock01_d"}),
        json!({"match_diffuse": "landscape\\rock01"}),
        json!({"match_diffuse": "Landscape/ROCK01.dds"}),
        json!({"match_diffuse": "ock01"}),
        json!({"match_diffuse": "other\\rock01"}),
        json!({"match_diffuse": ""}),
        json!({"match_diffuse": "\\"}),
        json!({"match_diffuse": "rock01\\"}),
        json!({"match_diffuse": "."}),
        json!({"match_diffuse": ".dds"}),
        json!({"match_diffuse": 5}),
        json!({"texture": "rock01"}),
        json!({"texture": "stone"}),
        json!({"match_normal": "rock01"}),
        json!({"match_normal": "rock01_n"}),
        json!({"match_normal": "landscape\\rock01_n.dds"}),
        json!({"match_normal": "white"}),
        json!({"match_diffuse": "white", "match_normal": "rock01"}),
        json!({"path_contains": "rock"}),
        json!({"path_contains": "LANDSCAPE\\"}),
        json!({"path_contains": "nothing"}),
        json!({"match1": "landscape\\rock01_d.dds"}),
        json!({"match1": "textures\\landscape\\rock01_d.dds"}),
        json!({"match2": "landscape\\rock01_n.dds"}),
        json!({"match8": "x"}),
        json!({"match_diffuse": "rock01", "nif_filter": "rocks"}),
        json!({"match_diffuse": "rock01", "nif_filter": "trees"}),
        json!({"match_diffuse": "white"}),
        json!({"match_diffuse": "pbr\\white"}),
        json!({"rename": "rock01"}),
        json!({}),
        json!(3),
        json!("rock01"),
        json!(null),
    ];
    raw.iter().map(|e| prepare(e, &Value::Null)).collect()
}

fn slot_sets() -> Vec<Vec<&'static str>> {
    vec![
        vec![
            "textures\\landscape\\rock01_d.dds",
            "textures\\landscape\\rock01_n.dds",
        ],
        vec![
            "textures\\landscape\\rock01.dds",
            "textures\\landscape\\rock01_n.dds",
        ],
        vec![
            "textures\\pbr\\landscape\\rock01.dds",
            "textures\\pbr\\landscape\\rock01_n.dds",
        ],
        vec!["TEXTURES\\Landscape\\Rock01.DDS"],
        vec!["textures/landscape/rock01.dds"],
        vec!["textures\\white.dds"],
        vec!["textures\\pbr\\white.dds"],
        vec!["white.dds"],
        vec!["rock01"],
        vec![
            "textures\\other\\rock01.dds",
            "textures\\other\\white_n.dds",
        ],
        vec![""],
        vec![],
        vec!["", "textures\\landscape\\rock01_msn.dds"],
        vec!["textures\\.dds", "textures\\"],
    ]
}

#[test]
fn candidates_give_exactly_the_scan_s_matches() {
    let es = entries();
    let ix = MatchIndex::new(&es);
    let mut hits = 0;
    for slots in slot_sets() {
        let m = shape(&slots);
        for nif in [NIF, "meshes\\trees\\x.nif"] {
            let scan: Vec<usize> = (0..es.len())
                .filter(|&i| shape_matches(&es[i], nif, &m.slots))
                .collect();
            let cands = ix.candidates(&ShapeKeys::of(&m.slots));
            let via: Vec<usize> = cands
                .iter()
                .copied()
                .filter(|&i| shape_matches(&es[i], nif, &m.slots))
                .collect();
            assert_eq!(via, scan, "{slots:?} {nif}");
            hits += scan.len();
        }
    }
    assert!(hits > 20, "the table exercises matches: {hits}");
}

/// The point of the index: thousands of entries with other names are not candidates.
#[test]
fn other_names_are_not_candidates() {
    let mut raw: Vec<Value> = (0..5000)
        .map(|i| json!({"match_diffuse": format!("set{i}\\tex{i}")}))
        .collect();
    raw.push(json!({"match_diffuse": "rock01"}));
    let es: Vec<Value> = raw.iter().map(|e| prepare(e, &Value::Null)).collect();
    let ix = MatchIndex::new(&es);
    let m = shape(&["textures\\landscape\\rock01.dds"]);
    assert_eq!(ix.candidates(&ShapeKeys::of(&m.slots)), [5000]);
}
