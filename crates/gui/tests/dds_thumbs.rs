//! Note FI1: a skipped DDS input's tile shows its stored image, decoded by the GUI off the UI thread.
mod support;
use engine::config::Format;
use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
use sdc_gui::model::preview::dds_thumbnail;
use std::path::Path;
use std::time::{Duration, Instant};
use support::*;

/// A pure-red `size` square DDS of `format` with `levels` mips (written by the engine's writer, never copied).
fn red_dds(path: &Path, format: Format, size: u32, levels: usize) {
    let desc = DdsDesc {
        width: size,
        height: size,
        format,
        srgb: false,
        header: ResolvedHeader::Legacy,
    };
    let data: Vec<Vec<u8>> = (0..levels)
        .map(|i| {
            let s = (size >> i).max(1) as usize;
            match format {
                // One BC1 block: both endpoints 0xF800 (565 red), every index 0.
                Format::Bc1 => [0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0].repeat(s.div_ceil(4).pow(2)),
                // `Format::Rgba8` is written as B8G8R8A8 (DECISIONS S4).
                _ => [0, 0, 255, 255].repeat(s * s),
            }
        })
        .collect();
    std::fs::write(path, dds_bytes(&desc, &data).unwrap()).unwrap();
}

fn assert_red_thumb(path: &Path) {
    let t = dds_thumbnail(path, encoders_sys::Bc1Model::Ideal)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(t.width.max(t.height) <= 128, "{}x{}", t.width, t.height);
    assert_eq!(t.rgba.len(), (t.width * t.height * 4) as usize);
    let c = (((t.height / 2) * t.width + t.width / 2) * 4) as usize;
    let px = &t.rgba[c..c + 3];
    for (got, want) in px.iter().zip([255u8, 0, 0]) {
        assert!(got.abs_diff(want) <= 2, "{}: {px:?}", path.display());
    }
}

#[test]
fn dds_thumbnail_is_at_most_128_and_rgba8() {
    let d = tempfile::tempdir().unwrap();
    let bc1 = d.path().join("bc1.dds");
    red_dds(&bc1, Format::Bc1, 256, 9);
    assert_red_thumb(&bc1);
    let rgba = d.path().join("rgba.dds");
    red_dds(&rgba, Format::Rgba8, 256, 9);
    assert_red_thumb(&rgba);
}

#[test]
fn a_mipless_512_dds_gets_a_thumbnail() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("one.dds");
    red_dds(&p, Format::Rgba8, 512, 1);
    assert_red_thumb(&p);
}

/// The mod folder plus a skipped `landscape/stored.dds`, opened, with its thumbnail landed.
fn opened_with_dds(dir: &Path) -> sdc_gui::model::AppModel {
    let root = mod_folder(dir);
    red_dds(&root.join("landscape/stored.dds"), Format::Bc1, 256, 9);
    let mut m = opened(dir);
    let end = Instant::now() + Duration::from_secs(30);
    while m.item("landscape/stored.dds").unwrap().thumb.is_none() && Instant::now() < end {
        m.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    m
}

#[test]
fn a_skipped_dds_tile_gets_its_thumbnail_after_the_plan() {
    let d = tempfile::tempdir().unwrap();
    let m = opened_with_dds(d.path());
    let item = m.item("landscape/stored.dds").unwrap();
    assert!(matches!(
        item.status,
        sdc_gui::model::items::ItemStatus::Skipped(_)
    ));
    assert!(item.thumb.is_some());
}

#[test]
fn a_replan_reuses_the_cached_thumbnail() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_with_dds(d.path());
    let asked = m.dds_thumb_requests;
    assert!(asked >= 1);
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.dds_thumb_requests, asked);
    assert!(m.item("landscape/stored.dds").unwrap().thumb.is_some());
}

/// Polls until no tile image is being decoded.
fn wait_thumbs(m: &mut sdc_gui::model::AppModel) {
    let end = Instant::now() + Duration::from_secs(30);
    while m.dds_thumbs_pending() && Instant::now() < end {
        m.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!m.dds_thumbs_pending(), "tile images still decoding");
}

/// A BC1 header relabelled DXT3 (BC2, which the decoder refuses) with zeroed DXT3-sized data.
fn bc2_dds(path: &Path, size: u32, levels: usize) {
    red_dds(path, Format::Bc1, size, levels);
    let mut bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[84..88], b"DXT1");
    bytes[84..88].copy_from_slice(b"DXT3");
    bytes.truncate(128);
    let data: usize = (0..levels)
        .map(|i| ((size >> i).max(1) as usize).div_ceil(4).pow(2) * 16)
        .sum();
    bytes.resize(128 + data, 0);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_refused_dds_is_read_once_until_it_changes() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let p = root.join("landscape/stored.dds");
    bc2_dds(&p, 256, 9);
    let mut m = opened(d.path());
    wait_thumbs(&mut m);
    let asked = m.dds_thumb_requests;
    assert!(asked >= 1);
    assert!(m.item("landscape/stored.dds").unwrap().thumb.is_none());
    for _ in 0..2 {
        m.replan();
        assert!(m.wait_idle(Duration::from_secs(60)));
        wait_thumbs(&mut m);
    }
    assert_eq!(m.dds_thumb_requests, asked, "a refused DDS was read again");
    // A changed file (new size, so a new stamp) is tried again.
    red_dds(&p, Format::Bc1, 128, 8);
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    wait_thumbs(&mut m);
    assert!(
        m.dds_thumb_requests > asked,
        "a changed DDS was not retried"
    );
}
