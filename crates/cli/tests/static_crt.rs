//! The shipped exe must not need the VC++ redistributable (spec §2: static CRT, single exe).
#[test]
fn sdc_exe_does_not_import_vcruntime() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_sdc")).expect("read sdc.exe");
    let hay = String::from_utf8_lossy(&exe).to_ascii_lowercase();
    // "vcruntime140" also covers vcruntime140_1.dll; "msvcp140" covers the dynamic C++ runtime.
    for dll in ["vcruntime140", "msvcp140"] {
        assert!(
            !hay.contains(dll),
            "sdc.exe references {dll}: +crt-static is not in effect for the C++ code"
        );
    }
}
