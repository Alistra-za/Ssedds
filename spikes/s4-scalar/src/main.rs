#[path = "../../../crates/bench/src/metrics.rs"]
#[allow(dead_code)]
mod metrics;
use rayon::prelude::*;
use std::time::Instant;

extern "C" {
    fn s4_init();
    fn s4_compress(n: u32, blocks: *mut u64, px: *const u32);
}

fn main() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var("SDC_BENCH_ROOT").unwrap_or(r"F:\ai\dds\bench".into()),
    );
    unsafe { s4_init() };
    let warm = [0u32; 16];
    let mut wb = [0u64; 2];
    unsafe { s4_compress(1, wb.as_mut_ptr(), warm.as_ptr()) };
    let (mut mpix, mut secs1, mut secs_n) = (0.0, 0.0, 0.0);
    let (mut max_d, mut identical, mut total) = (0f64, 0u32, 0u32);
    for set in ["rgb_real", "alpha_real"] {
        let mut files: Vec<_> = std::fs::read_dir(root.join("images").join(set))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        files.sort();
        for f in files {
            let img = image::open(&f)?.to_rgba8();
            let (w, h) = img.dimensions();
            let px: Vec<[u8; 4]> = img.pixels().map(|p| p.0).collect();
            let mut blocks = vec![0u32; (w * h) as usize];
            let bw = (w / 4) as usize;
            for (i, chunk) in blocks.chunks_mut(16).enumerate() {
                let (bx, by) = (i % bw, i / bw);
                for t in 0..16 {
                    chunk[t] =
                        u32::from_le_bytes(px[(by * 4 + t / 4) * w as usize + bx * 4 + t % 4]);
                }
            }
            let n = blocks.len() / 16;
            let mut out1 = vec![0u64; n * 2];
            let t = Instant::now();
            unsafe { s4_compress(n as u32, out1.as_mut_ptr(), blocks.as_ptr()) };
            secs1 += t.elapsed().as_secs_f64();
            let mut out_n = vec![0u64; n * 2];
            let t = Instant::now();
            out_n
                .par_chunks_mut(128)
                .zip(blocks.par_chunks(64 * 16))
                .for_each(|(o, b)| unsafe {
                    s4_compress((b.len() / 16) as u32, o.as_mut_ptr(), b.as_ptr())
                });
            secs_n += t.elapsed().as_secs_f64();
            mpix += (w * h) as f64 / 1e6;
            let bytes: Vec<u8> = out_n.iter().flat_map(|v| v.to_le_bytes()).collect();
            let mut dec = vec![[0u8; 4]; (w * h) as usize];
            for i in 0..n {
                let mut o = [0u8; 64];
                bcdec_rs::bc7(&bytes[i * 16..i * 16 + 16], &mut o, 16);
                let (bx, by) = (i % bw, i / bw);
                for t in 0..16 {
                    dec[(by * 4 + t / 4) * w as usize + bx * 4 + t % 4]
                        .copy_from_slice(&o[t * 4..t * 4 + 4]);
                }
            }
            let bench = std::fs::read(
                root.join("out/bc7e_u5")
                    .join(set)
                    .join(f.file_stem().unwrap())
                    .with_extension("dds"),
            )?;
            let off = if &bench[84..88] == b"DX10" { 148 } else { 128 };
            let same = bench[off..] == bytes[..];
            let (rgb, a) = (metrics::rgb_psnr(&px, &dec), metrics::a_psnr(&px, &dec));
            // PSNR of the ISPC reference payload, decoded the same way, for the per-image delta.
            let mut ref_dec = vec![[0u8; 4]; (w * h) as usize];
            for i in 0..n {
                let mut o = [0u8; 64];
                bcdec_rs::bc7(&bench[off + i * 16..off + i * 16 + 16], &mut o, 16);
                let (bx, by) = (i % bw, i / bw);
                for t in 0..16 {
                    ref_dec[(by * 4 + t / 4) * w as usize + bx * 4 + t % 4]
                        .copy_from_slice(&o[t * 4..t * 4 + 4]);
                }
            }
            let (rrgb, ra) = (
                metrics::rgb_psnr(&px, &ref_dec),
                metrics::a_psnr(&px, &ref_dec),
            );
            let d = (rgb - rrgb).abs().max(if ra.is_finite() && a.is_finite() {
                (a - ra).abs()
            } else {
                0.0
            });
            max_d = max_d.max(d);
            total += 1;
            identical += same as u32;
            println!(
                "{set}/{}: rgb {rgb:.4} (ISPC {rrgb:.4}) a {a:.4} (ISPC {ra:.4}) dmax {d:.4} bytes-identical-to-ISPC {same}",
                f.file_stem().unwrap().to_string_lossy(),
            );
        }
    }
    println!(
        "max per-image |dPSNR| vs ISPC u5: {max_d:.4} dB; bytes identical on {identical}/{total}"
    );
    println!(
        "scalar u5: {:.3} Mpix/s single-thread, {:.3} Mpix/s on {} threads",
        mpix / secs1,
        mpix / secs_n,
        rayon::current_num_threads()
    );
    Ok(())
}
