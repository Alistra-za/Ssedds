//! v2 Phase D, D6c: sdc's own NIF material against the capture (extraction_matches_capture, report-only). A hybrid
//! replay: geometry, scene cbuffers, engine textures, samplers and depth from the capture; the material (PS b1 used
//! variables, the PS b2 material fields, VS b1 TexcoordOffset, material textures) from sdc's own `Renderer::load`.
//! Differences isolate material mapping (04 Q4-Q8). Needs SDC_GOLDEN, SDC_CS_SHADERS and SDC_ACHERUS.
mod common;

use render3d::compile::{compile, used_vars, CbVar, SrvKind, VarKind};
use render3d::golden::{self, Noise};
use render3d::permutation::Defines;
use render3d::replay::{cb_bytes, vs_defines};
use render3d::source::SourceSet;
use render3d::textures::register_for_slot;
use render3d::{load_fixture, parse, DebugOutput, ParsedNif, Renderer, Replay, ShapeId};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Shape name → game path. Sources in the D6c report (Skyrim.esm STAT MODL; the s6 copy for ShaderSpheres02).
const NIFS: &[(&str, &str)] = &[
    ("Coffin", r"meshes\clutter\common\commoncoffinopen02.nif"),
    ("Object174", r"meshes\furniture\noble\nobletablelow02.nif"),
    ("dibella_L.004", r"meshes\clutter\statues\statuedibella.nif"),
    (
        "BlackreachCrystalL01",
        r"meshes\clutter\blackreach\blackreachcrystall01.nif",
    ),
    (
        "NorRubblePiece01",
        r"meshes\dungeons\nordic\rubble\norrubblepiece01.nif",
    ),
    (
        "ShaderSpheres02",
        r"meshes\architecture\sovengarde\testcollision01.nif",
    ),
    (
        "SOVStairsPiece01",
        r"meshes\architecture\sovengarde\sovstairspiece01.nif",
    ),
];

const EXTRACTION: &[&str] = &[
    "Skyrim_AE_1.6.1170_frame69672/ev31535_Coffin",
    "Skyrim_AE_1.6.1170_frame69672/ev31833_Object174",
    // Added: the frame's other Object174, the one whose textures are the NobleTableLow02 winner's (D6c report).
    "Skyrim_AE_1.6.1170_frame69672/ev31903_Object174",
    "Skyrim_AE_1.6.1170_frame69672/ev32004_dibella_L.004_3",
    "Skyrim_AE_1.6.1170_frame82089/ev35495_BlackreachCrystalL01_10",
    "Skyrim_AE_1.6.1170_frame82089/ev35708_NorRubblePiece01_0",
    "Skyrim_AE_1.6.1170_frame82089/ev37237_ShaderSpheres02_10",
    "Skyrim_AE_1.6.1170_frame87144/ev35533_SOVStairsPiece01_6",
];

/// Material defines compared (VC is VS-only in sdc and the geometry is the capture's).
const M: &[&str] = &[
    "ENVMAP",
    "PARALLAX",
    "FACEGEN",
    "FACEGEN_RGB_TINT",
    "HAIR",
    "MULTI_LAYER_PARALLAX",
    "EYE",
    "SKINNED",
    "MODELSPACENORMALS",
    "PROJECTED_UV",
    "TRUE_PBR",
    "DO_ALPHA_TEST",
    "DEPTH_WRITE_DECALS",
    "GLINT",
    "ANISO_LIGHTING",
    "SPECULAR",
    "SOFT_LIGHTING",
    "RIM_LIGHTING",
    "BACK_LIGHTING",
];

/// The PS b2 (PerGeometry) fields that come from the material (Lighting.hlsl:564-582).
const B2_MATERIAL: &[&str] = &["MaterialData", "AlphaTestRef", "EmitColor"];

/// Engine textures: bound by the game whatever the material (noise, shadow mask).
const ENGINE_REGS: &[u32] = &[11, 14];

/// The owner's game files, read-only: MO2 loose winner first, else the highest-ranked active BSA. No Store, no
/// scan.db.
struct Game {
    origins: Vec<modscan::mo2::LooseOrigin>,
    bsas: Vec<vfs::bsa::Bsa>,
    /// Canonical path → (bsa index, entry index), the highest rank winning.
    index: HashMap<String, (usize, usize)>,
}

impl Game {
    fn load(acherus: &Path) -> Game {
        let inst = modscan::mo2::Instance::load(acherus, None).unwrap();
        let (mut origins, _) = modscan::mo2::loose_origins(&inst).unwrap();
        origins.sort_by_key(|o| std::cmp::Reverse(o.priority));
        let (active, _) = modscan::archives::active_bsas(&inst, &origins);
        let (mut bsas, mut index) = (Vec::new(), HashMap::new());
        // active_bsas is lowest rank first: later inserts win.
        for a in &active {
            let Ok(b) = vfs::bsa::Bsa::open(&a.path) else {
                continue;
            };
            for (i, e) in b.entries.iter().enumerate() {
                index.insert(e.norm.clone(), (bsas.len(), i));
            }
            bsas.push(b);
        }
        Game {
            origins,
            bsas,
            index,
        }
    }

