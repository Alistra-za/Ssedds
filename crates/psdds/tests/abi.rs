//! The ABI version and the version query (Task 1 of the shared psdds.dll plan).
use psdds::{psdds_abi_version, psdds_version, PsddsControls, PsddsSettings};
use std::ffi::CStr;
use std::mem::offset_of;

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

/// ABI 4 (Note DH): alpha_mode is appended to the settings and alpha_mode_enabled to the controls.
#[test]
fn settings_layout_is_eight_fields() {
    assert_eq!(psdds_abi_version(), 4);
    assert_eq!(std::mem::size_of::<PsddsSettings>(), 32);
    assert_eq!(offset_of!(PsddsSettings, discard_alpha), 24);
    assert_eq!(offset_of!(PsddsSettings, alpha_mode), 28);
    assert_eq!(std::mem::size_of::<PsddsControls>(), 16);
    assert_eq!(offset_of!(PsddsControls, alpha_mode_enabled), 12);
    let h = header();
    assert!(h.contains("discard_alpha") && h.contains("psdds_preset_settings"));
    let at = |s: &str| h.find(s).unwrap_or_else(|| panic!("{s} in header"));
    assert!(at("uint32_t discard_alpha;") < at("uint32_t alpha_mode;"));
    assert!(at("uint32_t alpha_mode;") < at("} PsddsSettings;"));
    assert!(at("uint32_t quality_enabled;") < at("uint32_t alpha_mode_enabled;"));
    assert!(at("uint32_t alpha_mode_enabled;") < at("} PsddsControls;"));
}
