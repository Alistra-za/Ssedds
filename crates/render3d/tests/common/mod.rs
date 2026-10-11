//! Helpers shared by the WARP integration tests: the repo-owned stand-in shader, nifly fixtures, tiny DDS files.
#![allow(dead_code)]
use render3d::device::Gpu;
use render3d::Renderer;
use std::path::{Path, PathBuf};

/// The stand-in hooks of Task 8b: opaque permutations compile, pixels = 0.25·diffuse + EmitColor (+ green when
/// PBRFlags ≠ 0), no lighting.
pub const FLAT: &[&str] = &["STANDIN_OPAQUE_OK", "STANDIN_FLAT"];

pub fn standin_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/standin"
    ))
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../nif/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A WARP renderer on the stand-in shader with exactly `features` as CS feature defines.
pub fn renderer(cache: &Path, features: &[&str]) -> Renderer {
    let fp = render3d::locate::fingerprint(&standin_root(), None).unwrap();
    let mut r = Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.to_path_buf()).unwrap();
    r.set_features(features.iter().map(|s| s.to_string()).collect());
    r
}

/// Sum of one channel (0 = red, 1 = green, 2 = blue) over an RGBA8 frame.
pub fn channel(px: &[u8], c: usize) -> u64 {
    px.chunks(4).map(|p| p[c] as u64).sum()
}

/// Pixels brighter than the clear colour.
pub fn covered(px: &[u8]) -> usize {
    px.chunks(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
        .count()
}

/// Files in the shader cache directory (`compile` writes `<key>.cso` + `<key>.json` per compile, compile.rs:111-112).
pub fn cache_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

/// An uncompressed RGBA8 DDS, one mip, legacy header with masks R 0xff, G 0xff00, B 0xff0000, A 0xff000000
/// (`engine::dds::read_dds` reads it as `Rgba8 { srgb: false }`, dds.rs:533). `texels` row by row, top first.
pub fn dds_rgba8(w: u32, h: u32, texels: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(texels.len(), (w * h) as usize);
    let mut b = vec![0u8; 128];
    let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0, u32::from_le_bytes(*b"DDS "));
    put(4, 124);
    put(8, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    put(12, h);
    put(16, w);
    put(20, w * 4);
    put(28, 1);
    put(76, 32);
    put(80, 0x41); // DDPF_RGB | DDPF_ALPHAPIXELS
    put(88, 32);
    put(92, 0xff);
    put(96, 0xff00);
    put(100, 0xff_0000);
    put(104, 0xff00_0000);
    put(108, 0x1000); // DDSCAPS_TEXTURE
    b.extend(texels.iter().flatten());
    b
}
