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
    renderer_at(&standin_root(), cache, features)
}

/// [`renderer`] on the shaders in `root` (a modified copy of the stand-in).
pub fn renderer_at(root: &Path, cache: &Path, features: &[&str]) -> Renderer {
    let fp = render3d::locate::fingerprint(root, None).unwrap();
    let mut r = Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.to_path_buf()).unwrap();
    r.set_features(features.iter().map(|s| s.to_string()).collect());
    r
}

/// A built-in plane as a parsed shape: Z test on, Own_Emit with `emissive` (so STANDIN_FLAT shows it), `slsf2` as
/// given (bit 4 = Double_Sided).
pub fn plane_shape(
    id: u32,
    world: [[f32; 4]; 3],
    emissive: [f32; 3],
    slsf2: u32,
) -> render3d::ParsedShape {
    let mut m = nif::material::MaterialState::builtin_default();
    m.slsf1 |= (1 << 31) | (1 << 22); // Z_Buffer_Test, Own_Emit
    m.slsf2 = slsf2;
    m.params.emissive_color = emissive;
    m.params.emissive_multiple = 1.0;
    render3d::ParsedShape {
        id: render3d::ShapeId(id),
        name: format!("plane{id}"),
        mesh: std::sync::Arc::new(render3d::builtin_mesh(render3d::Builtin::Plane)),
        world,
        material: m,
    }
}

pub const IDENT: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// A FLAT stand-in renderer with `shapes` loaded (no errors).
pub fn planes(cache: &Path, shapes: Vec<render3d::ParsedShape>) -> Renderer {
    let mut r = renderer(cache, FLAT);
    let parsed = render3d::ParsedNif {
        shapes,
        ..Default::default()
    };
    let rep = r.load_parsed(&parsed, &|_| None);
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
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

/// An uncompressed DX10-header DDS, one mip: `R8G8B8A8_UNORM` (28) or `R8G8B8A8_UNORM_SRGB` (29). The same texels in
/// the two formats sample differently on the GPU (the sRGB view linearises), which is what the A/B shows.
pub fn dds_rgba8_dx10(w: u32, h: u32, texels: &[[u8; 4]], srgb: bool) -> Vec<u8> {
    assert_eq!(texels.len(), (w * h) as usize);
    let mut b = vec![0u8; 148];
    let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0, u32::from_le_bytes(*b"DDS "));
    put(4, 124);
    put(8, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    put(12, h);
    put(16, w);
    put(20, w * 4);
    put(28, 1);
    put(76, 32);
    put(80, 0x4); // DDPF_FOURCC
    put(84, u32::from_le_bytes(*b"DX10"));
    put(108, 0x1000); // DDSCAPS_TEXTURE
    put(128, if srgb { 29 } else { 28 });
    put(132, 3); // D3D10_RESOURCE_DIMENSION_TEXTURE2D
    put(140, 1); // array size
    b.extend(texels.iter().flatten());
    b
}

/// A 1×1 legacy cube map (all six faces `texel`): a kind a 2D register must refuse.
pub fn dds_cube_rgba8(texel: [u8; 4]) -> Vec<u8> {
    let mut b = dds_rgba8(1, 1, &[texel]);
    b[108..112].copy_from_slice(&0x1008u32.to_le_bytes()); // TEXTURE | COMPLEX
    b[112..116].copy_from_slice(&(0x200u32 | 0xFC00).to_le_bytes()); // CUBEMAP | all six faces
    for _ in 0..5 {
        b.extend(texel);
    }
    b
}
