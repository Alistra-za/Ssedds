//! PGPatcher 2.2's logged 3D indices (owner's PGPatcher logs in the MO2 instance's tools\ParallaxGen\log, 2026-10-14,
//! `[block/name/index]` line prefixes) reproduced from the exact copies PGPatcher read (plan ruling R3). Read-only.
//! `SDC_ACHERUS='<your MO2 instance>' cargo test -p modscan --release --test index3d -- --ignored`

fn check(bytes: &[u8], logged: &[(u32, u32)], what: &str) {
    let scan = nif::scan::scan(bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
    for (block, index) in logged {
        assert_eq!(
            scan.index3d.get(block),
            Some(index),
            "{what}: block {block} ({:?})",
            scan.index3d
        );
    }
}

#[test]
#[ignore]
fn owner_meshes_match_the_pgpatcher_log() {
    let mods = std::path::PathBuf::from(
        std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root"),
    )
    .join("mods");
    let loose: [(&str, &[(u32, u32)]); 3] = [
        (
            r"Skyking Alchemy Ingredients\meshes\plants\birdsnest02.nif",
            &[(10, 2), (15, 0), (20, 1)],
        ),
        (
            r"Rudy HQ - Miscellaneous SE\meshes\loadscreenart\loadscreenadventure01.nif",
            &[
                (3, 10),
                (6, 11),
                (9, 12),
                (12, 13),
                (15, 0),
                (19, 1),
                (23, 14),
                (26, 15),
                (30, 16),
                (36, 17),
                (39, 2),
                (43, 3),
                (46, 4),
                (49, 5),
                (52, 6),
                (55, 7),
                (58, 8),
                (61, 9),
            ],
        ),
        (
            r"Improved Solitude Windmill\meshes\rbimprovedsolitudewindmillmeshes\rbfarmhousewindmill.nif",
            &[
                (133, 2),
                (137, 3),
                (141, 4),
                (145, 5),
                (149, 6),
                (153, 1),
                (160, 7),
                (163, 0),
                (167, 8),
                (171, 9),
                (174, 10),
                (177, 11),
                (181, 12),
                (189, 13),
                (201, 14),
            ],
        ),
    ];
    for (file, logged) in loose {
        check(&std::fs::read(mods.join(file)).unwrap(), logged, file);
    }
    let bsa = vfs::bsa::Bsa::open(
        &mods
            .join("Vibrant Weapons EAE")
            .join("Vibrant weapons EAE.bsa"),
    )
    .unwrap();
    let want = nif::path::canon_mesh(r"meshes\vibrant weapons\magic\fireshockwarhammer.nif");
    let e = bsa
        .entries
        .iter()
        .find(|e| e.norm == want)
        .expect("fireshockwarhammer.nif is in the BSA");
    check(
        &bsa.read(e).unwrap(),
        &[(34, 1), (79, 0)],
        "fireshockwarhammer.nif",
    );
}
