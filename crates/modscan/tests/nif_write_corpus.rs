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

/// Spec C 2 examples A-I and C 7 quirk examples (nifparseref/plan/nif-edit-v2/02-corpus.md); meshes-relative.
const SAMPLES: &[&str] = &[
    // A, B, C, I
    "meshes/actors/atronachflame/character assets/fxatronachfiretrail.nif",
    "meshes/actors/dragon/character assets/bonedragon.nif",
    "meshes/actors/dragon/character assets/decaydragon2.nif",
    "meshes/architecture/riften/ratwayhall/riftenrwdoorspecial01.nif",
    "meshes/barreraveneno.nif",
    "meshes/clockwork/furniture/blacksmithforgemarker.nif",
    "meshes/clutter/beehive/beehive01.nif",
    "meshes/dlc01/effects/ashpileghostdlc01soulcairn.nif",
    "meshes/dlc02/architecture/ravenrock/dlc2blacksmithforge01rrnop.nif",
    "meshes/dlc02/dungeons/dwemer/animated/dwekagrumezpanel01/kagrumezcontrolgemact01.nif",
    "meshes/actors/alduin/alduin.nif",
    "meshes/actors/alduin/farskin_alduin.nif",
    "meshes/actors/dlc02/winglessdragon/characterassets/winglessdragon.nif",
    "meshes/actors/dragon/character assets/bossdragon.nif",
    "meshes/dlc02/clothes/miraakrobes/miraakrobesnoskin_0.nif",
    "meshes/dragon war/dragonfrostwaveproject02.nif",
    "meshes/dwemer/dwesmelter.nif",
    "meshes/dlc01/effects/draguremergeground01cap.nif",
    "meshes/effects/greybeardwords/fxgreybeardshoutkrein.nif",
    "meshes/actors/dlc01/spriggan/sprigganfxtestunified.nif",
    "meshes/actors/dlc02/spider_poison/characterassets/spiderpoisonexplosion.nif",
    "meshes/actors/dragon/character assets/alduinunderskin.nif",
    "meshes/actors/dragon/trailertemp/tempdragondeathposebake.nif",
    "meshes/actors/spriggan/sprigganfxtestunified.nif",
    "meshes/clutter/magicanomalyspawner/magicanomalyspawner.nif",
    // D/E
    "meshes/animobjects/dlc01bloodpotion.nif",
    "meshes/animobjects/ms12whitephialrepaired.nif",
    "meshes/animobjects/poisondamagehealth01.nif",
    "meshes/_byoh/architecture/byohhouse/trophy/byohhousetrophyspriggan01.nif",
    "meshes/_resourcepack/effects/rp_fxambsandpile01animated.nif",
    "meshes/animobjects/animobjecttgfalmereye.nif",
    "meshes/arch/eyeouter/lensfx2.nif",
    // F
    "meshes/creationclub/bgssse067/effects/lavabubbles01.nif",
    "meshes/dlc01/dungeons/dwemer/animated/dwespecialforge01/dwespecialforgelava01.nif",
    "meshes/actors/dragon/character assets/decaydragonbloodhead.nif",
    "meshes/actors/dragon/character assets/decaydragonbloodtail.nif",
    "meshes/actors/dragon/character assets/decaydragonbloodwingl.nif",
    "meshes/actors/dragon/character assets/decaydragonbloodwingr.nif",
    "meshes/actors/dragon/character assets/fxdragonresurrectiondirt.nif",
    "meshes/actors/dragon/dragontoweridle.nif",
    "meshes/dlc01/clutter/gargoyle/gargoyleingredient.nif",
    "meshes/actors/dlc02/spider_poison/characterassets/zombiespiderimpact.nif",
    // G (the RaceMenu.bsa ones are not in the meshes BSA and report MISSING)
    "meshes/furniture/rtblacksmithforgemarker.nif",
    "meshes/actors/character/character assets/body_magicoverlay.nif",
    // H
    "meshes/architecture/solitude/clutter/slighthousefire.nif",
    "meshes/artsick/eyemod/colour/eyecolourblue.nif",
    "meshes/armor/[d] bdor/hair/pcw/251/251_0.nif",
    "meshes/actors/character/facegendata/facegeom/skyrim.esm/000132aa.nif",
    // multiple BSLSP controllers on one property
    "meshes/actors/dlc01/elk/blackreachdeer.nif",
    "meshes/actors/dlc01/sabrecat/dlc1sabrecat.nif",
    "meshes/actors/dragon/character assets/underskin.nif",
    "meshes/architecture/winterhold/winterholdsymbol02.nif",
    // C 7 quirks
    "meshes/shock03.nif",
    "meshes/venom02.nif",
    "meshes/architecture/riften/riftendoor02.nif",
    "meshes/clutter/civilwar/catapult/civilwarcatapult01.nif",
    "meshes/cs/skin/skinmaterial.nif",
    "meshes/architecture/gm_cabininterior.nif",
    "meshes/4uplod-m/caramelhl.nif",
    "meshes/magic/dragonflamewaveproject01.nif",
    "meshes/clutter/quest/barenziahcrownwearable_go.nif",
    "meshes/cotn/winterhold/winterholdbarn.nif",
    "meshes/oaristys/clutter/hangingtankardsshelf03.nif",
    "meshes/dlc01/plants/cavewormgroup/caveworm3group.nif",
    "meshes/architecture/riften/rtmarketstalldoor01.nif",
    "meshes/magus/mysticism/reanimateritualhandeffects1h.nif",
];

