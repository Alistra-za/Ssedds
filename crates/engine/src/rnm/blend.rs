//! M6 Track F2 (spec §3, DECISIONS Note W1, W8, W10): blend RNM layers into a preprocessed base normal. The base is
//! DirectX tangent vectors in RGB (after its own `flip_green`); alpha (A) is never read or written, so a `_cnr`'s
//! coat roughness passes through unchanged. Layers load one at a time, are resampled to the base size, applied and
//! dropped.
use super::{RnmFile, RnmJob, RnmLayer};
use crate::config::{BentMode, LayerId, Profile};
use crate::image::Rgba32F;
use crate::load::load_image;
use crate::mips::area_resample;

type V3 = [f32; 3];

pub(crate) fn unit(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-8 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// A stored texel (0..1) as a tangent normal at strength `s` (rnm.py / DAZ): `t = (2c − 1)·s`,
/// `n = normalize(t.x, t.y, |t.z| + 1 − s)`; `flip` negates Y (an OpenGL source into DirectX).
pub fn decode(c: [f32; 4], s: f32, flip: bool) -> V3 {
    let y = (2.0 * c[1] - 1.0) * s;
    let t = [
        (2.0 * c[0] - 1.0) * s,
        if flip { -y } else { y },
        (2.0 * c[2] - 1.0) * s,
    ];
    unit([t[0], t[1], t[2].abs() + 1.0 - s])
}

/// Reoriented Normal Mapping (Barre-Brisebois & Hill): `detail` reoriented onto `base`.
pub fn rnm(base: V3, detail: V3) -> V3 {
    let t = [base[0], base[1], base[2] + 1.0];
    let u = [-detail[0], -detail[1], detail[2]];
    let k = (t[0] * u[0] + t[1] * u[1] + t[2] * u[2]) / t[2].max(1e-6);
    unit([t[0] * k - u[0], t[1] * k - u[1], t[2] * k - u[2]])
}

/// Note W8: bump height (mean RGB) to DirectX normals at the bump's own size. Iray's slope `k·(h[+1] − h[−1])` per
/// texel, +v up the image (OpenGL `(−k·du, −k·dv, 1)`), then Y negated for DirectX.
pub fn bump_normals(img: &Rgba32F, k: f32) -> Rgba32F {
    let (w, h) = (img.width as i64, img.height as i64);
    let hgt = |x: i64, y: i64| {
        let p = img.at(x.clamp(0, w - 1) as u32, y.clamp(0, h - 1) as u32);
        (p[0] + p[1] + p[2]) / 3.0
    };
    let mut out = Rgba32F::new(img.width, img.height, [0.0, 0.0, 1.0, 1.0]);
    for y in 0..h {
        for x in 0..w {
            let du = hgt(x + 1, y) - hgt(x - 1, y);
            let dv = hgt(x, y - 1) - hgt(x, y + 1);
            let n = unit([-k * du, k * dv, 1.0]);
            *out.at_mut(x as u32, y as u32) = [n[0], n[1], n[2], 1.0];
        }
    }
    out
}

/// Note W10: `img` at `size`: unchanged when equal, the area filter when no axis grows, else bilinear (texel
/// centres, clamped edges).
pub fn resample(img: Rgba32F, size: (u32, u32)) -> Rgba32F {
    if (img.width, img.height) == size {
        return img;
    }
    if img.width >= size.0 && img.height >= size.1 {
        return area_resample(&img, size, false);
    }
    let (sx, sy) = (
        img.width as f32 / size.0 as f32,
        img.height as f32 / size.1 as f32,
    );
    let axis = |o: u32, s: f32, n: u32| {
        let f = ((o as f32 + 0.5) * s - 0.5).clamp(0.0, (n - 1) as f32);
        let i = f.floor() as u32;
        (i, (i + 1).min(n - 1), f - i as f32)
    };
    let mut out = Rgba32F::new(size.0, size.1, [0.0; 4]);
    for y in 0..size.1 {
        let (y0, y1, ty) = axis(y, sy, img.height);
        for x in 0..size.0 {
            let (x0, x1, tx) = axis(x, sx, img.width);
            let (a, b, c, d) = (
                img.at(x0, y0),
                img.at(x1, y0),
                img.at(x0, y1),
                img.at(x1, y1),
            );
            let px = out.at_mut(x, y);
            for i in 0..4 {
                let top = a[i] + (b[i] - a[i]) * tx;
                let bot = c[i] + (d[i] - c[i]) * tx;
                px[i] = top + (bot - top) * ty;
            }
        }
    }
    out
}

/// rnm.py's bent weight: `strength · (1 − ao)^power · mask`.
fn bent_weight(strength: f32, power: f32, ao: f32, mask: f32) -> f32 {
    strength * (1.0 - ao.clamp(0.0, 1.0)).powf(power) * mask
}

fn load(f: &RnmFile) -> Result<Rgba32F, String> {
    load_image(&f.path)
        .map(|(img, _)| img)
        .map_err(|e| format!("{}: {e}", f.path.display()))
}

/// One channel of `f` at `size`, clamped to 0..1.
fn plane(f: &RnmFile, size: (u32, u32), channel: usize) -> Result<Vec<f32>, String> {
    Ok(resample(load(f)?, size)
        .px
        .iter()
        .map(|p| p[channel].clamp(0.0, 1.0))
        .collect())
}

/// Blend `job`'s layers into `work` in `p.rnm`'s order (F2 spec §3.1).
pub fn apply(work: &mut Rgba32F, job: &RnmJob, p: &Profile) -> Result<(), String> {
    let size = (work.width, work.height);
    let spec = &p.rnm;
    for px in &mut work.px {
        let n = unit([px[0], px[1], px[2]]);
        px[..3].copy_from_slice(&n);
    }
    for id in spec.effective_order() {
        let Some(l): Option<&RnmLayer> = job.layers.iter().find(|l| l.id == id) else {
            continue;
        };
        let v = spec.layer(id);
        let s = v.strength as f32;
        // A bump is height, already DirectX from `bump_normals`: its flip defaults off. Normal layers follow the base (W8).
        let bump = matches!(id, LayerId::Bump | LayerId::Bump2);
        let flip = v.flip_green.unwrap_or(!bump && p.channels.flip_green);
        let mask = match (&l.mask, v.mask) {
            (Some(m), true) => Some(plane(m, size, 0)?),
            _ => None,
        };
        let m = |i: usize| mask.as_ref().map_or(1.0, |v| v[i]);
        match id {
            LayerId::Bump | LayerId::Bump2 => {
                let n = resample(bump_normals(&load(&l.file)?, s), size);
                for (i, px) in work.px.iter_mut().enumerate() {
                    let q = n.px[i];
                    let mut d = unit([q[0], if flip { -q[1] } else { q[1] }, q[2]]);
                    if mask.is_some() {
                        // A bump mask scales the slope exactly (rnm.py `scale_slope`).
                        let z = d[2].max(1e-6);
                        d = unit([d[0] / z * m(i), d[1] / z * m(i), 1.0]);
                    }
                    let r = rnm([px[0], px[1], px[2]], d);
                    px[..3].copy_from_slice(&r);
                }
            }
            LayerId::Bent if spec.bent.mode == BentMode::AoBlend => {
                let src = resample(load(&l.file)?, size);
                let ao = match (&job.ao, spec.bent.ao) {
                    (Some(a), true) => Some(plane(&a.file, size, a.channel as usize)?),
                    _ => None,
                };
                let power = spec.ao_power() as f32;
                for (i, px) in work.px.iter_mut().enumerate() {
                    let b = decode(src.px[i], 1.0, flip);
                    let w = match &ao {
                        Some(a) => bent_weight(s, power, a[i], m(i)),
                        None => s * m(i),
                    };
                    let r = unit([
                        px[0] * (1.0 - w) + b[0] * w,
                        px[1] * (1.0 - w) + b[1] * w,
                        px[2] * (1.0 - w) + b[2] * w,
                    ]);
                    px[..3].copy_from_slice(&r);
                }
            }
            _ => {
                let src = resample(load(&l.file)?, size);
                for (i, px) in work.px.iter_mut().enumerate() {
                    let d = decode(src.px[i], s * m(i), flip);
                    let r = rnm([px[0], px[1], px[2]], d);
                    px[..3].copy_from_slice(&r);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Subtype, Target};
    use crate::presets::PresetRegistry;

    fn close(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < eps)
    }
    const FLAT: [f32; 4] = [0.5, 0.5, 1.0, 1.0];

    #[test]
    fn flat_detail_leaves_the_base() {
        let base = unit([0.3, -0.2, 0.9]);
        assert!(close(rnm(base, decode(FLAT, 0.6, false)), base, 1e-5));
    }

    #[test]
    fn flat_base_gives_the_detail() {
        let d = decode([0.7, 0.4, 0.9, 1.0], 1.0, false);
        assert!(close(rnm([0.0, 0.0, 1.0], d), d, 1e-5));
    }

    #[test]
    fn strength_zero_is_flat_and_flip_negates_y() {
        assert!(close(
            decode([0.9, 0.1, 0.6, 1.0], 0.0, false),
            [0.0, 0.0, 1.0],
            1e-6
        ));
        let a = decode([0.7, 0.3, 0.9, 1.0], 1.0, false);
        let b = decode([0.7, 0.3, 0.9, 1.0], 1.0, true);
        assert!(close(a, [b[0], -b[1], b[2]], 1e-6));
    }

    /// Review Focus 5: height rising toward the image top tilts the DirectX normal to +Y; rising to the right, to -X.
    #[test]
    fn bump_ramp_tilts_dx_plus_y() {
        let mut up = Rgba32F::new(8, 8, [0.0; 4]);
        let mut right = Rgba32F::new(8, 8, [0.0; 4]);
        for y in 0..8 {
            for x in 0..8 {
                let v = (7 - y) as f32 / 7.0;
                *up.at_mut(x, y) = [v, v, v, 1.0];
                let h = x as f32 / 7.0;
                *right.at_mut(x, y) = [h, h, h, 1.0];
            }
        }
        let n = bump_normals(&up, 1.0).at(4, 4);
        assert!(n[1] > 0.05 && n[0].abs() < 1e-6, "{n:?}");
        let n = bump_normals(&right, 1.0).at(4, 4);
        assert!(n[0] < -0.05 && n[1].abs() < 1e-6, "{n:?}");
    }

    /// Review Focus 4: any size to any size, both directions, no panic.
    #[test]
    fn resample_any_size() {
        let img = Rgba32F::new(96, 40, [0.25, 0.5, 0.75, 1.0]);
        let up = resample(img.clone(), (128, 128));
        assert_eq!((up.width, up.height), (128, 128));
        assert!((up.at(127, 127)[1] - 0.5).abs() < 1e-6);
        let down = resample(img.clone(), (7, 3));
        assert_eq!((down.width, down.height), (7, 3));
        let mixed = resample(img, (40, 96));
        assert_eq!((mixed.width, mixed.height), (40, 96));
    }

    /// rnm.py bent weight: strength * (1 - AO)^power.
    #[test]
    fn bent_weight_follows_the_occlusion() {
        for (ao, want) in [
            (1.0f32, 0.0f32),
            (0.5, 0.35 * 0.5f32.powf(1.5)),
            (0.0, 0.35),
        ] {
            assert!((bent_weight(0.35, 1.5, ao, 1.0) - want).abs() < 1e-6);
        }
        assert_eq!(bent_weight(0.35, 1.5, 0.0, 0.0), 0.0);
    }

    fn png(dir: &std::path::Path, name: &str, f: impl Fn(u32, u32) -> [u8; 4]) -> RnmFile {
        let path = dir.join(name);
        let mut im = image::RgbaImage::new(16, 16);
        for (x, y, p) in im.enumerate_pixels_mut() {
            *p = image::Rgba(f(x, y));
        }
        im.save(&path).unwrap();
        RnmFile { path, stamp: None }
    }

    fn job(id: LayerId, file: RnmFile) -> RnmJob {
        RnmJob {
            layers: vec![RnmLayer {
                id,
                file,
                mask: None,
            }],
            ao: None,
        }
    }

    #[test]
    fn apply_never_touches_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let f = png(dir.path(), "d.png", |x, y| {
            [(x * 16) as u8, (y * 16) as u8, 200, 255]
        });
        let p = PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, Subtype::CoatNormal)
            .unwrap();
        let mut work = Rgba32F::new(16, 16, [0.5, 0.5, 1.0, 1.0]);
        for (i, px) in work.px.iter_mut().enumerate() {
            px[3] = i as f32 / 255.0;
        }
        let before = work.clone();
        apply(&mut work, &job(LayerId::Detail, f), &p).unwrap();
        assert!(work
            .px
            .iter()
            .zip(&before.px)
            .all(|(a, b)| a[3].to_bits() == b[3].to_bits()));
        assert!(work
            .px
            .iter()
            .zip(&before.px)
            .any(|(a, b)| a[..3] != b[..3]));
    }

    /// Note W8: a bump is height, so the base's OpenGL flip must not reach it.
    #[test]
    fn apply_bump_ignores_base_flip() {
        let dir = tempfile::tempdir().unwrap();
        let f = png(dir.path(), "b.png", |_, y| {
            let v = (255 - y * 17) as u8;
            [v, v, v, 255]
        });
        let mut p = PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, Subtype::Normal)
            .unwrap();
        p.channels.flip_green = true;
        let mut work = Rgba32F::new(16, 16, [0.0, 0.0, 1.0, 1.0]);
        apply(&mut work, &job(LayerId::Bump, f), &p).unwrap();
        for y in 1..15 {
            for x in 1..15 {
                assert!(work.at(x, y)[1] > 0.0, "({x},{y}) {:?}", work.at(x, y));
            }
        }
    }
}
