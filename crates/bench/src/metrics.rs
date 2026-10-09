//! Metrics computed exactly as F:/ai/dds/bench/scripts/run_bench.py and run_bench2.py (float64).

pub fn psnr_from_mse(mse: f64) -> f64 {
    if mse <= 1e-12 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

pub fn channel_psnr(src: &[[u8; 4]], dec: &[[u8; 4]], ch: &[usize]) -> f64 {
    assert_eq!(src.len(), dec.len());
    let mut sum = 0.0f64;
    for (s, d) in src.iter().zip(dec) {
        for &c in ch {
            let e = s[c] as f64 - d[c] as f64;
            sum += e * e;
        }
    }
    psnr_from_mse(sum / (src.len() * ch.len()) as f64)
}

pub fn rgb_psnr(src: &[[u8; 4]], dec: &[[u8; 4]]) -> f64 {
    channel_psnr(src, dec, &[0, 1, 2])
}

pub fn a_psnr(src: &[[u8; 4]], dec: &[[u8; 4]]) -> f64 {
    channel_psnr(src, dec, &[3])
}

/// Multi-exposure PSNR (run_bench2.py hdr_metrics): exposures 2^-6..2^6, gamma 1/2.2, clip to [0, 255].
pub fn mpsnr(reference: &[[f32; 3]], decoded: &[[f32; 3]]) -> f64 {
    assert_eq!(reference.len(), decoded.len());
    let n = (reference.len() * 3) as f64;
    let mut mses = vec![];
    for c in -6..=6 {
        let s = 2f64.powi(c);
        let t = |v: f32| (255.0 * ((v as f64).max(0.0) * s).powf(1.0 / 2.2)).clamp(0.0, 255.0);
        let mut sum = 0.0;
        for (r, d) in reference.iter().zip(decoded) {
            for (&rv, &dv) in r.iter().zip(d) {
                let e = t(rv) - t(dv);
                sum += e * e;
            }
        }
        mses.push(sum / n);
    }
    psnr_from_mse(mean(&mses))
}

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}
