use encoders_sys::{bc1_decode_block, Bc1Model};
use engine::config::*;
use engine::dds::*;
use engine::decode::*;
use engine::encode::{encode_level, EncodeCtx};
use engine::presets::PresetRegistry;
use engine::quantize::EncoderInput;

fn put(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// xorshift64 bytes.
fn noise(seed: u64, n: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 24) as u8
        })
        .collect()
}

/// Smooth RGBA image: the same function of normalised position at every level size, so a mip chain of it
/// stays consistent and block codecs reproduce it closely.
fn smooth(w: u32, h: u32) -> Vec<[u8; 4]> {
    let mut v = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            // Collinear in RGB, so even a single 4-colour block spans the image closely.
            let t = (fx + fy) / 2.0;
            v.push([
                (60.0 + 100.0 * t) as u8,
                (50.0 + 110.0 * t) as u8,
                (70.0 + 60.0 * t) as u8,
                255,
            ]);
        }
    }
    v
}

fn smooth_hdr(w: u32, h: u32) -> Vec<[u16; 4]> {
    smooth(w, h)
        .iter()
        .map(|p| {
            let f = |c: u8| half::f16::from_f32(0.05 + c as f32 / 255.0 * 3.0).to_bits();
            [f(p[0]), f(p[1]), f(p[2]), 0x3C00]
        })
        .collect()
}

fn chain_dims(w: u32, h: u32) -> Vec<(u32, u32)> {
    let mut v = vec![(w, h)];
    let (mut w, mut h) = (w, h);
    while w > 1 || h > 1 {
        w = (w / 2).max(1);
        h = (h / 2).max(1);
        v.push((w, h));
    }
    v
}

fn profile(st: Subtype, format: Format) -> Profile {
    let mut p = PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, st)
        .unwrap();
    p.format = format;
    p
}