    /// (origin, bytes) of a game path (`meshes\…` or canonical `textures/…`).
    fn file(&self, rel: &str) -> Option<(String, Vec<u8>)> {
        let norm = nif::path::canon_mesh(rel);
        let win = norm.replace('/', "\\");
        if let Some(o) = self.origins.iter().find(|o| o.root.join(&win).is_file()) {
            return Some((
                format!("loose {}", o.name),
                std::fs::read(o.root.join(&win)).ok()?,
            ));
        }
        let (b, e) = *self.index.get(&norm)?;
        let bsa = &self.bsas[b];
        Some((
            format!("bsa {}", bsa.path.file_name()?.to_string_lossy()),
            bsa.read(&bsa.entries[e]).ok()?,
        ))
    }
}

struct Env {
    root: PathBuf,
    shaders: PathBuf,
    game: Game,
    cache: tempfile::TempDir,
}

fn env() -> Option<Env> {
    let (Ok(root), Ok(shaders), Ok(acherus)) = (
        std::env::var("SDC_GOLDEN"),
        std::env::var("SDC_CS_SHADERS"),
        std::env::var("SDC_ACHERUS"),
    ) else {
        println!("skipped: set SDC_GOLDEN, SDC_CS_SHADERS and SDC_ACHERUS");
        return None;
    };
    Some(Env {
        root: root.into(),
        shaders: shaders.into(),
        game: Game::load(Path::new(&acherus)),
        cache: tempfile::tempdir().unwrap(),
    })
}

fn key(d: &str) -> &str {
    d.split('=').next().unwrap_or(d)
}

fn to_defines(list: &[String]) -> Defines {
    Defines(
        list.iter()
            .map(|d| match d.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (d.clone(), "1".to_string()),
            })
            .collect(),
    )
}

fn json(p: &Path) -> Value {
    serde_json::from_str(&render3d::replay::nonfinite_to_null(
        &std::fs::read_to_string(p).unwrap(),
    ))
    .unwrap()
}

/// sdc's load of the subject's NIF: the shape, how it was found, and the shape's slot textures by register.
struct SdcShape {
    id: ShapeId,
    rule: String,
    nif_origin: String,
    /// (register, canonical path, bytes or None when the resolver found nothing).
    slots: Vec<(u32, String, Option<Vec<u8>>)>,
    /// LoadReport errors + compile errors.
    errors: Vec<String>,
}

/// Loads the subject's NIF into `ren` (features = the capture's non-material defines, so ENABLE_LL and the CS
/// features match) and finds the shape by identity.shapeName, then ShapeId(shapeIndex) when several share it.
fn load_sdc(ren: &mut Renderer, env: &Env, dir: &Path, r: &Replay) -> Result<SdcShape, String> {
    let ident = &json(&dir.join("state.json"))["identity"];
    let name = ident["shapeName"].as_str().ok_or("no identity.shapeName")?;
    let index = ident["shapeIndex"].as_u64();
    let path = NIFS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("{name}: not in NIFS"))?
        .1;
    let (nif_origin, bytes) = env
        .game
        .file(path)
        .ok_or_else(|| format!("{path}: not found"))?;
    let parsed: ParsedNif = parse(&bytes);
    let with = |n: &str| -> Vec<&render3d::ParsedShape> {
        parsed
            .shapes
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case(n))
            .collect()
    };
    // The capture splits a "name:N" NIF shape name into shapeName and shapeIndex.
    let suffixed = index.map(|i| format!("{name}:{i}"));
    let by_index = index.and_then(|i| parsed.shapes.iter().find(|s| s.id == ShapeId(i as u32)));
    let (shape, how) = match (with(name).as_slice(), suffixed.as_deref().map(with)) {
        ([one], _) => (*one, "shapeName unique".to_string()),
        (_, Some(v)) if v.len() == 1 => (v[0], format!("NIF name {:?}", v[0].name)),
        (several, _)
            if several.len() > 1 && by_index.is_some_and(|b| b.name.eq_ignore_ascii_case(name)) =>
        {
            (
                by_index.unwrap(),
                format!("shapeName x{}, ShapeId(shapeIndex)", several.len()),
            )
        }
        _ => {
            return Err(format!(
                "{name} (index {index:?}): no unique shape in {path}; shapes {:?}",
                parsed
                    .shapes
                    .iter()
                    .map(|s| (s.id.0, &s.name))
                    .collect::<Vec<_>>()
            ))
        }
    };
    let rule = format!(
        "{how} -> block {} {:?}; ShapeId(shapeIndex {index:?}) {}",
        shape.id.0,
        shape.name,
        match by_index {
            Some(b) if b.id == shape.id => "is the same block".to_string(),
            Some(b) => format!("is block {} {:?}", b.id.0, b.name),
            None => "absent".to_string(),
        }
    );
    let m = shape.material.clone();
    let id = shape.id;
    // CS features as captured: everything but the material defines, DEFERRED, VC and K=V values.
    let features: Vec<String> = r.defines[0]
        .iter()
        .filter(|d| {
            !d.contains('=') && !M.contains(&d.as_str()) && !matches!(d.as_str(), "DEFERRED" | "VC")
        })
        .cloned()
        .collect();
    ren.set_features(features);
    let rep = ren.load_parsed_sync(&parsed, &|p| env.game.file(p).map(|f| f.1));
    let msn = m.slsf1 & (1 << 12) != 0;
    let slots = m
        .slots
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.norm.is_empty())
        .filter_map(|(slot, p)| {
            let reg = register_for_slot(m.shader_type, m.is_pbr(), msn, slot)?;
            Some((reg, p.norm.clone(), env.game.file(&p.norm).map(|f| f.1)))
        })
        .collect();
    Ok(SdcShape {
        id,
        rule,
        nif_origin,
        slots,
        errors: rep.errors.into_iter().chain(rep.compile_errors).collect(),
    })
}

