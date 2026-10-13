//! M6 Task 9 (DECISIONS S7): header-only fixes, payload byte-identical; unpatchable faults are refused with a reason.
use engine::codes::{Code, Severity};
use engine::config::{Format, HeaderForm};
use engine::dds::*;
use engine::dds_patch::*;

fn put_u32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn u(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// A valid full-chain 64x64 file; level `i` is filled with byte `0x10 + i`.
fn good(format: Format, srgb: bool, header: ResolvedHeader) -> Vec<u8> {
    let mut levels = vec![];
    let (mut w, mut h) = (64u32, 64u32);
    loop {
        levels.push(vec![0x10 + levels.len() as u8; level_size(format, w, h)]);
        if w == 1 && h == 1 {
            break;
        }
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    dds_bytes(
        &DdsDesc {
            width: 64,
            height: 64,
            format,
            srgb,
            header,
        },
        &levels,
    )
    .unwrap()
}

/// Mip count 1, no MIPMAPCOUNT flag, no COMPLEX|MIPMAP cap: the common "mips not declared" fault.
fn undeclared_mips(mut b: Vec<u8>) -> Vec<u8> {
    put_u32(&mut b, 28, 1);
    let flags = u(&b, 8) & !DDSD_MIPMAPCOUNT;
    put_u32(&mut b, 8, flags);
    put_u32(&mut b, 108, DDSCAPS_TEXTURE);
    b
}

fn patched(b: &[u8], req: &PatchRequest) -> HeaderPatch {
    plan_patch(b, req).unwrap().expect("a patch")
}

#[test]
fn a_wrong_mip_count_is_patched_and_the_payload_is_byte_identical() {
    let orig = undeclared_mips(good(Format::Bc7, false, ResolvedHeader::Dx10));
    let p = patched(&orig, &PatchRequest::default());
    assert_eq!((p.old_header_len, p.new_header_len), (148, 148));
    assert_eq!(&p.bytes[148..], &orig[148..], "payload byte-identical");
    assert!(
        p.fixes.iter().any(|f| f.contains("mip count 1 → 7")),
        "{:?}",
        p.fixes
    );
    let f = read_dds(&p.bytes).unwrap();
    assert_eq!(f.mip_count, 7);
    assert!(check_header(&f).is_empty(), "{:?}", check_header(&f));
    let w = p.warning();
    assert_eq!(
        (w.code, w.severity),
        (Code::DdsHeaderPatched, Severity::Info)
    );
}

#[test]
fn a_wrong_srgb_tag_is_swapped_only_on_request() {
    let orig = good(Format::Bc7, false, ResolvedHeader::Dx10);
    assert!(
        plan_patch(&orig, &PatchRequest::default())
            .unwrap()
            .is_none(),
        "no guess"
    );
    let p = patched(
        &orig,
        &PatchRequest {
            srgb: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(u(&p.bytes, 128), 99);
    assert_eq!(&p.bytes[148..], &orig[148..]);
    assert!(
        p.fixes.iter().any(|f| f.contains("UNORM → sRGB")),
        "{:?}",
        p.fixes
    );
}

#[test]
fn a_legacy_dxt1_that_needs_srgb_becomes_dx10() {
    let orig = good(
        Format::Bc1,
        false,
        resolve_header(Format::Bc1, false, HeaderForm::Auto),
    );
    assert_eq!(&orig[84..88], b"DXT1");
    let p = patched(
        &orig,
        &PatchRequest {
            srgb: Some(true),
            ..Default::default()
        },
    );
    assert_eq!((p.old_header_len, p.new_header_len), (128, 148));
    assert_eq!(&p.bytes[148..], &orig[128..]);
    assert_eq!(
        read_dds(&p.bytes).unwrap().format,
        DdsPixelFormat::Bc1 { srgb: true }
    );
}

#[test]
fn ati2_becomes_dx10_bc5() {
    let mut orig = good(Format::Bc5, false, ResolvedHeader::Legacy);
    orig[84..88].copy_from_slice(b"ATI2");
    let p = patched(&orig, &PatchRequest::default());
    assert_eq!(u(&p.bytes, 128), 83);
    assert_eq!(&p.bytes[148..], &orig[128..]);
}

#[test]
fn missing_linear_size_reserved_and_normal_flag_are_fixed() {
    let mut orig = good(Format::Bc7, false, ResolvedHeader::Dx10);
    put_u32(&mut orig, 20, 0);
    orig[40..44].copy_from_slice(b"NVTT");
    let pf = u(&orig, 80) | DDPF_NORMAL;
    put_u32(&mut orig, 80, pf);
    let p = patched(&orig, &PatchRequest::default());
    assert!(check_header(&read_dds(&p.bytes).unwrap()).is_empty());
    assert_eq!(&p.bytes[148..], &orig[148..]);
}

#[test]
fn a_valid_file_needs_no_patch() {
    for b in [
        good(Format::Bc7, true, ResolvedHeader::Dx10),
        good(Format::Rgba8, false, ResolvedHeader::Legacy),
    ] {
        assert!(plan_patch(&b, &PatchRequest::default()).unwrap().is_none());
    }
}

/// With trailing bytes no level count fills the payload exactly, so the header's own count (7, it fits) is kept;
/// the reserved-field fault is fixed and the 7 trailing bytes are copied.
#[test]
fn trailing_bytes_after_the_chain_are_kept() {
    let mut orig = good(Format::Bc7, false, ResolvedHeader::Dx10);
    orig[40..44].copy_from_slice(b"NVTT");
    orig.extend_from_slice(&[0xAB; 7]);
    let p = patched(&orig, &PatchRequest::default());
    assert_eq!(&p.bytes[148..], &orig[148..]);
    assert!(p.bytes.ends_with(&[0xAB; 7]));
    assert_eq!(read_dds(&p.bytes).unwrap().mip_count, 7);
    assert!(
        p.fixes.iter().any(|f| f.contains("reserved")),
        "{:?}",
        p.fixes
    );
}

fn refused(b: &[u8], req: &PatchRequest) -> String {
    let e = plan_patch(b, req).unwrap_err();
    assert_eq!(e.warning().code, Code::DdsHeaderUnpatchable);
    e.0
}

#[test]
fn a_short_payload_is_refused() {
    let mut b = good(Format::Bc7, false, ResolvedHeader::Dx10);
    b.truncate(b.len() - 5);
    assert!(refused(&b, &PatchRequest::default()).contains("no whole mip chain"));
}

#[test]
fn unpatchable_faults_are_refused_with_a_reason() {
    let bc4 = good(Format::Bc4, false, ResolvedHeader::Dx10);
    assert!(refused(
        &bc4,
        &PatchRequest {
            srgb: Some(true),
            ..Default::default()
        }
    )
    .contains("no _SRGB form"));
    let bc7 = good(Format::Bc7, false, ResolvedHeader::Dx10);
    assert!(refused(
        &bc7,
        &PatchRequest {
            expect_format: Some(Format::Bc3),
            ..Default::default()
        }
    )
    .contains("wrong block format"));
    // One level stored, a full chain wanted.
    let one = dds_bytes(
        &DdsDesc {
            width: 64,
            height: 64,
            format: Format::Bc7,
            srgb: false,
            header: ResolvedHeader::Dx10,
        },
        &[vec![0; level_size(Format::Bc7, 64, 64)]],
    )
    .unwrap();
    assert!(refused(
        &one,
        &PatchRequest {
            require_full_mips: true,
            ..Default::default()
        }
    )
    .contains("missing mips"));
    // BC1 in 3-colour mode with index 3 everywhere: transparent texels.
    let mut bc1 = good(Format::Bc1, false, ResolvedHeader::Dx10);
    for blk in bc1[148..].chunks_mut(8) {
        blk.copy_from_slice(&[0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    }
    assert!(refused(&bc1, &PatchRequest::default()).contains("BC1 with 1-bit alpha"));
    // A cubemap and garbage.
    let mut cube = good(Format::Bc7, false, ResolvedHeader::Dx10);
    put_u32(&mut cube, 136, 4);
    let n = cube.len() - 148;
    cube.extend(std::iter::repeat_n(0u8, n * 5));
    assert!(refused(&cube, &PatchRequest::default()).contains("cubemap"));
    assert!(refused(b"not a dds at all", &PatchRequest::default()).contains("unreadable"));
}

/// R6/P5: a complex_grass file (w x 2w) stores log2(w)+1 levels, ending at 1x2: one short of the full chain.
#[test]
fn complex_grass_chain_one_level_short_is_accepted_two_short_is_refused() {
    let file = |n: u32| {
        let levels: Vec<Vec<u8>> = (0..n)
            .map(|i| vec![0x10 + i as u8; level_size(Format::Bc7, 64 >> i, 128 >> i)])
            .collect();
        undeclared_mips(
            dds_bytes(
                &DdsDesc {
                    width: 64,
                    height: 128,
                    format: Format::Bc7,
                    srgb: false,
                    header: ResolvedHeader::Dx10,
                },
                &levels,
            )
            .unwrap(),
        )
    };
    let req = PatchRequest {
        require_full_mips: true,
        complex_grass: true,
        ..Default::default()
    };
    let p = patched(&file(7), &req);
    assert!(
        p.fixes.iter().any(|f| f.contains("mip count 1 → 7")),
        "{:?}",
        p.fixes
    );
    assert!(refused(&file(6), &req).contains("missing mips"));
    let plain = PatchRequest {
        complex_grass: false,
        ..req
    };
    assert!(refused(&file(7), &plain).contains("missing mips"));
}
