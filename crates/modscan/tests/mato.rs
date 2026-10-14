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
