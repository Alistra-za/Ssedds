//! Spec §2 "Results must not change": a fixture modlist scanned cold, then warm, dumps every schema-4 table (ids
//! replaced by natural keys, mtimes by presence) identical to `fixtures/scan_dump.txt`, written once on the baseline
//! code (nif/main 9cb9954) with `SDC_BLESS=1`.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
mod common;
use b::{alt, edid, form, plugin, rec, sub, top, zstr};
use common::{bsa, instance_fixture, put};
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use rusqlite::types::ValueRef;
use std::sync::atomic::AtomicBool;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scan_dump.txt");

fn nif() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap()
}

fn fixture_nif(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../nif/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// The skinned NIF with its first texture set rewritten (same byte length, so no header edit) to three non-empty
/// slots 0, 1 and 7, two of them non-canonical (a non-dds extension, a backslash with upper case).
fn multi_slot_nif() -> Vec<u8> {
    let mut n = nif();
    let old = [
        &[9u8, 0, 0, 0, 18, 0, 0, 0][..],
        b"textures\\white.dds",
        &[0u8; 32],
    ]
    .concat();
    let at = n.windows(old.len()).position(|w| w == old).unwrap();
    let mut new = 9u32.to_le_bytes().to_vec();
    for slot in 0..9 {
        let s: &[u8] = match slot {
            0 => b"x.dds",
            1 => b"y.jpeg",
            7 => b"Z\\q.dds",
            _ => b"",
        };
        new.extend((s.len() as u32).to_le_bytes());
        new.extend(s);
    }
    assert_eq!(new.len(), old.len());
    n[at..at + old.len()].copy_from_slice(&new);
    n
}

/// Loose NIFs in two mods (one path in both), a parse failure, a PGPatcher mod deriving one path, loose and BSA
/// textures, an active plugin-named BSA, and a plugin with a TXST, an alternate texture and a landscape texture.
fn fixture() -> tempfile::TempDir {
    let d = instance_fixture("+PGP\n+B\n+A\n");
    let r = d.path();
    let n = nif();
    put(r, "mods/A/meshes/x.nif", &n);
    put(r, "mods/A/meshes/a_only.nif", &n);
    put(r, "mods/A/meshes/bad.nif", b"not a nif");
    put(r, "mods/B/meshes/x.nif", &n);
    put(r, "mods/A/meshes/multi.nif", &multi_slot_nif());
    put(
        r,
        "mods/B/meshes/fo4.nif",
        &fixture_nif("TestNifFile_Skinned_FO4.nif"),
    );
    put(
        r,
        "mods/B/meshes/le.nif",
        &fixture_nif("TestNifFile_Animated_LE.nif"),
    );
    put(r, "mods/PGP/meshes/x.nif", &n);
    put(r, "mods/PGP/PGPatcher.esp", b"e");
    let crc = crc32fast::hash(&n);
    put(
        r,
        "mods/PGP/ParallaxGen_Diff.json",
        format!(r#"{{"meshes\\x.nif":{{"crc32original":{crc},"crc32patched":1}}}}"#).as_bytes(),
    );
    put(r, "mods/A/textures/a/a_d.dds", b"DDS ");
    put(r, "mods/B/textures/a/a_d.dds", b"DDS ");
    put(
        r,
        "mods/A/Foo.bsa",
        &bsa(&[
            ("meshes\\bsa", "inbsa.nif", &n),
            ("textures\\bsa", "b_d.dds", b"DDS "),
        ]),
    );
    put(
        r,
        "mods/A/Foo.esp",
        &plugin(
            0,
            &[],
            &[
                top(
                    b"TXST",
                    &[rec(
                        b"TXST",
                        0x800,
                        &[edid("ATxst"), sub(b"TX00", &zstr("a\\a_d.dds"))],
                    )],
                ),
                top(
                    b"STAT",
                    &[rec(
                        b"STAT",
                        0x801,
                        &[
                            edid("AStat"),
                            sub(b"MODL", &zstr("x.nif")),
                            sub(b"MODS", &alt(&[("Shape0", 0x800, 0)])),
                        ],
                    )],
                ),
                top(
                    b"LTEX",
                    &[rec(b"LTEX", 0x802, &[edid("ALtex"), form(b"TNAM", 0x800)])],
                ),
            ],
        ),
    );
    // Bar overrides Foo's TXST (master index 0) and adds an EFSH with a direct texture; Baz is a light plugin.
    put(
        r,
        "mods/B/Bar.esp",
        &plugin(
            0,
            &["Foo.esp"],
            &[
                top(
                    b"TXST",
                    &[rec(
                        b"TXST",
                        0x800,
                        &[edid("ATxst"), sub(b"TX00", &zstr("a\\b_d.dds"))],
                    )],
                ),
                top(
                    b"EFSH",
                    &[rec(
                        b"EFSH",
                        0x0100_0801,
                        &[edid("BEfsh"), sub(b"ICON", &zstr("fx\\spark.dds"))],
                    )],
                ),
            ],
        ),
    );
    put(
        r,
        "mods/B/Baz.esp",
        &plugin(
            0x200,
            &[],
            &[top(
                b"EFSH",
                &[rec(
                    b"EFSH",
                    0x800,
                    &[edid("ZEfsh"), sub(b"ICON", &zstr("fx\\z.dds"))],
                )],
            )],
        ),
    );
    put(
        r,
        "profiles/Default/plugins.txt",
        b"*Foo.esp
*Bar.esp
*Baz.esp
",
    );
    d
}

/// `texset` ids rendered as `n:slot0,…,slot8` (slot paths, `-` for empty).
fn texset_cte() -> String {
    let slots: Vec<String> = (0..9)
        .map(|i| format!("ifnull((SELECT p FROM path WHERE id = t.t{i}), '-')"))
        .collect();
    format!(
        "WITH ts(id, k) AS (SELECT t.id, t.n || ':' || {} FROM texset t) ",
        slots.join(" || ',' || ")
    )
}

/// Every schema-4 table, ids replaced by natural keys, as sorted `table|col|col…` lines.
fn dump(s: &Store, root: &std::path::Path) -> String {
    let ts = texset_cte();
    let copy = |k: &str| {
        format!(
            "JOIN nif_copy c ON c.id = {k} JOIN path cp ON cp.id = c.path_id JOIN origin co ON co.id = c.origin_id"
        )
    };
    let queries: Vec<(&str, String)> = vec![
        ("path", "SELECT p FROM path".into()),
        ("origin", "SELECT o.kind, o.name, o.root, (SELECT x.root FROM origin x WHERE x.id = o.parent_origin), o.priority, o.enabled, o.derived_by, o.derived_state FROM origin o".into()),
        ("nif_copy", "SELECT p.p, o.root, c.size, c.mtime IS NOT NULL, c.crc32, c.parse_status, c.bsver, c.le_mesh, c.is_derived, sp.p || '@' || so.root, c.derive_check, c.problem FROM nif_copy c JOIN path p ON p.id = c.path_id JOIN origin o ON o.id = c.origin_id LEFT JOIN nif_copy s ON s.id = c.source_copy_id LEFT JOIN path sp ON sp.id = s.path_id LEFT JOIN origin so ON so.id = s.origin_id".into()),
        ("shape", format!("{ts}SELECT cp.p, co.root, s.block, s.name, s.kind, s.shader_kind, s.shader_type, s.slsf1, s.slsf2, ts.k, s.alpha_flags, s.alpha_threshold, s.params, s.skinned, s.vertex_desc, s.num_vertices, s.num_triangles FROM shape s {} LEFT JOIN ts ON ts.id = s.texset_id", copy("s.copy_id"))),
        ("tex_raw", format!("SELECT cp.p, co.root, r.block, r.slot, r.raw, r.flags FROM tex_raw r {}", copy("r.copy_id"))),
        ("texset", format!("{ts}SELECT k FROM ts")),
        ("tex_use", format!("{ts}SELECT p.p, ts.k, u.slot FROM tex_use u JOIN path p ON p.id = u.tex_path_id JOIN ts ON ts.id = u.texset_id")),
        ("nif_winner", format!("SELECT p.p, cp.p, co.root, w.n_copies FROM nif_winner w JOIN path p ON p.id = w.path_id {}", copy("w.copy_id"))),
        ("nif_winner_src", format!("SELECT p.p, cp.p, co.root FROM nif_winner_src w JOIN path p ON p.id = w.path_id {}", copy("w.copy_id"))),
        ("pgp_diff", "SELECT p.p, d.crc_orig, d.crc_patched FROM pgp_diff d JOIN path p ON p.id = d.path_id".into()),
        ("tex_winner", "SELECT p.p, o.root, w.n_copies FROM tex_winner w JOIN path p ON p.id = w.path_id JOIN origin o ON o.id = w.origin_id".into()),
        ("plugin", "SELECT name, path, size, lo_index, light, flags, masters_json, error_text FROM plugin".into()),
        ("rec", "SELECT pl.name, r.local, r.formid, r.sig, r.edid, r.owner_ok, r.txst_flags, r.decal FROM rec r JOIN plugin pl ON pl.id = r.plugin_id".into()),
        ("rec_winner", "SELECT w.formid, pl.name, w.n_overrides FROM rec_winner w JOIN plugin pl ON pl.id = w.plugin_id".into()),
        ("txst_slot", "SELECT pl.name, t.local, t.formid, t.slot, p.p FROM txst_slot t JOIN plugin pl ON pl.id = t.plugin_id JOIN path p ON p.id = t.path_id".into()),
        ("model_ref", "SELECT pl.name, m.local, m.formid, m.slot, p.p FROM model_ref m JOIN plugin pl ON pl.id = m.plugin_id JOIN path p ON p.id = m.path_id".into()),
        ("alt_tex", "SELECT pl.name, a.local, a.formid, a.model_slot, a.shape_name, a.txst_formid, a.txst_local, a.idx FROM alt_tex a JOIN plugin pl ON pl.id = a.plugin_id".into()),
        ("txst_ref", "SELECT pl.name, t.local, t.formid, t.role, t.txst_formid, t.txst_local FROM txst_ref t JOIN plugin pl ON pl.id = t.plugin_id".into()),
        ("tex_ref", "SELECT pl.name, t.local, t.formid, t.role, p.p FROM tex_ref t JOIN plugin pl ON pl.id = t.plugin_id JOIN path p ON p.id = t.path_id".into()),
    ];
    let roots = [
        root.display().to_string(),
        std::fs::canonicalize(root).unwrap().display().to_string(),
    ];
    let mut lines = vec![];
    for (name, sql) in &queries {
        let mut st = s.conn().prepare(sql).unwrap();
        let n = st.column_count();
        let mut rows = st.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            let cols: Vec<String> = (0..n)
                .map(|i| match r.get_ref(i).unwrap() {
                    ValueRef::Null => "NULL".into(),
                    ValueRef::Integer(v) => v.to_string(),
                    ValueRef::Real(v) => v.to_string(),
                    ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                    ValueRef::Blob(b) => format!("blob{}", b.len()),
                })
                .collect();
            let mut line = format!("{name}|{}", cols.join("|"));
            for r in &roots {
                line = line.replace(r.as_str(), "<root>");
            }
            lines.push(line.replace('\\', "/"));
        }
    }
    lines.sort();
    lines.join("\n") + "\n"
}

fn scan_and_dump(d: &tempfile::TempDir, db: &str) -> String {
    let inst = Instance::load(d.path(), None).unwrap();
    let mut store = Store::open(&d.path().join(db)).unwrap();
    run(
        &Source::Mo2(inst),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &|| vec![],
    )
    .unwrap();
    dump(&store, d.path())
}

fn golden() -> String {
    std::fs::read_to_string(GOLDEN)
        .expect("golden dump missing: run once with SDC_BLESS=1 on the baseline code")
        .replace("\r\n", "\n")
}

/// Deletes a loose NIF and a loose texture, rewrites another NIF (new content, new mtime), for the warm rescan.
fn change(d: &tempfile::TempDir) {
    let r = d.path();
    std::fs::remove_file(r.join("mods/A/meshes/a_only.nif")).unwrap();
    std::fs::remove_file(r.join("mods/B/textures/a/a_d.dds")).unwrap();
    let p = r.join("mods/B/meshes/x.nif");
    let mut n = nif();
    let at = n.windows(9).position(|w| w == b"white.dds").unwrap();
    n[at..at + 9].copy_from_slice(b"whit3.dds");
    std::fs::write(&p, n).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(later)
        .unwrap();
}

#[test]
fn cold_and_warm_scans_match_the_baseline_dump() {
    let d = fixture();
    let cold = scan_and_dump(&d, "scan.db");
    let warm = scan_and_dump(&d, "scan.db");
    change(&d);
    let changed = scan_and_dump(&d, "scan.db");
    // Reusing the store after deletes and an edit must equal a scan of the changed tree from scratch (orphaned `path`
    // rows are kept by design, so that table is excluded; the golden still records it).
    let no_path = |d: &str| -> String {
        d.lines()
            .filter(|l| !l.starts_with("path|"))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };
    assert_eq!(
        no_path(&changed),
        no_path(&scan_and_dump(&d, "fresh.db")),
        "warm rescan differs from a fresh scan"
    );
    let got = format!(
        "== cold ==
{cold}== warm ==
{warm}== changed ==
{changed}"
    );
    if std::env::var_os("SDC_BLESS").is_some() {
        std::fs::create_dir_all(std::path::Path::new(GOLDEN).parent().unwrap()).unwrap();
        std::fs::write(GOLDEN, &got).unwrap();
    }
    assert_eq!(got, golden(), "dumps differ from the baseline");
}

/// Ruling R4: a cold scan that dies part-way (here after its NIF writes; no `end_scan`, connection never closed, as
/// in a killed process) is finished by the next scan with the same tables as a clean one.
#[test]
fn a_scan_that_died_mid_way_is_finished_by_the_next_one() {
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    let inst = Instance::load(d.path(), None).unwrap();
    let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &Source::Mo2(inst),
            &mut s,
            &AtomicBool::new(false),
            &|p| {
                if p.phase == "resolve" {
                    panic!("simulated crash")
                }
            },
            &|| vec![],
        )
    }));
    assert!(died.is_err());
    // Deliberate leak (a killed process never closes its connection); `drop(s)` would also do, but then the TempDir
    // could be removed cleanly on Windows, which is not what a crash leaves behind.
    std::mem::forget(s);
    let got = scan_and_dump(&d, "scan.db");
    assert_eq!(got, scan_and_dump(&d, "fresh.db"));
    // The indexes the schema declares exist after the crash-then-rescan.
    let ro = Store::open_read_only(&d.path().join("scan.db")).unwrap();
    let n: i64 = ro
        .conn()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let fresh = Store::open_read_only(&d.path().join("fresh.db")).unwrap();
    let m: i64 = fresh
        .conn()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        m > 0 && n == m,
        "indexes after crash+rescan: {n}, fresh: {m}"
    );
}

/// PT4-1: `end_scan` runs on the cancel and error paths (synchronous back to NORMAL, per-scan state reset).
#[test]
fn end_scan_runs_after_a_cancelled_and_after_a_failed_scan() {
    let sync = |s: &Store| -> i64 {
        s.conn()
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap()
    };
    let d = fixture();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    let inst = Instance::load(d.path(), None).unwrap();
    let r = run(
        &Source::Mo2(inst),
        &mut s,
        &AtomicBool::new(true),
        &|_| {},
        &|| vec![],
    );
    assert!(r.is_err(), "pre-set cancel flag must cancel");
    assert_eq!(sync(&s), 1, "cancelled cold scan restores NORMAL");
    // A failing cold scan: a table the NIF writer needs is gone.
    let d2 = fixture();
    let mut s2 = Store::open(&d2.path().join("scan.db")).unwrap();
    s2.conn().execute_batch("DROP TABLE shape").unwrap();
    let inst = Instance::load(d2.path(), None).unwrap();
    let r = run(
        &Source::Mo2(inst),
        &mut s2,
        &AtomicBool::new(false),
        &|_| {},
        &|| vec![],
    );
    assert!(r.is_err());
    assert_eq!(sync(&s2), 1, "failed cold scan restores NORMAL");
}