/// Hand-built file: `pf` is (flags, fourcc, bits, [r, g, b, a masks]).
fn raw_file(
    (w, h, mips): (u32, u32, u32),
    pf: (u32, [u8; 4], u32, [u32; 4]),
    caps2: u32,
    dx10: Option<[u32; 5]>,
    data: &[u8],
) -> Vec<u8> {
    let mut b = vec![0u8; 128];
    b[..4].copy_from_slice(b"DDS ");
    put(&mut b, 4, 124);
    put(
        &mut b,
        8,
        DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT,
    );
    put(&mut b, 12, h);
    put(&mut b, 16, w);
    put(&mut b, 28, mips);
    put(&mut b, 76, 32);
    put(&mut b, 80, pf.0);
    b[84..88].copy_from_slice(&pf.1);
    put(&mut b, 88, pf.2);
    for (i, m) in pf.3.iter().enumerate() {
        put(&mut b, 92 + i * 4, *m);
    }
    put(&mut b, 108, DDSCAPS_TEXTURE);
    put(&mut b, 112, caps2);
    if let Some(d) = dx10 {
        for v in d {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    b.extend_from_slice(data);
    b
}

const RGB_FLAGS: u32 = 0x41;

#[test]
fn round_trips_writer_output_and_passes_header_check() {
    for (f, srgb, h) in [
        (Format::Bc7, true, HeaderForm::Auto),
        (Format::Bc1, false, HeaderForm::Auto),
        (Format::Bc4, false, HeaderForm::Legacy),
        (Format::Bc6hUf16, false, HeaderForm::Auto),
    ] {
        let desc = DdsDesc {
            width: 16,
            height: 8,
            format: f,
            srgb,
            header: resolve_header(f, srgb, h),
        };
        let levels: Vec<Vec<u8>> = [(16, 8), (8, 4), (4, 2), (2, 1), (1, 1)]
            .iter()
            .map(|&(w, h)| vec![0u8; level_size(f, w, h)])
            .collect();
        let file = read_dds(&dds_bytes(&desc, &levels).unwrap()).unwrap();
        assert_eq!(
            (file.width, file.height, file.mip_count, file.levels.len()),
            (16, 8, 5, 5)
        );
        assert_eq!(file.format.as_format(), Some((f, srgb)));
        assert!(
            check_header(&file).is_empty(),
            "{f:?}: {:?}",
            check_header(&file)
        );
    }
}

/// Decode `data` straight with the underlying block decoder, independent of `decode_level`.
fn reference_decode(fmt: DdsPixelFormat, w: u32, h: u32, data: &[u8]) -> Vec<[f32; 4]> {
    let (bw, bh) = (w.div_ceil(4) as usize, h.div_ceil(4) as usize);
    let bs = fmt.block_bytes().unwrap();
    let mut out = vec![[0f32; 4]; (w * h) as usize];
    for by in 0..bh {
        for bx in 0..bw {
            let blk = &data[(by * bw + bx) * bs..][..bs];
            let mut px = [[0f32; 4]; 16];
            match fmt {
                DdsPixelFormat::Bc1 { .. } => {
                    let d = bc1_decode_block(blk.try_into().unwrap(), Bc1Model::Ideal);
                    for (o, p) in px.iter_mut().zip(d) {
                        *o = p.map(f32::from);
                    }
                }
                DdsPixelFormat::Bc3 { .. } | DdsPixelFormat::Bc7 { .. } => {
                    let mut d = [0u8; 64];
                    if matches!(fmt, DdsPixelFormat::Bc3 { .. }) {
                        bcdec_rs::bc3(blk, &mut d, 16);
                    } else {
                        bcdec_rs::bc7(blk, &mut d, 16);
                    }
                    for (o, p) in px.iter_mut().zip(d.chunks(4)) {
                        *o = [p[0], p[1], p[2], p[3]].map(f32::from);
                    }
                }
                DdsPixelFormat::Bc4 => {
                    let mut d = [0u8; 16];
                    bcdec_rs::bc4(blk, &mut d, 4, false);
                    for (o, p) in px.iter_mut().zip(d) {
                        *o = [p, p, p, 255].map(f32::from);
                    }
                }
                DdsPixelFormat::Bc5 => {
                    let mut d = [0u8; 32];
                    bcdec_rs::bc5(blk, &mut d, 8, false);
                    for (o, p) in px.iter_mut().zip(d.chunks(2)) {
                        *o = [p[0], p[1], 0, 255].map(f32::from);
                    }
                }
                DdsPixelFormat::Bc6hUf16 => {
                    let mut d = [0f32; 48];
                    bcdec_rs::bc6h_float(blk, &mut d, 12, false);
                    for (o, p) in px.iter_mut().zip(d.chunks(3)) {
                        *o = [p[0], p[1], p[2], 1.0];
                    }
                }
                _ => unreachable!(),
            }
            for y in 0..4 {
                for x in 0..4 {
                    let (gx, gy) = (bx * 4 + x, by * 4 + y);
                    if gx < w as usize && gy < h as usize {
                        out[gy * w as usize + gx] = px[y * 4 + x];
                    }
                }
            }
        }
    }
    out
}

/// Encode a smooth image at every mip level, write with every header form that applies, read it back, and check
/// the block bytes survive unchanged, `decode_level` matches an independent decode of the same blocks exactly,
/// and the decoded pixels are close to the encoder input.
#[test]
fn encode_write_read_decode_round_trip_every_format_and_header_form() {
    use DdsPixelFormat as P;
    // (format, srgb, header form, subtype, expected pixel format, max abs error vs input on R, G, B)
    let cases = [
        (
            Format::Bc1,
            false,
            HeaderForm::Auto,
            Subtype::Diffuse,
            P::Bc1 { srgb: false },
            24.0,
        ),
        (
            Format::Bc1,
            false,
            HeaderForm::Dx10,
            Subtype::Diffuse,
            P::Bc1 { srgb: false },
            24.0,
        ),
        (
            Format::Bc1,
            true,
            HeaderForm::Auto,
            Subtype::Diffuse,
            P::Bc1 { srgb: true },
            24.0,
        ),
        (
            Format::Bc3,
            false,
            HeaderForm::Auto,
            Subtype::Diffuse,
            P::Bc3 { srgb: false },
            24.0,
        ),
        (
            Format::Bc3,
            false,
            HeaderForm::Dx10,
            Subtype::Diffuse,
            P::Bc3 { srgb: false },
            24.0,
        ),
        (
            Format::Bc3,
            true,
            HeaderForm::Auto,
            Subtype::Diffuse,
            P::Bc3 { srgb: true },
            24.0,
        ),
        (
            Format::Bc4,
            false,
            HeaderForm::Legacy,
            Subtype::Envmask,
            P::Bc4,
            8.0,
        ),
        (
            Format::Bc4,
            false,
            HeaderForm::Auto,
            Subtype::Envmask,
            P::Bc4,
            8.0,
        ),
        (
            Format::Bc5,
            false,
            HeaderForm::Legacy,
            Subtype::Flow,
            P::Bc5,
            8.0,
        ),
        (
            Format::Bc5,
            false,
            HeaderForm::Auto,
            Subtype::Flow,
            P::Bc5,
            8.0,
        ),
        (
            Format::Bc7,
            false,
            HeaderForm::Auto,
            Subtype::Diffuse,
            P::Bc7 { srgb: false },
            6.0,
        ),
        (
            Format::Bc7,
            true,
            HeaderForm::Auto,
            Subtype::Diffuse,
            P::Bc7 { srgb: true },
            6.0,
        ),
    ];
    let (w, h) = (24u32, 12u32);
    for (format, srgb, form, st, want_fmt, tol) in cases {
        let p = profile(st, format);
        let dims = chain_dims(w, h);
        let mut inputs = vec![];
        let mut levels = vec![];
        for &(lw, lh) in &dims {
            let px = smooth(lw, lh);
            levels.push(
                encode_level(
                    &EncoderInput::Ldr(px.clone()),
                    lw,
                    lh,
                    &p,
                    &EncodeCtx::default(),
                )
                .ok()
                .unwrap(),
            );
            inputs.push(px);
        }
        let header = resolve_header(format, srgb, form);
        let desc = DdsDesc {
            width: w,
            height: h,
            format,
            srgb,
            header,
        };
        let bytes = dds_bytes(&desc, &levels).unwrap();
        let file = read_dds(&bytes).unwrap_or_else(|e| panic!("{format:?} {form:?}: {e}"));
        let tag = format!("{format:?} srgb={srgb} {form:?}");
        assert_eq!(file.format, want_fmt, "{tag}");
        assert_eq!(
            file.header_kind,
            if header == ResolvedHeader::Dx10 {
                HeaderKind::Dx10
            } else {
                HeaderKind::Legacy
            },
            "{tag}"
        );
        assert_eq!(file.levels.len(), dims.len(), "{tag}");
        assert!(
            check_header(&file).is_empty(),
            "{tag}: {:?}",
            check_header(&file)
        );
        for (i, ((lw, lh, range), blocks)) in file.levels.iter().zip(&levels).enumerate() {
            assert_eq!((*lw, *lh), dims[i], "{tag} level {i}");
            let got = &file.data[range.clone()];
            assert_eq!(got, &blocks[..], "{tag} level {i}: block bytes differ");
            let model = matches!(want_fmt, P::Bc1 { .. }).then_some(Bc1Model::Ideal);
            let Decoded::Ldr(px) = decode_level(file.format, *lw, *lh, got, model).unwrap() else {
                panic!("{tag}: expected LDR")
            };
            let reference = reference_decode(file.format, *lw, *lh, got);
            assert_eq!(px.len(), reference.len());
            let mut worst = 0f32;
            for ((d, r), src) in px.iter().zip(&reference).zip(&inputs[i]) {
                // Exact against the independent decode of the same blocks.
                assert_eq!(d.map(f32::from), *r, "{tag} level {i}");
                let chans = match want_fmt {
                    P::Bc4 => 1,
                    P::Bc5 => 2,
                    _ => 3,
                };
                for c in 0..chans {
                    worst = worst.max((f32::from(d[c]) - f32::from(src[c])).abs());
                }
            }
            // Sub-block levels are padded by replication, so the error stays small there too.
            assert!(
                worst <= tol,
                "{tag} level {i} ({lw}x{lh}): max error {worst} > {tol}"
            );
        }
    }
}

#[test]
fn bc6h_round_trip_through_dds() {
    let (w, h) = (16u32, 8u32);
    let p = profile(Subtype::PbrEmissive, Format::Bc6hUf16);
    assert_eq!(p.format, Format::Bc6hUf16);
    let dims = chain_dims(w, h);
    let mut inputs = vec![];
    let mut levels = vec![];
    for &(lw, lh) in &dims {
        let px = smooth_hdr(lw, lh);
        levels.push(
            encode_level(
                &EncoderInput::Hdr(px.clone()),
                lw,
                lh,
                &p,
                &EncodeCtx::default(),
            )
            .ok()
            .unwrap(),
        );
        inputs.push(px);
    }
    let desc = DdsDesc {
        width: w,
        height: h,
        format: Format::Bc6hUf16,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    let file = read_dds(&dds_bytes(&desc, &levels).unwrap()).unwrap();
    assert_eq!(file.format, DdsPixelFormat::Bc6hUf16);
    assert!(check_header(&file).is_empty());
    for (i, (lw, lh, range)) in file.levels.iter().enumerate() {
        let got = &file.data[range.clone()];
        assert_eq!(got, &levels[i][..]);
        let Decoded::Hdr(px) = decode_level(file.format, *lw, *lh, got, None).unwrap() else {
            panic!("expected HDR")
        };
        let reference = reference_decode(file.format, *lw, *lh, got);
        for ((d, r), src) in px.iter().zip(&reference).zip(&inputs[i]) {
            assert_eq!([d[0], d[1], d[2], 1.0], *r);
            for c in 0..3 {
                let s = half::f16::from_bits(src[c]).to_f32();
                assert!(
                    (d[c] - s).abs() <= 0.1 * s + 0.05,
                    "level {i}: {} vs {s}",
                    d[c]
                );
            }
        }
    }
}

/// Reads texconv/bench-produced files when `SDC_BENCH_ROOT` points at the bench tree (read-only; skipped
/// otherwise so the suite carries no absolute path).
#[test]
fn reads_bench_outputs_and_rejects_garbage() {
    if let Some(root) = std::env::var_os("SDC_BENCH_ROOT").map(std::path::PathBuf::from) {
        let b = std::fs::read(root.join("out/bc7e_u5/rgb_real/Bricks104.dds")).unwrap();
        let f = read_dds(&b).unwrap();
        assert_eq!(f.format, DdsPixelFormat::Bc7 { srgb: false });
        assert_eq!(f.levels.len() as u32, f.mip_count);
        let Decoded::Ldr(px) = decode_level(
            f.format,
            f.width,
            f.height,
            &f.data[f.levels[0].2.clone()],
            None,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(px.len(), (f.width * f.height) as usize);
        let b =
            std::fs::read(root.join("out2/bc4_rgbcx/mask_real/Bricks104_Roughness.dds")).unwrap();
        assert_eq!(read_dds(&b).unwrap().format, DdsPixelFormat::Bc4);
    } else {
        eprintln!("SDC_BENCH_ROOT not set: skipping the bench-file reads");
    }
    assert_eq!(read_dds(b"nope").unwrap_err(), DdsError::NotDds);
    assert_eq!(read_dds(b"").unwrap_err(), DdsError::NotDds);
    let mut t = dds_bytes(
        &DdsDesc {
            width: 8,
            height: 8,
            format: Format::Bc7,
            srgb: false,
            header: ResolvedHeader::Dx10,
        },
        &[vec![0; 64]],
    )
    .unwrap();
    t.truncate(160);
    assert_eq!(read_dds(&t).unwrap_err(), DdsError::Truncated);
}

#[test]
fn header_check_catches_violations() {
    let desc = DdsDesc {
        width: 8,
        height: 8,
        format: Format::Bc7,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    let good = dds_bytes(&desc, &[vec![0; 64], vec![0; 16], vec![0; 16], vec![0; 16]]).unwrap();
    assert!(check_header(&read_dds(&good).unwrap()).is_empty());

    let mut b = good.clone();
    put(&mut b, 80, DDPF_FOURCC | DDPF_NORMAL); // NVTT -normal style flag
    put(&mut b, 108, DDSCAPS_TEXTURE); // missing COMPLEX|MIPMAP
    let v = check_header(&read_dds(&b).unwrap());
    assert!(v.iter().any(|s| s.contains("DDPF_NORMAL")), "{v:?}");
    assert!(v.iter().any(|s| s.contains("MIPMAP")), "{v:?}");

    let mut b = good.clone();
    put(
        &mut b,
        8,
        DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT,
    );
    put(&mut b, 20, 12345);
    put(&mut b, 36, 1); // reserved1[1]
    let v = check_header(&read_dds(&b).unwrap());
    assert!(v.iter().any(|s| s.contains("LINEARSIZE")), "{v:?}");
    assert!(v.iter().any(|s| s.contains("MIPMAPCOUNT")), "{v:?}");
    assert!(v.iter().any(|s| s.contains("linear size")), "{v:?}");
    assert!(v.iter().any(|s| s.contains("reserved")), "{v:?}");

    for off in [116, 120, 124] {
        let mut b = good.clone();
        put(&mut b, off, 1);
        assert!(read_dds(&b).unwrap().reserved_nonzero, "offset {off}");
    }

    let mut b = good.clone();
    put(&mut b, 132, 2); // dimension: 1D
    put(&mut b, 140, 2); // array size
    put(&mut b, 144, 1); // misc2
    let tail = vec![0u8; 112]; // room for the second array slice
    b.extend_from_slice(&tail);
    let v = check_header(&read_dds(&b).unwrap());
    assert!(v.iter().any(|s| s.contains("DX10 header")), "{v:?}");

    // Mip count larger than the chain: kept in the header field, flagged, and the level table stays bounded.
    let mut b = good.clone();
    put(&mut b, 28, 9);
    let f = read_dds(&b).unwrap();
    assert_eq!((f.mip_count, f.levels.len()), (9, 4));
    assert!(check_header(&f).iter().any(|s| s.contains("exceeds")));

    // ATI1 legacy FourCC is readable but flagged.
    let ati = raw_file((4, 4, 1), (0x4, *b"ATI1", 0, [0; 4]), 0, None, &[0; 8]);
    let f = read_dds(&ati).unwrap();
    assert_eq!(f.format, DdsPixelFormat::Bc4);
    assert!(check_header(&f).iter().any(|s| s.contains("ATI1")));
}

#[test]
fn cubemap_flag_is_reported_and_all_faces_required() {
    let mip = [0u8; 8]; // one 4x4 BC1 block
    let faces = 6;
    let data: Vec<u8> = mip.repeat(faces);
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DXT1", 0, [0; 4]),
        0x200 | 0xFC00,
        None,
        &data,
    );
    let f = read_dds(&b).unwrap();
    assert!(f.is_cubemap);
    assert_eq!(f.levels, vec![(4, 4, 0..8)]);
    assert!(check_header(&f).iter().any(|s| s.contains("cubemap")));
    // One face short.
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DXT1", 0, [0; 4]),
        0x200 | 0xFC00,
        None,
        &data[..40],
    );
    assert_eq!(read_dds(&b).unwrap_err(), DdsError::Truncated);
    // DX10 cubemap through miscFlags, one cube.
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DX10", 0, [0; 4]),
        0,
        Some([71, 3, 4, 1, 0]),
        &data,
    );
    assert!(read_dds(&b).unwrap().is_cubemap);
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DX10", 0, [0; 4]),
        0,
        Some([71, 3, 4, 1, 0]),
        &data[..40],
    );
    assert_eq!(read_dds(&b).unwrap_err(), DdsError::Truncated);
    // DX10 array of 3 slices needs 3x the data; array size 0 is read as 1.
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DX10", 0, [0; 4]),
        0,
        Some([71, 3, 0, 3, 0]),
        &data[..24],
    );
    assert!(read_dds(&b).is_ok());
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DX10", 0, [0; 4]),
        0,
        Some([71, 3, 0, 3, 0]),
        &data[..23],
    );
    assert_eq!(read_dds(&b).unwrap_err(), DdsError::Truncated);
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DX10", 0, [0; 4]),
        0,
        Some([71, 3, 0, 0, 0]),
        &data[..8],
    );
    assert!(read_dds(&b).is_ok());
}