/// One compared cbuffer field.
struct Field {
    cb: &'static str,
    var: String,
    offset: u32,
    capture: Vec<f32>,
    sdc: Option<Vec<f32>>,
    ok: bool,
}

fn floats(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// uint/int/bool exact; floats within 1e-5·max(1, |capture|).
fn same(kind: VarKind, a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| match kind {
            VarKind::Float => (x - y).abs() <= 1e-5 * y.abs().max(1.0),
            _ => x.to_bits() == y.to_bits(),
        })
}

/// The capture's used material fields (PS b1 PerMaterial, the PS b2 material fields, VS b1 TexcoordOffset), each
/// with its reflected layout var and cbuffer size, keyed by fixture cbuffer stem.
fn material_fields(env: &Env, r: &Replay) -> Result<Vec<(&'static str, CbVar, u32)>, String> {
    let src = SourceSet::new(env.shaders.clone()).get(Path::new("Lighting.hlsl"))?;
    let ps_list = &r.defines[0];
    let ps_d: Vec<String> = std::iter::once("PSHADER".to_string())
        .chain(ps_list.iter().cloned())
        .collect();
    let mut out = Vec::new();
    for (stage, target, defs) in [
        ("ps", "ps_5_0", ps_d),
        ("vs", "vs_5_0", vs_defines(ps_list, &r.mesh)),
    ] {
        let c = compile(&src, &to_defines(&defs), target, env.cache.path())?;
        let used: BTreeSet<(String, String)> = used_vars(&c.bytecode)?.into_iter().collect();
        for l in &c.reflection.cbuffers {
            let stem: &'static str = match (stage, l.slot) {
                ("ps", 1) => "ps_b1",
                ("ps", 2) => "ps_b2",
                ("vs", 1) => "vs_b1",
                _ => continue,
            };
            for v in &l.vars {
                let wanted = match stem {
                    "ps_b1" => true,
                    "ps_b2" => B2_MATERIAL.contains(&v.name.as_str()),
                    _ => v.name == "TexcoordOffset",
                };
                if wanted && used.contains(&(l.name.clone(), v.name.clone())) {
                    out.push((stem, v.clone(), l.size));
                }
            }
        }
    }
    out.sort_by_key(|(s, v, _)| (*s, v.offset));
    Ok(out)
}

fn compare_fields(
    ren: &Renderer,
    id: ShapeId,
    r: &Replay,
    fields: &[(&'static str, CbVar, u32)],
) -> Vec<Field> {
    fields
        .iter()
        .map(|(stem, v, size)| {
            let raw = cb_bytes(&r.cbuffers[*stem], *size);
            let capture = floats(&raw[v.offset as usize..(v.offset + v.size) as usize]);
            let sdc = if *stem == "vs_b1" {
                ren.draw_vs_cb_floats(id, &v.name)
            } else {
                ren.draw_cb_floats(id, &v.name)
            };
            let ok = sdc.as_ref().is_some_and(|s| same(v.kind, s, &capture));
            Field {
                cb: stem,
                var: v.name.clone(),
                offset: v.offset,
                capture,
                sdc,
                ok,
            }
        })
        .collect()
}

/// 04 Q4-Q8 group of a material field.
fn group(var: &str) -> &'static str {
    match var {
        "SpecularColor" => "Q5 specular",
        "EnvmapData" | "MaterialData" => "Q6 envmap/LOD",
        "EmitColor" => "Q7 emissive",
        "PBRFlags" | "PBRParams1" | "PBRParams2" | "MultiLayerParallaxData" => "Q8 PBR",
        _ => "other",
    }
}

fn dxgi_name(f: i32) -> String {
    match f {
        2 => "R32G32B32A32_FLOAT",
        10 => "R16G16B16A16_FLOAT",
        28 => "R8G8B8A8_UNORM",
        29 => "R8G8B8A8_UNORM_SRGB",
        71 => "BC1_UNORM",
        72 => "BC1_SRGB",
        74 => "BC2_UNORM",
        75 => "BC2_SRGB",
        77 => "BC3_UNORM",
        78 => "BC3_SRGB",
        80 => "BC4_UNORM",
        83 => "BC5_UNORM",
        87 => "B8G8R8A8_UNORM",
        91 => "B8G8R8A8_UNORM_SRGB",
        95 => "BC6H_UF16",
        98 => "BC7_UNORM",
        99 => "BC7_SRGB",
        _ => return format!("DXGI {f}"),
    }
    .to_string()
}

/// The texel data of a DDS file: past the 128-byte header, and the DX10 extension when present.
fn dds_data(b: &[u8]) -> Option<&[u8]> {
    let skip = if b.get(84..88)? == b"DX10" { 148 } else { 128 };
    b.get(skip..)
}

fn dds_wh(b: &[u8]) -> Option<(u32, u32)> {
    let u = |o: usize| Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?));
    Some((u(16)?, u(12)?))
}

