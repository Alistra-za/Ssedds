//! M3a spec §8: a folder decided and exported in the GUI, run through `sdc convert`, gives the same manifest
//! records and the same DDS bytes.
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype};
use image::{ImageBuffer, Luma, Rgb, Rgba};
use sdc_gui::model::{Answer, AppModel};
use sdc_gui::settings::GuiSettings;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

fn folder(dir: &Path) -> std::path::PathBuf {
    let r = dir.join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    let grad = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(grad(x, y)))
        .save(r.join("rock01.png"))
        .unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| {
        let g = grad(x, y);
        Rgba([g[0], g[1], g[2], if x < 32 { 255 } else { 0 }])
    })
    .save(r.join("leaf.png"))
    .unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
        .save(r.join("rock01_normal.png"))
        .unwrap();
    ImageBuffer::from_fn(48, 48, |x, y| Rgb(grad(x, y)))
        .save(r.join("odd.png"))
        .unwrap();
    for (n, v) in [("rock01_roughness.png", 120u8), ("rock01_ao.png", 200)] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
            .save(r.join(n))
            .unwrap();
    }
    dir.join("textures")
}

/// output → (profile hash, source hash, DDS bytes).
fn outputs(out: &Path) -> BTreeMap<String, (String, String, Vec<u8>)> {
    let text = std::fs::read_to_string(engine::manifest::Manifest::at(out).path()).unwrap();
    let mut m = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let o = v["output"].as_str().unwrap().to_string();
        let bytes = std::fs::read(out.join(&o)).unwrap();
        m.insert(
            o,
            (
                v["profile_sha256"].as_str().unwrap().into(),
                v["source_sha256"].as_str().unwrap().into(),
                bytes,
            ),
        );
    }
    m
}

#[test]
fn a_gui_export_and_sdc_convert_agree() {
    let d = tempfile::tempdir().unwrap();
    let root = folder(d.path());
    let settings = GuiSettings {
        export_folder: Some(d.path().join("gui-out")),
        ..GuiSettings::default()
    };
    let session = Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap());
    let mut m = AppModel::new(settings, None, session);
    m.open_folder(root.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    // Note FB1: a project's export folder is its own pick; gui.toml's app-wide folder is not inherited.
    m.set_export_folder(d.path().join("gui-out"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/leaf.png", Answer::Alpha(AlphaPolicy::Cutout));
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Subtype(Subtype::Normal),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Green(GreenConvention::Directx),
    );
    m.set_field(
        "landscape/leaf.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.export_clicked();
    assert!(m.wait_idle(Duration::from_secs(120)));
    assert_eq!(m.export.summary.as_ref().unwrap().failed, 0);

    // The developer's own %APPDATA%\sdc\config.toml must not leak into the CLI run.
    let out = Command::new(env!("CARGO_BIN_EXE_sdc"))
        .arg("convert")
        .arg(&root)
        .args(["--recursive", "-o"])
        .arg(d.path().join("cli-out"))
        .args(["--bc1-decoder", "ideal"])
        .env("APPDATA", d.path().join("no-appdata"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "sdc convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (gui, cli) = (
        outputs(&d.path().join("gui-out")),
        outputs(&d.path().join("cli-out")),
    );
    // rock01, leaf, the normal, the 48 px texture and the packed _rmaos.
    assert_eq!(
        gui.len(),
        5,
        "outputs: {:?}",
        gui.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        gui.keys().collect::<Vec<_>>(),
        cli.keys().collect::<Vec<_>>()
    );
    for (k, g) in &gui {
        assert_eq!(g, &cli[k], "{k} differs");
    }
}
