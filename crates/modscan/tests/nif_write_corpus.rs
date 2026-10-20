//! Phase A merge gate (spec 5.6): the NIF writer and the Phase A encoders reproduce the owner's real files byte for
//! byte. Owner data is read-only; the winners gate writes only %TEMP%\sdc-v2a-corpus.db.
use nif::extra::{encode_extra, read_extra, EDITABLE};
use nif::header::Header;
use nif::model::read_model;
use nif::net::{encode_net, read_net};
use nif::props::{decode_texture_set_raw, encode_texture_set};
use nif::shape_head::ShapeKind;
use nif::write::NifDoc;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Res {
    identity: Vec<String>,
    /// Header fields the file has wrong (a rebuilt header differs); informational (G4).
    header_wrong: bool,
    decode: Vec<String>,
    encode: Vec<String>,
}

/// None when `nif::scan::scan` does not accept the file.
fn check(b: &[u8]) -> Option<Res> {
    nif::scan::scan(b).ok()?;
    let mut r = Res::default();
    match NifDoc::open(b) {
        Ok(mut d) => {
            if d.bytes() != b {
                r.identity.push("verbatim write differs".into());
            }
            d.rebuild_header();
            r.header_wrong = d.bytes() != b;
        }
        Err(e) => r.identity.push(format!("open: {e}")),
    }
    match read_model(b) {
        Ok(m) => r.decode.extend(
            m.problems
                .into_iter()
                .filter(|p| p.starts_with("block ") && p.contains("declared")),
        ),
        Err(e) => r.decode.push(format!("read_model: {e}")),
    }
    let Ok(h) = Header::parse(b) else {
        return Some(r);
    };
    for i in 0..h.sizes.len() {
        let kind = h.kind(i);
        let (bs, size) = (h.offsets()[i], h.sizes[i] as usize);
        let Some(blk) = b.get(bs..bs + size) else {
            continue;
        };
        let what = |m: &str| format!("block {i} {}: {m}", String::from_utf8_lossy(kind));
        let net_kind = ShapeKind::from_name(kind).is_some()
            || kind == b"BSLightingShaderProperty"
            || kind == b"NiAlphaProperty"
            || (kind.ends_with(b"Node") && kind != b"BSFurnitureMarkerNode");
        if net_kind {
            match read_net(&h, b, i) {
                Ok(n) => {
                    let rebuilt = [
                        &blk[..n.start - bs],
                        &encode_net(n.name, &n.extra, n.controller),
                        &blk[n.end - bs..],
                    ]
                    .concat();
                    if rebuilt != blk {
                        r.encode.push(what("encode_net differs"));
                    }
                }
                Err(e) => r.decode.push(what(&format!("read_net: {e}"))),
            }
        } else if kind == b"BSShaderTextureSet" {
            match decode_texture_set_raw(&h, b, i) {
                Ok(t) if encode_texture_set(&t.slots) != blk => {
                    r.encode.push(what("encode_texture_set differs"))
                }
                Ok(_) => {}
                Err(e) => r.decode.push(what(&format!("texture set: {e}"))),
            }
        } else if EDITABLE.iter().any(|k| k.as_bytes() == kind) {
            match read_extra(&h, b, i) {
                Ok(x) => {
                    if let Some(v) = x.value {
                        let word = |at: usize| {
                            blk.get(at..at + 4)
                                .map_or(-1, |w| i32::from_le_bytes(w.try_into().unwrap()))
                        };
                        let (_, enc) = encode_extra(word(0), &v, word(4));
                        if enc != blk {
                            r.encode.push(what("encode_extra differs"));
                        }
                    }
                }
                Err(e) => r.decode.push(what(&format!("extra: {e}"))),
            }
        }
    }
    Some(r)
}