/// Writes sdc's material into the capture's replay: the compared fields sdc has, and sdc's slot textures at its
/// registers (capture-only material registers are dropped: unbound, reads 0). Returns notes.
fn swap_material(
    ren: &Renderer,
    r: &mut Replay,
    fields: &[Field],
    sizes: &HashMap<&str, u32>,
    sdc: &SdcShape,
) -> Vec<String> {
    let mut notes = Vec::new();
    for f in fields {
        let Some(v) = &f.sdc else {
            notes.push(format!("{}.{}: sdc has none, capture kept", f.cb, f.var));
            continue;
        };
        let b = r.cbuffers.get_mut(f.cb).unwrap();
        *b = cb_bytes(b, sizes[f.cb]);
        for (i, x) in v.iter().enumerate() {
            let o = f.offset as usize + 4 * i;
            b[o..o + 4].copy_from_slice(&x.to_le_bytes());
        }
    }
    let sdc_regs: BTreeSet<u32> = sdc.slots.iter().map(|s| s.0).collect();
    // A material register the NIF leaves empty gets sdc's 1x1 default, as sdc's own draw binds it.
    let others: Vec<u32> = r
        .textures
        .keys()
        .copied()
        .filter(|reg| !ENGINE_REGS.contains(reg) && !sdc_regs.contains(reg))
        .collect();
    for reg in others {
        match ren.draw_default_at(sdc.id, reg) {
            Some((SrvKind::Cube, c)) => {
                r.textures.insert(reg, common::dds_cube_rgba8(c));
            }
            Some((SrvKind::Tex2D, c)) => {
                r.textures.insert(reg, common::dds_rgba8(1, 1, &[c]));
            }
            d => {
                r.textures.remove(&reg);
                notes.push(format!("t{reg}: sdc default {d:?}, left unbound"));
            }
        }
    }
    for (reg, path, bytes) in &sdc.slots {
        match bytes {
            Some(b) => {
                r.textures.insert(*reg, b.clone());
            }
            None => {
                r.textures.remove(reg);
                notes.push(format!("t{reg} {path}: not found, unbound"));
            }
        }
    }
    notes
}