#[test]
fn bc4_and_bc5_decode_layouts() {
    let block4 = [255u8, 0, 0, 0, 0, 0, 0, 0]; // endpoint0 = 255, all indices 0
    let Decoded::Ldr(p) = decode_level(DdsPixelFormat::Bc4, 4, 4, &block4, None).unwrap() else {
        panic!()
    };
    assert_eq!(p[0], [255, 255, 255, 255]);
    let mut block5 = [0u8; 16];
    block5[0] = 200;
    block5[8] = 100;
    let Decoded::Ldr(p) = decode_level(DdsPixelFormat::Bc5, 4, 4, &block5, None).unwrap() else {
        panic!()
    };
    assert_eq!(p[0], [200, 100, 0, 255]);
}

#[test]
fn bc1_with_a_model_decodes_through_rgbcx() {
    let block = [0x1f, 0xf8, 0xe0, 0x07, 0x1b, 0x1b, 0x1b, 0x1b];
    for m in [
        Bc1Model::Ideal,
        Bc1Model::Nvidia,
        Bc1Model::Amd,
        Bc1Model::IdealRound4,
    ] {
        let Decoded::Ldr(px) =
            decode_level(DdsPixelFormat::Bc1 { srgb: false }, 4, 4, &block, Some(m)).unwrap()
        else {
            panic!()
        };
        let want = bc1_decode_block(&block, m);
        assert_eq!(px, want.to_vec(), "{m:?}");
    }
    let Decoded::Ldr(px) =
        decode_level(DdsPixelFormat::Bc1 { srgb: false }, 4, 4, &block, None).unwrap()
    else {
        panic!()
    };
    assert_eq!(px[0][3], 255);
}

