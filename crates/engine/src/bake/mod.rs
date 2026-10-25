//! Normal-variance bake (spec §6.2, DECISIONS #6, Note R): a normal map's footprint length L → κ = (3L − L³)/(1 − L²)
//! → v = 0.25/κ → α′² = saturate(α² + min(2v, t²)), r′ = (α′²)^¼ [MIP, Unity HDRP `CommonMaterial.hlsl`]. The
//! footprint lengths come from a box pyramid of the `_n`'s mip 0 as written: unit length unless `mips.normal_base = keep`
//! (DECISIONS R1, R5).
pub mod plan;

use crate::config::{MipKeyword, MipLevels, Profile};
use crate::image::Rgba32F;
use crate::policy::SourceStamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The 8-bit noise floor (owner, 2026-10-11, DECISIONS Note R M4-final-2): a footprint whose average normal is at
/// least this long adds no variance, so a visually flat `_n` with quantisation noise keeps glossy roughness glossy
/// while real bumps still widen it. Measured (golden `a_flat_but_noisy_normal_keeps_mirror_roughness_at_every_mip`,
/// `a_tilt_just_above_the_noise_floor_still_widens`): ±2-code noise on X and Y leaves every footprint within about
/// 1e-4 of unit length (it lifted r = 0 to 0.10 at mip 1 without the floor), and the worst case for ±k-code noise
/// is 1 − cos(θ) with θ ≈ k·(2/255)·√2, under 1e-3 up to k = 4. The bumpy golden normals sit far below the floor.
/// The cost is that a deliberate tilt of up to about ±2.2° (X ±5 codes, L = 0.99923) also adds nothing; ±6 codes
/// (L = 0.99889) widens. Applies to the `_n` → roughness bake and the `_cnr` self-bake alike.
pub const NOISE_FLOOR_L: f64 = 0.999;

/// `min(2v, t²)` for a footprint whose average normal has length `len` (spec §6.2 steps 2 and 4): 0 at or above
/// the noise floor [`NOISE_FLOOR_L`] (or NaN), `t²` for a zero-length one. Computed in f64.
pub fn variance_add(len: f32, clamp_t: f64) -> f32 {
    let l = f64::from(len);
    let cap = clamp_t * clamp_t;
    if l.is_nan() || l >= NOISE_FLOOR_L {
        return 0.0;
    }
    if l <= 0.0 {
        return cap as f32;
    }
    let kappa = (3.0 * l - l * l * l) / (1.0 - l * l);
    let v = 0.25 / kappa;
    (2.0 * v).min(cap) as f32
}

/// r′ = (min(α² + add, 1))^¼ from the footprint mean `mean_alpha` of α = r² (spec §6.2 steps 3–4). With nothing to
/// add it is √α, the plain chain's value.
pub fn baked_roughness(mean_alpha: f32, add: f32) -> f32 {
    let a = mean_alpha.max(0.0);
    if add <= 0.0 {
        a.sqrt()
    } else {
        (a * a + add).min(1.0).powf(0.25)
    }
}

/// log2 of the `_n` : roughness-member size ratio (1 for a 2:1 `_n`, −1 for a half-size one), or why the set does
/// not bake (`SET-BAKE-RATIO`, DECISIONS R5). Both bases must be powers of two with one aspect, so every footprint
/// is whole texels of one pyramid level.
pub fn bake_shift(normal: (u32, u32), member: (u32, u32)) -> Result<i32, String> {
    let pot = |(w, h): (u32, u32)| w.is_power_of_two() && h.is_power_of_two();
    if !pot(normal) || !pot(member) {
        return Err(format!(
            "_n {}x{} and member {}x{} must both be powers of two",
            normal.0, normal.1, member.0, member.1
        ));
    }
    let log = |v: u32| v.trailing_zeros() as i32;
    let (sx, sy) = (log(normal.0) - log(member.0), log(normal.1) - log(member.1));
    if sx != sy {
        return Err(format!(
            "_n {}x{} and member {}x{} differ in aspect, so their size ratio is not one power of two",
            normal.0, normal.1, member.0, member.1
        ));
    }
    Ok(sx)
}

