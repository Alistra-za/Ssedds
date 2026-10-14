//! Design §6: winning MATO records with the single-pass flags of the winning copy.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{edid, plugin, rec, sub, top};
use common::{instance_fixture, put};
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::sync::atomic::AtomicBool;

/// `n`-byte MATO DATA with the flags u32 at offset 44 (none when `n` < 48).
fn data(n: usize, flags: u32) -> Vec<u8> {
    let mut d = vec![0u8; n];
    if n >= 48 {
        d[44..48].copy_from_slice(&flags.to_le_bytes());
    }
    d
}

fn mato(id: u32, name: &str, n: usize, flags: u32) -> Vec<u8> {
    rec(b"MATO", id, &[edid(name), sub(b"DATA", &data(n, flags))])
}

#[test]
fn winning_matos_carry_the_winners_data_flags() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(
        r,
        "Game/Data/Skyrim.esm",
        &plugin(
            esp::FLAG_ESM,
            &[],
            &[top(
                b"MATO",
                &[
                    mato(0xA00, "SnowMat1P", 52, 1),
                    mato(0xA01, "AshMat1P", 40, 0),
                    mato(0xA02, "PlainMat", 52, 0),
                ],
            )],
        ),
    );
    // A.esp makes PlainMat single-pass: the winning copy's flags count.
    put(
        r,
        "mods/A/A.esp",
        &plugin(
            0,
            &["Skyrim.esm"],
            &[top(b"MATO", &[mato(0xA02, "PlainMat", 48, 1)])],
        ),
    );
    put(r, "profiles/Default/plugins.txt", b"*A.esp\n");
    let mut s = Store::open(&r.join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(r, None).unwrap());
    run(&src, &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    let got: Vec<(String, String, Option<u32>)> = s
        .winning_records("MATO")
        .unwrap()
        .into_iter()
        .map(|w| (w.edid, w.plugin, w.mato_flags))
        .collect();
    assert_eq!(
        got,
        vec![
            ("AshMat1P".into(), "Skyrim.esm".into(), None),
            ("PlainMat".into(), "A.esp".into(), Some(1)),
            ("SnowMat1P".into(), "Skyrim.esm".into(), Some(1)),
        ]
    );
    assert!(s.winning_records("TXST").unwrap().is_empty());
}

/// Owner data (design §9): the MATO query and single-pass flags on the owner's load order. Read-only; the store is
/// a temp file. `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p modscan --release --test mato -- --ignored --nocapture`
#[test]
#[ignore]
fn owner_matos_and_single_pass_flags() {
    use modscan::mo2::loose_origins;
    use modscan::plugins::scan_plugins;
    let root = std::path::PathBuf::from(
        std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root"),
    );
    let db = tempfile::tempdir().unwrap();
    let inst = Instance::load(&root, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let mut store = Store::open(&db.path().join("scan.db")).unwrap();
    scan_plugins(&inst, &loose, &mut store, &AtomicBool::new(false), &|_| {}).unwrap();
    let m = store.winning_records("MATO").unwrap();
    let single = m
        .iter()
        .filter(|w| w.mato_flags.is_some_and(|f| f & 1 == 1))
        .count();
    let unknown = m.iter().filter(|w| w.mato_flags.is_none()).count();
    println!(
        "MATO winners: {} (single-pass {single}, flags unknown (DATA < 48 bytes) {unknown})",
        m.len()
    );
    for w in m.iter().filter(|w| {
        w.edid.starts_with("SnowMaterialObject") || w.edid.starts_with("AshMaterialSolstheim")
    }) {
        println!("{w:?}");
    }
    assert!(!m.is_empty() && single > 0);
}
