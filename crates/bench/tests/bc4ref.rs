//! Pins the BC4 reference search (docs/research/bc4-family.md) to the upstream rgbcx path and to its never-worse rule.
use sdc_bench::bc4ref::{search, shipped};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 24) as u8
    }
}

/// Uniform, two-level 0/255 plus noise, ramps and near-constant blocks, in rotation.
fn blocks(n: usize) -> Vec<[u8; 16]> {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    (0..n)
        .map(|i| {
            let mut v = [0u8; 16];
            match i % 4 {
                0 => v.iter_mut().for_each(|x| *x = r.byte()),
                1 => v.iter_mut().for_each(|x| {
                    let base = if r.next() & 1 == 0 { 0i32 } else { 255 };
                    let noise = if r.next().is_multiple_of(3) {
                        (r.byte() % 9) as i32
                    } else {
                        0
                    };
                    *x = (if base == 0 {
                        base + noise
                    } else {
                        base - noise
                    }) as u8;
                }),
                2 => {
                    let (a, b) = (r.byte() as i32, r.byte() as i32);
                    for (k, x) in v.iter_mut().enumerate() {
                        *x = (a + (b - a) * k as i32 / 15) as u8;
                    }
                }
                _ => {
                    let c = r.byte();
                    v.iter_mut()
                        .for_each(|x| *x = c.saturating_add(r.byte() % 3).saturating_sub(1))
                }
            }
            v
        })
        .collect()
}

fn px(v: &[u8; 16], ch: usize) -> [[u8; 4]; 16] {
    let mut p = [[7u8; 4]; 16];
    for i in 0..16 {
        p[i][ch] = v[i];
    }
    p
}

fn sse(b: &[u8; 8], v: &[u8; 16]) -> u64 {
    let mut d = [0u8; 16];
    bcdec_rs::bc4(b, &mut d, 4, false);
    d.iter()
        .zip(v)
        .map(|(&x, &y)| (x as i64 - y as i64).pow(2) as u64)
        .sum()
}

fn flips(b: &[u8; 8], v: &[u8; 16]) -> u32 {
    let mut d = [0u8; 16];
    bcdec_rs::bc4(b, &mut d, 4, false);
    d.iter()
        .zip(v)
        .filter(|(&x, &y)| (x >= 128) != (y >= 128))
        .count() as u32
}

#[test]
fn shipped_port_equals_ffi() {
    for v in blocks(20_000) {
        for ch in 0..4 {
            let p = px(&v, ch);
            for r in [4, 5, 10] {
                assert_eq!(
                    shipped(&v, r),
                    encoders_sys::bc4_encode_block_flags(
                        &p,
                        ch,
                        r,
                        encoders_sys::BC4_USE_ALL_MODES
                    ),
                    "{v:?} ch {ch} r {r}"
                );
            }
        }
    }
}

#[test]
fn search_never_worse() {
    for v in blocks(20_000) {
        let s = shipped(&v, 5);
        let a = search(&v, 5, false);
        assert!(sse(&a, &v) <= sse(&s, &v), "{v:?}");
        let g = search(&v, 5, true);
        assert!(
            sse(&g, &v) <= sse(&s, &v) || flips(&g, &v) <= flips(&s, &v),
            "{v:?}"
        );
        assert!(flips(&g, &v) <= flips(&s, &v), "{v:?}");
    }
}

#[test]
fn const_and_binary_blocks_unchanged() {
    let mut checker = [0u8; 16];
    for (i, x) in checker.iter_mut().enumerate() {
        *x = if (i + i / 4) % 2 == 0 { 0 } else { 255 };
    }
    for v in [[0u8; 16], [255; 16], [77; 16], checker] {
        assert_eq!(search(&v, 5, false), shipped(&v, 5), "{v:?}");
        assert_eq!(search(&v, 5, true), shipped(&v, 5), "{v:?}");
    }
}

#[test]
fn trimmed_range_beats_shipped_on_near_extremes() {
    let v = [
        0, 0, 1, 2, 40, 80, 120, 160, 200, 253, 254, 255, 255, 255, 128, 64,
    ];
    assert!(sse(&search(&v, 5, false), &v) < sse(&shipped(&v, 5), &v));
}
