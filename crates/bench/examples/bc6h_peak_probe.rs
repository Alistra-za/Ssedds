//! Diagnostic: encodes one HDR `.f16` with ISPC BC6H and dumps the 4x4 block holding the source luminance peak.
//! Usage: `cargo run -p sdc-bench --example bc6h_peak_probe -- <file.f16> [level 0..4]`
use encoders_sys::{bc6h_compress_rows, Bc6hLevel};
use sdc_bench::corpus::load_f16;

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn bit(b: &[u8; 16], i: usize) -> u32 {
    ((b[i / 8] >> (i % 8)) & 1) as u32
}

/// ISPC mode index (kernel.ispc get_mode_prefix order) of a BC6H block.
fn ispc_mode(b: &[u8; 16]) -> Option<usize> {
    const PREFIX: [u32; 14] = [0, 1, 2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15];
    let two = bit(b, 0) | bit(b, 1) << 1;
    let m = if two < 2 {
        two
    } else {
        (0..5).map(|i| bit(b, i) << i).sum()
    };
    PREFIX.iter().position(|&p| p == m)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let img = load_f16(std::path::Path::new(&a[1])).expect("load");
    let level = match a.get(2).map(|s| s.as_str()).unwrap_or("4") {
        "0" => Bc6hLevel::Veryfast,
        "1" => Bc6hLevel::Fast,
        "2" => Bc6hLevel::Basic,
        "3" => Bc6hLevel::Slow,
        "5" => Bc6hLevel::VeryslowPeak,
        _ => Bc6hLevel::Veryslow,
    };
    let (w, h) = (img.width as usize, img.height as usize);
    let f = |p: [u16; 4]| [0, 1, 2].map(|c| half::f16::from_bits(p[c]).to_f32());
    let (pi, _) = img
        .px
        .iter()
        .enumerate()
        .map(|(i, &p)| (i, lum(f(p))))
        .fold((0, f32::MIN), |m, x| if x.1 > m.1 { x } else { m });
    let (bx, by) = ((pi % w) / 4, (pi / w) / 4);
    let mut strip = img.px[by * 4 * w..(by * 4 + 4) * w].to_vec();
    if a.get(3).is_some_and(|s| s == "floor") {
        // Experiment: exact-zero channels in the peak block become that channel's smallest positive value there.
        for c in 0..3 {
            let texels = (0..16).map(|t| (t / 4) * w + bx * 4 + t % 4);
            let lo = texels
                .clone()
                .map(|i| strip[i][c])
                .filter(|&v| v > 0)
                .min()
                .unwrap_or(0);
            for i in texels {
                if strip[i][c] == 0 {
                    strip[i][c] = lo;
                }
            }
        }
        println!("(zero channels floored in the encoder input)");
    }
    let mut out = vec![0u8; w / 4 * 16];
    bc6h_compress_rows(&strip, w as u32, 4, &mut out, level);
    let blk: [u8; 16] = out[bx * 16..bx * 16 + 16].try_into().unwrap();
    let mut dec = [0f32; 48];
    bcdec_rs::bc6h_float(&blk, &mut dec, 12, false);
    println!(
        "image {}x{} peak texel ({}, {}) block ({bx}, {by}) level {level:?}",
        w,
        h,
        pi % w,
        pi / w
    );
    println!(
        "block bytes {}",
        blk.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let mode = ispc_mode(&blk);
    // Two-region modes (ISPC 0..9) store the partition in bits 77..81.
    let part = mode
        .filter(|&m| m < 10)
        .map(|_| (77..82).map(|i| bit(&blk, i) << (i - 77)).sum::<u32>());
    println!("ispc mode {mode:?} partition {part:?}");
    if mode == Some(10) {
        // Mode 10 (D3D mode 11, prefix 00011): raw 10-bit endpoints rw gw bw rx gx bx at bits 5..65.
        let v = |s: usize| (0..10).map(|i| bit(&blk, s + i) << i).sum::<u32>();
        println!(
            "endpoints e0 = {:?} e1 = {:?} (10-bit)",
            [v(5), v(15), v(25)],
            [v(35), v(45), v(55)]
        );
    }
    println!("texel  src(r g b) lum  [half bits]  |  dec(r g b) lum");
    for t in 0..16 {
        let p = img.px[(by * 4 + t / 4) * w + bx * 4 + t % 4];
        let s = f(p);
        let d = [dec[t * 3], dec[t * 3 + 1], dec[t * 3 + 2]];
        println!(
            "{t:2} {:>10.2} {:>10.2} {:>10.2} {:>10.2} [{:04x} {:04x} {:04x}] | {:>10.2} {:>10.2} {:>10.2} {:>10.2}",
            s[0], s[1], s[2], lum(s), p[0], p[1], p[2], d[0], d[1], d[2], lum(d)
        );
    }
    // The encoder's own error: squared difference of half bits * 64/31 (kernel.ispc bc6h_setup), summed over RGB.
    let uf = |v: f32| half::f16::from_f32(v).to_bits() as f64 * 64.0 / 31.0;
    let err: f64 = (0..16)
        .map(|t| {
            let p = img.px[(by * 4 + t / 4) * w + bx * 4 + t % 4];
            (0..3)
                .map(|c| (p[c] as f64 * 64.0 / 31.0 - uf(dec[t * 3 + c])).powi(2))
                .sum::<f64>()
        })
        .sum();
    println!("encoder-domain error {err:.0}");
    let _ = h;
}
