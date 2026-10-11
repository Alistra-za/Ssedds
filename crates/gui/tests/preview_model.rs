mod support;
use engine::config::Target;
use sdc_gui::model::preview::*;
use sdc_gui::model::*;
use std::time::{Duration, Instant};
use support::*;

fn settle(m: &mut AppModel) {
    assert!(m.wait_idle(Duration::from_secs(120)));
}

#[test]
fn encoding_is_off_by_default_and_encode_now_encodes_once() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(!m.preview.live);
    m.expand("landscape/rock01_rmaos");
    settle(&mut m);
    assert!(matches!(m.preview.source, SourceState::Ready(_)));
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    m.tick(Instant::now() + Duration::from_secs(5));
    settle(&mut m);
    assert!(
        matches!(m.preview.encoded, EncodedState::NotEncoded),
        "live off: nothing encodes by itself"
    );
    m.encode_now();
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not done")
    };
    assert_eq!(out.result.source.first_level, 0);
    assert_eq!(out.decoded.len(), out.result.levels.len());
}

#[test]
fn an_edit_makes_the_result_stale_and_live_re_encodes_after_the_debounce() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    settle(&mut m);
    m.set_field(
        "landscape/rock01_rmaos",
        "encoder.effort",
        serde_json::json!("fast"),
    );
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Stale(_)));
    m.set_live(true);
    let t0 = Instant::now();
    m.tick(t0);
    assert!(
        !matches!(m.preview.encoded, EncodedState::Encoding { .. }),
        "debounced"
    );
    m.tick(t0 + DEBOUNCE + Duration::from_millis(1));
    assert!(matches!(
        m.preview.encoded,
        EncodedState::Encoding { previous: Some(_) }
    ));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
}

#[test]
fn mode_change_cancels_and_drops_a_preview_of_a_vanished_job() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let set = m
        .item("landscape/rock01_rmaos")
        .unwrap()
        .set_key
        .clone()
        .unwrap();
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    // Switch target while the encode may still be running: cpm packs rock01_m instead.
    m.set_target(Target::Cpm);
    settle(&mut m);
    assert!(m.item("landscape/rock01_rmaos").is_none());
    assert_eq!(m.expanded, None);
    assert_eq!(m.preview.item, None);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    assert_eq!(m.selection, Selection::Set(set));
}

#[test]
fn the_size_limit_skips_large_mips() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.settings.preview_max = 16;
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not done")
    };
    assert_eq!(out.result.source.first_level, 2, "64 and 32 are above 16");
}

#[test]
fn items_without_a_job_preview_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_roughness.png");
    assert_eq!(m.preview.notice.as_deref(), Some("Nothing to preview"));
    assert!(!m.busy(), "nothing was started");
    // An unresolved job previews its current settings.
    m.expand("landscape/leaf.png");
    assert_eq!(
        m.preview.notice.as_deref(),
        Some("Preview of the suggested settings")
    );
    settle(&mut m);
    assert!(matches!(m.preview.source, SourceState::Ready(_)));
}

#[test]
fn a_dds_input_previews_its_own_bytes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.export_clicked();
    m.proceed_anyway();
    settle(&mut m);
    let dds = walk(&d.path().join("export"))
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "dds"))
        .expect("the export wrote a .dds");
    let root = d.path().join("textures/landscape");
    std::fs::copy(&dds, root.join("stored.dds")).unwrap();
    m.open_folder(d.path().join("textures"));
    settle(&mut m);
    m.expand("landscape/stored.dds");
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not done: {:?}", m.preview.encoded)
    };
    assert!(!out.result.levels.is_empty());
    assert_eq!(out.decoded.len(), out.result.levels.len());
    assert!(out.result.source.levels.is_empty(), "no source side");
    assert_eq!(
        m.preview.notice.as_deref(),
        Some("DDS input: no source image")
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn a_result_for_a_collapsed_texture_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_rmaos");
    m.encode_now();
    m.collapse();
    assert!(!m.busy(), "collapsing cancels the work");
    std::thread::sleep(Duration::from_millis(500));
    m.poll();
    assert_eq!(m.preview.item, None);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
}