/// One pyramid level: the length of the average base normal over each texel's footprint.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthLevel {
    pub width: u32,
    pub height: u32,
    pub len: Vec<f32>,
}

/// Footprint lengths of a normal map for every level >= 1 of its full box chain (level `m` at index `m − 1`), from
/// the `_n`'s mip 0 as written (the caller renormalises it unless `keep`; DECISIONS R1, R5). About 1.33 bytes per base texel.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthPyramid {
    /// Base width and height.
    pub base: (u32, u32),
    pub levels: Vec<LengthLevel>,
}

impl LengthPyramid {
    /// Box-filter `base` (a `Normal`-domain image) down to 1x1, keeping only each level's lengths. Only one
    /// accumulation level is alive at a time.
    pub fn build(base: &Rgba32F) -> LengthPyramid {
        let full = crate::mips::level_count(
            base.width,
            base.height,
            &MipLevels::Keyword(MipKeyword::Full),
        );
        let mut levels = Vec::with_capacity(full.saturating_sub(1) as usize);
        let mut acc: Option<Rgba32F> = None;
        for _ in 1..full {
            let next = crate::mips::box_down(acc.as_ref().unwrap_or(base));
            levels.push(LengthLevel {
                width: next.width,
                height: next.height,
                len: next
                    .px
                    .iter()
                    .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
                    .collect(),
            });
            acc = Some(next);
        }
        LengthPyramid {
            base: (base.width, base.height),
            levels,
        }
    }

    /// Level `m` (>= 1), or `None` for 0 or past the last level.
    pub fn level(&self, m: usize) -> Option<&LengthLevel> {
        m.checked_sub(1).and_then(|i| self.levels.get(i))
    }
}

/// The bake of one roughness member: its `_n`'s footprint lengths, the level shift and the clamp `t`.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalVariance {
    lengths: LengthPyramid,
    shift: i32,
    clamp_t: f64,
}

impl NormalVariance {
    /// For a member whose base is `member` (width, height); an error when the sizes cannot bake ([`bake_shift`]).
    pub fn new(
        lengths: LengthPyramid,
        member: (u32, u32),
        clamp_t: f64,
    ) -> Result<NormalVariance, String> {
        let shift = bake_shift(lengths.base, member)?;
        Ok(NormalVariance {
            lengths,
            shift,
            clamp_t,
        })
    }

    pub fn shift(&self) -> i32 {
        self.shift
    }

    /// `min(2v, t²)` for texel (x, y) of the member's mip `level`; 0 where the footprint covers at most one `_n`
    /// texel (`level + shift <= 0`).
    pub fn add_at(&self, level: usize, x: u32, y: u32) -> f32 {
        let m = level as i64 + i64::from(self.shift);
        if m < 1 {
            return 0.0;
        }
        match self.lengths.level(m as usize) {
            Some(l) if x < l.width && y < l.height => {
                variance_add(l.len[(y * l.width + x) as usize], self.clamp_t)
            }
            _ => 0.0,
        }
    }
}

/// How a roughness member bakes its set's `_n` (`ResolvedJob::bake`, spec §6.2). Set by `plan::plan_bakes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BakeJob {
    /// The set's `pbr_normal` source.
    pub normal: PathBuf,
    /// That `_n`'s resolved profile: the member decodes the `_n` with it exactly as the `_n`'s own job does
    /// (swizzle, green flip, Z rebuild; DECISIONS R4).
    pub normal_profile: Profile,
    /// log2(`_n` size / member size) ([`bake_shift`]) as planned.
    pub shift: i32,
    /// The `_n`'s size and modification time when `Engine::plan` read it (final review I7); `None` is not checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<SourceStamp>,
    /// The `_n` job's active RNM blend at plan time (Note W18, owner 2026-10-12): the lengths come from the `_n` as its
    /// own encode writes it, after the layers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rnm: Option<crate::rnm::RnmJob>,
}

