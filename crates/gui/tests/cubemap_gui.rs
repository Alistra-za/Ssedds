//! Cubemap GUI debts: the sub-type dropdown offers Cubemap, the size estimate counts six faces, a six-face set is named so.
mod support;
use engine::config::{Subtype, Target};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::settings::GuiSettings;
use sdc_gui::ui::settings_panel::{offered_groups, OTHER_HEADING};
use std::time::Duration;

fn open(dir: &std::path::Path) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.join("textures"));
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}

/// Bytes of a full BC7 chain of an `n` x `n` image: 16 bytes per 4x4 block, blocks rounded up.
fn bc7_chain_bytes(mut n: u32) -> u64 {
    let mut sum = 0;
    loop {
        let b = u64::from(n.div_ceil(4));
        sum += b * b * 16;
        if n == 1 {
            return sum;
        }
        n /= 2;
    }
}

#[test]
fn cubemap_is_offered_in_the_subtype_dropdown() {
    let groups = offered_groups(Target::Cs);
    let other = groups.iter().find(|(h, _)| *h == OTHER_HEADING).unwrap();
    assert!(other.1.contains(&Subtype::Cubemap), "{other:?}");
}

#[test]
fn cube_size_estimate_counts_six_faces() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/a");
    std::fs::create_dir_all(&r).unwrap();
    image::RgbImage::from_fn(384, 64, |x, _| image::Rgb([(x % 256) as u8, 9, 9]))
        .save(r.join("chrome.png"))
        .unwrap();
    let mut m = open(d.path());
    m.decide("a/chrome.png", Answer::Subtype(Subtype::Cubemap));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_field(
        "a/chrome.png",
        "cubemap.layout",
        serde_json::json!("hstrip"),
    );
    m.set_field("a/chrome.png", "format", serde_json::json!("bc7"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let e = loop {
        if let Some(e) = m.size_of("a/chrome.png") {
            break e;
        }
        assert!(m.wait_idle(Duration::from_secs(60)));
    };
    assert_eq!(e.chain_bytes, 6 * bc7_chain_bytes(64));
}

#[test]
fn six_face_item_says_six_faces() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/sky");
    std::fs::create_dir_all(&r).unwrap();
    for f in ["px", "nx", "py", "ny", "pz", "nz"] {
        image::RgbImage::from_pixel(16, 16, image::Rgb([5, 6, 7]))
            .save(r.join(format!("env_{f}.png")))
            .unwrap();
    }
    let mut m = open(d.path());
    for f in ["px", "nx", "py", "ny", "pz", "nz"] {
        m.decide(
            &format!("sky/env_{f}.png"),
            Answer::Subtype(Subtype::Cubemap),
        );
        assert!(m.wait_idle(Duration::from_secs(60)));
    }
    let names: Vec<_> = m.items.iter().map(|i| i.name.clone()).collect();
    assert_eq!(
        names.iter().filter(|n| n.ends_with("(6 faces)")).count(),
        1,
        "{names:?}"
    );
}
