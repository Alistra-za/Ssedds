//! The ABI version and the version query (Task 1 of the shared psdds.dll plan).
use psdds::{psdds_abi_version, psdds_version, PsddsSettings};
use std::ffi::CStr;

fn header() -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../photoshop/psdds.h");
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn abi_matches_header() {
    let h = header();
    let at = h
        .find("#define PSDDS_ABI_VERSION ")
        .expect("define in header")
        + "#define PSDDS_ABI_VERSION ".len();
    let digits: String = h[at..].chars().take_while(char::is_ascii_digit).collect();
    assert_eq!(h[at + digits.len()..].chars().next(), Some('u'));
    assert_eq!(digits.parse::<u32>().unwrap(), psdds_abi_version());
}

#[test]
fn version_is_workspace() {
    let v = unsafe { CStr::from_ptr(psdds_version()) };
    assert_eq!(v.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn settings_layout_is_seven_fields() {
    assert_eq!(std::mem::size_of::<PsddsSettings>(), 28);
    let h = header();
    assert!(h.contains("discard_alpha") && h.contains("psdds_preset_settings"));
}
