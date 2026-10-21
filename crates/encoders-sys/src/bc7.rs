use crate::{ensure_init, ffi};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bc7Level {
    Ultrafast = 0,
    Veryfast = 1,
    Fast = 2,
    Basic = 3,
    Slow = 4,
    Veryslow = 5,
    Slowest = 6,
    /// sdc patch 0002: veryslow's search plus a wider, fully refined partition ranking, uber 4, two least-squares
    /// passes and a quantised endpoint search (docs/research/bc7e-tuning.md). Needs the patch-0002 objects.
    Max = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bc7Params {
    pub level: Bc7Level,
    pub perceptual: bool,
    /// Force mode-6 p-bits to 1 on all-255 blocks (needs the patched bc7e objects, Task 14).
    pub exact_opaque: bool,
}

/// True once the vendored bc7e objects carry patch 0001 (exact opaque alpha).
pub const BC7E_EXACT_OPAQUE_NATIVE: bool = cfg!(sdc_bc7e_patched);
/// True once the vendored bc7e objects carry patch 0002 (the `Max` level).
pub const BC7E_MAX_NATIVE: bool = cfg!(sdc_bc7e_patch2);

/// ISPC code path of the bc7e objects. The dispatcher picks the best one the CPU runs, and the paths do not
/// encode bit-identically, so a pinned output hash is only meaningful per target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bc7Target {
    Sse2 = 0,
    Sse4 = 1,
    Avx = 2,
    Avx2 = 3,
}

impl Bc7Target {
    pub const ALL: [Bc7Target; 4] = [Self::Sse2, Self::Sse4, Self::Avx, Self::Avx2];

    /// The target ISPC's dispatcher picks on this CPU.
    pub fn best() -> Bc7Target {
        *Self::ALL.iter().rev().find(|t| t.supported()).unwrap()
    }

    /// Whether this CPU can run the target (ISPC 1.31: sse4 is SSE4.2; avx2 also needs FMA, F16C, BMI1/2).
    pub fn supported(self) -> bool {
        match self {
            Self::Sse2 => true,
            Self::Sse4 => std::arch::is_x86_feature_detected!("sse4.2"),
            Self::Avx => std::arch::is_x86_feature_detected!("avx"),
            Self::Avx2 => {
                std::arch::is_x86_feature_detected!("avx2")
                    && std::arch::is_x86_feature_detected!("fma")
                    && std::arch::is_x86_feature_detected!("f16c")
                    && std::arch::is_x86_feature_detected!("bmi1")
                    && std::arch::is_x86_feature_detected!("bmi2")
            }
        }
    }
}

/// Compress `pixels.len() / 16` blocks. `pixels` holds 16 texels per block (see `gather_blocks`).
pub fn bc7e_compress_blocks(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params) {
    compress(pixels, out, p, -1);
}

/// `bc7e_compress_blocks` on one ISPC code path instead of the CPU dispatch (tests pin output per target).
/// The best supported target goes through the dispatcher, which already runs that path; forcing it would
/// re-run its table init under concurrent encodes. Panics when the CPU cannot run `target`.
pub fn bc7e_compress_blocks_on(
    pixels: &[[u8; 4]],
    out: &mut [u8],
    p: Bc7Params,
    target: Bc7Target,
) {
    assert!(
        target.supported(),
        "this CPU cannot run the {target:?} bc7e path"
    );
    let forced = if target == Bc7Target::best() {
        -1
    } else {
        target as i32
    };
    compress(pixels, out, p, forced);
}

fn compress(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params, target: i32) {
    assert_eq!(pixels.len() % 16, 0, "pixels must hold whole 4x4 blocks");
    let n = pixels.len() / 16;
    assert_eq!(out.len(), n * 16, "out must hold 16 bytes per block");
    assert!(
        !p.exact_opaque || BC7E_EXACT_OPAQUE_NATIVE,
        "exact_opaque needs the patched bc7e objects; use bc7_fixup::force_opaque_mode6 instead"
    );
    assert!(
        p.level != Bc7Level::Max || BC7E_MAX_NATIVE,
        "Bc7Level::Max needs the patch-0002 bc7e objects"
    );
    if n == 0 {
        return;
    }
    ensure_init();
    // Copy into u32/u64 buffers so the ISPC code sees naturally aligned data.
    let src: Vec<u32> = pixels.iter().map(|c| u32::from_le_bytes(*c)).collect();
    let mut dst = vec![0u64; n * 2];
    // SAFETY: `src` holds 16 * n texels and `dst` 2 * n u64 (16 bytes per block); `level` comes
    // from the closed `Bc7Level` enum (0..=7), which the shim indexes without a range check; a forced
    // `target` was checked against the CPU by `bc7e_compress_blocks_on`.
    unsafe {
        ffi::sdc_bc7e_compress(
            n as u32,
            dst.as_mut_ptr(),
            src.as_ptr(),
            p.level as i32,
            p.perceptual as i32,
            p.exact_opaque as i32,
            target,
        )
    };
    for (i, w) in dst.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
}

