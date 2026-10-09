//! The shipped exe must not need the VC++ redistributable (spec §2: static CRT, single exe).
#[test]
fn sdc_exe_does_not_import_vcruntime() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_sdc")).expect("read sdc.exe");
    let hay = String::from_utf8_lossy(&exe).to_ascii_lowercase();
    assert!(
        !hay.contains("vcruntime140.dll"),
        "sdc.exe imports vcruntime140.dll: +crt-static is not in effect"
    );
}