fn fmt_v(v: &[f32]) -> String {
    format!(
        "[{}]",
        v.iter()
            .map(|x| format!("{x:.6}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[test]
#[ignore]
fn extraction_matches_capture() {
    let Some(env) = env() else { return };
    let fp = render3d::locate::fingerprint(&env.shaders, None).unwrap();
    let mut ren = Renderer::new(fp, env.cache.path().to_path_buf()).unwrap();
    let noise = Noise::load(
        &env.root
            .join("noise_Skyrim_AE_1.6.1170_frame207657_Skyrim_AE_1.6.1170_frame207658.json"),
    )
    .unwrap();
    let mut t = String::new();
    let (mut reached, mut nonfinite, mut define_mismatch) = (0usize, Vec::new(), 0usize);
    let mut firsts: Vec<String> = Vec::new();
    for s in EXTRACTION {
        let dir = env.root.join(s);
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let _ = writeln!(t, "\n== {s}");
        let mut r = load_fixture(&dir).unwrap();
        // (1)-(2) sdc's load and the shape.
        let sdc = match load_sdc(&mut ren, &env, &dir, &r) {
            Ok(x) => x,
            Err(e) => {
                let _ = writeln!(t, "shape: ERROR {e}");
                continue;
            }
        };
        let _ = writeln!(
            t,
            "nif: {} | shape: {} | load errors: {:?}",
            sdc.nif_origin, sdc.rule, sdc.errors
        );
        // (3) Defines.
        let cap: BTreeSet<&str> = r.defines[0]
            .iter()
            .map(|d| key(d))
            .filter(|k| M.contains(k))
            .collect();
        let Some((sdc_defs, _, sdc_srvs)) = ren.draw_diag(sdc.id) else {
            let _ = writeln!(t, "defines: ERROR no draw_diag");
            continue;
        };
        let mine: BTreeSet<&str> = sdc_defs
            .iter()
            .map(|d| d.0.as_str())
            .filter(|k| M.contains(k))
            .collect();
        let defines_equal = cap == mine;
        if !defines_equal {
            define_mismatch += 1;
        }
        let _ = writeln!(
            t,
            "defines: {} capture-only {:?} sdc-only {:?} (common {:?})",
            if defines_equal { "EQUAL" } else { "DIFFER" },
            cap.difference(&mine).collect::<Vec<_>>(),
            mine.difference(&cap).collect::<Vec<_>>(),
            cap.intersection(&mine).collect::<Vec<_>>()
        );
        // (4) Cbuffer fields.
        let layout = match material_fields(&env, &r) {
            Ok(l) => l,
            Err(e) => {
                let _ = writeln!(t, "cbuffers: ERROR {e}");
                continue;
            }
        };
        reached += 1;
        let fields = compare_fields(&ren, sdc.id, &r, &layout);
        let first = fields.iter().find(|f| !f.ok);
        match first {
            Some(f) => {
                let line = format!(
                    "{name}: {}.{} [{}] capture {} sdc {}",
                    f.cb,
                    f.var,
                    group(&f.var),
                    fmt_v(&f.capture),
                    f.sdc
                        .as_deref()
                        .map_or("not reflected by sdc".into(), fmt_v)
                );
                let _ = writeln!(t, "first differing field: {line}");
                firsts.push(line);
            }
            None => {
                let _ = writeln!(t, "first differing field: none");
            }
        }
        let _ = writeln!(t, "cb | var | offset | group | capture | sdc | match");
        for f in &fields {
            let note = if f.var == "MaterialData" && !f.ok {
                " (runtime LOD fade)"
            } else {
                ""
            };
            let _ = writeln!(
                t,
                "{} | {} | {} | {} | {} | {} | {}{note}",
                f.cb,
                f.var,
                f.offset,
                group(&f.var),
                fmt_v(&f.capture),
                f.sdc
                    .as_deref()
                    .map_or("not reflected by sdc".into(), fmt_v),
                if f.ok { "ok" } else { "DIFF" }
            );
        }
        // (5) Texture registers.
        let tex = json(&dir.join("tex.json"));
        let cap_tex: HashMap<u32, &Value> = tex
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["isTexture"] == true && e["saved"] == true)
            .map(|e| (e["bind"].as_u64().unwrap() as u32, e))
            .collect();
        let cap_regs: BTreeSet<u32> = cap_tex
            .keys()
            .copied()
            .filter(|r| !ENGINE_REGS.contains(r))
            .collect();
        let sdc_regs: BTreeSet<u32> = sdc.slots.iter().map(|s| s.0).collect();
        let _ = writeln!(
            t,
            "textures: capture-only {:?} sdc-only {:?} (engine t11/t14 not compared)",
            cap_regs.difference(&sdc_regs).collect::<Vec<_>>(),
            sdc_regs.difference(&cap_regs).collect::<Vec<_>>()
        );
        for (reg, path, bytes) in &sdc.slots {
            let Some(c) = cap_tex.get(reg) else { continue };
            let sdc_fmt = sdc_srvs
                .iter()
                .find(|s| s.0 == *reg)
                .map_or("unbound".to_string(), |s| dxgi_name(s.1));
            let wh = bytes.as_deref().and_then(dds_wh);
            let cap_wh = (
                c["w"].as_u64().unwrap_or(0) as u32,
                c["h"].as_u64().unwrap_or(0) as u32,
            );
            let cap_fmt = c["viewFormat"].as_str().unwrap_or("?");
            let ok = wh == Some(cap_wh) && sdc_fmt == cap_fmt;
            // Same pixels: equal bytes past each file's own header (the extractor may write DX10 where the file is legacy).
            let cap_bytes = std::fs::read(dir.join(format!("tex/t{reg}.dds"))).unwrap_or_default();
            let content = match bytes {
                Some(b) if dds_data(b).is_some() && dds_data(b) == dds_data(&cap_bytes) => {
                    "same data"
                }
                Some(_) => "OTHER DATA",
                None => "not found",
            };
            let _ = writeln!(
                t,
                "  t{reg} {path}: sdc {wh:?} {sdc_fmt} | capture {cap_wh:?} {cap_fmt} | {} | {content}",
                if ok { "ok" } else { "DIFF" }
            );
        }
        // (6) Pixels, only with equal material defines.
        if !defines_equal {
            let _ = writeln!(t, "pixels: skipped (material defines differ)");
            continue;
        }
        let sizes: HashMap<&str, u32> = layout.iter().map(|(s, _, z)| (*s, *z)).collect();
        let notes = swap_material(&ren, &mut r, &fields, &sizes, &sdc);
        let out = match ren.render_replay(&r, true) {
            Ok(o) => o,
            Err(e) => {
                let _ = writeln!(t, "pixels: replay ERROR {e}");
                continue;
            }
        };
        let _ = writeln!(
            t,
            "pixels: swap notes {notes:?}; replay warnings {:?}",
            out.warnings
        );
        let n = (r.crop[2] * r.crop[3]) as usize;
        let mask = golden::read_mask(&dir.join("mask.bin"), n).unwrap();
        let mut row = String::from("pixels RT0..RT7 over%/max:");
        for (i, rt) in out.mrt.iter().enumerate() {
            if rt.iter().any(|v| !v.is_finite()) {
                nonfinite.push(format!("{name} RT{i}"));
            }
            let file = format!("mrt{i}.f32");
            let after = golden::read_f32(&dir.join(&file), n * 4).unwrap();
            let (_, max) = noise.limit(&name, &file).unwrap();
            let d = golden::diff_tol(
                rt,
                &after,
                &mask,
                golden::format_tol(&r.formats[i]).unwrap(),
                max,
            );
            let _ = write!(row, " {:.2}%/{:.3e}", 100.0 * d.over, d.max_abs);
        }
        if let (Some(main), Some(rc)) = (&out.main, &r.composite) {
            if main.iter().any(|v| !v.is_finite()) {
                nonfinite.push(format!("{name} main"));
            }
            let after = golden::read_f32(&dir.join("mainrw_after.f32"), n * 4).unwrap();
            let (_, max) = noise.limit(&name, "mainrw_after.f32").unwrap();
            let mrt0 = golden::read_f32(&dir.join("mrt0.f32"), n * 4).unwrap();
            let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
            let clean: Vec<bool> = (0..n)
                .map(|p| {
                    mask[p] && bits(&mrt0[4 * p..4 * p + 4]) == bits(&rc.before[4 * p..4 * p + 4])
                })
                .collect();
            for (label, m) in [("mask", &mask), ("clean", &clean)] {
                let d = golden::diff_tol(
                    main,
                    &after,
                    m,
                    golden::format_tol("R16G16B16A16_FLOAT").unwrap(),
                    max,
                );
                let _ = write!(
                    row,
                    " | main [{label}] {:.2}%/{:.3e} ({}px)",
                    100.0 * d.over,
                    d.max_abs,
                    d.pixels
                );
            }
        }
        let _ = writeln!(t, "{row}");
    }
    let _ = writeln!(
        t,
        "\nsubjects {}: reached cbuffers {reached}; material define mismatches {define_mismatch}",
        EXTRACTION.len()
    );
    let _ = writeln!(t, "first differing fields:");
    for f in &firsts {
        let _ = writeln!(t, "  {f}");
    }
    let _ = writeln!(t, "non-finite: {nonfinite:?}");
    print!("{t}");
    let out = std::env::temp_dir().join("golden-extraction.txt");
    std::fs::write(&out, &t).unwrap();
    println!("table written to {}", out.display());
    assert!(
        reached >= 5,
        "only {reached} subjects reached the cbuffer step"
    );
    assert!(nonfinite.is_empty(), "non-finite output: {nonfinite:?}");
}

/// Part 3 mode B: the DBG subject with sdc's material swapped in (as in extraction_matches_capture); the NIF is
/// unloaded afterwards so debug-view changes recompile nothing but the replay.
fn hybrid(ren: &mut Renderer, env: &Env, dir: &Path) -> Result<(Replay, Vec<String>), String> {
    let mut r = load_fixture(dir)?;
    let sdc = load_sdc(ren, env, dir, &r)?;
    let layout = material_fields(env, &r)?;
    let fields = compare_fields(ren, sdc.id, &r, &layout);
    let sizes: HashMap<&str, u32> = layout.iter().map(|(s, _, z)| (*s, *z)).collect();
    let mut notes = swap_material(ren, &mut r, &fields, &sizes, &sdc);
    notes.extend(
        fields
            .iter()
            .filter(|f| !f.ok)
            .map(|f| format!("{}.{} differs", f.cb, f.var)),
    );
    ren.load_parsed_sync(&ParsedNif::default(), &|_| None);
    Ok((r, notes))
}

const UNPARSED: &str = "unparsed var";

/// The value a debug view writes at the traced pixel, from the trace's final values (None = a null component).
/// Err(UNPARSED) when a traced var is not an array of numbers and nulls.
fn traced(view: DebugOutput, vars: &Value, pbr: bool) -> Result<[Option<f32>; 3], &'static str> {
    let comp = |k: &str, i: usize| -> Result<Option<f32>, &'static str> {
        match vars[k].as_array().and_then(|a| a.get(i)) {
            Some(Value::Null) => Ok(None),
            Some(v) => v.as_f64().map(|x| Some(x as f32)).ok_or(UNPARSED),
            None => Err(UNPARSED),
        }
    };
    let vec3 = |k: &str, f: fn(f32) -> f32| -> Result<[Option<f32>; 3], &'static str> {
        Ok([comp(k, 0)?.map(f), comp(k, 1)?.map(f), comp(k, 2)?.map(f)])
    };
    let scalar = |k: &str, f: fn(f32) -> f32| {
        if vars.get(k).is_none() {
            return Err("not traced");
        }
        let v = comp(k, 0)?.map(f);
        Ok([v, v, v])
    };
    match view {
        DebugOutput::NormalWorld if vars.get("worldNormal").is_some() => {
            vec3("worldNormal", |n| n * 0.5 + 0.5)
        }
        DebugOutput::Roughness if pbr => scalar("material.Roughness", |x| x),
        DebugOutput::Roughness => scalar("glossiness", |g| 1.0 - g),
        DebugOutput::Metallic if pbr => scalar("material.Metallic", |x| x),
        DebugOutput::Metallic => Err("non-PBR: no metallic term"),
        DebugOutput::Albedo if vars.get("material.BaseColor").is_some() => {
            vec3("material.BaseColor", |x| x)
        }
        _ => Err("not traced"),
    }
}