#[test]
fn live_encode_is_saved_and_encodes_on_expand() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let path = d.path().join("gui.toml");
    m.settings_path = Some(path.clone());
    m.set_live(true);
    assert!(sdc_gui::settings::GuiSettings::load(&path).0.live_encode);
    m.expand("landscape/rock01_rmaos");
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::NotEncoded));
    let t0 = Instant::now();
    m.tick(t0);
    m.tick(t0 + DEBOUNCE);
    assert!(matches!(m.preview.encoded, EncodedState::Encoding { .. }));
    settle(&mut m);
    assert!(matches!(m.preview.encoded, EncodedState::Done(_)));
}

#[test]
fn the_gpu_limit_lowers_the_preview_limit() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.gpu_max_dim = 32;
    assert_eq!(m.settings.preview_max, 4096);
    assert_eq!(m.preview_limit(), 32);
    m.expand("landscape/rock01_rmaos");
    settle(&mut m);
    let SourceState::Ready(src) = &m.preview.source else {
        panic!("no source")
    };
    assert_eq!(src.first_level, 1, "64 px does not fit 32 px");
    assert_eq!(
        m.preview.notice.as_deref(),
        Some("This GPU shows at most 32 px: previewing from the first mip that fits")
    );
    m.encode_now();
    settle(&mut m);
    let EncodedState::Done(out) = &m.preview.encoded else {
        panic!("not done")
    };
    assert_eq!(out.result.source.first_level, 1);
}

#[test]
fn a_gpu_limit_that_changes_nothing_adds_no_notice() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.gpu_max_dim = 1024;
    m.settings.preview_max = 2048;
    m.expand("landscape/rock01_rmaos");
    settle(&mut m);
    assert!(m.preview.notice.is_none(), "{:?}", m.preview.notice);
}

/// Writes a 16x16 full-chain DDS of `pf` with every texel `px` (raw level bytes per texel).
fn raw_dds(path: &std::path::Path, dxgi: u32, bpp: u32, texel: &[u8]) {
    let mut levels = vec![];
    let mut s = 16u32;
    loop {
        levels.extend(
            std::iter::repeat_n(texel, (s * s) as usize)
                .flatten()
                .copied(),
        );
        if s == 1 {
            break;
        }
        s /= 2;
    }
    let mut hd = [0u32; 31];
    hd[0] = 124;
    hd[1] = 0x1 | 0x2 | 0x4 | 0x8 | 0x1000 | 0x2_0000;
    hd[2] = 16;
    hd[3] = 16;
    hd[4] = 16 * bpp;
    hd[5] = 1;
    hd[6] = 5;
    hd[18] = 32;
    hd[19] = 0x4;
    hd[20] = u32::from_le_bytes(*b"DX10");
    hd[26] = 0x1000 | 0x8 | 0x40_0000;
    let mut b = b"DDS ".to_vec();
    for v in hd {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for v in [dxgi, 3, 0, 1, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend(levels);
    std::fs::write(path, b).unwrap();
}

/// DECISIONS S5: N11 ends. sdc's own rgba8 and a third-party BGRX8 preview from the CPU decode.
#[test]
fn uncompressed_dds_inputs_preview_from_the_cpu_decode() {
    for (name, dxgi, bpp, texel, want) in [
        (
            "own.dds",
            87u32,
            4u32,
            vec![30u8, 20, 10, 40],
            [10.0f32, 20.0, 30.0, 40.0],
        ),
        (
            "third.dds",
            88,
            4,
            vec![30, 20, 10, 0],
            [10.0, 20.0, 30.0, 255.0],
        ),
    ] {
        let d = tempfile::tempdir().unwrap();
        let root = support::mod_folder(d.path());
        raw_dds(&root.join("landscape").join(name), dxgi, bpp, &texel);
        let mut m = opened(d.path());
        m.expand(&format!("landscape/{name}"));
        settle(&mut m);
        let EncodedState::Done(out) = &m.preview.encoded else {
            panic!("{name}: {:?}", m.preview.encoded)
        };
        let px = out.decoded[0][0];
        for c in 0..4 {
            assert!((px[c] * 255.0 - want[c]).abs() < 0.5, "{name} c{c}: {px:?}");
        }
    }
}