#[test]
fn reads_and_decodes_uncompressed_and_legacy_forms() {
    let rgba = [10u8, 20, 30, 40, 50, 60, 70, 80];
    let m = |r, g, b, a| [r, g, b, a];
    type Case = (DdsPixelFormat, Vec<u8>, [u8; 4], [u8; 4]);
    let cases: Vec<Case> = vec![
        (
            DdsPixelFormat::Rgba8 { srgb: false },
            rgba.to_vec(),
            [10, 20, 30, 40],
            [50, 60, 70, 80],
        ),
        (
            DdsPixelFormat::Bgra8 { srgb: false },
            rgba.to_vec(),
            [30, 20, 10, 40],
            [70, 60, 50, 80],
        ),
        (
            DdsPixelFormat::Bgrx8,
            rgba.to_vec(),
            [30, 20, 10, 255],
            [70, 60, 50, 255],
        ),
        (
            DdsPixelFormat::R8,
            vec![7, 9],
            [7, 7, 7, 255],
            [9, 9, 9, 255],
        ),
        (
            DdsPixelFormat::Rg8,
            vec![1, 2, 3, 4],
            [1, 2, 0, 255],
            [3, 4, 0, 255],
        ),
    ];
    let legacy = [
        (32, m(0xff, 0xff00, 0xff_0000, 0xff00_0000)),
        (32, m(0xff_0000, 0xff00, 0xff, 0xff00_0000)),
        (32, m(0xff_0000, 0xff00, 0xff, 0)),
        (8, m(0xff, 0, 0, 0)),
        (16, m(0xff, 0xff00, 0, 0)),
    ];
    let dxgi = [28, 87, 88, 61, 49];
    for (i, (fmt, data, p0, p1)) in cases.iter().enumerate() {
        let (bits, masks) = legacy[i];
        for file in [
            raw_file((2, 1, 1), (RGB_FLAGS, [0; 4], bits, masks), 0, None, data),
            raw_file(
                (2, 1, 1),
                (0x4, *b"DX10", 0, [0; 4]),
                0,
                Some([dxgi[i], 3, 0, 1, 0]),
                data,
            ),
        ] {
            let f = read_dds(&file).unwrap();
            assert_eq!(f.format, *fmt);
            assert_eq!(f.levels, vec![(2, 1, 0..data.len())]);
            assert_eq!(
                decode_level(f.format, 2, 1, &f.data, None).unwrap(),
                Decoded::Ldr(vec![*p0, *p1]),
                "{fmt:?}"
            );
        }
    }
    // DX10 sRGB RGBA8 keeps its tag.
    let f = read_dds(&raw_file(
        (2, 1, 1),
        (0x4, *b"DX10", 0, [0; 4]),
        0,
        Some([29, 3, 0, 1, 0]),
        &rgba,
    ))
    .unwrap();
    assert_eq!(f.format, DdsPixelFormat::Rgba8 { srgb: true });
    assert_eq!(f.format.as_format(), None);

    // Half and float RGBA, DX10 and the legacy D3DFMT FourCCs.
    let one = half::f16::from_f32(1.0).to_le_bytes();
    let half_px: Vec<u8> = [one, [0, 0], one, one].concat();
    for file in [
        raw_file(
            (1, 1, 1),
            (0x4, *b"DX10", 0, [0; 4]),
            0,
            Some([10, 3, 0, 1, 0]),
            &half_px,
        ),
        raw_file(
            (1, 1, 1),
            (0x4, [113, 0, 0, 0], 0, [0; 4]),
            0,
            None,
            &half_px,
        ),
    ] {
        let f = read_dds(&file).unwrap();
        assert_eq!(f.format, DdsPixelFormat::Rgba16f);
        assert_eq!(
            decode_level(f.format, 1, 1, &f.data, None).unwrap(),
            Decoded::Hdr(vec![[1.0, 0.0, 1.0]])
        );
    }
    let f32_px: Vec<u8> = [1.5f32, 2.5, 3.5, 1.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    for file in [
        raw_file(
            (1, 1, 1),
            (0x4, *b"DX10", 0, [0; 4]),
            0,
            Some([2, 3, 0, 1, 0]),
            &f32_px,
        ),
        raw_file(
            (1, 1, 1),
            (0x4, [116, 0, 0, 0], 0, [0; 4]),
            0,
            None,
            &f32_px,
        ),
    ] {
        let f = read_dds(&file).unwrap();
        assert_eq!(f.format, DdsPixelFormat::Rgba32f);
        assert_eq!(
            decode_level(f.format, 1, 1, &f.data, None).unwrap(),
            Decoded::Hdr(vec![[1.5, 2.5, 3.5]])
        );
    }

    // Legacy FourCC forms for the block formats, including the DXT2/3/4/5 and BC4U/BC5U/ATI aliases.
    for (cc, fmt, size) in [
        (*b"DXT1", DdsPixelFormat::Bc1 { srgb: false }, 8),
        (*b"DXT2", DdsPixelFormat::Bc2 { srgb: false }, 16),
        (*b"DXT3", DdsPixelFormat::Bc2 { srgb: false }, 16),
        (*b"DXT4", DdsPixelFormat::Bc3 { srgb: false }, 16),
        (*b"DXT5", DdsPixelFormat::Bc3 { srgb: false }, 16),
        (*b"ATI1", DdsPixelFormat::Bc4, 8),
        (*b"BC4U", DdsPixelFormat::Bc4, 8),
        (*b"ATI2", DdsPixelFormat::Bc5, 16),
        (*b"BC5U", DdsPixelFormat::Bc5, 16),
    ] {
        let f = read_dds(&raw_file(
            (4, 4, 1),
            (0x4, cc, 0, [0; 4]),
            0,
            None,
            &vec![0; size],
        ))
        .unwrap();
        assert_eq!(f.format, fmt, "{cc:?}");
        assert_eq!(f.header_kind, HeaderKind::Legacy);
        assert_eq!(&f.fourcc, &cc);
    }
}

#[test]
fn malformed_headers_are_rejected_without_panicking() {
    let ok = |cc: [u8; 4], dx10: Option<[u32; 5]>| {
        raw_file((4, 4, 1), (0x4, cc, 0, [0; 4]), 0, dx10, &[0; 16])
    };
    // Unknown FourCC and unknown / typeless / unsupported-sRGB DXGI values.
    assert!(
        matches!(read_dds(&ok(*b"XXXX", None)), Err(DdsError::Unsupported(s)) if s.contains("XXXX"))
    );
    for d in [0, 1, 70, 93, 100, 115, 0xFFFF_FFFF] {
        assert!(
            matches!(
                read_dds(&ok(*b"DX10", Some([d, 3, 0, 1, 0]))),
                Err(DdsError::Unsupported(_))
            ),
            "dxgi {d}"
        );
    }
    // Unknown uncompressed masks, and RGBX (no matching pixel format).
    let b = raw_file(
        (4, 4, 1),
        (RGB_FLAGS, [0; 4], 24, [0xff_0000, 0xff00, 0xff, 0]),
        0,
        None,
        &[0; 48],
    );
    assert!(matches!(read_dds(&b), Err(DdsError::Unsupported(_))));
    // Zero dimensions, wrong structure sizes.
    for (w, h) in [(0, 4), (4, 0), (0, 0)] {
        let b = raw_file((w, h, 1), (0x4, *b"DXT1", 0, [0; 4]), 0, None, &[0; 8]);
        assert!(
            matches!(read_dds(&b), Err(DdsError::Unsupported(_))),
            "{w}x{h}"
        );
    }
    for (off, v) in [(4, 123), (4, 0), (76, 31), (76, 0)] {
        let mut b = ok(*b"DXT1", None);
        put(&mut b, off, v);
        assert!(
            matches!(read_dds(&b), Err(DdsError::Unsupported(_))),
            "{off}={v}"
        );
    }
    // DX10 marker without the extension block.
    let mut b = ok(*b"DX10", None);
    b.truncate(140);
    assert_eq!(read_dds(&b).unwrap_err(), DdsError::Truncated);
    // Magic only, and every prefix of a valid file.
    assert_eq!(read_dds(b"DDS ").unwrap_err(), DdsError::Truncated);
}

#[test]
fn absurd_dimensions_and_counts_fail_cheaply() {
    // 4 GiB-ish dimensions with 16 bytes of data: Truncated, no allocation from the header.
    for (w, h) in [
        (u32::MAX, u32::MAX),
        (u32::MAX, 1),
        (1, u32::MAX),
        (1 << 20, 1 << 20),
        (65536, 65536),
    ] {
        for (cc, dx10) in [
            (*b"DXT1", None),
            (*b"DX10", Some([98, 3, 0, 1, 0])),
            (*b"DX10", Some([2, 3, 0, 1, 0])),
        ] {
            let b = raw_file((w, h, 1), (0x4, cc, 0, [0; 4]), 0, dx10, &[0; 16]);
            assert_eq!(read_dds(&b).unwrap_err(), DdsError::Truncated, "{w}x{h}");
        }
    }
    // Mip count far beyond any chain, with and without enough data.
    for mips in [32, 33, 1000, u32::MAX] {
        let b = raw_file(
            (4, 4, mips),
            (0x4, *b"DXT1", 0, [0; 4]),
            0,
            None,
            &[0; 8 + 8 + 8],
        );
        let f = read_dds(&b).unwrap();
        assert_eq!((f.mip_count, f.levels.len()), (mips, 3));
        assert!(!check_header(&f).is_empty());
    }
    let b = raw_file(
        (4096, 4096, u32::MAX),
        (0x4, *b"DXT1", 0, [0; 4]),
        0,
        None,
        &[0; 100],
    );
    assert_eq!(read_dds(&b).unwrap_err(), DdsError::Truncated);
    // Huge array size and cube counts are bounded by the data actually present.
    let b = raw_file(
        (4, 4, 1),
        (0x4, *b"DX10", 0, [0; 4]),
        0,
        Some([71, 3, 4, u32::MAX, 0]),
        &[0; 64],
    );
    assert_eq!(read_dds(&b).unwrap_err(), DdsError::Truncated);
}

#[test]
fn level_size_overflow_is_checked_in_reader_and_writer() {
    use DdsPixelFormat as P;
    assert_eq!(P::Rgba32f.checked_level_size(u32::MAX, u32::MAX), None);
    assert_eq!(P::Rgba32f.level_size(u32::MAX, u32::MAX), usize::MAX);
    assert_eq!(
        P::Bc7 { srgb: false }.checked_level_size(u32::MAX, u32::MAX),
        None
    );
    assert_eq!(P::R8.checked_level_size(3, 5), Some(15));
    assert_eq!(checked_level_size(Format::Bc7, u32::MAX, u32::MAX), None);
    assert_eq!(level_size(Format::Bc7, u32::MAX, u32::MAX), usize::MAX);
    assert_eq!(checked_level_size(Format::Bc1, 5, 5), Some(32));

    let desc = DdsDesc {
        width: u32::MAX,
        height: u32::MAX,
        format: Format::Bc7,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    assert_eq!(
        dds_bytes(&desc, &[vec![0; 16]]),
        Err(DdsError::SizeOverflow {
            level: 0,
            width: u32::MAX,
            height: u32::MAX
        })
    );
    // Also when only a deeper level would overflow nothing else misreports it.
    let r = decode_level(P::Rgba32f, u32::MAX, u32::MAX, &[0; 64], None);
    assert_eq!(r.unwrap_err(), DdsError::Truncated);
}

#[test]
fn every_truncation_and_every_header_byte_flip_is_handled() {
    let desc = DdsDesc {
        width: 8,
        height: 8,
        format: Format::Bc7,
        srgb: true,
        header: ResolvedHeader::Dx10,
    };
    let levels = vec![noise(1, 64), noise(2, 16), noise(3, 16), noise(4, 16)];
    let good = dds_bytes(&desc, &levels).unwrap();
    assert!(read_dds(&good).is_ok());
    for n in 0..good.len() {
        let r = read_dds(&good[..n]);
        assert!(r.is_err(), "prefix {n} unexpectedly parsed");
    }
    // Trailing bytes are tolerated.
    let mut longer = good.clone();
    longer.extend_from_slice(&[1, 2, 3]);
    assert!(read_dds(&longer).is_ok());
    // Any single-byte change in the 148-byte header may be rejected or accepted, but must not panic, and an
    // accepted file must decode level 0 without panicking.
    for i in 0..148 {
        for v in [0u8, 1, 0x7f, 0x80, 0xff] {
            let mut b = good.clone();
            b[i] = v;
            if let Ok(f) = read_dds(&b) {
                for (w, h, r) in &f.levels {
                    let _ = decode_level(f.format, *w, *h, &f.data[r.clone()], None);
                }
                let _ = check_header(&f);
            }
        }
    }
}

#[test]
fn decode_rejects_short_data_and_survives_arbitrary_blocks() {
    use DdsPixelFormat as P;
    let block_formats = [
        P::Bc1 { srgb: false },
        P::Bc2 { srgb: false },
        P::Bc3 { srgb: false },
        P::Bc4,
        P::Bc5,
        P::Bc6hUf16,
        P::Bc6hSf16,
        P::Bc7 { srgb: true },
    ];
    for fmt in block_formats {
        let bs = fmt.block_bytes().unwrap();
        // Non-multiple-of-4 sizes, arbitrary and structured block bytes.
        for (w, h) in [(1, 1), (3, 5), (4, 4), (7, 9), (13, 2)] {
            let n = fmt.level_size(w, h);
            for seed in 0..64u64 {
                let mut data = noise(seed, n);
                if seed % 8 == 0 {
                    data.iter_mut()
                        .for_each(|b| *b = if seed % 16 == 0 { 0 } else { 0xff });
                }
                let d = decode_level(fmt, w, h, &data, None).unwrap();
                let len = match d {
                    Decoded::Ldr(v) => v.len(),
                    Decoded::Hdr(v) => v.len(),
                };
                assert_eq!(len, (w * h) as usize, "{fmt:?} {w}x{h}");
            }
            assert_eq!(
                decode_level(fmt, w, h, &vec![0; n - 1], None).unwrap_err(),
                DdsError::Truncated
            );
            assert_eq!(
                decode_level(fmt, w, h, &[], None).unwrap_err(),
                DdsError::Truncated
            );
            assert!(n >= bs);
        }
        // Every BC7 mode byte and BC6H mode pattern.
        for first in 0..=255u8 {
            let mut blk = noise(first as u64 + 1000, bs);
            blk[0] = first;
            decode_level(fmt, 4, 4, &blk, None).unwrap();
        }
        // Absurd dimensions with a small buffer: error, not allocation.
        assert_eq!(
            decode_level(fmt, u32::MAX, u32::MAX, &vec![0; bs], None).unwrap_err(),
            DdsError::Truncated
        );
        assert_eq!(
            decode_level(fmt, 1 << 20, 1 << 20, &vec![0; bs], None).unwrap_err(),
            DdsError::Truncated
        );
        assert!(matches!(
            decode_level(fmt, 0, 4, &vec![0; bs], None),
            Err(DdsError::Unsupported(_))
        ));
        assert!(matches!(
            decode_level(fmt, 4, 0, &vec![0; bs], None),
            Err(DdsError::Unsupported(_))
        ));
    }
    for fmt in [
        P::Rgba8 { srgb: false },
        P::Bgra8 { srgb: false },
        P::Bgrx8,
        P::R8,
        P::Rg8,
        P::Rgba16f,
        P::Rgba32f,
    ] {
        assert_eq!(
            decode_level(fmt, u32::MAX, 3, &[0; 32], None).unwrap_err(),
            DdsError::Truncated
        );
        assert_eq!(
            decode_level(fmt, 2, 2, &[0; 3], None).unwrap_err(),
            DdsError::Truncated
        );
        let n = fmt.level_size(3, 2);
        let d = decode_level(fmt, 3, 2, &noise(5, n + 5), None).unwrap(); // extra bytes ignored
        match d {
            Decoded::Ldr(v) => assert_eq!(v.len(), 6),
            Decoded::Hdr(v) => assert_eq!(v.len(), 6),
        }
    }
}

/// Codex-B I2: an uncompressed DDS declares `DDSD_PITCH` and a row pitch (bytes per row), not `DDSD_LINEARSIZE`
/// and the surface size.
#[test]
fn uncompressed_header_uses_the_row_pitch() {
    let base = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT;
    // DDPF_RGB | DDPF_ALPHAPIXELS, 32 bits, RGBA masks: legacy RGBA8.
    let rgba8 = (0x41, [0; 4], 32, [0xff, 0xff00, 0xff_0000, 0xff00_0000]);
    let mut good = raw_file((4, 4, 1), rgba8, 0, None, &[0; 64]);
    put(&mut good, 8, base | DDSD_PITCH);
    put(&mut good, 20, 16);
    let f = read_dds(&good).unwrap();
    assert_eq!(f.format, DdsPixelFormat::Rgba8 { srgb: false });
    assert!(check_header(&f).is_empty(), "{:?}", check_header(&f));

    // A pitch that is not one row: here the whole surface, as a compressed header would carry.
    let mut b = good.clone();
    put(&mut b, 20, 64);
    let v = check_header(&read_dds(&b).unwrap());
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("pitch 64 != row pitch 16"), "{v:?}");

    // LINEARSIZE instead of PITCH.
    let mut b = good.clone();
    put(&mut b, 8, base | DDSD_LINEARSIZE);
    let v = check_header(&read_dds(&b).unwrap());
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("PITCH"), "{v:?}");

    // Row pitch is width x bytes per pixel, also for an odd width: R8 3x2 has pitch 3.
    let r8 = (0x2_0000, [0; 4], 8, [0xff, 0, 0, 0]);
    let mut b = raw_file((3, 2, 1), r8, 0, None, &[0; 6]);
    put(&mut b, 8, base | DDSD_PITCH);
    put(&mut b, 20, 3);
    let f = read_dds(&b).unwrap();
    assert_eq!(f.format, DdsPixelFormat::R8);
    assert!(check_header(&f).is_empty(), "{:?}", check_header(&f));
}

