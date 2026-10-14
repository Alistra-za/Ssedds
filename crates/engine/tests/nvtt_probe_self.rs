//! Audit 2026-10-13b B-I1 (DECISIONS AF3): the probe never runs the program it is part of (tessalith.exe picked as
//! the NVTT path relaunched itself). Alone in its file: without the refusal the probe runs this test binary, whose
//! only test then returns at once (the environment variable), instead of probing itself again.
use engine::nvtt::*;

#[test]
fn the_probe_refuses_this_program_itself() {
    if std::env::var_os("SDC_PROBE_SELF_CHILD").is_some() {
        return;
    }
    std::env::set_var("SDC_PROBE_SELF_CHILD", "1");
    let me = std::env::current_exe().unwrap();
    match probe_path(&me) {
        NvttProbe::Missing(why) => assert!(why.contains("itself"), "{why}"),
        f => panic!("{f:?}"),
    }
}