impl BakeJob {
    /// What the member's output depends on besides the `_n`'s bytes: the decode settings, how mip 0 is written, and
    /// the level shift (DECISIONS R12). Recorded in the manifest so `if-changed` re-encodes when any changes; the
    /// `_n`'s path and its own output settings (format, effort) are not part of it.
    /// The `_n`'s `resize` joins the tuple only when set, so manifests written before it stay up to date (S-mips-7).
    pub fn identity_sha256(&self) -> String {
        let np = &self.normal_profile;
        let base = (
            &np.channels,
            np.alpha.policy,
            np.mips.normal_base,
            self.shift,
        );
        // The RNM settings join only when layers apply (Note W18), so layer-free manifests stay up to date.
        let resized = np.resize != crate::config::ResizeMode::None;
        match (resized, self.rnm.is_some()) {
            (false, false) => crate::manifest::sha256_json(&base),
            (true, false) => {
                crate::manifest::sha256_json(&(base.0, base.1, base.2, base.3, np.resize))
            }
            (false, true) => {
                crate::manifest::sha256_json(&(base.0, base.1, base.2, base.3, &np.rnm))
            }
            (true, true) => {
                crate::manifest::sha256_json(&(base.0, base.1, base.2, base.3, np.resize, &np.rnm))
            }
        }
        .expect("a tuple of plain config values serialises")
    }

    /// Decode the `_n` as its own job does (spec §5.4), write its mip 0 as that job does (unit length unless
    /// `mips.normal_base = keep`, DECISIONS R1) and keep only the footprint lengths of that written base. The image is
    /// dropped before this returns (DECISIONS R4).
    pub fn load_lengths(&self) -> Result<LengthPyramid, String> {
        let (img, info) = crate::load::load_image(&self.normal).map_err(|e| e.to_string())?;
        lengths_from(
            img,
            crate::ops::SourceTransfer::of(&info),
            &self.normal_profile,
            self.rnm.as_ref(),
        )
    }

    /// Peak bytes the bake holds, for `--mem-budget` admission (DECISIONS R4, Task 7 review I2): the `_n`'s
    /// [`decode_peak`], plus (Note BB) its RNM layers' [`crate::rnm::RnmJob::admission_bytes`] at the `_n`'s
    /// preprocessed size, which `load_lengths` blends into. 0 when the header cannot be read (the load then fails
    /// with its own error).
    pub fn estimated_bytes(&self) -> u64 {
        let layers = || {
            let r = self.rnm.as_ref()?;
            let dims = crate::load::image_size(&self.normal).ok()?;
            let np = &self.normal_profile;
            Some(r.admission_bytes(&np.rnm, crate::resize::target_dims(dims, np.resize)))
        };
        decode_peak(&self.normal) + layers().unwrap_or(0)
    }

    /// Peak bytes of baking a `w`×`h` `_n` whose decoded texels are `decoded` bytes wide. The peak is inside
    /// `load::load_image`, which holds three buffers at once: the decoded image (`decoded`: 3 for 8-bit RGB, 8 for
    /// 16-bit RGBA), its `to_rgba32f` copy (16) and the collected `Vec` (16). Everything after is smaller:
    /// `preprocess` and the renormalise work in place on the 16-byte base, and the pyramid build adds one
    /// quarter-size accumulation level (4), the next one (1) and the lengths (1.33), about 22.4 per texel.
    /// A DDS input (M6) instead holds its file data (every mip) and the decoded RGBA32F mip 0 at once: ~32 per texel
    /// for a float DDS; not modelled here.
    pub fn peak_bytes(w: u32, h: u32, decoded: u64) -> u64 {
        u64::from(w) * u64::from(h) * (decoded + 16 + 16)
    }
}

/// [`BakeJob::load_lengths`] on an image already decoded: preprocess `img` with the `_n`'s `normal_profile`, blend
/// its `rnm` layers, write mip 0 as the `_n`'s job does (unit length unless `mips.normal_base = keep`) and keep only
/// the footprint lengths. The image is dropped before this returns.
pub fn lengths_from(
    img: Rgba32F,
    transfer: crate::ops::SourceTransfer,
    normal_profile: &Profile,
    rnm: Option<&crate::rnm::RnmJob>,
) -> Result<LengthPyramid, String> {
    let mut work = crate::ops::preprocess_with(img, normal_profile, transfer);
    if let Some(r) = rnm {
        crate::rnm::blend::apply(&mut work.img, r, normal_profile)?;
    }
    if normal_profile.mips.normal_base == crate::config::NormalBase::Renormalize {
        for px in &mut work.img.px {
            crate::mips::normalize_vector(px, work.domain);
        }
    }
    Ok(LengthPyramid::build(&work.img))
}

