//! CS's glint noise map, CPU port of noisegen.cs.hlsl: checked against an independent computation (64-bit masked
//! hash arithmetic, f64 uniform, Acklam's inverse normal CDF in f64) and for its distribution.
use half::f16;
use render3d::glint_noise::{cell, noise_map, SIZE};

/// noisegen's WangHash + one xorshift step, written with u64 arithmetic masked to 32 bits.
fn hash64(mut s: u64) -> u64 {
    const M: u64 = 0xFFFF_FFFF;
    s = (s ^ 61) ^ (s >> 16);
    s = (s * 9) & M;
    s ^= s >> 4;
    s = (s * 0x27d4_eb2d) & M;
    s ^ (s >> 15)
}

fn step64(s: u64) -> u64 {
    const M: u64 = 0xFFFF_FFFF;
    let mut s = s;
    s = (s ^ (s << 13)) & M;
    s ^= s >> 17;
    (s ^ (s << 5)) & M
}

/// Acklam's inverse standard normal CDF (relative error < 1.2e-9).
#[allow(clippy::excessive_precision)]
fn inv_norm(p: f64) -> f64 {
    let a = [
        -3.969683028665376e+01,
        2.209460984245205e+02,
        -2.759285104469687e+02,
        1.383577518672690e+02,
        -3.066479806614716e+01,
        2.506628277459239e+00,
    ];
    let b = [
        -5.447609879822406e+01,
        1.615858368580409e+02,
        -1.556989798598866e+02,
        6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    let c = [
        -7.784894002430293e-03,
        -3.223964580411365e-01,
        -2.400758277161838e+00,
        -2.549732539343734e+00,
        4.374664141464968e+00,
        2.938163982698783e+00,
    ];
    let d = [
        7.784695709041462e-03,
        3.224671290700398e-01,
        2.445134137142996e+00,
        3.754408661907416e+00,
    ];
    let tail = |q: f64| {
        (((((c[0] * q + c[1]) * q + c[2]) * q + c[3]) * q + c[4]) * q + c[5])
            / ((((d[0] * q + d[1]) * q + d[2]) * q + d[3]) * q + 1.0)
    };
    if p < 0.02425 {
        tail((-2.0 * p.ln()).sqrt())
    } else if p > 1.0 - 0.02425 {
        -tail((-2.0 * (1.0 - p).ln()).sqrt())
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((a[0] * r + a[1]) * r + a[2]) * r + a[3]) * r + a[4]) * r + a[5]) * q
            / (((((b[0] * r + b[1]) * r + b[2]) * r + b[3]) * r + b[4]) * r + 1.0)
    }
}

/// (uniform, gaussian) of cell (x, y), independently.
fn expected(x: u64, y: u64) -> (f64, f64) {
    let n = u64::from(SIZE);
    let offset = (n * n * 0x69420) & 0xFFFF_FFFF;
    let s0 = hash64((y * 123 * n + x * 123 + offset) & 0xFFFF_FFFF);
    let s1 = step64(s0);
    let s2 = step64(s1);
    let two32 = 4_294_967_296.0;
    (s1 as f64 / two32, inv_norm(s2 as f64 / two32))
}

fn unpack(v: f32) -> (f32, f32) {
    let b = v.to_bits();
    (
        f16::from_bits((b >> 16) as u16).to_f32(),
        f16::from_bits(b as u16).to_f32(),
    )
}

#[test]
fn pinned_texels_match_an_independent_computation() {
    let map = noise_map();
    assert_eq!(map.len(), (SIZE * SIZE * 4) as usize);
    let n = u64::from(SIZE);
    for (x, y) in [(0u64, 0u64), (5, 7), (64, 3), (127, 0), (127, 127)] {
        let at = ((y * n + x) * 4) as usize;
        // Components: the cell, +y, +x, +x+y (wrapping at the edge).
        let cells = [
            (x, y),
            (x, (y + 1) % n),
            ((x + 1) % n, y),
            ((x + 1) % n, (y + 1) % n),
        ];
        for (k, (cx, cy)) in cells.into_iter().enumerate() {
            let (u, g) = unpack(map[at + k]);
            let (eu, eg) = expected(cx, cy);
            // f16 keeps 11 significant bits: half a step is under 2.5e-4 below 1, and about 0.1% of |g|.
            assert!(
                (f64::from(u) - eu).abs() <= 2.5e-4,
                "({x},{y})[{k}] u {u} vs {eu}"
            );
            assert!(
                (f64::from(g) - eg).abs() <= 4e-3 * eg.abs().max(1.0),
                "({x},{y})[{k}] g {g} vs {eg}"
            );
        }
    }
}

#[test]
fn uniform_in_unit_interval_and_gaussian_standard() {
    // Before packing u is in [0, 1); f16 rounds the top 2^-12 of it to 1.0, as CS's f32tof16 does.
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (u, g) = cell(x, y);
            assert!(
                (0.0..1.0).contains(&u) && g.is_finite(),
                "({x},{y}) {u} {g}"
            );
        }
    }
    let map = noise_map();
    let (mut sum, mut sq) = (0.0f64, 0.0f64);
    for v in &map {
        let (u, g) = unpack(*v);
        assert!((0.0..=1.0).contains(&u), "u {u}");
        assert!(g.is_finite(), "g {g}");
    }
    // Component 0 holds every cell once.
    for v in map.iter().step_by(4) {
        let g = f64::from(unpack(*v).1);
        sum += g;
        sq += g * g;
    }
    let n = f64::from(SIZE * SIZE);
    let mean = sum / n;
    let std = (sq / n - mean * mean).sqrt();
    assert!(mean.abs() < 0.05, "mean {mean}");
    assert!((std - 1.0).abs() < 0.05, "std {std}");
}
