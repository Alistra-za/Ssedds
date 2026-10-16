//! A BSA that fails to open in one run (locked, mid-redeploy) keeps its cached rows (audit 2026-10-16 RS-4).
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

#[test]
fn unopenable_bsa_keeps_its_cache_until_it_reads_again() {
    let d = common::instance_fixture("+A\n");
    let r = d.path();
    let n = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
    let good = common::bsa(&[("meshes", "x.nif", &n)]);
    common::put(r, "mods/A/A.bsa", &good);
    common::put(r, "mods/A/A.esp", &b::plugin(0, &[], &[]));
    common::put(r, "profiles/Default/plugins.txt", b"*A.esp\n");
    let scan = || {
        let mut s = Store::open(&r.join("scan.db")).unwrap();
        let src = Source::Mo2(Instance::load(r, None).unwrap());
        let rep = run(&src, &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
        (s, rep)
    };
    let copies = |s: &Store| -> i64 {
        s.conn()
            .query_row("SELECT count(*) FROM nif_copy", [], |r| r.get(0))
            .unwrap()
    };
    let (s, _) = scan();
    assert_eq!(copies(&s), 1);
    drop(s);
    common::put(r, "mods/A/A.bsa", b"not a bsa");
    let (s, rep) = scan();
    assert_eq!(rep.problems.len(), 1, "the open failure is reported");
    assert_eq!(copies(&s), 1, "the failed archive's rows stay as cache");
    drop(s);
    common::put(r, "mods/A/A.bsa", &good);
    let (s, rep) = scan();
    assert_eq!(copies(&s), 1);
    assert_eq!(rep.reused, 1, "and are reused once it opens again");
}
