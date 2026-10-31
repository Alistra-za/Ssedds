//! Diagnostic: which blocks does VeryslowPeak re-encode, and what does each cost in the mPSNR error sum?
//! Usage: `cargo run --release -p sdc-bench --example bc6h_guard_stats -- <file.f16>`
use encoders_sys::Bc6hLevel;
use sdc_bench::codec::{decode_bc6h_image, encode_bc6h_image, half_to_rgb_f32};
use sdc_bench::corpus::load_f16;

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The mPSNR squared-error sum of one texel over the 13 exposures.
fn mse_sum(r: [f32; 3], d: [f32; 3]) -> f64 {
    let mut s = 0.0;
    for c in -6..=6 {
        let e = 2f64.powi(c);
        let t = |v: f32| (255.0 * ((v as f64).max(0.0) * e).powf(1.0 / 2.2)).clamp(0.0, 255.0);
        for k in 0..3 {
            s += (t(r[k]) - t(d[k])).powi(2);
        }
    }
    s
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let img = load_f16(std::path::Path::new(&a[1])).expect("load");
    let (w, h) = (img.width as usize, img.height as usize);
    let src = half_to_rgb_f32(&img);
    let d0 = decode_bc6h_image(
        &encode_bc6h_image(&img, Bc6hLevel::Veryslow),
        w as u32,
        h as u32,
    );
    let d1 = decode_bc6h_image(
        &encode_bc6h_image(&img, Bc6hLevel::VeryslowPeak),
        w as u32,
        h as u32,
    );
    let mut rows = vec![];
    let total: f64 = (0..w * h).map(|i| mse_sum(src[i], d0[i])).sum();
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            let idx: Vec<usize> = (0..16)
                .map(|t| (by * 4 + t / 4) * w + bx * 4 + t % 4)
                .collect();
            if idx.iter().all(|&i| d0[i] == d1[i]) {
                continue;
            }
            let k = *idx
                .iter()
                .max_by(|&&i, &&j| lum(src[i]).total_cmp(&lum(src[j])))
                .unwrap();
            // Peak over the block's geometric-mean luminance (the shape of a small highlight).
            let gm = (idx.iter().map(|&i| lum(src[i]).max(1e-6).ln()).sum::<f32>() / 16.0).exp();
            let cost: f64 = idx
                .iter()
                .map(|&i| mse_sum(src[i], d1[i]) - mse_sum(src[i], d0[i]))
                .sum();
            rows.push((
                cost,
                lum(src[k]),
                lum(src[k]) / gm,
                lum(d0[k]) / lum(src[k]),
                lum(d1[k]) / lum(src[k]),
            ));
        }
    }
    let changed_cost: f64 = rows.iter().map(|r| r.0).sum();
    println!(
        "{} blocks changed of {}; cost {:.0} of veryslow total {:.0} ({:+.3}%)",
        rows.len(),
        w * h / 16,
        changed_cost,
        total,
        100.0 * changed_cost / total
    );
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("cost  peak_lum  peak/geomean  ratio_veryslow  ratio_peak");
    for r in rows.iter().take(25) {
        println!(
            "{:>10.0} {:>10.2} {:>10.1} {:>8.3} {:>8.3}",
            r.0, r.1, r.2, r.3, r.4
        );
    }
    // How the changed blocks spread over peak/geomean contrast: count and cost by bucket.
    for lo in [0.0f32, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0] {
        let sel: Vec<_> = rows.iter().filter(|r| r.2 >= lo).collect();
        println!(
            "contrast >= {lo:>4}: {:>6} blocks, cost {:>12.0}",
            sel.len(),
            sel.iter().map(|r| r.0).sum::<f64>()
        );
    }
}
