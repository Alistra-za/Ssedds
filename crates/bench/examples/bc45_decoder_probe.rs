//! Task 10 fix round 1: why BC4/BC5 PSNR is not monotonic in the rgbcx search radius. Decodes every block with
//! bcdec (the engine/bench decoder: interpolants rounded to nearest) and with rgbcx's own model
//! (`bc4_block::get_block_values` through the shim; floored before the M6 Track D rgbcx patch, rounded since), prints the
//! radius table under both, and dumps one Wood094 BC5 block whose bcdec error rises from radius 5 to 10 (if any).
//! Reads images2 only; prints to stdout.
use sdc_bench::corpus::{bench_root, load_png_rgba8, set_files, stem};
use sdc_bench::metrics::{mean, psnr_from_mse};
use sdc_bench::radius::{encode, RADII};

fn sels(b: &[u8]) -> [usize; 16] {
    let bits = u64::from_le_bytes(b[..8].try_into().unwrap()) >> 16;
    std::array::from_fn(|i| ((bits >> (3 * i)) & 7) as usize)
}

/// Decode one 8-byte BC4 block both ways.
fn dec(b: &[u8]) -> ([u8; 16], [u8; 16]) {
    let mut bc = [0u8; 16];
    bcdec_rs::bc4(b, &mut bc, 4, false);
    let v = encoders_sys::bc4_block_values(b[0], b[1]);
    let s = sels(b);
    (bc, std::array::from_fn(|i| v[s[i]]))
}

fn main() -> anyhow::Result<()> {
    let root = bench_root();
    for (format, set, chans) in [("bc4", "mask_real", 1usize), ("bc5", "normal_real", 2)] {
        let files = set_files(&root, "images2", set, "png")?;
        let imgs: Vec<_> = files
            .iter()
            .map(|f| Ok((stem(f), load_png_rgba8(f)?)))
            .collect::<anyhow::Result<_>>()?;
        let mut enc_by_r = vec![];
        println!("{format}: radius mean_psnr_bcdec mean_psnr_rgbcx_model");
        for r in RADII {
            let (mut pb, mut pr, mut per_img) = (vec![], vec![], vec![]);
            for (name, img) in &imgs {
                let blocks = encoders_sys::gather_blocks(&img.px, img.width, img.height);
                let data = encode(format, &blocks, r);
                let (mut eb, mut er) = (0u64, 0u64);
                let bsz = 8 * chans;
                for (bi, blk) in data.chunks(bsz).enumerate() {
                    for c in 0..chans {
                        let (b, m) = dec(&blk[8 * c..8 * c + 8]);
                        for t in 0..16 {
                            let s = blocks[bi * 16 + t][c] as i64;
                            eb += ((b[t] as i64 - s).pow(2)) as u64;
                            er += ((m[t] as i64 - s).pow(2)) as u64;
                        }
                    }
                }
                // Padded edge texels count here; the corpus is all multiples of 4, so this equals channel_psnr.
                let n = (blocks.len() * chans) as f64;
                pb.push(psnr_from_mse(eb as f64 / n));
                pr.push(psnr_from_mse(er as f64 / n));
                per_img.push((name.clone(), data));
            }
            println!("{format} {r} {:.4} {:.4}", mean(&pb), mean(&pr));
            enc_by_r.push((r, per_img));
        }
        if format == "bc5" {
            let pick = |r: u32| &enc_by_r.iter().find(|e| e.0 == r).unwrap().1;
            let (i5, i10) = (pick(5), pick(10));
            let w = i5.iter().position(|x| x.0 == "Wood094").unwrap();
            let wi = &imgs[w].1;
            let blocks = encoders_sys::gather_blocks(&wi.px, wi.width, wi.height);
            let (d5, d10) = (&i5[w].1, &i10[w].1);
            let (mut rises, mut shown) = (0usize, false);
            for bi in 0..d5.len() / 16 {
                for c in [0usize, 1] {
                    let o = bi * 16 + 8 * c;
                    let src: Vec<i64> = (0..16).map(|t| blocks[bi * 16 + t][c] as i64).collect();
                    let sse = |v: &[u8; 16]| -> i64 {
                        (0..16).map(|t| (v[t] as i64 - src[t]).pow(2)).sum()
                    };
                    let (b5, m5) = dec(&d5[o..o + 8]);
                    let (b10, m10) = dec(&d10[o..o + 8]);
                    if sse(&b10) > sse(&b5) {
                        rises += 1;
                        if !shown {
                            shown = true;
                            println!("Wood094 block {bi} channel {c}: source {src:?}");
                            for (r, d, b, m) in
                                [(5, &d5[o..o + 8], b5, m5), (10, &d10[o..o + 8], b10, m10)]
                            {
                                println!(
                                    "  r{r}: e0={} e1={} mode={} sels={:?}\n    bcdec  {:?} sse={}\n    rgbcx  {:?} sse={}",
                                    d[0],
                                    d[1],
                                    if d[0] > d[1] { "8-value" } else { "6-value" },
                                    sels(d),
                                    b,
                                    sse(&b),
                                    m,
                                    sse(&m)
                                );
                            }
                        }
                    }
                }
            }
            println!(
                "Wood094 BC5 channel-blocks whose bcdec SSE rises r5->r10: {rises} of {}",
                d5.len() / 8
            );
        }
    }
    Ok(())
}
