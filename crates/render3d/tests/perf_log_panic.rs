//! Own test binary: the panic hook is process-wide.
use render3d::perf_log;

#[test]
fn a_panic_is_logged_with_its_message_and_thread() {
    perf_log::log_panics();
    let (r, ev) = perf_log::capture(|| std::panic::catch_unwind(|| panic!("sdc boom")));
    assert!(r.is_err());
    let p: Vec<_> = ev.iter().filter(|e| e["event"] == "panic").collect();
    assert_eq!(p.len(), 1, "{ev:?}");
    assert!(
        p[0]["message"].as_str().unwrap().contains("sdc boom"),
        "{ev:?}"
    );
    assert!(
        p[0]["message"]
            .as_str()
            .unwrap()
            .contains("perf_log_panic.rs"),
        "{ev:?}"
    );
    assert!(p[0]["thread"].is_string());
}
