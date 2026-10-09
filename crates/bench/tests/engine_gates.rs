use sdc_bench::{corpus::bench_root, engine_gates};

#[test]
fn engine_path_reproduces_bench_bytes() {
    let r = engine_gates::engine_parity(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn full_chain_exact_opaque_gate() {
    let r = engine_gates::full_chain_exact_opaque(&bench_root()).unwrap();
    assert!(r.pass, "{r}");
}
