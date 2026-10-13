//! Resize (spec §5.4 step 6, S-mips-6; M6 Track D T4): `resize = pot-down` takes each axis to the largest power of two that fits, `resize = half` halves both axes when the larger is above 512,
//! in the content's domain and with the mip filter. Alpha is always the area-exact box; RGB is the box too unless
//! [`mips::effective_filter`] gives tent, Mitchell or Kaiser. Roughness is resized on r², alpha-weighted colour on
//! area × alpha, exactly as the box chain does. Normals are not renormalised here (that is `base_level`'s job).
use crate::config::{MipFilter, Profile, ResizeMode};
use crate::mips;
use crate::ops::Work;

/// The larger side a `half` resize keeps (M6 Track D spec §2.2).
pub const HALF_FLOOR: u32 = 512;

/// The size `resize` makes of a `w`×`h` image: `pot-down` takes the largest power of two ≤ each axis; `half` halves
/// both axes (integer division, never below 1) when the larger one is above [`HALF_FLOOR`], so the shape never changes
/// and there is no power-of-two step (owner, 2026-10-12).
pub fn target_dims((w, h): (u32, u32), mode: ResizeMode) -> (u32, u32) {
    let prev_pow2 = |v: u32| 1u32 << (31 - v.max(1).leading_zeros());
    match mode {
        ResizeMode::None => (w, h),
        ResizeMode::PotDown => (prev_pow2(w), prev_pow2(h)),
        ResizeMode::Half if w.max(h) > HALF_FLOOR => ((w / 2).max(1), (h / 2).max(1)),
        ResizeMode::Half => (w, h),
    }
}

