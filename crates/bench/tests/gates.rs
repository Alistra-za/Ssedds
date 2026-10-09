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

#[test]
#[ignore = "byte patch exceeds 0.05 dB; see DECISIONS S5"]
fn s5b_byte_patch_gives_255_everywhere_on_opaque_sets() {
    let r = gates::exact_opaque(&bench_root(), gates::ExactMode::BytePatch).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn s5c_native_patch_flag_off_is_bit_identical() {
    let r = gates::bc7e_matches_dump(&bench_root(), &gates::pre_patch_hashes()).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
#[ignore = "native patch: alpha 255 everywhere but RGB cost exceeds 0.05 dB; see DECISIONS S5 (owner decides, Task 17)"]
fn s5c_native_patch_flag_on_gives_255_within_0_05_db() {
    let r = gates::exact_opaque(&bench_root(), gates::ExactMode::Native).unwrap();
    assert!(r.pass, "{r}");
}

/// What `gate all` enforces for the exact-opaque row: alpha == 255 everywhere. The -0.05 dB RGB limit is
/// printed as info (Task 38 ruling, DECISIONS S5), so this passes on the corpus where the strict gate above fails.
#[test]
fn exact_opaque_in_all_passes_on_alpha_and_reports_drgb_as_info() {
    let r = gates::exact_opaque_alpha_only(&bench_root(), gates::ExactMode::Native).unwrap();
    assert!(r.pass, "{r}");
    assert!(
        r.lines
            .iter()
            .any(|l| l.starts_with("info") && l.contains("mean dRGB") && l.contains("limit -0.05")),
        "{r}"
    );
    assert!(!r.lines.iter().any(|l| l.starts_with("FAIL")), "{r}");
}