/// Loose file under the mods folder (first mod that has it), else the BSA.
fn sample_bytes(root: &Path, bsa: Option<&vfs::bsa::Bsa>, rel: &str) -> Option<(String, Vec<u8>)> {
    for m in std::fs::read_dir(root.join("mods")).ok()?.flatten() {
        if let Ok(b) = std::fs::read(m.path().join(rel)) {
            return Some((m.file_name().to_string_lossy().into_owned(), b));
        }
    }
    let bsa = bsa?;
    let e = bsa.entries.iter().find(|e| e.norm == rel)?;
    Some(("BSA".into(), bsa.read(e).ok()?))
}

type SampleOp = (&'static str, Vec<u32>, nif::edit::Op);

/// One Phase A op of each kind that fits the file, with the blocks the op may change.
fn sample_ops(b: &[u8]) -> Vec<SampleOp> {
    use nif::edit::{anchor_role, anchor_shape, Op, Role};
    use nif::extra::ExtraValue;
    use nif::fields::{locate, Field, Value};
    let (m, h) = (read_model(b).unwrap(), Header::parse(b).unwrap());
    let mut v: Vec<SampleOp> = vec![];
    if let Some((s, l)) = m
        .shapes
        .iter()
        .find_map(|s| s.shader.and_then(|x| m.lighting.get(&x)).map(|l| (s, l)))
    {
        let at = anchor_role(&m, Some(s.block), Role::Shader).unwrap();
        let base = locate(&h, b, l.prop.block as usize, Field::Glossiness)
            .unwrap()
            .1;
        v.push((
            "gloss",
            vec![l.prop.block],
            Op::SetField {
                at,
                field: Field::Glossiness,
                value: Value::F32(7.5f32.to_bits()),
                base,
            },
        ));
        let slot0 = l
            .texture_set
            .and_then(|t| m.texsets.get(&t).map(|x| (t, x)))
            .and_then(|(t, x)| Some((t, x.raw.slots.first()?)))
            .filter(|(_, s)| s.is_ascii());
        if let Some((set, slot)) = slot0 {
            v.push((
                "texpath",
                vec![set, l.prop.block],
                Op::SetTexturePath {
                    at: anchor_shape(&m, s.block),
                    slot: 0,
                    path: "textures\\a11\\sample.dds".into(),
                    base: String::from_utf8(slot.clone()).unwrap(),
                },
            ));
        }
    }
    // the most shared alpha, so SplitAlpha has something to split
    let alpha_shape = m
        .shapes
        .iter()
        .filter_map(|s| s.alpha.and_then(|a| m.alphas.get(&a)).map(|a| (s, a)))
        .max_by_key(|(_, a)| a.prop.users.len());
    if let Some((s, a)) = alpha_shape {
        let Value::U16(flags) = locate(&h, b, a.prop.block as usize, Field::AlphaFlags)
            .unwrap()
            .1
        else {
            unreachable!()
        };
        v.push((
            "alpha-bits",
            vec![a.prop.block],
            Op::SetBits {
                at: anchor_role(&m, Some(s.block), Role::Alpha).unwrap(),
                field: Field::AlphaFlags,
                mask: 1,
                value: u32::from(!flags & 1),
                base: u32::from(flags & 1),
            },
        ));
        v.push((
            "split-alpha",
            vec![s.block],
            Op::SplitAlpha {
                at: anchor_shape(&m, s.block),
            },
        ));
    }
    if let (Some(root), Some(at)) = (m.root, anchor_role(&m, None, Role::Root)) {
        v.push((
            "add-extra",
            vec![root],
            Op::AddExtra {
                at,
                kind: "NiIntegerExtraData".into(),
                name: "A11Sample".into(),
                value: ExtraValue::Int(7),
            },
        ));
    }
    v
}

#[test]
#[ignore]
fn real_file_edit_samples() {
    use nif::edit::{apply, Outcome};
    let Some(root) = env_path("SDC_ACHERUS") else {
        return;
    };
    let bsa = std::env::var_os("SDC_BSA").and_then(|p| vfs::bsa::Bsa::open(Path::new(&p)).ok());
    let (mut failures, mut ran, mut found) = (vec![], 0, 0);
    let mut per_kind: std::collections::BTreeMap<&str, u32> = Default::default();
    for rel in SAMPLES {
        let Some((src, b)) = sample_bytes(&root, bsa.as_ref(), rel) else {
            println!("MISSING {rel}");
            continue;
        };
        let Ok(m0) = read_model(&b) else {
            println!("UNREADABLE {rel} ({src})");
            continue;
        };
        found += 1;
        let h0 = Header::parse(&b).unwrap();
        let mut line = vec![];
        for (label, touched, op) in sample_ops(&b) {
            let r = apply(&b, &[op]).unwrap();
            let fail = |why: String| format!("{rel} [{label}]: {why}");
            match &r.outcomes[0] {
                Outcome::Applied | Outcome::ReAnchored { .. } => {
                    ran += 1;
                    *per_kind.entry(label).or_default() += 1;
                    line.push(format!("{label}=ok"));
                    let m = match read_model(&r.bytes) {
                        Ok(m) => m,
                        Err(e) => {
                            failures.push(fail(format!("read_model: {e}")));
                            continue;
                        }
                    };
                    let new: Vec<_> = (m.problems.iter())
                        .filter(|p| !m0.problems.contains(p))
                        .collect();
                    if !new.is_empty() {
                        failures.push(fail(format!("new problems {new:?}")));
                    }
                    let h = Header::parse(&r.bytes).unwrap();
                    if let Err(e) = h.check_file_length(&r.bytes) {
                        failures.push(fail(format!("check_file_length: {e}")));
                    }
                    let blk = |h: &Header, b: &[u8], k: usize| {
                        b[h.offsets()[k]..h.offsets()[k] + h.sizes[k] as usize].to_vec()
                    };
                    for k in (0..h0.sizes.len()).filter(|&k| !touched.contains(&(k as u32))) {
                        if blk(&h, &r.bytes, k) != blk(&h0, &b, k) {
                            failures.push(fail(format!("untouched block {k} changed")));
                        }
                    }
                }
                Outcome::Refused(why) => {
                    line.push(format!("{label}=refused({why})"));
                    if r.bytes != b {
                        failures.push(fail("refused but bytes changed".into()));
                    }
                }
                o => failures.push(fail(format!("unexpected {o:?}"))),
            }
        }
        println!("{rel} ({src}): {}", line.join(" "));
    }
    println!(
        "{ran} ops applied in {found} files; per kind {per_kind:?}; {} failures",
        failures.len()
    );
    for f in &failures {
        println!("  FAIL {f}");
    }
    assert!(
        found >= 63,
        "only {found} of the sample files were found (63 expected)"
    );
    for k in ["gloss", "texpath", "alpha-bits", "split-alpha", "add-extra"] {
        assert!(per_kind.get(k).is_some_and(|&n| n > 0), "no {k} op applied");
    }
    assert!(failures.is_empty(), "failures above");
}

/// SLSF2 flag census per (BSLightingShaderProperty shader type, CS TruePBR bit 23). Research for v2 A17; asserts only
/// that files were found. Env: SDC_ACHERUS (winners) and SDC_BSA (one archive); output also to the scratchpad file.
#[cfg(windows)]
#[test]
#[ignore]
fn slsf_bit_census() {
    use modscan::{mo2::Instance, scan::run, scan::Source, store::Store};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    type Open = Arc<(vfs::bsa::Bsa, HashMap<String, usize>)>;
    const BITS: [u32; 6] = [12, 24, 25, 26, 27, 30];
    type Hits = Vec<((u32, bool), Vec<String>)>;
    type Cells = BTreeMap<(u32, bool), BTreeMap<String, (u64, Vec<String>)>>;

    // Counter names hit by every BSLSP block of one file.
    fn census(b: &[u8]) -> Option<Hits> {
        nif::scan::scan(b).ok()?;
        let m = read_model(b).ok()?;
        Some(
            m.lighting
                .values()
                .map(|l| {
                    let (t, f) = (l.lighting.shader_type, l.lighting.slsf2);
                    let bit = |n: u32| (f >> n) & 1 == 1;
                    let mut c = vec!["shapes".to_string()];
                    c.extend(BITS.iter().filter(|&&n| bit(n)).map(|n| format!("bit{n}")));
                    if t == 11 && !bit(24) {
                        c.push("type11_without_bit24".into());
                    }
                    if t != 11 && bit(24) {
                        c.push("bit24_with_type_not_11".into());
                    }
                    ((t, bit(23)), c)
                })
                .collect(),
        )
    }
    fn merge(cells: &mut Cells, hits: Hits, origin: &str) {
        for (k, names) in hits {
            for n in names {
                let e = cells.entry(k).or_default().entry(n).or_default();
                e.0 += 1;
                if e.1.len() < 5 && !e.1.iter().any(|x| x == origin) {
                    e.1.push(origin.to_string());
                }
            }
        }
    }
    fn table(title: &str, files: u64, cells: &Cells) -> String {
        let mut s = format!("== {title}: {files} NIFs accepted ==\n");
        for ((t, pbr), c) in cells {
            s += &format!("\ntype {t} pbr={pbr}\n");
            for (n, (cnt, ex)) in c {
                s += &format!("  {n}: {cnt}\n");
                for e in ex {
                    s += &format!("      {e}\n");
                }
            }
        }
        s
    }

    let (Some(root), Some(bsa_path)) = (env_path("SDC_ACHERUS"), env_path("SDC_BSA")) else {
        return;
    };
    let mut out = String::new();

    let bsa = vfs::bsa::Bsa::open(&bsa_path).unwrap();
    let name = bsa_path.file_name().unwrap().to_string_lossy().to_string();
    let found: Vec<(String, Hits)> = bsa
        .entries
        .par_iter()
        .filter(|e| e.norm.ends_with(".nif"))
        .filter_map(|e| Some((e.norm.clone(), census(&bsa.read(e).ok()?)?)))
        .collect();
    assert!(!found.is_empty(), "no BSA nif accepted");
    let mut cells = Cells::new();
    let n = found.len() as u64;
    for (p, h) in found {
        merge(&mut cells, h, &format!("{name}:{p}"));
    }
    out += &table(&format!("BSA {name}"), n, &cells);

    let db = std::env::temp_dir().join("sdc-v2a-corpus.db");
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut store = Store::open(&db).unwrap();
    run(
        &Source::Mo2(Instance::load(&root, None).unwrap()),
        &mut store,
        &AtomicBool::new(false),
        &|_| {},
        &modscan::win::process_names,
    )
    .unwrap();
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
    let found: Vec<(String, Hits)> = locs
        .par_iter()
        .filter_map(|(p, kind, root)| {
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
            }?;
            // origin = mod folder (or archive) name + data path
            let n = if *kind == modscan::store::KIND_BSA {
                2
            } else {
                1
            };
            let mut c: Vec<&str> = root.rsplit(['\\', '/']).take(n).collect();
            c.reverse();
            let tail = c.join("/");
            Some((format!("{tail}:{p}"), census(&bytes)?))
        })
        .collect();
    assert!(!found.is_empty(), "no winner nif accepted");
    let mut cells = Cells::new();
    let n = found.len() as u64;
    for (o, h) in found {
        merge(&mut cells, h, &o);
    }
    out += &format!("\n{}", table("mod winners", n, &cells));

    println!("{out}");
    let _ = std::fs::write(std::env::temp_dir().join("slsf-census.txt"), &out);
}