/// Codex-B I3: a legacy volume declaration (`DDSD_DEPTH` with depth > 1, or `DDSCAPS2_VOLUME`) is not a 2D
/// texture. It is rejected as unsupported, whether or not every depth slice is present, instead of being read as
/// its first slice.
#[test]
fn legacy_volume_textures_are_unsupported() {
    const VOLUME: u32 = 0x20_0000; // DDSCAPS2_VOLUME
    let block = [0u8; 8]; // one 4x4 BC1 block
    let volume = |depth_flag: bool, depth: u32, caps2: u32, data: &[u8]| {
        let mut b = raw_file((4, 4, 1), (0x4, *b"DXT1", 0, [0; 4]), caps2, None, data);
        let flags = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_LINEARSIZE;
        put(
            &mut b,
            8,
            if depth_flag {
                flags | DDSD_DEPTH
            } else {
                flags
            },
        );
        put(&mut b, 20, 8);
        put(&mut b, 24, depth);
        put(&mut b, 108, DDSCAPS_TEXTURE | DDSCAPS_COMPLEX);
        b
    };
    let unsupported =
        |b: &[u8]| matches!(read_dds(b), Err(DdsError::Unsupported(s)) if s == "volume texture");
    // The review's case: depth 2 declared, only the first slice present (truncated depth payload).
    assert!(unsupported(&volume(true, 2, VOLUME, &block)));
    // Every slice present: still a volume.
    assert!(unsupported(&volume(true, 2, VOLUME, &block.repeat(2))));
    // Either declaration alone.
    assert!(unsupported(&volume(true, 2, 0, &block.repeat(2))));
    assert!(unsupported(&volume(false, 0, VOLUME, &block)));
    // A depth of 1 is a plain 2D texture.
    let f = read_dds(&volume(true, 1, 0, &block)).unwrap();
    assert_eq!(f.levels, vec![(4, 4, 0..8)]);
}