fn report(gate: &str, files: Vec<(String, Option<Res>)>) {
    let total = files.len();
    let (mut checked, mut hdr) = (0, vec![]);
    let (mut id, mut dec, mut enc) = (vec![], vec![], vec![]);
    for (p, r) in files {
        let Some(r) = r else { continue };
        checked += 1;
        if r.header_wrong {
            hdr.push(p.clone());
        }
        id.extend(r.identity.into_iter().map(|m| format!("{p}: {m}")));
        dec.extend(r.decode.into_iter().map(|m| format!("{p}: {m}")));
        enc.extend(r.encode.into_iter().map(|m| format!("{p}: {m}")));
    }
    println!(
        "{gate}: {total} .nif seen, {checked} accepted by scan; {} identity failures, {} files with header fields already wrong, {} decode mismatches, {} encode mismatches",
        id.len(),
        hdr.len(),
        dec.len(),
        enc.len()
    );
    for (name, v) in [
        ("identity failure", &id),
        ("header fields already wrong", &hdr),
        ("decode mismatch", &dec),
        ("encode mismatch", &enc),
    ] {
        for p in v.iter().take(20) {
            println!("  {name}: {p}");
        }
    }
    assert!(checked > 0, "{gate}: no file checked");
    assert!(
        id.is_empty() && dec.is_empty() && enc.is_empty(),
        "{gate}: failures above"
    );
}

fn nifs_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            nifs_under(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nif")) {
            out.push(p);
        }
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    let v = std::env::var_os(var).map(PathBuf::from);
    if v.is_none() {
        println!("skipped: set {var}");
    }
    v
}

#[test]
#[ignore]
fn loose_corpus_identity() {
    let Some(root) = env_path("SDC_NIF_CORPUS") else {
        return;
    };
    let mut v = vec![];
    nifs_under(&root, &mut v);
    let files = v
        .par_iter()
        .map(|p| {
            let r = std::fs::read(p).ok().and_then(|b| check(&b));
            (p.display().to_string(), r)
        })
        .collect();
    report("loose_corpus_identity", files);
}

#[test]
#[ignore]
fn bsa_identity() {
    let Some(path) = env_path("SDC_BSA") else {
        return;
    };
    let bsa = vfs::bsa::Bsa::open(&path).unwrap();
    let files = bsa
        .entries
        .par_iter()
        .filter(|e| e.norm.ends_with(".nif"))
        .map(|e| (e.norm.clone(), bsa.read(e).ok().and_then(|b| check(&b))))
        .collect();
    report("bsa_identity", files);
}

#[cfg(windows)]
#[test]
#[ignore]
fn winners_identity() {
    use modscan::{mo2::Instance, scan::run, scan::Source, store::Store};
    use std::collections::HashMap;
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    type Open = Arc<(vfs::bsa::Bsa, HashMap<String, usize>)>;

    let Some(root) = env_path("SDC_ACHERUS") else {
        return;
    };
    let db = std::env::temp_dir().join("sdc-v2a-corpus.db");
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut store = Store::open(&db).unwrap();
    let r = run(
        &Source::Mo2(Instance::load(&root, None).unwrap()),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &modscan::win::process_names,
    )
    .unwrap();
    println!("scan: nifs={} parsed={}", r.nifs, r.parsed);
    let ids: Vec<i64> = store
        .conn()
        .prepare("SELECT copy_id FROM nif_winner")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let locs: Vec<(String, i32, String)> = ids
        .into_iter()
        .filter_map(|id| store.copy_location(id).unwrap())
        .filter(|(p, ..)| !p.contains("facegendata/"))
        .map(|(p, k, root, _)| (p, k, root))
        .collect();
    let archives: Mutex<HashMap<String, Open>> = Mutex::new(HashMap::new());
    let files = locs
        .par_iter()
        .map(|(p, kind, root)| {
            let bytes = if *kind == modscan::store::KIND_BSA {
                let a = archives
                    .lock()
                    .unwrap()
                    .entry(root.clone())
                    .or_insert_with(|| {
                        let b = vfs::bsa::Bsa::open(Path::new(root)).unwrap();
                        let ix = (b.entries.iter().enumerate())
                            .map(|(i, e)| (e.norm.clone(), i))
                            .collect();
                        Arc::new((b, ix))
                    })
                    .clone();
                a.1.get(p).and_then(|&i| a.0.read(&a.0.entries[i]).ok())
            } else {
                std::fs::read(Path::new(root).join(p)).ok()
            };
            (p.clone(), bytes.and_then(|b| check(&b)))
        })
        .collect();
    report("winners_identity", files);
}
