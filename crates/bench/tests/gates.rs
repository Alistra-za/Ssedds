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
