//! M6 Track F2 Task 7: layers are hidden, their rows come from the base normal (F2 spec §5, Note W11).
mod support;
use engine::config::LayerId::{self, *};
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::rnm::{moved, order_after_move, rnm_rows};
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::path::Path;
use std::time::Duration;
use support::session;

fn fixture(dir: &Path, files: &[&str]) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    for f in files {
        if *f == "iron.png" {
            ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
                .save(r.join(f))
                .unwrap();
        } else {
            ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
                .save(r.join(f))
                .unwrap();
        }
    }
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

const FULL: [&str; 5] = [
    "iron.png",
    "iron_n.png",
    "iron_bump.png",
    "iron_detailnorm.png",
    "iron_detailnormmask.png",
];

#[test]
fn layers_are_not_items() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path(), &FULL);
    for i in &m.items {
        let n = i.input.to_string_lossy().replace("\\", "/");
        assert!(
            !n.ends_with("_bump.png")
                && !n.ends_with("_detailnorm.png")
                && !n.ends_with("_detailnormmask.png"),
            "layer file became an item: {n}"
        );
    }
    let job = m.item("m/iron_n.png").unwrap().job.as_ref().unwrap();
    assert_eq!(job.rnm.as_ref().unwrap().layers.len(), 2);
}

#[test]
fn rows_follow_the_order_with_defaults() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &FULL);
    let job = m.item("m/iron_n.png").unwrap().job.clone().unwrap();
    let rows = rnm_rows(&job, &m.rnm_keys, &|_| None);
    let ids: Vec<LayerId> = rows.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![Bump, Detail]);
    assert_eq!((rows[0].strength, rows[1].strength), (0.4, 0.6));
    assert!(!rows[0].flip_green, "a bump never follows the base (W8)");
    assert!(matches!(rows[1].mask, Some((_, true))));
    assert_eq!(rows[0].key, "m/iron_bump.png");
    assert_eq!(
        order_after_move(&job, Detail, true),
        vec![Detail, Bump, Bent, Bump2, Detail2]
    );

    m.set_field("m/iron_n.png", "rnm.order", json!(["detail", "bump"]));
    idle(&mut m);
    let job = m.item("m/iron_n.png").unwrap().job.clone().unwrap();
    let ids: Vec<LayerId> = rnm_rows(&job, &m.rnm_keys, &|_| None)
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, vec![Detail, Bump]);
}

#[test]
fn moved_swaps_with_the_neighbour() {
    let present = [Bump, Detail, Detail2];
    assert_eq!(moved(&present, Detail, true), vec![Detail, Bump, Detail2]);
    assert_eq!(moved(&present, Bump, true), vec![Bump, Detail, Detail2]);
    assert_eq!(moved(&present, Detail2, false), vec![Bump, Detail, Detail2]);
}

#[test]
fn no_base_notes_are_kept_per_set() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path(), &["iron.png", "iron_bump.png"]);
    let found = m
        .rnm_notes
        .values()
        .flatten()
        .any(|w| w.code.as_str() == "RNM-NO-BASE");
    assert!(found, "notes: {:?}", m.rnm_notes);
}
