//! Complex grass (design `2026-10-12-complex-grass-design.md`): one image, height = 2 × width. The top half is a
//! diffuse with alpha transparency, the bottom half a normal map with specular in alpha. Community Shaders'
//! `RunGrass.hlsl` reads both halves from one view and treats a texture as complex when the decoded bottom-left texel
//! of mip 0 is a unit vector (`|length(rgb * 2 - 1) - 1| < ComplexGrassThreshold`); ENB is assumed to do the same
//! (owner ruling 2026-10-12).
use crate::analyse::code8;
use crate::image::Rgba32F;
use schemars::JsonSchema;
use serde::Serialize;

/// The owner's `ComplexGrassThreshold` (CS `SettingsUser.json`, "Grass Lighting"), design §2.
pub const CS_GRASS_THRESHOLD: f32 = 0.03;
/// What the encoded detection texel keeps below the threshold, so the file passes any threshold >= 0.02 (§3.6).
pub const GRASS_GUARD_MARGIN: f32 = 0.01;
/// `|L - 1|` below this is a unit-length normal (design §4, §5).
pub const GRASS_UNIT_TOL: f32 = 0.1;
/// Share of bottom-half texels that must be unit-length normals (design §4, §5).
pub const GRASS_UNIT_SHARE: f64 = 0.99;

/// What `analyse` records of an image whose height is twice its width (design §4, §5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct GrassFacts {
    /// Every top-half alpha code is 251 or more (DECISIONS Note L: 251..=254 is BC encode noise).
    pub top_opaque: bool,
    /// Share of top-half texels whose alpha code is <= 5 or >= 250: the diffuse cutout/blend proposal reads it.
    pub top_near_binary: f64,
    /// Share of bottom-half texels that decode to `|L - 1| < 0.1`.
    pub bottom_unit_share: f64,
    /// `|L - 1|` of the bottom-left texel `(0, h - 1)`, the one CS tests.
    pub corner_err: f32,
}

/// `|length(rgb * 2 - 1) - 1|` of stored values in [0, 1]; a non-finite input is infinitely far from unit length.
pub fn unit_err(rgb: [f32; 3]) -> f32 {
    let v = rgb.map(|c| c * 2.0 - 1.0);
    let e = ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs();
    if e.is_finite() {
        e
    } else {
        f32::INFINITY
    }
}

/// The facts of `img` when its height is twice its width; `None` for every other shape.
pub fn facts(img: &Rgba32F) -> Option<GrassFacts> {
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h != 2 * w {
        return None;
    }
    let (top, bottom) = img.px.split_at(w * w);
    let n = (w * w) as f64;
    let share = |half: &[[f32; 4]], keep: &dyn Fn(&[f32; 4]) -> bool| {
        half.iter().filter(|p| keep(p)).count() as f64 / n
    };
    let c = img.px[(h - 1) * w];
    Some(GrassFacts {
        top_opaque: top.iter().all(|p| code8(p[3]) >= 251),
        top_near_binary: share(top, &|p| {
            let a = code8(p[3]);
            a <= 5 || a >= 250
        }),
        bottom_unit_share: share(bottom, &|p| unit_err([p[0], p[1], p[2]]) < GRASS_UNIT_TOL),
        corner_err: unit_err([c[0], c[1], c[2]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_only_for_one_by_two() {
        assert!(facts(&Rgba32F::new(4, 4, [0.5; 4])).is_none());
        assert!(facts(&Rgba32F::new(8, 4, [0.5; 4])).is_none());
        let mut img = Rgba32F::new(4, 8, [0.5, 0.5, 1.0, 12.0 / 255.0]);
        for px in &mut img.px[..16] {
            *px = [0.3, 0.6, 0.2, 1.0];
        }
        img.px[1][3] = 0.0;
        img.px[2][3] = 0.5;
        let f = facts(&img).unwrap();
        assert!(!f.top_opaque);
        assert!((f.top_near_binary - 15.0 / 16.0).abs() < 1e-12);
        assert_eq!(f.bottom_unit_share, 1.0);
        assert!(f.corner_err < 0.01, "{}", f.corner_err);
    }

    /// The owner's example corner (127, 126, 255) has length 1.0001 (design §2).
    #[test]
    fn owner_corner_is_unit() {
        let e = unit_err([127.0 / 255.0, 126.0 / 255.0, 1.0]);
        assert!(e < 0.005, "{e}");
        assert_eq!(unit_err([0.5, 0.5, f32::NAN]), f32::INFINITY);
    }
}
