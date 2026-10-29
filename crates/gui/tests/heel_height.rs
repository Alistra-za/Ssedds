//! Meshes UI T5b (Note CZ19): the heel height helpers, and the owner-data check of Calculate against the corpus.
use nif::extra::ExtraValue;
use sdc_gui::model::meshes::nif_edit::{heel_z_of, sdta_npc_z, sdta_with_npc_z};
use std::io::Read;
use std::path::Path;

#[test]
fn sdta_reads_and_writes_the_last_npc_entry() {
    let two = r#"[{"name":"NPC","pos":[0,0,4]},{"name":"NPC Head [Head]","pos":[0,0,1]},{"name":"NPC","pos":[0,0,7]}]"#;
    assert_eq!(sdta_npc_z(two), Some(7.0), "the last NPC entry wins");
    let out = sdta_with_npc_z(two, 9.5).unwrap();
    assert_eq!(sdta_npc_z(&out), Some(9.5));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["pos"][2], 4.0, "the earlier NPC entry is kept");
    assert_eq!(v[1]["name"], "NPC Head [Head]", "other entries are kept");
    // No NPC entry: one is added; not JSON: None.
    let none = sdta_with_npc_z(r#"[{"name":"Other","pos":[1,2,3]}]"#, 5.0).unwrap();
    assert_eq!(sdta_npc_z(&none), Some(5.0));
    assert_eq!(sdta_with_npc_z("not json", 5.0), None);
    assert_eq!(sdta_npc_z("[]"), None);
}

#[test]
fn heel_z_of_a_fixture_is_finite_and_a_bad_block_is_none() {
    let b = std::fs::read(format!(
        "{}/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let m = nif::model::read_model(&b).unwrap();
    let z = heel_z_of(&b, m.shapes[0].block).expect("a lit skinned shape");
    assert!(z.is_finite());
    assert_eq!(heel_z_of(&b, 9999), None);
}

#[path = "../../nif/tests/builder/mod.rs"]
mod builder;

/// T5b review Minor 2: the sign, the rounding to 0.1 and the node chain of `heel_z_of`.
#[test]
fn heel_z_of_is_minus_the_lowest_root_space_z_rounded() {
    use builder::scenes::heel_tri;
    let z = |zs, node| heel_z_of(&heel_tri(zs, node), 1);
    assert_eq!(
        z([-3.04, 2.0, 5.0], 0.0),
        Some(3.0),
        "the lowest vertex, negated and rounded"
    );
    assert_eq!(
        z([-3.04, 2.0, 5.0], 1.0),
        Some(2.0),
        "the parent node's translation applies"
    );
    assert_eq!(z([0.0, 2.0, 5.0], 0.0), Some(0.0), "never -0");
    assert_eq!(
        z([1.26, 2.0, 5.0], 0.0),
        Some(-1.3),
        "above the root: a negative height"
    );
}

/// T5b review Important 2: the SDTA's Z is written as given, so it equals HH_OFFSET's f32 (2.25 stays 2.25, 7.3 is
/// written as 7.3).
#[test]
fn sdta_writes_the_value_as_given() {
    let s = sdta_with_npc_z("[]", 2.25).unwrap();
    assert_eq!(sdta_npc_z(&s), Some(2.25));
    let s = sdta_with_npc_z("[]", 7.3).unwrap();
    assert!(s.contains("7.3]"), "{s}");
    assert_eq!(sdta_npc_z(&s), Some(7.3));
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// The stored heel height of a shape: HH_OFFSET when present (it wins in game), else the SDTA's last NPC Z.
fn stored(m: &nif::model::NifModel, s: &nif::model::ShapeModel) -> Option<(f32, &'static str)> {
    let extras = m.extras_of(&s.net);
    let named = |n: &[u8]| {
        extras
            .iter()
            .find(|x| x.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n)))
    };
    if let Some(ExtraValue::Float(bits)) = named(b"HH_OFFSET").and_then(|x| x.value.clone()) {
        return Some((f32::from_bits(bits), "HH_OFFSET"));
    }
    match named(b"SDTA").and_then(|x| x.value.clone()) {
        Some(ExtraValue::Str(s)) => sdta_npc_z(&s).map(|z| (z, "SDTA")),
        _ => None,
    }
}

/// Owner data, read-only (not a gate): every loose NIF under `SDC_ACHERUS\mods` whose header names HH_OFFSET or
/// SDTA; per shape with a stored heel height, Calculate's value next to it. Prints the spread for the task report.
#[test]
#[ignore]
fn heel_height_calculation_against_the_corpus() {
    let Ok(root) = std::env::var("SDC_ACHERUS") else {
        println!("skipped: set SDC_ACHERUS");
        return;
    };
    let mods = Path::new(&root).join("mods");
    let mut rows: Vec<(String, String, &str, f32, Option<f32>)> = vec![];
    for f in walkdir::WalkDir::new(&mods)
        .into_iter()
        .filter_map(Result::ok)
    {
        let p = f.path();
        if !f.file_type().is_file() || !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nif"))
        {
            continue;
        }
        // The header's string table names every extra data: 256 KB covers it.
        let mut head = vec![0u8; 256 * 1024];
        let Ok(n) = std::fs::File::open(p).and_then(|mut h| h.read(&mut head)) else {
            continue;
        };
        if !contains(&head[..n], b"HH_OFFSET") && !contains(&head[..n], b"SDTA") {
            continue;
        }
        let Ok(b) = std::fs::read(p) else { continue };
        let Ok(m) = nif::model::read_model(&b) else {
            continue;
        };
        for s in &m.shapes {
            if let Some((v, from)) = stored(&m, s) {
                let rel = p.strip_prefix(&mods).unwrap_or(p).display().to_string();
                let name = s.name.as_deref().unwrap_or_default().to_string();
                rows.push((rel, name, from, v, heel_z_of(&b, s.block)));
            }
        }
    }
    let diffs: Vec<f32> = rows
        .iter()
        .filter_map(|r| r.4.map(|c| (c - r.3).abs()))
        .collect();
    let mut sorted = diffs.clone();
    sorted.sort_by(f32::total_cmp);
    let within = |d: f32| diffs.iter().filter(|&&x| x <= d).count();
    for r in &rows {
        println!(
            "{} | {} | {} {:.2} | calc {}",
            r.0,
            r.1,
            r.2,
            r.3,
            r.4.map_or("none".into(), |c| format!("{c:.1}"))
        );
    }
    println!(
        "shapes {} computed {} | within 0.5: {} within 1.0: {} within 2.0: {} | median {:.2} max {:.2}",
        rows.len(),
        diffs.len(),
        within(0.5),
        within(1.0),
        within(2.0),
        sorted.get(sorted.len() / 2).copied().unwrap_or(f32::NAN),
        sorted.last().copied().unwrap_or(f32::NAN)
    );
    assert!(
        !rows.is_empty(),
        "no shape with heel data found under {}",
        mods.display()
    );
}