#[test]
fn dxgi_round_trips_through_the_pixel_format() {
    for d in [
        2u32, 10, 28, 29, 49, 61, 71, 72, 74, 75, 77, 78, 80, 83, 87, 88, 91, 95, 96, 98, 99,
    ] {
        let f = from_dxgi(d).unwrap_or_else(|| panic!("{d}"));
        assert_eq!(f.dxgi(), d);
    }
    assert_eq!(from_dxgi(91), Some(DdsPixelFormat::Bgra8 { srgb: true }));
    assert_eq!(from_dxgi(12345), None);
}

#[test]
fn decode_level_rgba_keeps_float_alpha_and_scales_ldr() {
    use engine::decode::decode_level_rgba;
    let h: Vec<u8> = [2.0f32, 0.5, 0.25, 0.75]
        .iter()
        .flat_map(|v| half::f16::from_f32(*v).to_le_bytes())
        .collect();
    assert_eq!(
        decode_level_rgba(DdsPixelFormat::Rgba16f, 1, 1, &h, None).unwrap(),
        vec![[2.0, 0.5, 0.25, 0.75]]
    );
    assert_eq!(
        decode_level_rgba(DdsPixelFormat::R8, 1, 1, &[51], None).unwrap(),
        vec![[0.2, 0.2, 0.2, 1.0]]
    );
    assert_eq!(
        decode_level_rgba(DdsPixelFormat::Rgba16f, 2, 2, &h, None).unwrap_err(),
        DdsError::Truncated
    );
}

