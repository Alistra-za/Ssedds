//! 01 §5.2 golden cases: source NIF + the winning PGPatcher JSON → `simulate_file` → compare with the shape in
//! `ParallaxGen_Output` on the 01 §5.1 field list. Opt-in and read-only on owner data; without
//! SDC_ACHERUS the (ignored) tests panic rather than pass silently:
//! `$env:SDC_ACHERUS='<your MO2 instance>'; cargo test -p pgpsim --test golden -- --ignored`
use nif::material::MaterialState;
use nif::props::LightingTrailer;
use nif::scan::{scan, Shader};
use pgpsim::{simulate_file, SimResult};
use serde_json::Value;
use std::path::Path;

struct Case {
    nif: &'static str,
    shape: &'static str,
    source_mod: &'static str,
    json_mod: &'static str,
    json: &'static str,
    matched: &'static str,
}

const PLAIN: Case = Case {
    nif: "meshes\\xmead\\reserve.nif",
    shape: "MeadSolid:3",
    source_mod: "Xocco3's Mead Replacer PBR",
    json_mod: "Xocco3's Mead Replacer PBR",
    json: "reserve.json",
    matched: "textures\\pbr\\xmead\\reserve_bc",
};
const EMISSIVE_PARALLAX: Case = Case {
    nif: "meshes\\clutter\\eyeofmagnus\\eyeofmagnus01.nif",
    shape: "SectionD10:0",
    source_mod: "Faultier's PBR Clutter",
    json_mod: "Faultier's PBR Clutter",
    json: "Faultier's PBR Clutter.json",
    matched: "textures\\pbr\\clutter\\eyeofmagnus",
};
const SUBSURFACE: Case = Case {
    nif: "meshes\\furniture\\firespitcooking.nif",
    shape: "PotOpen:18",
    source_mod: "Faultier's PBR Clutter",
    json_mod: "Faultier's PBR Clutter",
    json: "Faultier's PBR Clutter.json",
    matched: "textures\\pbr\\clutter\\common\\stew01",
};
const COAT: Case = Case {
    nif: "meshes\\dungeons\\caves\\ice\\largehall\\caveilhall1way128u01.nif",
    shape: "CaveILHall1Way128U01:6",
    source_mod: "Icy Cave Remaster - Ice Material Patch",
    json_mod: "Exist's Caves - PBR Retexture",
    json: "dungeonscaves.json",
    matched: "textures\\pbr\\dungeons\\caves\\icefrozen01",
};
const COAT_EMISSIVE_SLOTN: Case = Case {
    nif: "meshes\\dlc01\\traps\\lighttrap\\lighttrap01.nif",
    shape: "DawnStone01:0",
    source_mod: "Faultier's PBR Clutter",
    json_mod: "Faultier's PBR Clutter",
    json: "Faultier's PBR Clutter.json",
    matched: "textures\\pbr\\clutter\\dawnbreakerpedestal\\crystal",
};
const FUZZ_TEXTURE: Case = Case {
    nif: "meshes\\dlc01\\clothes\\vampire\\vampirerobesfkha_1.nif",
    shape: "robes_bodice",
    source_mod: "CBBE 3BA Vanilla Outfits Redone",
    json_mod: "Faultier's PBR Armors and Clothes",
    json: "Faultier's vampire set.json",
    matched: "textures\\pbr\\dlc01\\clothes\\vampire\\robesf",
};
const FUZZ_SLOT7: Case = Case {
    nif: "meshes\\tapestries\\smalltreetapestry.nif",
    shape: "Plane030",
    source_mod: "Faultier's PBR blue palace terrace",
    json_mod: "Faultier's PBR blue palace terrace",
    json: "Faultier's PBR blue palace terrace.json",
    matched: "textures\\pbr\\1_castle_1\\new folder\\tapestries\\smalltreetapestry1k",
};
const GLINT: Case = Case {
    nif: "meshes\\architecture\\sovengarde\\sovstairspiece01.nif",
    shape: "SOVStairsPiece01:0",
    source_mod: "Sovngarde Meshes - FIXES",
    json_mod: "Faultier's PBR Sovngarde",
    json: "Faultier's PBR aaSovngarde.json",
    matched: "textures\\pbr\\architecture\\sovngarde\\sovbasestone01",
};
const GLINT_RENAME: Case = Case {
    nif: "meshes\\nicola89b\\architecture\\windhelm\\windhelmbridge.nif",
    shape: "Arch_snow:0",
    source_mod: "Windhelm Bridge Reforged - The Citadel of Snow",
    json_mod: "Faultier's PBR Windhelm",
    json: "Faultier's PBR Windhelm.json",
    matched: "textures\\pbr\\landscape\\snow01",
};

