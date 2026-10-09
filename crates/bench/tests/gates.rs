use sdc_bench::{codec::RgbcxKind, corpus::bench_root, gates};

#[test]
fn s3_bc7e_u5_reproduces_bench_within_0_01_db() {
    let r = gates::bc7e_u5(&bench_root(), false).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn s3_rgbcx_bc4_mask_real_is_bit_exact() {
    let r = gates::rgbcx_bit_exact(
        &bench_root(),
        "out2/bc4_rgbcx",
        "images2",
        &["mask_real"],
        RgbcxKind::Bc4,
    )
    .unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn rgbcx_bc1_bc3_bc4_bc5_bit_exact_in_bench_configuration() {
    for r in gates::rgbcx_all(&bench_root()).unwrap() {
        assert!(r.pass, "{r}");
    }
}

#[test]
fn bc1_transparent_black_off_matches_bc7enc_dash_b() {
    let work = tempfile::tempdir().unwrap();
    let r = gates::bc1_black_off_vs_tool(&bench_root(), work.path()).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn bc6h_veryslow_reproduces_51_50_db() {
    let r = gates::bc6h_veryslow(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn s5a_alpha_checker_agrees_with_recorded_measurements() {
    // DECISIONS Note C: bc7e -u6 Bricks104 decodes 254 on 21.0% of pixels; NVTT Production on 0%.
    let r = gates::alpha_scan(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}
