//! In-house iterative radix-2 Cooley–Tukey FFT in f64 (no complex-number dependency).
use rayon::prelude::*;
use std::ops::{Add, Mul, Sub};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    pub(crate) const fn new(re: f64, im: f64) -> C {
        C { re, im }
    }
    pub(crate) fn conj(self) -> C {
        C::new(self.re, -self.im)
    }
    pub(crate) fn scale(self, s: f64) -> C {
        C::new(self.re * s, self.im * s)
    }
}

impl Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for C {
    type Output = C;
    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }
}

impl Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

/// `exp(∓2πi·j/n)` for `j < n/2` (forward: minus).
fn twiddles(n: usize, inverse: bool) -> Vec<C> {
    let sign = if inverse { 2.0 } else { -2.0 };
    (0..n / 2)
        .map(|j| {
            let a = sign * std::f64::consts::PI * j as f64 / n as f64;
            C::new(a.cos(), a.sin())
        })
        .collect()
}

/// In-place unscaled transform of one power-of-two line; `tw` from [`twiddles`] for its length.
fn fft1(a: &mut [C], tw: &[C]) {
    let n = a.len();
    let bits = n.trailing_zeros();
    if bits == 0 {
        return;
    }
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let step = n / len;
        for block in a.chunks_exact_mut(len) {
            let (lo, hi) = block.split_at_mut(len / 2);
            for (j, (u, v)) in lo.iter_mut().zip(hi.iter_mut()).enumerate() {
                let t = *v * tw[j * step];
                (*u, *v) = (*u + t, *u - t);
            }
        }
        len *= 2;
    }
}

/// `out[x·h + y] = data[y·w + x]`.
fn transpose(data: &[C], w: usize, h: usize) -> Vec<C> {
    let mut out = vec![C::default(); w * h];
    out.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        for (y, c) in col.iter_mut().enumerate() {
            *c = data[y * w + x];
        }
    });
    out
}

/// 2-D DFT of a row-major `w × h` field, in place: rows, then columns through a transpose. The inverse scales by
/// `1/(w·h)`. Panics unless `w` and `h` are powers of two.
pub(crate) fn fft2(data: &mut [C], w: usize, h: usize, inverse: bool) {
    assert!(
        w.is_power_of_two() && h.is_power_of_two() && data.len() == w * h,
        "fft2 needs a power-of-two field, got {w}x{h} ({} values)",
        data.len()
    );
    let tw = twiddles(w, inverse);
    data.par_chunks_mut(w).for_each(|r| fft1(r, &tw));
    let mut t = transpose(data, w, h);
    let tw = twiddles(h, inverse);
    t.par_chunks_mut(h).for_each(|c| fft1(c, &tw));
    let s = if inverse { 1.0 / (w * h) as f64 } else { 1.0 };
    data.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, c) in row.iter_mut().enumerate() {
            *c = t[x * h + y].scale(s);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(s: &mut u64) -> f64 {
        *s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    fn naive(x: &[C], w: usize, h: usize) -> Vec<C> {
        let mut out = vec![C::default(); w * h];
        for l in 0..h {
            for k in 0..w {
                let mut acc = C::default();
                for y in 0..h {
                    for xx in 0..w {
                        let a = -2.0
                            * std::f64::consts::PI
                            * ((k * xx) as f64 / w as f64 + (l * y) as f64 / h as f64);
                        acc = acc + x[y * w + xx] * C::new(a.cos(), a.sin());
                    }
                }
                out[l * w + k] = acc;
            }
        }
        out
    }

    fn max_err(a: &[C], b: &[C]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(a, b)| (a.re - b.re).abs().max((a.im - b.im).abs()))
            .fold(0.0, f64::max)
    }

    #[test]
    fn forward_then_inverse_is_identity_and_matches_a_naive_dft() {
        let mut seed = 7u64;
        for (w, h) in [(16, 16), (8, 32)] {
            let x: Vec<C> = (0..w * h)
                .map(|_| C::new(lcg(&mut seed), lcg(&mut seed)))
                .collect();
            let mut f = x.clone();
            fft2(&mut f, w, h, false);
            let err = max_err(&f, &naive(&x, w, h));
            assert!(err < 1e-9, "{w}x{h} vs naive DFT: {err}");
            fft2(&mut f, w, h, true);
            let err = max_err(&f, &x);
            assert!(err < 1e-9, "{w}x{h} round trip: {err}");
        }
    }
}
