//! Cube checks shared by `sdc inspect` and `sdc validate` (DECISIONS Note DS12). They read raw texels.
use crate::dds::{DdsFile, DdsPixelFormat};
use crate::decode::{decode_level, Decoded};
use encoders_sys::Bc1Model;

/// True when `game_path` has a `textures` component directly followed by `cubemaps` or its stem ends in `_e` (ASCII case-insensitive, either separator).
fn expects_cube(game_path: &str) -> bool {
    let mut parts = game_path.split(['/', '\\']).filter(|c| !c.is_empty());
    let stem = parts
        .next_back()
        .map(|f| f.rsplit_once('.').map_or(f, |s| s.0));
    stem.is_some_and(|s| s.to_ascii_lowercase().ends_with("_e"))
        || under_textures_cubemaps(&parts.collect::<Vec<_>>().join("/"))
}

/// True when `path` has a `textures` component directly followed by `cubemaps` (ASCII case-insensitive, either
/// separator): the folder the game reads as cubes, shared with `suggest` (DS23).
pub(crate) fn under_textures_cubemaps(path: &str) -> bool {
    path.split(['/', '\\'])
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|w| w[0].eq_ignore_ascii_case("textures") && w[1].eq_ignore_ascii_case("cubemaps"))
}

fn is_srgb(f: DdsPixelFormat) -> bool {
    use DdsPixelFormat::*;
    matches!(
        f,
        Bc1 { srgb: true }
            | Bc2 { srgb: true }
            | Bc3 { srgb: true }
            | Bc7 { srgb: true }
            | Rgba8 { srgb: true }
            | Bgra8 { srgb: true }
    )
}

/// Whether the +Y face's mip-0 texel decodes to a non-black RGB (false when it cannot be decoded).
fn pos_y_is_lit(f: &DdsFile) -> bool {
    let n = f.levels.len();
    let Some((w, h, r)) = f.surfaces().get(2 * n).cloned() else {
        return false;
    };
    match decode_level(f.format, w, h, &f.data[r], Some(Bc1Model::Ideal)) {
        Ok(Decoded::Ldr(px)) => px.first().is_some_and(|p| p[..3].iter().any(|&c| c > 0)),
        Ok(Decoded::Hdr(px)) => px.first().is_some_and(|p| p.iter().any(|&c| c > 0.0)),
        Err(_) => false,
    }
}

/// The cube-related issues of a DDS read at `game_path`, in a fixed order.
pub fn inspect_issues(game_path: &str, f: &DdsFile) -> Vec<String> {
    let mut v = Vec::new();
    if !f.is_cubemap {
        if expects_cube(game_path) {
            v.push("2D texture where a cubemap is expected (the game reads _e and textures\\cubemaps\\ files as cubes)".into());
        }
        return v;
    }
    if f.mip_count == 1 && f.width > 1 {
        v.push(
            "cube has no mips: curved surfaces will sparkle; re-encode it with a full chain".into(),
        );
    }
    if f.width == 1 && f.height == 1 && pos_y_is_lit(f) {
        v.push("1x1 cube with a non-black +Y: Community Shaders treats it as a static, near-black reflection, not a dynamic-cubemap marker".into());
    }
    if is_srgb(f.format) {
        v.push("cube uses an sRGB format; Community Shaders expects UNORM cubes".into());
    }
    v
}
