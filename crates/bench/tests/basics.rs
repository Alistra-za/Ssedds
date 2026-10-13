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
fn reads_bench_results_csv() {
    let Some(root) = corpus::corpus_root() else {
        eprintln!("skipped: no bench corpus (set SDC_BENCH_ROOT)");
        return;
    };
    let rows = corpus::read_results(&root.join("results/results.csv"), "bc7e_u5").unwrap();
    let r = corpus::find(&rows, "rgb_real", "Bricks104").expect("Bricks104 row");
    assert!((r.f("rgb_psnr").unwrap() - 47.81554222506591).abs() < 1e-12);
}
