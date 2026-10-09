use encoders_sys::Bc1Model;
use engine::codes::{Code, Severity};
use engine::config::*;
use engine::dds::*;
use engine::encode::{encode_level, EncodeCtx};
use engine::presets::PresetRegistry;
use engine::quantize::EncoderInput;
use engine::validate::*;

fn profile(st: Subtype) -> Profile {
    PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, st)
        .unwrap()
}

type Level = (u32, u32, Vec<[u8; 4]>);

/// Wrap already encoded levels (largest first) in a DDS for `p`.
fn wrap(p: &Profile, w: u32, h: u32, enc: &[Vec<u8>]) -> Vec<u8> {
    let desc = DdsDesc {
        width: w,
        height: h,
        format: p.format,
        srgb: p.srgb_tag(),
        header: resolve_header(p.format, p.srgb_tag(), p.header),
    };
    dds_bytes(&desc, enc).unwrap()
}

fn encode_all(p: &Profile, levels: &[Level]) -> (Vec<Vec<u8>>, Vec<EncoderInput>) {
    let inputs: Vec<EncoderInput> = levels
        .iter()
        .map(|(_, _, px)| EncoderInput::Ldr(px.clone()))
        .collect();
    let enc = levels
        .iter()
        .zip(&inputs)
        .map(|((w, h, _), i)| {
            encode_level(i, *w, *h, p, &EncodeCtx::default())
                .ok()
                .unwrap()
        })
        .collect();
    (enc, inputs)
}

/// Encode explicit levels (largest first) with the engine and wrap them in a DDS.
fn dds(p: &Profile, levels: &[Level]) -> (Vec<u8>, Vec<EncoderInput>) {
    let (enc, inputs) = encode_all(p, levels);
    let (w, h, _) = &levels[0];
    (wrap(p, *w, *h, &enc), inputs)
}

fn run(
    b: &[u8],
    p: &Profile,
    enc: Option<&[EncoderInput]>,
    base_coverage: Option<f64>,
    expected_levels: Option<u32>,
) -> ValidationReport {
    validate(&ValidationInput {
        bytes: b,
        profile: p,
        encoder_input: enc,
        base_coverage,
        source_peak: None,
        bc1_model: Bc1Model::Ideal,
        expected_levels,
    })
}

fn codes(r: &ValidationReport) -> Vec<Code> {
    r.warnings.iter().map(|w| w.code).collect()
}

fn mode6_p0_payload(blocks: usize) -> Vec<u8> {
    let mut v: u128 = 1 << 6;
    for i in 0..8 {
        v |= (if i >= 6 { 127u128 } else { 10 }) << (7 + 7 * i);
    }
    v.to_le_bytes().repeat(blocks)
}

fn flat(n: u32, px: [u8; 4]) -> Level {
    (n, n, vec![px; (n * n) as usize])
}

/// Deterministic opaque noise.
fn noise(w: u32, h: u32, seed: u32) -> Vec<[u8; 4]> {
    let mut s = seed.wrapping_mul(2_654_435_761).wrapping_add(12345);
    let mut next = move || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (s >> 24) as u8
    };
    (0..w * h).map(|_| [next(), next(), next(), 255]).collect()
}

#[test]
fn clean_bc7_passes_and_header_matches() {
    let p = profile(Subtype::Diffuse);
    let px: Vec<[u8; 4]> = (0..64)
        .map(|i| [i as u8 * 4, 100, 200 - i as u8, 255])
        .collect();
    let (b, inputs) = dds(
        &p,
        &[
            (8, 8, px),
            flat(4, [90, 100, 150, 255]),
            flat(2, [90, 100, 150, 255]),
            flat(1, [90, 100, 150, 255]),
        ],
    );
    let r = run(&b, &p, Some(&inputs), None, Some(4));
    assert!(r.summary.passed, "{:?}", r.warnings);
    assert_eq!(r.summary.levels, 4);
    assert_eq!(r.summary.alpha.len(), 4);
    assert!(r.summary.min_mip_psnr_db.unwrap() > p.validation.min_mip_psnr_db);
    assert!(r.summary.coverage.is_empty());
}

