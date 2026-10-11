mod builder;
use builder::{Nif, W};
use nif::header::Header;
use nif::NifError;

#[test]
fn parses_block_table_and_offsets() {
    let mut n = Nif::new(100);
    n.string("root");
    n.block("NiNode", vec![0; 10]);
    n.block("BSShaderTextureSet", W::default().u32(0).0);
    let b = n.bytes();
    let h = Header::parse(&b).unwrap();
    assert_eq!(h.bsver, 100);
    assert_eq!(h.kind(1), b"BSShaderTextureSet");
    assert_eq!(h.sizes, vec![10, 4]);
    let off = h.offsets();
    assert_eq!(off[1], off[0] + 10);
    assert_eq!(h.string(0), Some(&b"root"[..]));
    assert_eq!(h.string(-1), None);
    h.check_file_length(&b).unwrap();
}

#[test]
fn block_type_index_top_bit_is_masked() {
    let mut n = Nif::new(100);
    n.block("NiNode", vec![]);
    let mut b = n.bytes();
    // the type-index array sits right after the one type name; set its top bit (PhysX flag, plan 01 Q1)
    let pos = b.windows(6).position(|w| w == b"NiNode").unwrap() + 6;
    b[pos + 1] |= 0x80;
    let h = Header::parse(&b).unwrap();
    assert_eq!(h.kind(0), b"NiNode");
}

#[test]
fn rejects_fo4_and_mw_headers() {
    let mut n = Nif::new(130);
    n.block("NiNode", vec![]);
    assert!(matches!(
        Header::parse(&n.bytes()),
        Err(NifError::NotSkyrim { bsver: 130, .. })
    ));
    let mut n = Nif::new(100);
    n.user = 11;
    assert!(matches!(
        Header::parse(&n.bytes()),
        Err(NifError::NotSkyrim { user: 11, .. })
    ));
    let mw = include_bytes!("fixtures/TestNifFile_Skinned_MW.nif");
    assert!(matches!(Header::parse(mw), Err(NifError::NotSkyrim { .. })));
    let fo4 = include_bytes!("fixtures/TestNifFile_Skinned_FO4.nif");
    assert!(matches!(
        Header::parse(fo4),
        Err(NifError::NotSkyrim { bsver: 130, .. })
    ));
}

#[test]
fn truncated_file_is_error_not_panic() {
    let mut n = Nif::new(100);
    n.block("NiNode", vec![0; 10]);
    let b = n.bytes();
    for cut in [0, 5, 40, 60, b.len() - 3] {
        let r = Header::parse(&b[..cut]).and_then(|h| h.check_file_length(&b[..cut]));
        assert!(r.is_err(), "cut at {cut}");
    }
}

#[test]
fn file_length_identity_catches_wrong_sizes() {
    let mut n = Nif::new(100);
    n.block("NiNode", vec![0; 10]);
    let mut b = n.bytes();
    b.insert(b.len() - 8, 0); // one stray byte before the footer
    let h = Header::parse(&b).unwrap();
    assert!(matches!(
        h.check_file_length(&b),
        Err(NifError::FileLength { .. })
    ));
}

#[test]
fn real_se_and_le_fixtures_pass_the_identity() {
    for f in [
        "TestNifFile_Skinned_SE.nif",
        "TestNifFile_Furniture_Col_SE.nif",
        "TestNifFile_Animated_LE.nif",
    ] {
        let b =
            std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let h = Header::parse(&b).unwrap();
        assert!(h.bsver == 100 || h.bsver == 83, "{f}");
        h.check_file_length(&b).unwrap();
    }
}