/// Resize `work.img` to [`target_dims`] (a no-op when the size already fits).
pub fn resize_work(work: &mut Work, p: &Profile) {
    let to = target_dims((work.img.width, work.img.height), p.resize);
    if to == (work.img.width, work.img.height) {
        return;
    }
    let weighted = mips::weighted_rgb(work, p);
    let filt = mips::effective_filter(work, p);
    if work.roughness_r_squared {
        for px in &mut work.img.px {
            px[0] *= px[0];
        }
    }
    // Alpha (and RGB for box) is the area-exact box; `filt` is never non-box while `weighted`.
    let mut out = mips::area_resample(&work.img, to, weighted);
    if filt != MipFilter::Box {
        let direct = crate::filter::resample_rgb(&work.img, to, filt, p.mips.edge);
        for (px, d) in out.px.iter_mut().zip(&direct.px) {
            px[..3].copy_from_slice(&d[..3]);
        }
    }
    for px in &mut out.px {
        if work.roughness_r_squared {
            px[0] = px[0].max(0.0).sqrt();
        }
        // Only the polyphase pass overshoots (M8): a box resize stays area-exact, so a float LDR source keeps any
        // value > 1 until Quantise, as without resize.
        if filt != MipFilter::Box {
            mips::clamp_rgb(px, work.domain);
        }
    }
    work.img = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::image::Rgba32F;
    use crate::mips::{area_resample, build_chain};
    use crate::ops::{preprocess, Domain};
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        let mut p = PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, st)
            .unwrap();
        p.resize = ResizeMode::PotDown;
        p
    }
    fn work(img: Rgba32F, domain: Domain, alpha: bool) -> Work {
        Work {
            img,
            alpha_present: alpha,
            domain,
            roughness_r_squared: false,
            nonfinite_samples: 0,
        }
    }
    fn close(a: f32, b: f64) -> bool {
        (f64::from(a) - b).abs() < 1e-6
    }
    fn rnd_image(w: u32, h: u32, seed: u32) -> Rgba32F {
        let mut seed = seed;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16_777_216.0
        };
        let mut img = Rgba32F::new(w, h, [0.0; 4]);
        for p in &mut img.px {
            *p = [rnd(), rnd(), rnd(), rnd()];
        }
        img
    }

    /// M6 Track D (owner, 2026-10-12): both sides halve together when the larger is above 512; the shape never changes.
    #[test]
    fn half_halves_both_sides_when_the_larger_is_above_512() {
        let t = |w, h| target_dims((w, h), ResizeMode::Half);
        assert_eq!(t(2048, 2048), (1024, 1024));
        assert_eq!(t(1024, 512), (512, 256));
        assert_eq!(t(512, 256), (512, 256));
        assert_eq!(t(512, 512), (512, 512));
        assert_eq!(t(513, 100), (256, 50));
        assert_eq!(t(1000, 600), (500, 300));
        assert_eq!(t(1024, 1), (512, 1));
        assert_eq!(t(64, 64), (64, 64));
    }

    #[test]
    fn pot_down_takes_the_largest_power_of_two_per_axis() {
        let t = |w, h| target_dims((w, h), ResizeMode::PotDown);
        assert_eq!(t(1000, 600), (512, 512));
        assert_eq!(t(1536, 1024), (1024, 1024));
        assert_eq!(t(48, 40), (32, 32));
        assert_eq!(t(1024, 3), (1024, 2));
        assert_eq!(t(1, 1), (1, 1));
        assert_eq!(target_dims((1000, 600), ResizeMode::None), (1000, 600));
    }

    #[test]
    fn box_resize_is_area_exact() {
        // 6x1 -> 4x1, data content: each output covers 1.5 source texels.
        let mut img = Rgba32F::new(6, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[2][0] = 1.0;
        img.px[3][0] = 1.0;
        let mut w = work(img, Domain::Stored, false);
        resize_work(&mut w, &profile(Subtype::GenericData));
        assert_eq!((w.img.width, w.img.height), (4, 1));
        // [0, 0 | 0.5 of 0] = 0; [0.5 of 0, 1] = 1/1.5; [1, 0.5 of 0] = 1/1.5; [0.5 of 0, 0] = 0.
        let want = [0.0, 2.0 / 3.0, 2.0 / 3.0, 0.0];
        for (px, e) in w.img.px.iter().zip(want) {
            assert!(close(px[0], e) && px[3] == 1.0, "{:?}", w.img.px);
        }
    }

    #[test]
    fn power_of_two_sources_are_untouched_bit_for_bit() {
        let img = rnd_image(8, 8, 11);
        let p = profile(Subtype::Diffuse);
        let mut none = p.clone();
        none.resize = ResizeMode::None;
        assert_eq!(preprocess(img.clone(), &p), preprocess(img, &none));
    }

    #[test]
    fn alpha_is_area_box_even_with_kaiser() {
        let img = rnd_image(12, 12, 3);
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Data;
        p.mips.filter = MipFilter::Kaiser;
        let mut w = work(img.clone(), Domain::LinearLight, true);
        assert_eq!(mips::effective_filter(&w, &p), MipFilter::Kaiser);
        resize_work(&mut w, &p);
        let area = area_resample(&img, (8, 8), false);
        assert_eq!((w.img.width, w.img.height), (8, 8));
        assert!(w.img.px.iter().zip(&area.px).all(|(a, b)| a[3] == b[3]));
        assert!(w.img.px.iter().zip(&area.px).any(|(a, b)| a[0] != b[0])); // RGB is Kaiser
    }

    #[test]
    fn kaiser_resize_honours_the_clamp_edge() {
        let img = rnd_image(12, 12, 5);
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Remove;
        p.mips.filter = MipFilter::Kaiser;
        p.mips.edge = MipEdge::Clamp;
        let mut w = work(img.clone(), Domain::LinearLight, false);
        resize_work(&mut w, &p);
        let clamp = crate::filter::resample_rgb(&img, (8, 8), MipFilter::Kaiser, MipEdge::Clamp);
        let wrap = crate::filter::resample_rgb(&img, (8, 8), MipFilter::Kaiser, MipEdge::Wrap);
        for (a, b) in w.img.px.iter().zip(&clamp.px) {
            for c in 0..3 {
                assert_eq!(a[c], b[c].clamp(0.0, 1.0));
            }
        }
        assert!(w
            .img
            .px
            .iter()
            .zip(&wrap.px)
            .any(|(a, b)| a[0] != b[0].clamp(0.0, 1.0)));
    }

    /// Task 4 review: an axis already a power of two is not filtered (one identity tap), so every output row is the
    /// 1-D Mitchell resample of its source row alone.
    #[test]
    fn mitchell_pot_down_leaves_a_power_of_two_axis_unfiltered() {
        let img = rnd_image(1000, 512, 7);
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Remove;
        p.mips.filter = MipFilter::Mitchell;
        let mut w = work(img.clone(), Domain::LinearLight, false);
        assert_eq!(mips::effective_filter(&w, &p), MipFilter::Mitchell);
        resize_work(&mut w, &p);
        assert_eq!((w.img.width, w.img.height), (512, 512));
        for y in 0..512 {
            let mut row = Rgba32F::new(1000, 1, [0.0; 4]);
            row.px.copy_from_slice(&img.px[y * 1000..(y + 1) * 1000]);
            let want =
                crate::filter::resample_rgb(&row, (512, 1), MipFilter::Mitchell, p.mips.edge);
            for (a, b) in w.img.px[y * 512..(y + 1) * 512].iter().zip(&want.px) {
                for c in 0..3 {
                    assert_eq!(a[c], b[c].clamp(0.0, 1.0), "row {y}");
                }
            }
        }
    }

    /// M8: the box resize is area-exact and does not clamp, so a float LDR value above 1 survives until Quantise.
    #[test]
    fn box_resize_keeps_linear_light_values_above_one() {
        let img = Rgba32F::new(6, 1, [1.5, 1.5, 1.5, 1.0]);
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Remove;
        let mut w = work(img, Domain::LinearLight, false);
        assert_eq!(mips::effective_filter(&w, &p), MipFilter::Box);
        resize_work(&mut w, &p);
        assert_eq!((w.img.width, w.img.height), (4, 1));
        assert!(w.img.px.iter().all(|px| px[0] > 1.0), "{:?}", w.img.px);
    }

    #[test]
    fn roughness_is_resized_on_r_squared() {
        // 3x1, R = [0, 1, 0] -> 2x1: each output covers texel 0 or 2 whole plus half of texel 1.
        let mut img = Rgba32F::new(3, 1, [0.0, 0.0, 0.0, 1.0]);
        img.px[1][0] = 1.0;
        let w = preprocess(img, &profile(Subtype::Rmaos));
        assert!(w.roughness_r_squared);
        assert_eq!((w.img.width, w.img.height), (2, 1));
        // mean r² = (1 * 0 + 0.5 * 1) / 1.5 = 1/3, R = √(1/3); the area mean of r would be 1/3.
        let want = (1.0f64 / 3.0).sqrt();
        assert!(
            close(w.img.px[0][0], want) && close(w.img.px[1][0], want),
            "{:?}",
            w.img.px
        );
    }

    #[test]
    fn weighted_colour_resize_ignores_invisible_colour() {
        // [red a=1, green a=0, blue a=1] -> 2x1: output 0 weights red by 1 * 1, green by 0.5 * 0.
        let mut img = Rgba32F::new(3, 1, [0.0; 4]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        img.px[1] = [0.0, 1.0, 0.0, 0.0];
        img.px[2] = [0.0, 0.0, 1.0, 1.0];
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Cutout;
        let mut w = work(img, Domain::LinearLight, true);
        resize_work(&mut w, &p);
        let (o0, o1) = (w.img.px[0], w.img.px[1]);
        assert!(
            close(o0[0], 1.0) && close(o0[1], 0.0) && close(o0[2], 0.0),
            "{o0:?}"
        );
        assert!(
            close(o1[0], 0.0) && close(o1[1], 0.0) && close(o1[2], 1.0),
            "{o1:?}"
        );
        assert!(close(o0[3], 1.0 / 1.5) && close(o1[3], 1.0 / 1.5));
    }

    #[test]
    fn resized_normals_are_unit_length_at_mip_0_by_default() {
        let mut img = Rgba32F::new(6, 6, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..6 {
            for x in 0..6 {
                let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7).sin();
                let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
                let nz = (1.0 - nx * nx - ny * ny).sqrt();
                *img.at_mut(x, y) = [nx * 0.5 + 0.5, ny * 0.5 + 0.5, nz * 0.5 + 0.5, 1.0];
            }
        }
        let p = profile(Subtype::Normal);
        let w = preprocess(img, &p);
        let c = build_chain(&w, &p);
        assert_eq!((c[0].width, c[0].height), (4, 4));
        for v in &c[0].px {
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "{len}");
        }
    }
}