/// `load::load_image`'s peak on `path`, from its header: [`BakeJob::peak_bytes`] of its size and decoded texel width.
/// 0 when the header cannot be read.
pub(crate) fn decode_peak(path: &std::path::Path) -> u64 {
    use ::image::ImageDecoder;
    let header = || {
        let mut r = ::image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?;
        crate::load::lift_limits(&mut r);
        let d = r.into_decoder().ok()?;
        let (w, h) = d.dimensions();
        // A hostile header must not overflow the product; the load refuses a file this large anyway (review M1).
        if w.max(h) > crate::load::MAX_DIM {
            return None;
        }
        Some(BakeJob::peak_bytes(
            w,
            h,
            u64::from(d.color_type().bytes_per_pixel()),
        ))
    };
    // A PSD decodes straight to float RGBA (`psd::load`), with no decoded buffer of its own.
    header()
        .or_else(|| {
            let (w, h) = crate::load::image_size(path).ok()?;
            Some(BakeJob::peak_bytes(w, h, 0))
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Format, NormalBase, Profile, Subtype, Target};
    use crate::mips::{build_chain, build_chain_with};
    use crate::ops::preprocess;
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, st)
            .unwrap()
    }

    /// An encoded normal map whose unit-length texels change direction from texel to texel.
    fn bumpy(w: u32) -> Rgba32F {
        let mut img = Rgba32F::new(w, w, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..w {
            for x in 0..w {
                let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7).sin();
                let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
                let nz = (1.0 - nx * nx - ny * ny).sqrt();
                *img.at_mut(x, y) = [nx * 0.5 + 0.5, ny * 0.5 + 0.5, nz * 0.5 + 0.5, 1.0];
            }
        }
        img
    }

    /// A roughness member: R a ramp in [0.15, 0.85), G/B/A constant.
    fn ramp(w: u32) -> Rgba32F {
        let mut img = Rgba32F::new(w, w, [0.0, 0.0, 0.8, 1.0]);
        for y in 0..w {
            for x in 0..w {
                img.at_mut(x, y)[0] = 0.15 + 0.7 * ((x * 3 + y * 2) % w) as f32 / w as f32;
            }
        }
        img
    }

    /// Spec §6.2 in f64 (test code only, Global Constraints): the stored R of member texel (x, y) at `level`, with
    /// the `_n` `shift` levels larger (negative: smaller). Square images.
    fn reference(
        normals: &Rgba32F,
        rough: &Rgba32F,
        level: u32,
        shift: i32,
        x: u32,
        y: u32,
        t: f64,
    ) -> f64 {
        let f = 1u32 << level;
        let mut alpha = 0.0f64;
        for j in 0..f {
            for i in 0..f {
                let r = f64::from(rough.at(x * f + i, y * f + j)[0]);
                alpha += r * r;
            }
        }
        alpha /= f64::from(f * f);
        let m = level as i32 + shift;
        let add = if m < 1 {
            0.0
        } else {
            let g = 1u32 << m;
            let mut s = [0.0f64; 3];
            for j in 0..g {
                for i in 0..g {
                    let q = normals.at(x * g + i, y * g + j);
                    for c in 0..3 {
                        s[c] += f64::from(q[c]);
                    }
                }
            }
            let n = f64::from(g * g);
            let l = ((s[0] / n).powi(2) + (s[1] / n).powi(2) + (s[2] / n).powi(2)).sqrt();
            if l >= 0.999 {
                0.0
            } else if l <= 0.0 {
                t * t
            } else {
                // 2v = 2 * 0.25 / κ = 0.5 (1 − L²) / (3L − L³)
                (0.5 * (1.0 - l * l) / (3.0 * l - l * l * l)).min(t * t)
            }
        };
        if add == 0.0 {
            alpha.sqrt()
        } else {
            (alpha * alpha + add).min(1.0).powf(0.25)
        }
    }

    /// Mip 0 of a `_n` as its job writes it under the default `normal_base = renormalize` (DECISIONS R1).
    fn written(w: crate::ops::Work) -> Rgba32F {
        let mut img = w.img;
        for px in &mut img.px {
            crate::mips::normalize_vector(px, w.domain);
        }
        img
    }

    fn close(a: f32, b: f64) -> bool {
        (f64::from(a) - b).abs() < 1e-6
    }

    #[test]
    fn variance_add_follows_the_formula_and_the_clamp() {
        assert_eq!(variance_add(1.0, 0.25), 0.0);
        assert_eq!(variance_add(1.2, 0.25), 0.0); // longer than unit: no variance
        assert_eq!(variance_add(f32::NAN, 0.25), 0.0);
        // The 8-bit noise floor (DECISIONS Note R M4-final-2): nothing at or above L = 0.999, the formula below it.
        assert_eq!(variance_add(0.9995, 0.25), 0.0);
        assert_eq!(variance_add(0.999, 0.25), 0.0);
        assert!(variance_add(0.9985, 0.25) > 0.0);
        assert!(close(variance_add(0.0, 0.25), 0.0625)); // t²
                                                         // L = 0.9: κ = (2.7 − 0.729) / 0.19 = 10.3737, 2v = 0.0482 < t².
        let l = 0.9f64;
        let want = 0.5 * (1.0 - l * l) / (3.0 * l - l * l * l);
        assert!(close(variance_add(0.9, 0.25), want), "{want}");
        // L = 0.5: 2v = 0.27, clamped to t² = 0.0625; a wider clamp lets more through.
        assert!(close(variance_add(0.5, 0.25), 0.0625));
        assert!(close(variance_add(0.5, 1.0), 0.5 * 0.75 / 1.375));
    }

    #[test]
    fn baked_roughness_is_the_plain_sqrt_without_variance_and_saturates() {
        assert_eq!(baked_roughness(0.25, 0.0), 0.5);
        assert!((baked_roughness(0.25, 0.0625) - (0.0625f32 + 0.0625).powf(0.25)).abs() < 1e-6);
        assert!((baked_roughness(0.9, 0.0625) - (0.9f32 * 0.9 + 0.0625).powf(0.25)).abs() < 1e-6);
        assert_eq!(baked_roughness(1.0, 0.0625), 1.0); // saturate
    }

    #[test]
    fn bake_shift_needs_power_of_two_sizes_with_one_aspect() {
        assert_eq!(bake_shift((128, 128), (64, 64)), Ok(1));
        assert_eq!(bake_shift((32, 32), (64, 64)), Ok(-1));
        assert_eq!(bake_shift((64, 64), (64, 64)), Ok(0));
        assert_eq!(bake_shift((2048, 1024), (1024, 512)), Ok(1));
        assert!(bake_shift((64, 64), (64, 32)).is_err()); // aspect mismatch
        assert!(bake_shift((96, 96), (48, 48)).is_err()); // NPOT
    }

    #[test]
    fn the_pyramid_holds_footprint_lengths_of_the_base_it_is_given() {
        // (0.6, 0, 0.8) and (-0.6, 0, 0.8) alternate in x: every level-1 footprint averages to (0, 0, 0.8).
        let mut base = Rgba32F::new(4, 2, [0.6, 0.0, 0.8, 1.0]);
        for y in 0..2 {
            for x in (1..4).step_by(2) {
                base.at_mut(x, y)[0] = -0.6;
            }
        }
        let p = LengthPyramid::build(&base);
        assert_eq!(p.base, (4, 2));
        assert_eq!(p.levels.len(), 2); // 2x1 and 1x1
        let l1 = p.level(1).unwrap();
        assert_eq!((l1.width, l1.height), (2, 1));
        assert!(
            l1.len.iter().all(|&v| (v - 0.8).abs() < 1e-6),
            "{:?}",
            l1.len
        );
        assert!(p.level(0).is_none() && p.level(3).is_none());
    }

    #[test]
    fn normal_variance_rejects_a_member_whose_size_does_not_line_up() {
        let flat = Rgba32F::new(16, 16, [0.0, 0.0, 1.0, 1.0]);
        assert!(NormalVariance::new(LengthPyramid::build(&flat), (16, 8), 0.25).is_err());
        assert!(NormalVariance::new(LengthPyramid::build(&flat), (8, 8), 0.25).is_ok());
    }

    #[test]
    fn roughness_chain_matches_the_f64_reference_at_1_to_1_2_to_1_and_1_to_2() {
        for (nw, mw, shift) in [(16u32, 16u32, 0i32), (32, 16, 1), (8, 16, -1)] {
            let normals = written(preprocess(bumpy(nw), &profile(Subtype::PbrNormal)));
            let pr = profile(Subtype::Rmaos);
            let rough = preprocess(ramp(mw), &pr);
            let nv = NormalVariance::new(LengthPyramid::build(&normals), (mw, mw), 0.25).unwrap();
            assert_eq!(nv.shift(), shift);
            let baked = build_chain_with(&rough, &pr, Some(&nv));
            let plain = build_chain(&rough, &pr);
            let mut widened = 0;
            for (level, (l, q)) in baked.iter().zip(&plain).enumerate() {
                for y in 0..l.height {
                    for x in 0..l.width {
                        let want = reference(&normals, &rough.img, level as u32, shift, x, y, 0.25);
                        let got = f64::from(l.at(x, y)[0]);
                        assert!(
                            (got - want).abs() < 1e-4,
                            "{nw}:{mw} level {level} ({x},{y}): {got} vs {want}"
                        );
                        if l.at(x, y)[0] > q.at(x, y)[0] + 1e-3 {
                            widened += 1;
                        }
                        // G, B and A are never touched by the bake.
                        assert_eq!(&l.at(x, y)[1..], &q.at(x, y)[1..]);
                    }
                }
            }
            assert!(widened > 0, "{nw}:{mw}: the bake must widen something");
        }
    }

    #[test]
    fn a_flat_normal_map_leaves_the_roughness_chain_bit_identical() {
        let flat = preprocess(
            Rgba32F::new(32, 32, [0.5, 0.5, 1.0, 1.0]),
            &profile(Subtype::PbrNormal),
        )
        .img;
        let pr = profile(Subtype::Rmaos);
        let rough = preprocess(ramp(16), &pr);
        let nv = NormalVariance::new(LengthPyramid::build(&flat), (16, 16), 0.25).unwrap();
        assert_eq!(
            build_chain_with(&rough, &pr, Some(&nv)),
            build_chain(&rough, &pr)
        );
    }

    #[test]
    fn a_bake_given_to_a_non_roughness_chain_is_ignored() {
        let normals = written(preprocess(bumpy(16), &profile(Subtype::PbrNormal)));
        let pn = profile(Subtype::PbrNormal);
        let w = preprocess(bumpy(16), &pn);
        let nv = NormalVariance::new(LengthPyramid::build(&normals), (16, 16), 0.25).unwrap();
        assert_eq!(build_chain_with(&w, &pn, Some(&nv)), build_chain(&w, &pn));
    }

    #[test]
    fn a_bake_is_admitted_for_load_images_decode_peak() {
        let d = tempfile::tempdir().unwrap();
        let job = |n: std::path::PathBuf| BakeJob {
            normal: n,
            normal_profile: profile(Subtype::PbrNormal),
            shift: 0,
            stamp: None,
            rnm: None,
        };
        // 8-bit RGB: the decoded image (3) + `to_rgba32f` (16) + the collected `Vec` (16) per texel.
        let rgb8 = d.path().join("rgb8_n.png");
        ::image::RgbImage::new(64, 32).save(&rgb8).unwrap();
        assert_eq!(job(rgb8).estimated_bytes(), 64 * 32 * (3 + 16 + 16));
        // 16-bit RGBA: 8 + 16 + 16.
        let rgba16 = d.path().join("rgba16_n.png");
        ::image::ImageBuffer::<::image::Rgba<u16>, _>::new(4, 4)
            .save(&rgba16)
            .unwrap();
        assert_eq!(job(rgba16).estimated_bytes(), 4 * 4 * (8 + 16 + 16));
        assert_eq!(job(d.path().join("missing_n.png")).estimated_bytes(), 0);
    }

    /// Note BB: the bake blends the `_n`'s layers too, so it adds the largest layer's decode and its resample to
    /// the `_n` size.
    #[test]
    fn a_bake_with_rnm_layers_is_admitted_for_its_largest_layer() {
        let d = tempfile::tempdir().unwrap();
        let n = d.path().join("a_n.png");
        ::image::RgbImage::new(64, 32).save(&n).unwrap();
        let layer = |name: &str, w: u32| {
            let p = d.path().join(name);
            ::image::RgbImage::new(w, w / 2).save(&p).unwrap();
            crate::rnm::RnmLayer {
                id: crate::config::LayerId::Detail,
                file: crate::rnm::RnmFile {
                    path: p,
                    stamp: None,
                },
                mask: None,
            }
        };
        let mut job = BakeJob {
            normal: n,
            normal_profile: profile(Subtype::PbrNormal),
            shift: 0,
            stamp: None,
            rnm: None,
        };
        let base = job.estimated_bytes();
        let mut small = layer("small.png", 32);
        small.id = crate::config::LayerId::Bump;
        job.rnm = Some(crate::rnm::RnmJob {
            layers: vec![small, layer("big.png", 256)],
            ao: None,
        });
        assert_eq!(job.estimated_bytes(), base + 256 * 128 * 35 + 64 * 32 * 16);
    }

    #[test]
    fn the_bake_identity_follows_decode_settings_and_shift_only() {
        let b = BakeJob {
            normal: "a/iron_n.png".into(),
            normal_profile: profile(Subtype::PbrNormal),
            shift: 0,
            stamp: None,
            rnm: None,
        };
        let mut moved = b.clone();
        moved.normal = "b/iron_n.png".into();
        let mut format = b.clone();
        format.normal_profile.format = Format::Bc1;
        let mut flip = b.clone();
        flip.normal_profile.channels.flip_green = true;
        let mut shift = b.clone();
        shift.shift = 1;
        let mut keep = b.clone();
        keep.normal_profile.mips.normal_base = NormalBase::Keep;
        assert_eq!(b.identity_sha256(), moved.identity_sha256());
        assert_eq!(b.identity_sha256(), format.identity_sha256());
        assert_ne!(b.identity_sha256(), flip.identity_sha256());
        assert_ne!(b.identity_sha256(), shift.identity_sha256());
        assert_ne!(b.identity_sha256(), keep.identity_sha256());
    }

    /// Note W18: RNM settings join the identity only when the bake carries a blend.
    #[test]
    fn identity_includes_rnm_only_when_layers_apply() {
        let b = BakeJob {
            normal: "a/iron_n.png".into(),
            normal_profile: profile(Subtype::PbrNormal),
            shift: 0,
            stamp: None,
            rnm: None,
        };
        let mut layered = b.clone();
        layered.rnm = Some(crate::rnm::RnmJob {
            layers: vec![],
            ao: None,
        });
        let mut strong = layered.clone();
        strong.normal_profile.rnm.detail.strength = Some(0.5);
        assert_ne!(b.identity_sha256(), layered.identity_sha256());
        assert_ne!(layered.identity_sha256(), strong.identity_sha256());
        let mut off = b.clone();
        off.normal_profile.rnm.detail.strength = Some(0.5);
        assert_eq!(b.identity_sha256(), off.identity_sha256());
    }

    /// S-mips-7: existing manifests stay up to date without resize; `pot-down` changes the identity.
    #[test]
    fn identity_is_unchanged_without_resize() {
        let b = BakeJob {
            normal: "a/iron_n.png".into(),
            normal_profile: profile(Subtype::PbrNormal),
            shift: 0,
            stamp: None,
            rnm: None,
        };
        let np = &b.normal_profile;
        let four = crate::manifest::sha256_json(&(
            &np.channels,
            np.alpha.policy,
            np.mips.normal_base,
            b.shift,
        ))
        .unwrap();
        assert_eq!(b.identity_sha256(), four);
        let mut resized = b.clone();
        resized.normal_profile.resize = crate::config::ResizeMode::PotDown;
        assert_ne!(resized.identity_sha256(), four);
    }
}
