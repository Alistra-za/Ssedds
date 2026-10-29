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
fn layers_are_helpers_not_tiles() {
    // Note DY16 overturns Note W11 for listing: a layer file is a helper row, still never a tile.
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path(), &FULL);
    let tiles = m.visible_items();
    let mut layers = 0;
    for (n, i) in m.items.iter().enumerate() {
        let p = i.input.to_string_lossy().replace("\\", "/");
        if p.ends_with("_bump.png")
            || p.ends_with("_detailnorm.png")
            || p.ends_with("_detailnormmask.png")
        {
            layers += 1;
            assert_eq!(i.kind, sdc_gui::model::items::ItemKind::Helper, "{p}");
            assert!(!tiles.contains(&n), "layer file became a tile: {p}");
        }
    }
    assert_eq!(layers, 3);
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

#[test]
fn layer_target_edit_changes_the_set_preview_signature() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(
        d.path(),
        &[
            "iron.png",
            "iron_n.png",
            "iron_cnr.png",
            "iron_detailnorm.png",
        ],
    );
    let sig = |m: &mut AppModel, t: &str| {
        m.set_field("m/iron_detailnorm.png", "rnm.target", json!(t));
        idle(m);
        let item = m.item("m/iron_cnr.png").unwrap();
        assert!(item.job.as_ref().unwrap().active_rnm().is_some() || t == "n");
        sdc_gui::model::set_preview::signature(item)
    };
    let (a, b) = (sig(&mut m, "n"), sig(&mut m, "cnr"));
    assert_ne!(
        a, b,
        "the _cnr preview must refresh when a layer is retargeted"
    );
}

#[test]
fn editing_a_layer_file_flags_its_base_normal() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), &FULL);
    assert_eq!(m.check_sources(), 0);
    std::thread::sleep(Duration::from_millis(20));
    ImageBuffer::from_fn(32, 32, |_, _| Rgb([128u8, 128, 255]))
        .save(d.path().join("m/iron_detailnorm.png"))
        .unwrap();
    assert!(m.check_sources() >= 1);
    assert!(m.item("m/iron_n.png").unwrap().changed_on_disk);
}