/// Review Focus 2: a padded row pitch is reported by `check_header`, and the level still decodes as stored.
#[test]
fn a_padded_pitch_is_a_header_issue_not_a_panic() {
    let mut hd = [0u32; 31];
    hd[0] = 124;
    hd[1] = 0x1 | 0x2 | 0x4 | 0x8 | 0x1000;
    hd[2] = 2;
    hd[3] = 3;
    hd[4] = 16;
    hd[6] = 1;
    hd[18] = 32;
    hd[19] = 0x40 | 0x1;
    hd[21] = 32;
    hd[22] = 0xff_0000;
    hd[23] = 0xff00;
    hd[24] = 0xff;
    hd[25] = 0xff00_0000;
    hd[26] = 0x1000;
    let mut b = b"DDS ".to_vec();
    for v in hd {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&[7u8; 3 * 2 * 4]);
    let f = read_dds(&b).unwrap();
    assert!(
        check_header(&f)
            .iter()
            .any(|m| m.contains("pitch 16 != row pitch 12")),
        "{:?}",
        check_header(&f)
    );
    assert!(decode_level(f.format, 3, 2, &f.data[f.levels[0].2.clone()], None).is_ok());
}

/// Fix wave (final review minor 1): a DX10 header with `D3D10_RESOURCE_DIMENSION_TEXTURE3D` is a volume even without
/// the legacy volume flags. It is refused, not relabelled 2D (so `plan_patch` refuses it too).
#[test]
fn dx10_texture3d_without_volume_flags_is_unsupported() {
    let good = dds_bytes(
        &DdsDesc {
            width: 8,
            height: 8,
            format: Format::Bc7,
            srgb: false,
            header: ResolvedHeader::Dx10,
        },
        &[vec![0; 64]],
    )
    .unwrap();
    let mut b = good.clone();
    put(&mut b, 132, 4); // D3D10_RESOURCE_DIMENSION_TEXTURE3D
    assert!(
        matches!(read_dds(&b), Err(DdsError::Unsupported(s)) if s == "volume texture"),
        "{:?}",
        read_dds(&b)
    );
    assert!(engine::dds_patch::plan_patch(&b, &Default::default()).is_err());
}
