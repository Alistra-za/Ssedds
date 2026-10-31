//! Metrics computed exactly as the bench scripts run_bench.py and run_bench2.py (float64).

/// The engine's rule (validation and the engine-parity gate must agree, audit left-out).
pub use engine::validate::psnr_from_mse;

pub fn channel_psnr(src: &[[u8; 4]], dec: &[[u8; 4]], ch: &[usize]) -> f64 {
    assert_eq!(src.len(), dec.len());
    engine::validate::psnr(src, dec, ch)
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

/// RMSLE (run_bench2.py hdr_metrics): sqrt(mean((log1p(dec) - log1p(ref))^2)) over every RGB value.
pub fn rmsle(reference: &[[f32; 3]], decoded: &[[f32; 3]]) -> f64 {
    assert_eq!(reference.len(), decoded.len());
    let sum: f64 = reference
        .iter()
        .zip(decoded)
        .flat_map(|(r, d)| r.iter().zip(d))
        .map(|(&r, &d)| ((d as f64).ln_1p() - (r as f64).ln_1p()).powi(2))
        .sum();
    (sum / (reference.len() * 3) as f64).sqrt()
}

pub fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}