const VIEWS: [DebugOutput; 4] = [
    DebugOutput::NormalWorld,
    DebugOutput::Roughness,
    DebugOutput::Metallic,
    DebugOutput::Albedo,
];

/// Mode B subjects: (capture, subject folder name without the event).
const MODE_B: &[(&str, &str)] = &[
    ("Skyrim_AE_1.6.1170_frame26196", "Coffin"),
    ("Skyrim_AE_1.6.1170_frame26196", "Object174"),
    ("Skyrim_AE_1.6.1170_frame26196", "dibella_L.004_3"),
    ("Skyrim_AE_1.6.1170_frame61618", "BlackreachCrystalL01_10"),
    ("Skyrim_AE_1.6.1170_frame61618", "NorRubblePiece01_0"),
];

const WANTED: [&str; 5] = [
    "worldNormal",
    "material.Roughness",
    "material.Metallic",
    "material.BaseColor",
    "glossiness",
];

#[test]
#[ignore]
fn trace_values_match_unit_functions() {
    let Some(env) = env() else { return };
    let fp = render3d::locate::fingerprint(&env.shaders, None).unwrap();
    let mut ren = Renderer::new(fp, env.cache.path().to_path_buf()).unwrap();
    assert!(
        ren.debug_supported(),
        "the installed Lighting.hlsl has no debug anchor"
    );
    // D-P24: a trace counts only when its fragment is the one the replay keeps (depth within 2 D24 ulps).
    ren.set_replay_depth_readback(true);
    let mut t = String::from(
        "mode | subject | trace | view | traced | replay RT0 | over | traced z / replay z\n",
    );
    let mut skipped: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
    let (mut checked, mut failures, mut b_rows) = (0usize, Vec::new(), 0usize);
    // Compared Roughness + Metallic values; skipped occluded values whose colour was also over the limit.
    let (mut rough_metal, mut occluded_over) = (0usize, 0usize);
    let (mut occluded_dz, mut missing_z, mut unparsed) =
        (Vec::<f32>::new(), Vec::new(), Vec::new());
    for s in golden::subjects(&env.root, &["DBG"]).unwrap() {
        let name = s.dir.file_name().unwrap().to_string_lossy().into_owned();
        let who = format!("{}/{name}", s.capture);
        let mrt = json(&s.dir.join("mrt.json"));
        if mrt["maskSource"] == "overlay" {
            skipped.entry("overlay mask").or_default().push(who);
            continue;
        }
        let r = match load_fixture(&s.dir) {
            Ok(r) => r,
            Err(e) if e.contains("defines null") => {
                skipped.entry("defines null").or_default().push(who);
                continue;
            }
            Err(e) => {
                skipped
                    .entry("load error")
                    .or_default()
                    .push(format!("{who}: {e}"));
                continue;
            }
        };
        if r.blended {
            skipped.entry("blended").or_default().push(who);
            continue;
        }
        if r.two_sided_alpha_tested() && r.depth.is_none() {
            skipped
                .entry("two-sided without depth")
                .or_default()
                .push(who);
            continue;
        }
        let mut traces = Vec::new();
        for pick in ["lit", "mid", "dark"] {
            let p = s.dir.join(format!("trace_{pick}.json"));
            let Ok(text) = std::fs::read_to_string(&p) else {
                skipped
                    .entry("no trace file")
                    .or_default()
                    .push(format!("{who} {pick}"));
                continue;
            };
            let tr: Value =
                serde_json::from_str(&render3d::replay::nonfinite_to_null(&text)).unwrap();
            let why = if tr.get("error").is_some() {
                Some("error trace")
            } else if tr["debugInfo"] != true {
                Some("debugInfo not true")
            } else if !WANTED.iter().any(|k| tr["vars"].get(*k).is_some()) {
                Some("no wanted name")
            } else {
                None
            };
            match why {
                Some(w) => skipped.entry(w).or_default().push(format!("{who} {pick}")),
                None => traces.push((pick, tr)),
            }
        }
        if traces.is_empty() {
            continue;
        }
        let pbr = r.defines[0].iter().any(|d| d == "TRUE_PBR");
        let mode_b = MODE_B.iter().any(|(c, tag)| {
            *c == s.capture && name.split_once('_').is_some_and(|(_, rest)| rest == *tag)
        });
        let mut runs: Vec<(&str, Replay)> = vec![("A", r)];
        if mode_b {
            match hybrid(&mut ren, &env, &s.dir) {
                Ok((h, notes)) => {
                    let _ = writeln!(t, "B | {who} | notes {notes:?}");
                    runs.push(("B", h));
                }
                Err(e) => {
                    let _ = writeln!(t, "B | {who} | hybrid ERROR {e}");
                }
            }
        }
        for (mode, r) in &runs {
            let [cx, cy, cw, ch] = r.crop;
            for view in VIEWS {
                ren.set_debug_output(view).unwrap();
                let out = match ren.render_replay(r, false) {
                    Ok(o) => o,
                    Err(e) => {
                        skipped
                            .entry("replay error")
                            .or_default()
                            .push(format!("{mode} {who} {view:?}: {e}"));
                        continue;
                    }
                };
                for (pick, tr) in &traces {
                    let x = tr["x"].as_u64().unwrap() as u32;
                    let y = tr["y"].as_u64().unwrap() as u32;
                    if x < cx || y < cy || x >= cx + cw || y >= cy + ch {
                        skipped
                            .entry("traced pixel outside the crop")
                            .or_default()
                            .push(format!("{who} {pick}"));
                        continue;
                    }
                    let asserted = *mode == "A" && view != DebugOutput::Albedo;
                    let want = match traced(view, &tr["vars"], pbr) {
                        Ok(w) if w.iter().any(Option::is_some) => w,
                        Err(e) if e == UNPARSED => {
                            let line = format!("{mode} {who} {pick} {view:?}");
                            if asserted {
                                unparsed.push(line.clone());
                            }
                            skipped.entry(UNPARSED).or_default().push(line);
                            continue;
                        }
                        _ => continue,
                    };
                    let p = ((y - cy) * cw + (x - cx)) as usize;
                    let got: [f32; 4] = out.mrt[0][4 * p..4 * p + 4].try_into().unwrap();
                    // Untraced components and alpha compare with themselves.
                    let b: Vec<f32> = (0..4)
                        .map(|i| want.get(i).copied().flatten().unwrap_or(got[i]))
                        .collect();
                    let d = golden::diff_tol(
                        &got,
                        &b,
                        &[true],
                        golden::format_tol("R16G16B16A16_FLOAT").unwrap(),
                        0.0,
                    );
                    let zt = tr["vars"]["SV_POSITION"][2].as_f64().map(|z| z as f32);
                    let zr = out.depth.as_ref().unwrap()[p];
                    let visible = zt.is_some_and(|z| (z - zr).abs() <= 2.0 / 16_777_215.0);
                    let _ = writeln!(
                        t,
                        "{mode}{} | {who} | {pick} | {view:?} | {} | {} | {} | {zt:?} / {zr}{}",
                        if asserted { "" } else { " (report)" },
                        fmt_v(&b[..3]),
                        fmt_v(&got[..3]),
                        if d.over > 0.0 {
                            format!("OVER max {:.3e}", d.max_abs)
                        } else {
                            "0".into()
                        },
                        if visible { "" } else { " occluded" }
                    );
                    // A trace without a depth cannot be placed: a failure with its own reason, never an occlusion.
                    if asserted && zt.is_none() {
                        missing_z.push(format!("{who} {pick} {view:?}"));
                        continue;
                    }
                    if asserted && !visible {
                        occluded_dz.push((zt.unwrap() - zr).abs());
                        occluded_over += (d.over > 0.0) as usize;
                        skipped
                            .entry("occluded fragment")
                            .or_default()
                            .push(format!(
                                "{who} {pick} {view:?}: traced z {zt:?}, replay z {zr}{}",
                                if d.over > 0.0 { " (colour over)" } else { "" }
                            ));
                        continue;
                    }
                    if asserted {
                        checked += 1;
                        rough_metal +=
                            matches!(view, DebugOutput::Roughness | DebugOutput::Metallic) as usize;
                        if d.over > 0.0 {
                            failures.push(format!(
                                "{who} {pick} {view:?}: traced {} replay {}",
                                fmt_v(&b[..3]),
                                fmt_v(&got[..3])
                            ));
                        }
                    } else if *mode == "B" {
                        b_rows += 1;
                    }
                }
            }
        }
    }
    ren.set_debug_output(DebugOutput::Lit).unwrap();
    let _ = writeln!(
        t,
        "\nmode A compared values: {checked} (Roughness + Metallic {rough_metal}); failures: {}; occluded skips with colour over: {occluded_over}",
        failures.len()
    );
    for f in &failures {
        let _ = writeln!(t, "  {f}");
    }
    let _ = writeln!(
        t,
        "mode A traces: {} (compared {checked}, occluded {}); FAIL no SV_POSITION in trace: {missing_z:?}; unparsed compared vars: {unparsed:?}",
        checked + occluded_dz.len(),
        occluded_dz.len()
    );
    let _ = writeln!(t, "mode B rows (report-only): {b_rows}");
    for (why, list) in &skipped {
        let _ = writeln!(t, "skipped {why}: {}", list.len());
        for w in list {
            let _ = writeln!(t, "  {w}");
        }
    }
    print!("{t}");
    let out = std::env::temp_dir().join("golden-trace.txt");
    std::fs::write(&out, &t).unwrap();
    println!("table written to {}", out.display());
    // A value whose depth matches but whose colour differs is a failure, never a skip: every colour mismatch must
    // be an occluded fragment.
    assert!(
        failures.is_empty(),
        "mode A trace mismatches on the visible fragment: {failures:#?}"
    );
    assert!(checked >= 30, "only {checked} mode A values compared");
    assert!(
        missing_z.is_empty(),
        "no SV_POSITION in trace: {missing_z:#?}"
    );
    assert!(unparsed.is_empty(), "unparsed var: {unparsed:#?}");
    // Bounded: a replay regression must not turn into skips (10 of 90 at D-P24).
    let occluded = occluded_dz.len();
    assert!(
        occluded * 100 <= 12 * (checked + occluded),
        "{occluded} occluded of {} mode A traces (over 12%)",
        checked + occluded
    );
    assert!(
        occluded_dz.iter().all(|dz| *dz > 2.0 / 16_777_215.0),
        "an occluded trace within 2 D24 ulps: {occluded_dz:?}"
    );
    assert!(
        rough_metal >= 20,
        "only {rough_metal} Roughness + Metallic values compared"
    );
}

/// Fix round 2: a traced var that is not an array of numbers and nulls is "unparsed", never a silent skip.
#[test]
fn traced_reports_unparsed_vars() {
    let v = serde_json::json!({"worldNormal": [0.0, null, 1.0], "material.Roughness": 0.5, "glossiness": ["x"]});
    assert_eq!(
        traced(DebugOutput::NormalWorld, &v, false),
        Ok([Some(0.5), None, Some(1.0)])
    );
    assert_eq!(traced(DebugOutput::Roughness, &v, true), Err(UNPARSED));
    assert_eq!(traced(DebugOutput::Roughness, &v, false), Err(UNPARSED));
    assert_eq!(traced(DebugOutput::Metallic, &v, true), Err("not traced"));
}