/// The one lighting shape named `name` in the NIF at `path`, as a `MaterialState`.
fn shape_of(path: &Path, name: &str) -> MaterialState {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let s = scan(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let found: Vec<MaterialState> = s
        .shapes
        .iter()
        .filter(|r| r.head.name.as_deref() == Some(name))
        .filter_map(|r| match &r.shader {
            Shader::Lighting(l, slots) => {
                Some(MaterialState::from_lighting(&r.head, l, slots, r.alpha))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "{}: shape {name} found {} times",
        path.display(),
        found.len()
    );
    found.into_iter().next().expect("one shape")
}

/// A PGPatcher config file: an object needs both `default` and `entries`; a bare array has no default.
fn read_config(path: &Path) -> (Vec<Value>, Value) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let v: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match v {
        Value::Array(entries) => (entries, Value::Null),
        Value::Object(mut o) => {
            let default = o
                .remove("default")
                .expect("an object config needs `default`");
            let Some(Value::Array(entries)) = o.remove("entries") else {
                panic!(
                    "{}: an object config needs an `entries` array",
                    path.display()
                );
            };
            (entries, default)
        }
        other => panic!("{}: not a PGPatcher config: {other}", path.display()),
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4
}

fn mlp4(t: &LightingTrailer) -> Option<[f32; 4]> {
    match t {
        LightingTrailer::MultiLayer {
            inner_thickness,
            refraction_scale,
            inner_texture_scale,
            ..
        } => Some([
            *inner_thickness,
            *refraction_scale,
            inner_texture_scale[0],
            inner_texture_scale[1],
        ]),
        _ => None,
    }
}

/// 01 §5.1 field list: type, SLSF1/2, the 9 slots (lower-cased), glossiness, specular strength, specular
/// colour, LE1, LE2, emissive colour, emissive multiple, UV scale, and the 4 MLP floats when the output is
/// type 11. Floats at 1e-4.
fn diff(sim: &MaterialState, out: &MaterialState) -> Vec<String> {
    let mut d = Vec::new();
    if sim.shader_type != out.shader_type {
        d.push(format!(
            "type sim={} out={}",
            sim.shader_type, out.shader_type
        ));
    }
    if sim.slsf1 != out.slsf1 {
        d.push(format!(
            "slsf1 sim={:#010x} out={:#010x}",
            sim.slsf1, out.slsf1
        ));
    }
    if sim.slsf2 != out.slsf2 {
        d.push(format!(
            "slsf2 sim={:#010x} out={:#010x}",
            sim.slsf2, out.slsf2
        ));
    }
    let low = |m: &MaterialState| -> Vec<String> {
        m.slots.iter().map(|t| t.raw.to_ascii_lowercase()).collect()
    };
    if low(sim) != low(out) {
        d.push(format!("slots sim={:?} out={:?}", low(sim), low(out)));
    }
    let (a, b) = (&sim.params, &out.params);
    let scalars = [
        ("glossiness", a.glossiness, b.glossiness),
        (
            "specular_strength",
            a.specular_strength,
            b.specular_strength,
        ),
        (
            "lighting_effect_1",
            a.lighting_effect_1,
            b.lighting_effect_1,
        ),
        (
            "lighting_effect_2",
            a.lighting_effect_2,
            b.lighting_effect_2,
        ),
        (
            "emissive_multiple",
            a.emissive_multiple,
            b.emissive_multiple,
        ),
    ];
    for (k, x, y) in scalars {
        if !close(x, y) {
            d.push(format!("{k} sim={x} out={y}"));
        }
    }
    let vectors: [(&str, &[f32], &[f32]); 3] = [
        ("specular_color", &a.specular_color, &b.specular_color),
        ("emissive_color", &a.emissive_color, &b.emissive_color),
        ("uv_scale", &a.uv_scale, &b.uv_scale),
    ];
    for (k, x, y) in vectors {
        if x.iter().zip(y).any(|(p, q)| !close(*p, *q)) {
            d.push(format!("{k} sim={x:?} out={y:?}"));
        }
    }
    if out.shader_type == 11 {
        match (mlp4(&a.trailer), mlp4(&b.trailer)) {
            (Some(x), Some(y)) if x.iter().zip(&y).all(|(p, q)| close(*p, *q)) => {}
            (x, y) => d.push(format!("mlp sim={x:?} out={y:?}")),
        }
    }
    d
}

fn check(c: &Case) {
    let root = std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root");
    let mods = Path::new(&root).join("mods");
    let source = shape_of(&mods.join(c.source_mod).join(c.nif), c.shape);
    let out = shape_of(&mods.join("ParallaxGen_Output").join(c.nif), c.shape);
    let (entries, default) = read_config(&mods.join(c.json_mod).join("PBRNifPatcher").join(c.json));
    // The output NIF proves every texture existed when PGPatcher ran (01 §1.4), so no VFS lookup here.
    let r = simulate_file(&entries, &default, c.nif, &source, &|_| true);
    let SimResult::Patched {
        material,
        matched_path,
        ..
    } = &r
    else {
        panic!("{} / {}: expected Patched, got {r:?}", c.nif, c.shape);
    };
    assert_eq!(matched_path, c.matched, "{} / {}", c.nif, c.shape);
    let d = diff(material, &out);
    assert!(d.is_empty(), "{} / {}:\n{}", c.nif, c.shape, d.join("\n"));
}

#[test]
#[ignore]
fn golden_plain_reserve() {
    check(&PLAIN);
}

#[test]
#[ignore]
fn golden_emissive_parallax_eyeofmagnus() {
    check(&EMISSIVE_PARALLAX);
}

#[test]
#[ignore]
fn golden_subsurface_firespit() {
    check(&SUBSURFACE);
}

#[test]
#[ignore]
fn golden_coat_ice_cave() {
    check(&COAT);
}

#[test]
#[ignore]
fn golden_coat_emissive_slot_n_lighttrap() {
    check(&COAT_EMISSIVE_SLOTN);
}

#[test]
#[ignore]
fn golden_fuzz_texture_vampire_robes() {
    check(&FUZZ_TEXTURE);
}

#[test]
#[ignore]
fn golden_fuzz_slot7_tapestry() {
    check(&FUZZ_SLOT7);
}

#[test]
#[ignore]
fn golden_glint_sovngarde_stairs() {
    check(&GLINT);
}

#[test]
#[ignore]
fn golden_glint_rename_windhelm_bridge() {
    check(&GLINT_RENAME);
}
