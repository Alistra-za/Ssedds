use sdc_bench::{corpus::corpus_root, engine_gates};

#[test]
fn engine_path_reproduces_bench_bytes() {
    let Some(root) = corpus_root() else {
        eprintln!("skipped: no bench corpus (set SDC_BENCH_ROOT)");
        return;
    };
    let r = engine_gates::engine_parity(&root).unwrap();
    assert!(r.pass, "{r}");
}

#[test]
fn full_chain_exact_opaque_gate() {
    let Some(root) = corpus_root() else {
        eprintln!("skipped: no bench corpus (set SDC_BENCH_ROOT)");
        return;
    };
    let r = engine_gates::full_chain_exact_opaque(&root).unwrap();
    assert!(r.pass, "{r}");
}
