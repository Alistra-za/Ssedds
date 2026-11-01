use sdc_bench::{corpus, ddsraw, metrics};

#[test]
fn psnr_matches_the_python_formula() {
    assert_eq!(metrics::psnr_from_mse(0.0), 99.0);
    assert!((metrics::psnr_from_mse(1.0) - 48.130804).abs() < 1e-5);
    let a = vec![[10u8, 20, 30, 255]; 4];
    let mut b = a.clone();
    b[0][0] = 12; // one channel off by 2 over 12 samples -> mse = 4/12
    assert!((metrics::rgb_psnr(&a, &b) - metrics::psnr_from_mse(4.0 / 12.0)).abs() < 1e-9);
    assert_eq!(metrics::a_psnr(&a, &b), 99.0);
}

#[test]
fn mpsnr_of_identical_images_is_99() {
    let r = vec![[0.5f32, 2.0, 10.0]; 16];
    assert_eq!(metrics::mpsnr(&r, &r), 99.0);
}

#[test]
fn ddsraw_skips_legacy_and_dx10_headers() {
    let mut legacy = vec![0u8; 128 + 8];
    legacy[..4].copy_from_slice(b"DDS ");
    legacy[84..88].copy_from_slice(b"DXT1");
    assert_eq!(ddsraw::payload(&legacy).unwrap().len(), 8);
    let mut dx10 = vec![0u8; 148 + 16];
    dx10[..4].copy_from_slice(b"DDS ");
    dx10[84..88].copy_from_slice(b"DX10");
    assert_eq!(ddsraw::payload(&dx10).unwrap().len(), 16);
}

#[test]
fn f16_loader_reads_the_bench_format() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.f16");
    let mut bytes = vec![];
    for v in [0x0036_3146u32, 1, 1] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for h in [0x3C00u16, 0x4000, 0x0000, 0x3C00] {
        bytes.extend_from_slice(&h.to_le_bytes());
    }
    std::fs::write(&p, bytes).unwrap();
    let img = corpus::load_f16(&p).unwrap();
    assert_eq!((img.width, img.height), (1, 1));
    assert_eq!(img.px[0], [0x3C00, 0x4000, 0, 0x3C00]);
}

#[test]
fn encode_one_writes_a_decodable_bc7_max_dds() {
    if !encoders_sys::BC7E_MAX_NATIVE {
        eprintln!("skipped: no patch-0002 bc7e objects");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (src, dst) = (dir.path().join("x.png"), dir.path().join("x.dds"));
    image::ImageBuffer::from_fn(8, 8, |x, y| {
        image::Rgba([(x * 30) as u8, (y * 30) as u8, 90u8, 255])
    })
    .save(&src)
    .unwrap();
    sdc_bench::encodeone::encode_one("bc7", "max", &src, &dst).unwrap();
    let bytes = std::fs::read(&dst).unwrap();
    let payload = ddsraw::payload(&bytes).unwrap();
    assert_eq!(payload.len(), 4 * 16);
    let dec = sdc_bench::codec::decode_bc7_image(payload, 8, 8);
    assert!(metrics::rgb_psnr(&corpus::load_png_rgba8(&src).unwrap().px, &dec) > 35.0);
    assert!(sdc_bench::encodeone::encode_one("bc7", "slowest", &src, &dst).is_err());
}

/// Level 0 as the engine pipeline encodes it: load -> preprocess -> mips -> quantise -> encode_level.
fn engine_level0(src: &std::path::Path, p: &engine::config::Profile) -> Vec<u8> {
    let (img, _) = engine::load::load_image(src).unwrap();
    let work = engine::ops::preprocess(img, p);
    let l0 = &engine::mips::build_chain(&work, p)[0];
    let input = engine::quantize::to_encoder_input(l0, &work, p);
    engine::encode::encode_level(&input, l0.width, l0.height, p, &Default::default()).unwrap()
}

#[test]
fn encode_one_writes_the_engines_bytes() {
    use engine::config::{Effort, Format, Subtype, Target};
    let dir = tempfile::tempdir().unwrap();
    let (src, dst) = (dir.path().join("n.png"), dir.path().join("n.dds"));
    // Opaque white noise (DECISIONS fix batch 41): bc7e picks rotated mode 4/5 for many blocks. At Veryslow the
    // engine's alpha fix rewrites some of them; at Max it measured a no-op (0 of ~1800 rotated blocks per 256x256).
    let mut s = 0x1234_5678u32;
    image::ImageBuffer::from_fn(64, 64, |_, _| {
        let mut c = [255u8; 4];
        for v in &mut c[..3] {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *v = (s >> 24) as u8;
        }
        image::Rgba(c)
    })
    .save(&src)
    .unwrap();
    let reg = engine::presets::PresetRegistry::builtin().unwrap();
    for (fmt, level, st, format) in [
        ("bc7", "max", Subtype::Diffuse, Format::Bc7),
        ("bc7", "max", Subtype::GenericData, Format::Bc7),
        ("bc3", "default", Subtype::Diffuse, Format::Bc3),
        ("bc4", "default", Subtype::Height, Format::Bc4),
        ("bc4", "default", Subtype::MsnSpecular, Format::Bc4),
        ("bc5", "default", Subtype::Flow, Format::Bc5),
        ("bc5", "default", Subtype::GenericTwoChannel, Format::Bc5),
    ] {
        if format == Format::Bc7 && !encoders_sys::BC7E_MAX_NATIVE {
            eprintln!("skipped bc7: no patch-0002 bc7e objects");
            continue;
        }
        let mut p = reg.profile("cs-quality", Target::Cs, st).unwrap();
        p.format = format;
        if format == Format::Bc7 {
            p.encoder.effort = Effort::Max;
        }
        sdc_bench::encodeone::encode_one(fmt, level, &src, &dst).unwrap();
        let ours = std::fs::read(&dst).unwrap();
        let want = engine_level0(&src, &p);
        assert_eq!(ddsraw::payload(&ours).unwrap(), &want[..], "{fmt} {st:?}");
    }
}

#[test]
fn reads_bench_results_csv() {
    let Some(root) = corpus::corpus_root() else {
        eprintln!("skipped: no bench corpus (set SDC_BENCH_ROOT)");
        return;
    };
    let rows = corpus::read_results(&root.join("results/results.csv"), "bc7e_u5").unwrap();
    let r = corpus::find(&rows, "rgb_real", "Bricks104").expect("Bricks104 row");
    assert!((r.f("rgb_psnr").unwrap() - 47.81554222506591).abs() < 1e-12);
}
