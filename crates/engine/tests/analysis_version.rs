//! The analysis disk cache's version guard (DECISIONS Note DU): fails when what `analyse_with` and `make_thumbnail`
//! produce changes without `ANALYSIS_VERSION` being bumped. Same shape as encode_versions.rs: the pin is rewritten
//! when the version moved, and left alone (with a failure) when only the hash did.
use engine::analyse::{analyse_with, AnalyseHints};
use engine::analysis_store::ANALYSIS_VERSION;
use engine::dds::write_atomic;
use engine::image::{Rgba32F, SourceInfo};
use engine::session::make_thumbnail;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

fn img(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Rgba32F {
    let mut i = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            *i.at_mut(x, y) = f(x, y);
        }
    }
    i
}

fn info(bit_depth: u8, has_alpha: bool, is_float: bool) -> SourceInfo {
    SourceInfo {
        bit_depth,
        channels: 4,
        has_alpha,
        is_float,
    }
}

/// Sha256 of every fixture's analysis under every hint set used in practice, and of its thumbnail.
fn observed() -> String {
    let c = |v: u32, n: u32| v as f32 / n as f32;
    let fixtures = [
        // Colour with varying alpha.
        (
            img(64, 48, |x, y| [c(x, 63), c(y, 47), 0.35, c(x + y, 110)]),
            info(8, true, false),
        ),
        // A tilted normal map.
        (
            img(32, 32, |x, y| [0.5 + c(x, 128), 0.5 - c(y, 128), 0.9, 1.0]),
            info(8, false, false),
        ),
        // Float HDR.
        (
            img(40, 40, |x, y| [c(x, 10), c(y, 20), 0.25, 1.0]),
            info(32, false, true),
        ),
        // Complex grass shape: height twice the width.
        (
            img(16, 32, |x, y| {
                if y < 16 {
                    [0.25, 0.5, 0.15, if (x + y) % 3 == 0 { 1.0 } else { 0.0 }]
                } else {
                    [0.6, 0.5, 0.8, 0.05]
                }
            }),
            info(8, true, false),
        ),
        // Gray, near-opaque alpha.
        (
            img(24, 24, |x, _| [c(x, 23), c(x, 23), c(x, 23), 253.0 / 255.0]),
            info(8, true, false),
        ),
    ];
    let hint_sets = [
        AnalyseHints::default(),
        AnalyseHints {
            bands: true,
            ..Default::default()
        },
        AnalyseHints {
            bands: true,
            curl: true,
            steps: None,
        },
        AnalyseHints {
            bands: true,
            curl: false,
            steps: Some(0),
        },
        AnalyseHints {
            bands: true,
            curl: false,
            steps: Some(3),
        },
    ];
    let mut h = Sha256::new();
    for (im, info) in &fixtures {
        for hints in hint_sets {
            h.update(serde_json::to_vec(&analyse_with(im, info, hints)).unwrap());
        }
        let t = make_thumbnail(im, info);
        h.update(t.width.to_le_bytes());
        h.update(t.height.to_le_bytes());
        h.update(&t.rgba);
    }
    hex::encode(h.finalize())
}

/// `Err` carries the failure. The pin is rewritten when it is missing or the version moved, never when only the
/// hash did.
fn check_pin(file: &Path, version: u32, sha: &str) -> Result<(), String> {
    let old: Option<Value> = std::fs::read_to_string(file)
        .ok()
        .map(|s| serde_json::from_str(&s).unwrap());
    let result = match &old {
        Some(p) if p["analysis_version"] == version && p["sha256"] != sha => Err(format!(
            "what analyse_with / make_thumbnail produce changed: bump ANALYSIS_VERSION in \
             crates/engine/src/analysis_store.rs and accept the new pin ({})",
            file.display()
        )),
        _ => Ok(()),
    };
    let pin = json!({ "analysis_version": version, "sha256": sha });
    // Compare the parsed value, not the bytes: the checkout is CRLF.
    if result.is_ok() && old.as_ref() != Some(&pin) {
        let text = serde_json::to_string_pretty(&pin).unwrap() + "\n";
        write_atomic(file, text.as_bytes()).unwrap();
    }
    result
}

#[test]
fn analysis_output_changes_only_with_a_version_bump() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/analysis_version.json");
    if let Err(e) = check_pin(&file, ANALYSIS_VERSION, &observed()) {
        panic!("{e}");
    }
}

#[test]
fn check_pin_fails_an_unbumped_change_and_accepts_a_bumped_one() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("pin.json");
    assert!(check_pin(&f, 1, "aa").is_ok(), "a first run pins");
    assert!(check_pin(&f, 1, "aa").is_ok());
    assert!(check_pin(&f, 1, "bb").is_err(), "same version, new hash");
    assert!(check_pin(&f, 1, "aa").is_ok(), "the pin was left alone");
    assert!(check_pin(&f, 2, "bb").is_ok(), "bumped: accepted");
    assert!(check_pin(&f, 2, "bb").is_ok());
    assert!(check_pin(&f, 2, "aa").is_err());
}