/// Per-field overrides of a level's bc7e parameters, for the sdc-bench BC7 tuning study (docs/research/bc7e-tuning.md).
/// The engine sets only `weights` (`encoder.bc7_weights`, DECISIONS Note CP). `None` keeps the level's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bc7Tune {
    /// Partitions given a full encode after the cheap estimate (modes 0, 1/3, 2 and 7 together).
    pub partitions_to_try: Option<u32>,
    /// bc7e "uber" refinement level, 0..=4.
    pub uber_level: Option<u32>,
    /// Least-squares refinement passes after the first fit.
    pub refinement_passes: Option<u32>,
    /// Try every p-bit combination instead of rounding each endpoint to its nearest one.
    pub pbit_search: Option<bool>,
    /// Linear per-channel error weights (used with `perceptual = false`).
    pub weights: Option<[u32; 4]>,
    /// Partitions the cheap estimate ranks for modes 1, 3 and 7 (max 64).
    pub max_partitions_2subset: Option<u32>,
    /// Partitions the cheap estimate ranks for mode 0 (max 16) and mode 2 (max 64).
    pub max_partitions_3subset: Option<(u32, u32)>,
    /// Patch 0002: greedy ±1 search over the quantised endpoints after the least-squares and uber stages.
    pub endpoint_search: Option<bool>,
    /// Patch 0002: full refinement while ranking partitions (upstream refines only the winner past two).
    pub refine_partitions: Option<bool>,
}

/// `bc7e_compress_blocks` with `tune` applied over the level's parameters.
pub fn bc7e_compress_blocks_tuned(pixels: &[[u8; 4]], out: &mut [u8], p: Bc7Params, tune: Bc7Tune) {
    assert_eq!(pixels.len() % 16, 0, "pixels must hold whole 4x4 blocks");
    let n = pixels.len() / 16;
    assert_eq!(out.len(), n * 16, "out must hold 16 bytes per block");
    assert!(
        !p.exact_opaque || BC7E_EXACT_OPAQUE_NATIVE,
        "exact_opaque needs the patched bc7e objects"
    );
    assert!(
        p.level != Bc7Level::Max || BC7E_MAX_NATIVE,
        "Bc7Level::Max needs the patch-0002 bc7e objects"
    );
    assert!(
        (tune.endpoint_search.is_none() && tune.refine_partitions.is_none()) || BC7E_MAX_NATIVE,
        "the patch-0002 overrides need the patch-0002 bc7e objects"
    );
    if n == 0 {
        return;
    }
    ensure_init();
    let o = |v: Option<u32>| v.map_or(-1, |x| x as i32);
    let w = tune.weights;
    let m3 = tune.max_partitions_3subset;
    let t: [i32; 14] = [
        o(tune.partitions_to_try),
        o(tune.uber_level),
        o(tune.refinement_passes),
        tune.pbit_search.map_or(-1, |b| b as i32),
        o(w.map(|w| w[0])),
        o(w.map(|w| w[1])),
        o(w.map(|w| w[2])),
        o(w.map(|w| w[3])),
        o(tune.max_partitions_2subset),
        o(m3.map(|m| m.0)),
        o(m3.map(|m| m.1)),
        tune.endpoint_search.map_or(-1, |b| b as i32),
        tune.refine_partitions.map_or(-1, |b| b as i32),
        -1,
    ];
    let src: Vec<u32> = pixels.iter().map(|c| u32::from_le_bytes(*c)).collect();
    let mut dst = vec![0u64; n * 2];
    // SAFETY: as in `bc7e_compress_blocks`; `t` is 14 ints, which is what the shim reads.
    unsafe {
        ffi::sdc_bc7e_compress_tuned(
            n as u32,
            dst.as_mut_ptr(),
            src.as_ptr(),
            p.level as i32,
            p.perceptual as i32,
            p.exact_opaque as i32,
            t.as_ptr(),
        )
    };
    for (i, w) in dst.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
}