#[test]
fn alpha_254_is_info_with_flag_off_and_an_error_with_flag_on() {
    let off = profile(Subtype::Diffuse);
    let mut on = off.clone();
    on.alpha.exact_opaque = true;
    let desc = DdsDesc {
        width: 4,
        height: 4,
        format: Format::Bc7,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    let b = dds_bytes(&desc, &[mode6_p0_payload(1)]).unwrap();
    let r = run(&b, &off, None, None, None);
    assert!(r.summary.passed);
    let w = r
        .warnings
        .iter()
        .find(|w| w.code == Code::Alpha254)
        .unwrap();
    assert_eq!(w.severity, Severity::Info);
    assert!(w.data["mip0_share"].as_f64().unwrap() > 0.0);
    let r = run(&b, &on, None, None, None);
    assert!(!r.summary.passed);
    assert!(codes(&r).contains(&Code::ValAlphaGuarantee));
}

#[test]
fn header_and_format_mismatch_are_errors() {
    let p = profile(Subtype::PbrAlbedo); // expects BC7_UNORM_SRGB
    let desc = DdsDesc {
        width: 4,
        height: 4,
        format: Format::Bc7,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    let b = dds_bytes(&desc, &[vec![0x20; 16]]).unwrap();
    let r = run(&b, &p, None, None, Some(1));
    assert!(codes(&r).contains(&Code::ValHeader));
    assert!(!r.summary.passed);
}

#[test]
fn mip_count_mismatch_and_unreadable_files_are_header_errors() {
    let p = profile(Subtype::Diffuse);
    let (b, inputs) = dds(&p, &[flat(4, [9, 9, 9, 255]), flat(2, [9, 9, 9, 255])]);
    let r = run(&b, &p, Some(&inputs), None, Some(3));
    assert_eq!(codes(&r), vec![Code::ValHeader]);
    assert!(!r.summary.passed);
    assert!(run(&b, &p, Some(&inputs), None, Some(2)).summary.passed);
    // Fewer encoder-input levels than file levels.
    let r = run(&b, &p, Some(&inputs[..1]), None, None);
    assert!(codes(&r).contains(&Code::ValHeader));
    // Truncated, empty and non-DDS inputs report instead of panicking.
    for bytes in [
        &b[..100],
        &b[..b.len() - 1],
        &[][..],
        b"not a dds file".as_slice(),
    ] {
        let r = run(bytes, &p, None, None, None);
        assert!(!r.summary.passed, "{} bytes", bytes.len());
        assert!(codes(&r).iter().all(|c| *c == Code::ValHeader));
    }
}

#[test]
fn coverage_drift_and_complex_m_margin() {
    let mut p = profile(Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Cutout;
    let half: Vec<[u8; 4]> = (0..1024)
        .map(|i| [128, 128, 128, if i % 32 < 16 { 255 } else { 0 }])
        .collect();
    let (b, inputs) = dds(
        &p,
        &[(32, 32, half), (16, 16, vec![[128, 128, 128, 0]; 256])],
    );
    let r = run(&b, &p, Some(&inputs), Some(0.5), Some(2));
    assert!(codes(&r).contains(&Code::ValCoverage));
    assert_eq!(r.summary.coverage.len(), 2);
    // Drift is a warning: the file still passes.
    assert!(r.summary.passed, "{:?}", r.warnings);

    let p = profile(Subtype::ComplexMaterial);
    let lv = |n: u32| flat(n, [80, 160, 40, 250]);
    let (b, inputs) = dds(&p, &[lv(4), lv(2), lv(1)]);
    let r = run(&b, &p, Some(&inputs), None, Some(3));
    assert!(
        codes(&r).contains(&Code::ValMComplexMargin),
        "{:?}",
        r.warnings
    );
}

#[test]
fn coverage_is_not_checked_below_256_texels_and_needs_a_base() {
    let mut p = profile(Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Cutout;
    assert!(p.coverage_enabled());
    let a_flat = |n: u32, on: bool| flat(n, [128, 128, 128, if on { 255 } else { 0 }]);
    // 8x8 (64 texels) is all transparent against a full base: below the floor, so no warning, but recorded.
    let (b, inputs) = dds(&p, &[a_flat(16, true), a_flat(8, false)]);
    let r = run(&b, &p, Some(&inputs), Some(1.0), None);
    assert!(!codes(&r).contains(&Code::ValCoverage), "{:?}", r.warnings);
    assert_eq!(r.summary.coverage.len(), 2);
    assert!(r.summary.coverage[1] < 0.01);
    // Exactly 16x16 (256 texels) is checked.
    let (b, inputs) = dds(&p, &[a_flat(16, false)]);
    let r = run(&b, &p, Some(&inputs), Some(1.0), None);
    assert!(codes(&r).contains(&Code::ValCoverage));
    // No base coverage: nothing to compare.
    let r = run(&b, &p, Some(&inputs), None, None);
    assert!(r.summary.coverage.is_empty());
    // Coverage preservation off: unchecked.
    p.mips.coverage = CoverageMode::Off;
    let r = run(&b, &p, Some(&inputs), Some(1.0), None);
    assert!(!codes(&r).contains(&Code::ValCoverage));
}

#[test]
fn per_mip_error_is_a_warning_not_a_failure() {
    let p = profile(Subtype::Diffuse);
    let (b, _) = dds(&p, &[flat(8, [10, 10, 10, 255])]);
    let other = [EncoderInput::Ldr(vec![[200, 90, 30, 255]; 64])];
    let r = run(&b, &p, Some(&other), None, None);
    let w = r
        .warnings
        .iter()
        .find(|w| w.code == Code::ValMipError)
        .unwrap();
    assert_eq!(w.severity, Severity::Warning);
    assert!(r.summary.passed);
    assert!(r.summary.min_mip_psnr_db.unwrap() < p.validation.min_mip_psnr_db);
    // An encoder input whose size does not match the level is reported, not a panic.
    let short = [EncoderInput::Ldr(vec![[1, 2, 3, 255]; 10])];
    let r = run(&b, &p, Some(&short), None, None);
    assert!(codes(&r).contains(&Code::ValHeader));
}

#[test]
fn bc1_is_opaque_with_the_rgbcx_model_and_transparent_texels_are_errors() {
    encoders_sys::init(Bc1Model::Ideal).unwrap();
    let mut p = profile(Subtype::Diffuse);
    p.format = Format::Bc1;
    let (b, inputs) = dds(
        &p,
        &[
            (8, 8, noise(8, 8, 1)),
            (4, 4, noise(4, 4, 2)),
            (2, 2, noise(2, 2, 3)),
            (1, 1, noise(1, 1, 4)),
        ],
    );
    let r = run(&b, &p, Some(&inputs), None, Some(4));
    assert!(r.summary.passed, "{:?}", r.warnings);
    assert!(r.summary.alpha.iter().all(|a| a.eq255 == a.pixels));

    // A 3-colour block (c0 <= c1) whose indices are all 3 decodes to transparent black.
    let block = vec![0, 0, 1, 0, 0xFF, 0xFF, 0xFF, 0xFF];
    let b = wrap(&p, 4, 4, &[block]);
    let r = run(&b, &p, None, None, None);
    assert!(!r.summary.passed);
    assert!(codes(&r).contains(&Code::ValAlphaGuarantee));
}

#[test]
fn complex_m_grayscale_and_height_clip() {
    let p = profile(Subtype::ComplexMaterial);
    let px: Vec<[u8; 4]> = (0..64)
        .map(|i| [100, 100, 100, if i % 2 == 0 { 0 } else { 255 }])
        .collect();
    let (b, inputs) = dds(&p, &[(8, 8, px)]);
    let r = run(&b, &p, Some(&inputs), None, Some(1));
    let c = codes(&r);
    assert!(c.contains(&Code::ValMGrayscale), "{:?}", r.warnings);
    assert!(c.contains(&Code::ValMHeightClip), "{:?}", r.warnings);
    // Near-black grey with mid-range height trips neither.
    let px: Vec<[u8; 4]> = (0..64).map(|i| [10, 10, 10, 100 + i as u8]).collect();
    let (b, inputs) = dds(&p, &[(8, 8, px)]);
    let r = run(&b, &p, Some(&inputs), None, Some(1));
    assert!(!codes(&r).contains(&Code::ValMGrayscale));
    assert!(!codes(&r).contains(&Code::ValMHeightClip));
}

#[test]
fn pgpatcher_share_for_envmask_with_alpha() {
    let mut p = profile(Subtype::Envmask);
    p.format = Format::Bc7;
    p.alpha.policy = AlphaPolicy::Data;
    let px: Vec<[u8; 4]> = (0..64)
        .map(|i| [128, 128, 128, if i < 40 { 100 } else { 255 }])
        .collect();
    let (b, inputs) = dds(&p, &[(8, 8, px)]);
    let r = run(&b, &p, Some(&inputs), None, Some(1));
    assert!(
        codes(&r).contains(&Code::ValPgpatcher50),
        "{:?}",
        r.warnings
    );
    let (b, inputs) = dds(&p, &[flat(8, [128, 128, 128, 255])]);
    let r = run(&b, &p, Some(&inputs), None, Some(1));
    assert!(
        !codes(&r).contains(&Code::ValPgpatcher50),
        "{:?}",
        r.warnings
    );
}

#[test]
fn bc6h_peak_ratio() {
    let p = profile(Subtype::PbrEmissive);
    assert_eq!(p.format, Format::Bc6hUf16);
    let one = EncoderInput::Hdr(vec![[0x3C00; 4]; 16]);
    let enc = encode_level(&one, 4, 4, &p, &EncodeCtx::default())
        .ok()
        .unwrap();
    let b = wrap(&p, 4, 4, &[enc]);
    let with_peak = |peak: f32| {
        validate(&ValidationInput {
            bytes: &b,
            profile: &p,
            encoder_input: None,
            base_coverage: None,
            source_peak: Some(peak),
            bc1_model: Bc1Model::Ideal,
            expected_levels: Some(1),
        })
    };
    let ok = with_peak(1.0);
    assert!(ok.warnings.is_empty(), "{:?}", ok.warnings);
    assert!((ok.summary.bc6h_peak_ratio.unwrap() - 1.0).abs() < 0.05);
    let low = with_peak(10.0);
    assert!(low.summary.passed);
    assert!(codes(&low).contains(&Code::ValBc6hPeak));
    assert!(with_peak(0.0).summary.bc6h_peak_ratio.is_none());
    assert!(run(&b, &p, None, None, Some(1))
        .summary
        .bc6h_peak_ratio
        .is_none());
}

/// Task 32: exact opaque holds on every block, rotated mode 4/5 included, so a BC7 encode of opaque noise
/// decodes to alpha 255 at every texel of every level.
#[test]
fn bc7_exact_opaque_noise_decodes_to_255_everywhere() {
    for policy in [AlphaPolicy::Remove, AlphaPolicy::Data] {
        let mut p = profile(Subtype::Diffuse);
        p.format = Format::Bc7;
        p.alpha.policy = policy;
        p.alpha.exact_opaque = true;
        let levels: Vec<Level> = [(37u32, 21u32), (18, 10), (9, 5), (4, 2), (2, 1), (1, 1)]
            .iter()
            .enumerate()
            .map(|(i, &(w, h))| (w, h, noise(w, h, i as u32 + 7)))
            .collect();
        let (enc, inputs) = encode_all(&p, &levels);
        let b = wrap(&p, 37, 21, &enc);
        let r = run(&b, &p, Some(&inputs), None, Some(6));
        assert!(r.summary.passed, "{policy:?}: {:?}", r.warnings);
        assert!(!codes(&r).contains(&Code::Alpha254));
        assert_eq!(r.summary.alpha.len(), 6);
        for (l, a) in r.summary.alpha.iter().enumerate() {
            assert_eq!(a.eq255, a.pixels, "{policy:?} level {l}: {a:?}");
        }
    }
}

/// Dark opaque ramp that upstream bc7e encodes as mode 6 with a p-bit-0 endpoint (decoded alpha 254): the flag
/// is what makes it 255, and Validate catches the same encode without the flag.
#[test]
fn exact_opaque_flag_is_what_makes_dark_blocks_255() {
    let ramp = |w: u32, h: u32| -> Vec<[u8; 4]> {
        (0..w * h)
            .map(|i| {
                let t = ((i / w % 4) * 4 + i % 4) as u8;
                [t, t / 2, 100 + t, 255]
            })
            .collect()
    };
    let mut on = profile(Subtype::Diffuse);
    on.format = Format::Bc7;
    on.alpha.exact_opaque = true;
    let mut off = on.clone();
    off.alpha.exact_opaque = false;
    let levels: Vec<Level> = vec![(32, 32, ramp(32, 32)), (16, 16, ramp(16, 16))];
    let (enc_on, inputs) = encode_all(&on, &levels);
    let (enc_off, _) = encode_all(&off, &levels);
    let r = run(
        &wrap(&on, 32, 32, &enc_on),
        &on,
        Some(&inputs),
        None,
        Some(2),
    );
    assert!(r.summary.passed, "{:?}", r.warnings);
    // Same encode as the flag-off output, validated against the flag-on profile: a guarantee violation, both for
    // `remove` (decoded alpha) and for a data policy (all-255 blocks).
    for policy in [AlphaPolicy::Remove, AlphaPolicy::Data] {
        let mut strict = on.clone();
        strict.alpha.policy = policy;
        let r = run(
            &wrap(&strict, 32, 32, &enc_off),
            &strict,
            Some(&inputs),
            None,
            Some(2),
        );
        assert!(!r.summary.passed, "{policy:?}");
        assert!(codes(&r).contains(&Code::ValAlphaGuarantee), "{policy:?}");
    }
}

#[test]
fn passed_means_no_error_severity_warning() {
    let p = profile(Subtype::Diffuse);
    let r = run(b"DDS ", &p, None, None, None);
    assert!(!r.summary.passed);
    assert!(r.warnings.iter().any(|w| w.severity == Severity::Error));
    assert_eq!(ValidationSummary::default().levels, 0);
}
