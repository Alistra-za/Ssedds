//! M3b spec §10: build sets, get blocked, fix it, export DDS + PGPatcher JSON, export again with nothing changed.
mod support;
use engine::codes::Code;
use engine::config::Subtype;
use sdc_gui::model::pbr_export::{export_blocks, json_name};
use sdc_gui::model::sets::{set_views, DropChoice, DropOutcome, SlotCommand};
use sdc_gui::model::Answer;
use serde_json::{json, Value};
use std::time::Duration;
use support::*;

fn idle(m: &mut sdc_gui::model::AppModel) {
    assert!(m.wait_idle(Duration::from_secs(120)));
}

fn export(m: &mut sdc_gui::model::AppModel) {
    export_through(m);
    idle(m);
}

fn walk(dir: &std::path::Path) -> Vec<(String, std::time::SystemTime)> {
    let mut v = vec![];
    for e in walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
    {
        v.push((
            e.path().to_string_lossy().replace('\\', "/"),
            e.metadata().unwrap().modified().unwrap(),
        ));
    }
    v.sort();
    v
}

#[test]
fn build_sets_export_and_reexport_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let odd = m
        .items
        .iter()
        .find(|i| i.name == "odd.png")
        .unwrap()
        .key
        .clone();
    m.decide(&odd, Answer::Subtype(Subtype::PbrAlbedo));
    m.decide(&odd, Answer::Acknowledge(Code::DimNpot));
    idle(&mut m);
    let second = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    assert_eq!(
        m.drop_on_set(&rmaos, &second),
        DropOutcome::AskChoice {
            from: "landscape/rock01".into()
        }
    );
    assert_eq!(
        m.drop_choice(&rmaos, &second, DropChoice::Share),
        DropOutcome::Joined
    );
    m.reorder_set("landscape/rock01", 0);
    m.set_slot_command(
        "landscape/rock01",
        3,
        SlotCommand::Path(r"effects\white.dds".into()),
    )
    .unwrap();
    idle(&mut m);

    m.set_json_value(&second, "parallax", Some(json!(true)));
    idle(&mut m);
    assert!(
        export_blocks(&m)
            .iter()
            .any(|i| i.text.contains("parallax")),
        "{:?}",
        export_blocks(&m)
    );
    m.set_json_value(&second, "parallax", Some(json!(false)));
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![]);

    export(&mut m);
    let out = m.export_root();
    assert!(out.join("textures/pbr/landscape/rock01.dds").exists());
    assert!(out.join("textures/pbr/landscape/rock01_rmaos.dds").exists());
    let views = set_views(&m);
    let second_prefix = views
        .iter()
        .find(|v| v.id == second)
        .unwrap()
        .prefix
        .clone()
        .unwrap();
    let second_rmaos = format!("{}_rmaos.dds", second_prefix.replace('\\', "/"));
    assert!(
        !out.join(&second_rmaos).exists(),
        "the borrowed _rmaos is written once, at the owner's path"
    );
    let rmaos: Vec<_> = walk(&out.join("textures/pbr"))
        .into_iter()
        .map(|(f, _)| f)
        .filter(|f| f.ends_with("_rmaos.dds"))
        .collect();
    assert_eq!(rmaos.len(), 1, "exactly one _rmaos is written: {rmaos:?}");

    let p = out
        .join("PBRNIFPatcher")
        .join(format!("{}.json", json_name(&m)));
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{e}; notes {:?}", m.export.notes));
    let v: Value = serde_json::from_str(&text).unwrap();
    sdc_gui::pgp::validate::validate(&v).unwrap();
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{text}");
    assert_eq!(entries[0]["match1"], json!(r"landscape\rock01.dds"));
    assert!(entries[0].get("match_diffuse").is_none());
    assert_eq!(entries[0]["slot3"], json!(r"effects\white.dds"));
    assert_eq!(
        entries[1]["slot6"],
        json!(r"textures\pbr\landscape\rock01_rmaos.dds")
    );
    assert!(
        entries[1].get("parallax").is_none(),
        "the default block holds parallax: false, so an entry never repeats it: {text}"
    );
    let sha = m.sets_doc.as_ref().unwrap().last_json_sha256();
    assert!(sha.is_some(), "the JSON hash is recorded");

    // Export again: nothing changes.
    let before = walk(&out);
    export(&mut m);
    assert!(m.dialog.is_none(), "no overwrite prompt: {:?}", m.dialog);
    assert_eq!(m.sets_doc.as_ref().unwrap().last_json_sha256(), sha);
    let after = walk(&out);
    let changed: Vec<_> = after
        .iter()
        .filter(|(f, t)| {
            !f.ends_with(".json")
                && !f.contains("_sdc")
                && before.iter().any(|(g, u)| g == f && u != t)
        })
        .collect();
    assert!(changed.is_empty(), "re-export rewrote {changed:?}");
    assert_eq!(
        before.iter().map(|b| &b.0).collect::<Vec<_>>(),
        after.iter().map(|b| &b.0).collect::<Vec<_>>(),
        "no file appeared or vanished"
    );
}
