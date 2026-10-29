//! Replay mode (v2 Phase D, D2; deferred targets D3): a golden fixture's captured geometry, cbuffers, textures and
//! samplers.
mod common;

use render3d::device::MRT_FORMATS;
use render3d::golden::{self, Noise};
use render3d::{load_fixture, Renderer};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SUBJECTS: &[&str] = &[
    "Skyrim_AE_1.6.1170_frame69672/ev31535_Coffin",
    "Skyrim_AE_1.6.1170_frame116692/ev22673_Arch_Head",
];

const STANDIN: &[&str] = &["STANDIN_OPAQUE_OK", "STANDIN_FLAT", "STANDIN_REPLAY"];

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn put(dir: &Path, name: &str, v: &serde_json::Value) {
    fs::write(dir.join(name), serde_json::to_string_pretty(v).unwrap()).unwrap();
}

/// A one-triangle fixture in the golden layout for the stand-in shader: a full-screen triangle placed by VS b2,
/// PS b1 EmitColor (0.75, 0.25, 0.5) shorter than its reflection, t0 black, t1 and t6 bound to nothing.
fn write_fixture(dir: &Path, defines: &[&str]) {
    let ident3 = [1.0f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.];
    let ident4 = [
        1.0f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    put(
        dir,
        "state.json",
        &json!({
            "identity": {"vsInputs": ["POSITION", "TEXCOORD", "NORMAL", "BINORMAL"]},
            "World": ident3, "ViewProj": ident4, "bones": null,
            "defines": [defines],
            "stageB": {"blend": [{"enabled": false, "writeMask": 7}]},
        }),
    );
    put(
        dir,
        "extras.json",
        &json!({
            "samplers": {"vs": [], "ps": [{
                "bind": 0, "minify": "Point", "magnify": "Point", "mip": "Point", "func": "Normal",
                "addressU": "Wrap", "addressV": "Wrap", "addressW": "Wrap", "border": [0.0, 0.0, 0.0, 0.0],
                "compare": "Never", "minLOD": -3.4e38, "maxLOD": 3.4e38, "mipBias": 0.0, "maxAniso": 0.0}]},
            "rasterizer": {"cull": "None", "frontCCW": true},
            "viewports": [{"x": 0.0, "y": 0.0, "w": 8.0, "h": 8.0, "minDepth": 0.0, "maxDepth": 1.0}],
        }),
    );
    let input = |name: &str, format: &str, off: u32| json!({"name": name, "format": format, "slot": 0, "byteOffset": off, "perInstance": false});
    put(
        dir,
        "mesh.json",
        &json!({
            "topology": "TriangleList", "numIndices": 3, "numInstances": 0, "indexed": true,
            "inputs": [input("POSITION", "R32G32B32A32_FLOAT", 0), input("TEXCOORD", "R16G16_FLOAT", 16),
                       input("NORMAL", "R8G8B8A8_UNORM", 20), input("BINORMAL", "R8G8B8A8_UNORM", 24)],
            "vbs": [{"slot": 0, "file": "mesh.vb0", "stride": 28, "srcByteOffset": 0, "perInstance": false}],
            "ib": "mesh.ib", "indexStride": 2, "minIndex": 0, "maxIndex": 2,
        }),
    );
    let mut vb = Vec::new();
    for (x, y) in [(-1.0f32, -1.0f32), (-1.0, 3.0), (3.0, -1.0)] {
        vb.extend(f32s(&[x, y, 0.5, 1.0]));
        vb.extend([0u8; 4]); // uv (0, 0) as f16
        vb.extend([128u8, 128, 255, 255, 255, 128, 128, 255]);
    }
    fs::write(dir.join("mesh.vb0"), vb).unwrap();
    fs::write(dir.join("mesh.ib"), [0u8, 0, 1, 0, 2, 0]).unwrap();
    fs::write(dir.join("cb_vs_b1.bin"), f32s(&[0., 0., 1., 1.])).unwrap();
    let eye = [0.0f32; 4];
    fs::write(
        dir.join("cb_vs_b2.bin"),
        f32s(&[&ident3[..], &ident4, &eye].concat()),
    )
    .unwrap();
    fs::write(dir.join("cb_ps_b1.bin"), f32s(&[0.75, 0.25, 0.5, 0.0])).unwrap();
    fs::write(dir.join("cb_ps_b2.bin"), [0u8; 16]).unwrap();
    let unbound = |b: u32| json!({"bind": b, "type": "null", "isTexture": true});
    put(
        dir,
        "tex.json",
        &json!([{"bind": 0, "type": "Texture2D", "file": "tex/t0.dds", "saved": true}, unbound(1), unbound(6)]),
    );
    fs::create_dir_all(dir.join("tex")).unwrap();
    fs::write(
        dir.join("tex/t0.dds"),
        common::dds_rgba8(1, 1, &[[0, 0, 0, 255]]),
    )
    .unwrap();
    put(
        dir,
        "mrt.json",
        &json!({"size": [8, 8], "crop": [2, 2, 4, 4], "rts": [{"index": 0, "format": "R16G16B16A16_FLOAT",
                "files": {"before": "mrt0_before.f32", "after": "mrt0.f32"}}]}),
    );
    let before = f32s(&[0.0, 0.0, 0.0, 1.0].repeat(16));
    fs::write(dir.join("mrt0_before.f32"), &before).unwrap();
    fs::write(dir.join("mrt0.f32"), &before).unwrap();
}

#[test]
fn load_fixture_reads_a_synthetic_fixture() {
    let d = tempfile::tempdir().unwrap();
    write_fixture(d.path(), STANDIN);
    let r = load_fixture(d.path()).unwrap();
    let keys: BTreeSet<_> = r.cbuffers.keys().map(String::as_str).collect();
    assert_eq!(keys, BTreeSet::from(["vs_b1", "vs_b2", "ps_b1", "ps_b2"]));
    assert_eq!(
        r.cbuffers["ps_b1"],
        fs::read(d.path().join("cb_ps_b1.bin")).unwrap()
    );
    assert_eq!(
        r.world,
        Some([[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]])
    );
    assert_eq!(r.bones, None);
    assert_eq!(r.textures.keys().copied().collect::<Vec<_>>(), vec![0]);
    assert_eq!(r.null_srvs, vec![1, 6]);
    assert_eq!((r.crop, r.size), ([2, 2, 4, 4], (8, 8)));
    assert_eq!(r.mesh.inputs[1], ("TEXCOORD".to_string(), 0, 34, 0, 16));
    assert_eq!(r.mesh.index_count, 3);
    assert_eq!((r.samplers[0].0, r.samplers[0].1.filter), (0, 0));
    assert_eq!(r.samplers[0].1.address, [1, 1, 1]);
    assert_eq!(r.write_masks, vec![7]);
    assert!(!r.blended && !r.cull_back);
    assert_eq!(r.before[0].len(), 64);

    let fails = |f: &dyn Fn(&Path)| {
        let d = tempfile::tempdir().unwrap();
        write_fixture(d.path(), STANDIN);
        f(d.path());
        load_fixture(d.path())
            .err()
            .expect("load_fixture should fail")
    };
    let edit = |p: &Path, name: &str, f: &dyn Fn(&mut serde_json::Value)| {
        let mut v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(p.join(name)).unwrap()).unwrap();
        f(&mut v);
        put(p, name, &v);
    };
    let msg = fails(&|p| fs::remove_file(p.join("state.json")).unwrap());
    assert!(msg.contains("state.json"), "{msg}");
    let msg = fails(&|p| edit(p, "state.json", &|v| v["defines"] = serde_json::Value::Null));
    assert!(msg.contains("defines"), "{msg}");
    let msg = fails(&|p| {
        edit(p, "mesh.json", &|v| {
            v["inputs"].as_array_mut().unwrap().push(
                json!({"name": "COLOR", "format": "R10G10B10A2_UNORM", "slot": 0, "byteOffset": 28}),
            )
        })
    });
    assert!(msg.contains("R10G10B10A2_UNORM"), "{msg}");
}

fn edit_json(p: &Path, name: &str, f: &dyn Fn(&mut serde_json::Value)) {
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(p.join(name)).unwrap()).unwrap();
    f(&mut v);
    put(p, name, &v);
}

/// Err text from loading the synthetic fixture after `f` changed one file's JSON.
fn load_err(name: &str, f: &dyn Fn(&mut serde_json::Value)) -> String {
    let d = tempfile::tempdir().unwrap();
    write_fixture(d.path(), STANDIN);
    edit_json(d.path(), name, f);
    match load_fixture(d.path()) {
        Ok(_) => panic!("{name}: load_fixture should fail"),
        Err(e) => e,
    }
}

type Edit = &'static dyn Fn(&mut serde_json::Value);

#[test]
fn load_fixture_refuses_malformed_fixtures() {
    let cases: &[(&str, &str, Edit)] = &[
        ("mrt.json", "crop", &|v| {
            v["crop"] = json!([0, 0, 65536, 65536])
        }),
        ("mrt.json", "crop", &|v| v["crop"] = json!([6, 6, 4, 4])),
        ("mesh.json", "minIndex", &|v| v["minIndex"] = json!(70000)),
        ("mesh.json", "minIndex", &|v| v["minIndex"] = json!(1)),
        ("state.json", "defines", &|v| v["defines"] = json!([null])),
        ("state.json", "defines", &|v| {
            v["defines"] = json!([["STANDIN_FLAT", 5]])
        }),
        ("state.json", "enabled", &|v| {
            v["stageB"]["blend"][0]["enabled"] = json!("yes")
        }),
        ("state.json", "writeMask", &|v| {
            v["stageB"]["blend"][0]["writeMask"] = json!(16)
        }),
    ];
    for (file, field, f) in cases {
        let e = load_err(file, *f);
        assert!(e.contains(file) && e.contains(field), "{file}/{field}: {e}");
    }
}

/// D-P15: a logged list without DEFERRED on a draw the capture shows with 8 PS outputs gets DEFERRED, with a warning.
#[test]
fn load_fixture_refuses_a_zero_crop() {
    // trace 2026-10-28 #28: an empty crop with empty data files used to load, then panic in seeded_depth
    for crop in [[2, 2, 0, 4], [2, 2, 4, 0]] {
        let d = tempfile::tempdir().unwrap();
        write_fixture(d.path(), STANDIN);
        edit_json(d.path(), "mrt.json", &|v| v["crop"] = json!(crop));
        for f in ["mrt0_before.f32", "mrt0.f32"] {
            fs::write(d.path().join(f), []).unwrap();
        }
        match load_fixture(d.path()) {
            Ok(_) => panic!("{crop:?}: load_fixture should fail"),
            Err(e) => assert!(e.contains("mrt.json") && e.contains("crop"), "{e}"),
        }
    }
}

#[test]
fn load_fixture_adds_deferred_from_eight_outputs() {
    let load = |outputs: u64, defines: &[&str]| {
        let d = tempfile::tempdir().unwrap();
        write_fixture(d.path(), defines);
        edit_json(d.path(), "state.json", &|v| {
            v["identity"]["outputs"] = json!(outputs);
            let first = v["defines"][0].clone();
            v["defines"] = json!([first.clone(), first]);
        });
        load_fixture(d.path()).unwrap()
    };
    let r = load(8, STANDIN);
    assert_eq!(r.defines.len(), 2);
    for v in &r.defines {
        assert_eq!(v.iter().filter(|d| *d == "DEFERRED").count(), 1, "{v:?}");
    }
    assert_eq!(r.warnings, ["DEFERRED added from the capture's 8 outputs"]);
    let r = load(3, STANDIN);
    assert!(r
        .defines
        .iter()
        .all(|v| !v.contains(&"DEFERRED".to_string())));
    assert!(r.warnings.is_empty());
    let with: Vec<&str> = STANDIN.iter().copied().chain(["DEFERRED"]).collect();
    let r = load(8, &with);
    assert!(r
        .defines
        .iter()
        .all(|v| v.iter().filter(|d| *d == "DEFERRED").count() == 1));
    assert!(r.warnings.is_empty());
}

/// D-P16: RenderDoc's "NoCull" is no culling, like "None".
#[test]
fn load_fixture_accepts_no_cull() {
    let d = tempfile::tempdir().unwrap();
    write_fixture(d.path(), STANDIN);
    edit_json(d.path(), "extras.json", &|v| {
        v["rasterizer"]["cull"] = json!("NoCull")
    });
    assert!(!load_fixture(d.path()).unwrap().cull_back);
    let e = load_err("extras.json", &|v| v["rasterizer"]["cull"] = json!("Front"));
    assert!(e.contains("rasterizer"), "{e}");
}

/// D-P22: the two-sided alpha-tested class follows the variant the replay renders (the first), not any variant.
#[test]
fn two_sided_alpha_tested_reads_the_rendered_variant() {
    let class = |cull: &str, variants: serde_json::Value| {
        let d = tempfile::tempdir().unwrap();
        write_fixture(d.path(), STANDIN);
        edit_json(d.path(), "extras.json", &|v| {
            v["rasterizer"]["cull"] = json!(cull)
        });
        edit_json(d.path(), "state.json", &|v| v["defines"] = variants.clone());
        load_fixture(d.path()).unwrap().two_sided_alpha_tested()
    };
    let plain: Vec<&str> = STANDIN.to_vec();
    let alpha: Vec<&str> = STANDIN.iter().copied().chain(["DO_ALPHA_TEST"]).collect();
    assert!(class("NoCull", json!([alpha.clone(), plain.clone()])));
    assert!(class("None", json!([alpha.clone()])));
    assert!(!class("NoCull", json!([plain.clone(), alpha.clone()])));
    assert!(!class("Back", json!([alpha])));
}

#[test]
fn replay_refuses_an_empty_define_list() {
    let d = tempfile::tempdir().unwrap();
    write_fixture(d.path(), STANDIN);
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let mut r = load_fixture(d.path()).unwrap();
    r.defines.clear();
    let e = ren.render_replay(&r, false).err().unwrap();
    assert!(e.contains("define"), "{e}");
}

/// A normal frame binds t0 (the built-in's diffuse); a replay whose t0 is "null" must read 0 there, not that texture.
#[test]
fn replay_after_render_leaves_no_stale_bindings() {
    let d = tempfile::tempdir().unwrap();
    write_fixture(d.path(), STANDIN);
    edit_json(d.path(), "tex.json", &|v| {
        v[0] = json!({"bind": 0, "type": "null", "isTexture": true})
    });
    // White t0 would add 0.25 to the flat colour if a stale view leaked in.
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &["STANDIN_OPAQUE_OK", "STANDIN_FLAT"]);
    let rep = ren.load_builtin(
        render3d::Builtin::Sphere,
        nif::material::MaterialState::builtin_default(),
        &|_| None,
    );
    assert!(
        rep.errors.is_empty() && rep.compile_errors.is_empty(),
        "{rep:?}"
    );
    ren.render(&render3d::scene::Scene::neutral(), 64, 64)
        .unwrap();
    let r = load_fixture(d.path()).unwrap();
    let out = ren.render_replay(&r, false).unwrap();
    for px in out.mrt[0].chunks(4) {
        assert_eq!(px, [0.75, 0.25, 0.5, 1.0]);
    }
}

#[test]
fn replay_writes_fixture_cbuffers_verbatim() {
    let d = tempfile::tempdir().unwrap();
    write_fixture(d.path(), STANDIN);
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let r = load_fixture(d.path()).unwrap();
    let out = ren.render_replay(&r, false).unwrap();
    assert_eq!(out.mrt.len(), 1);
    assert!(out.main.is_none());
    assert_eq!(out.mrt[0].len(), 64);
    for px in out.mrt[0].chunks(4) {
        assert_eq!(px, [0.75, 0.25, 0.5, 1.0]);
    }
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let e = ren.render_replay(&r, true).err().unwrap();
    assert!(e.contains("8 RTs"), "{e}");
    fs::remove_file(d.path().join("cb_ps_b2.bin")).unwrap();
    let r = load_fixture(d.path()).unwrap();
    let e = ren.render_replay(&r, false).err().unwrap();
    assert!(e.contains("cb_ps_b2"), "{e}");
}

/// [`write_fixture`] with DEFERRED and the 8 CS targets: every RT seeded (0, 0, 0, 1); write masks as Coffin's
/// (RT0 and RT3 keep their alpha).
fn write_fixture_deferred(dir: &Path) {
    let defines: Vec<&str> = STANDIN.iter().copied().chain(["DEFERRED"]).collect();
    write_fixture(dir, &defines);
    let rts: Vec<_> = MRT_FORMATS
        .iter()
        .enumerate()
        .map(|(i, (_, f))| {
            json!({"index": i, "format": f,
                   "files": {"before": format!("mrt{i}_before.f32"), "after": format!("mrt{i}.f32")}})
        })
        .collect();
    edit_json(dir, "mrt.json", &|v| v["rts"] = json!(rts));
    let seed = f32s(&[0.0, 0.0, 0.0, 1.0].repeat(16));
    for i in 0..8 {
        fs::write(dir.join(format!("mrt{i}_before.f32")), &seed).unwrap();
        fs::write(dir.join(format!("mrt{i}.f32")), &seed).unwrap();
    }
    edit_json(dir, "state.json", &|v| {
        v["stageB"]["blend"] = (0..8)
            .map(|i| json!({"enabled": false, "writeMask": if i == 0 || i == 3 { 7 } else { 15 }}))
            .collect()
    });
}

#[test]
fn replay_deferred_reads_eight_targets() {
    let d = tempfile::tempdir().unwrap();
    write_fixture_deferred(d.path());
    let r = load_fixture(d.path()).unwrap();
    assert_eq!(r.before.len(), 8);
    let names: Vec<&str> = MRT_FORMATS.iter().map(|f| f.1).collect();
    assert_eq!(r.formats, names);
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let out = ren.render_replay(&r, true).unwrap();
    assert_eq!(out.mrt.len(), 8);
    let m: [[f32; 4]; 8] = [
        [0.75, 0.25, 0.5, 1.0],
        [0.25, 0.5, 0.0, 1.0],
        [1.0, 0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.5, 0.25, 0.125, 1.0],
        [0.25, 0.125, 0.5, 1.0],
        [2.0, 4.0, 0.0625, 1.0],
        [1.0, 0.0, 0.0, 1.0],
    ];
    for (i, rt) in out.mrt.iter().enumerate() {
        assert_eq!(rt.len(), 64, "RT{i}");
        for px in rt.chunks(4) {
            assert_eq!(px, m[i], "RT{i}");
        }
    }
    let fwd = ren.render_replay(&r, false).unwrap();
    assert_eq!(fwd.mrt[0], out.mrt[0]);
}

/// D6c: captured prepass depth over the crop (left half 0x00000000, right half 0x00FFFFFF) and the captured depth
/// state, with ViewProj's third row zeroed in cb_vs_b2.bin so every vertex has z = 0 (exact in D24).
fn write_depth(dir: &Path, depth_stencil: serde_json::Value) {
    let mut vs = fs::read(dir.join("cb_vs_b2.bin")).unwrap();
    // World is 12 floats, ViewProj row-major after it: row 2 = floats 20..24.
    vs[80..96].fill(0);
    fs::write(dir.join("cb_vs_b2.bin"), vs).unwrap();
    edit_json(dir, "extras.json", &|v| {
        v["depthStencil"] = depth_stencil.clone()
    });
    put(
        dir,
        "depth.json",
        &json!({"format": "R24G8_TYPELESS", "resourceId": "ResourceId::1", "beforeEvent": 1,
                "crop": [2, 2, 4, 4], "file": "depth_before.d24s8", "sameAtEvent": true, "pick": []}),
    );
    let px: Vec<u8> = (0..16u32)
        .flat_map(|i| if i % 4 < 2 { 0u32 } else { 0x00FF_FFFF }.to_le_bytes())
        .collect();
    fs::write(dir.join("depth_before.d24s8"), px).unwrap();
}

fn equal_no_write() -> serde_json::Value {
    json!({"depthEnable": true, "depthFunc": "Equal", "depthWrite": false, "stencilEnable": false})
}

#[test]
fn replay_seeds_captured_depth_and_uses_the_captured_depth_state() {
    let d = tempfile::tempdir().unwrap();
    write_fixture_deferred(d.path());
    write_depth(d.path(), equal_no_write());
    let r = load_fixture(d.path()).unwrap();
    let depth = r.depth.as_ref().expect("depth.json read");
    assert_eq!((depth.func, depth.write, depth.d24s8.len()), (3, false, 16));
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let drawn: [[f32; 4]; 8] = [
        [0.75, 0.25, 0.5, 1.0],
        [0.25, 0.5, 0.0, 1.0],
        [1.0, 0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        [0.5, 0.25, 0.125, 1.0],
        [0.25, 0.125, 0.5, 1.0],
        [2.0, 4.0, 0.0625, 1.0],
        [1.0, 0.0, 0.0, 1.0],
    ];
    for deferred in [true, false] {
        let out = ren.render_replay(&r, deferred).unwrap();
        assert_eq!(out.mrt.len(), if deferred { 8 } else { 1 });
        for (i, rt) in out.mrt.iter().enumerate() {
            for (p, px) in rt.chunks(4).enumerate() {
                let want = if p % 4 < 2 {
                    drawn[i]
                } else {
                    [0.0, 0.0, 0.0, 1.0]
                };
                assert_eq!(px, want, "deferred {deferred} RT{i} pixel {p}");
            }
        }
    }
    // D6c test hook: the seeded D24 depth reads back as 0 (drawn, Equal) and 1 (untouched).
    assert!(ren.render_replay(&r, true).unwrap().depth.is_none());
    ren.set_replay_depth_readback(true);
    let depth = ren.render_replay(&r, true).unwrap().depth.unwrap();
    let want: Vec<f32> = (0..16).map(|p| if p % 4 < 2 { 0.0 } else { 1.0 }).collect();
    assert_eq!(depth, want);
    // Without depth.json the whole crop is drawn, as before.
    fs::remove_file(d.path().join("depth.json")).unwrap();
    let r = load_fixture(d.path()).unwrap();
    assert!(r.depth.is_none());
    // The target's D32 depth: z = 0 everywhere.
    assert_eq!(
        ren.render_replay(&r, true).unwrap().depth,
        Some(vec![0.0; 16])
    );
    let out = ren.render_replay(&r, true).unwrap();
    for (i, rt) in out.mrt.iter().enumerate() {
        for px in rt.chunks(4) {
            assert_eq!(px, drawn[i], "no depth RT{i}");
        }
    }
}

#[test]
fn load_fixture_maps_the_captured_depth_state() {
    let load = |ds: serde_json::Value| {
        let d = tempfile::tempdir().unwrap();
        write_fixture(d.path(), STANDIN);
        write_depth(d.path(), ds);
        let r = load_fixture(d.path()).unwrap();
        let depth = r.depth.unwrap();
        (depth.func, depth.write)
    };
    let ds = |en: bool, f: &str, w: bool| json!({"depthEnable": en, "depthFunc": f, "depthWrite": w, "stencilEnable": false});
    assert_eq!(load(ds(true, "LessEqual", true)), (4, true));
    assert_eq!(load(ds(true, "AlwaysFalse", false)), (1, false));
    assert_eq!(load(ds(true, "AlwaysTrue", false)), (8, false));
    assert_eq!(load(ds(false, "Equal", true)), (8, false));
}

#[test]
fn load_fixture_rejects_bad_depth() {
    let fails = |f: &dyn Fn(&Path)| {
        let d = tempfile::tempdir().unwrap();
        write_fixture(d.path(), STANDIN);
        write_depth(d.path(), equal_no_write());
        f(d.path());
        match load_fixture(d.path()) {
            Ok(_) => panic!("load_fixture should fail"),
            Err(e) => e,
        }
    };
    let e = fails(&|p| fs::write(p.join("depth_before.d24s8"), [0u8; 60]).unwrap());
    assert!(e.contains("depth_before.d24s8"), "{e}");
    let e = fails(&|p| edit_json(p, "depth.json", &|v| v["crop"] = json!([2, 2, 4, 3])));
    assert!(e.contains("depth.json") && e.contains("crop"), "{e}");
    let e = fails(&|p| {
        edit_json(p, "extras.json", &|v| {
            v["depthStencil"]["depthFunc"] = json!("Bogus")
        })
    });
    assert!(e.contains("extras.json") && e.contains("Bogus"), "{e}");
    let e = fails(&|p| {
        edit_json(p, "extras.json", &|v| {
            v["depthStencil"]["stencilEnable"] = json!(true)
        })
    });
    assert!(
        e.contains("extras.json") && e.contains("stencil test not replayable"),
        "{e}"
    );
    // depthEnable false overrides func/write only after both are valid.
    let e = fails(&|p| {
        edit_json(p, "extras.json", &|v| {
            v["depthStencil"]["depthEnable"] = json!(false);
            v["depthStencil"]["depthFunc"] = json!("Bogus");
        })
    });
    assert!(e.contains("extras.json") && e.contains("Bogus"), "{e}");
    let e = fails(&|p| {
        edit_json(p, "extras.json", &|v| {
            v["depthStencil"]["depthEnable"] = json!(false);
            v["depthStencil"]
                .as_object_mut()
                .unwrap()
                .remove("depthWrite");
        })
    });
    assert!(e.contains("extras.json") && e.contains("depthWrite"), "{e}");
    // Only NotFound means "no depth": any other read error names depth.json.
    let e = fails(&|p| {
        fs::remove_file(p.join("depth.json")).unwrap();
        fs::create_dir(p.join("depth.json")).unwrap();
    });
    assert!(e.contains("depth.json"), "{e}");
}

/// [`write_fixture_deferred`] plus the subject-level composite.json in the golden layout (D4): the stand-in composite
/// with ADD, b5 BufferDim = (8, 8, 1/8, 1/8), MainRW seeded (0.25, 0.5, 0.75, 0.5) over the crop.
fn write_fixture_composite(dir: &Path) {
    write_fixture_deferred(dir);
    let srv = |name: &str, bind: u32, ty: &str| json!({"name": name, "bind": bind, "type": ty});
    put(
        dir,
        "composite.json",
        &json!({
            "threadGroups": [1, 1, 1],
            "srvs": [srv("SpecularTexture", 0, "Texture2D"), srv("NormalRoughnessTexture", 2, "Texture2D"),
                     srv("DepthTexture", 4, "null")],
            "uavs": [{"name": "MainRW", "bind": 0}],
            "cbuffers": [{"name": "SharedData", "bind": 5, "file": "cb_cs_b5.bin"}],
            "definesInferred": ["STANDIN_COMPOSITE_ADD"],
            "mainrw": {"crop": [2, 2, 4, 4], "files": ["mainrw_before.f32", "mainrw_after.f32"]},
        }),
    );
    fs::write(dir.join("cb_cs_b5.bin"), f32s(&[8.0, 8.0, 0.125, 0.125])).unwrap();
    let before = f32s(&[0.25, 0.5, 0.75, 0.5].repeat(16));
    fs::write(dir.join("mainrw_before.f32"), &before).unwrap();
    fs::write(dir.join("mainrw_after.f32"), &before).unwrap();
}

#[test]
fn replay_deferred_composites_main() {
    let d = tempfile::tempdir().unwrap();
    write_fixture_composite(d.path());
    let r = load_fixture(d.path()).unwrap();
    let rc = r.composite.as_ref().unwrap();
    assert_eq!(
        rc.cbuffers.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from(["cs_b5".to_string()])
    );
    assert_eq!(rc.before.len(), 64);
    assert_eq!(rc.thread_groups, [1, 1, 1]);
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let out = ren.render_replay(&r, true).unwrap();
    let main = out.main.unwrap();
    assert_eq!(main.len(), 64);
    for p in main.chunks(4) {
        assert_eq!(p, [0.75, 0.75, 0.875, 1.0]);
    }
    for p in out.mrt[0].chunks(4) {
        assert_eq!(p, [0.75, 0.25, 0.5, 1.0], "mrt[0] is the pre-composite RT0");
    }
    assert!(ren.render_replay(&r, false).unwrap().main.is_none());
}

#[test]
fn replay_composite_refuses_mismatched_fixtures() {
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let replay_err = |ren: &mut Renderer, dir: &Path| -> String {
        match load_fixture(dir).and_then(|r| ren.render_replay(&r, true)) {
            Ok(_) => panic!("should fail"),
            Err(e) => e,
        }
    };
    let d = tempfile::tempdir().unwrap();
    write_fixture_composite(d.path());
    fs::remove_file(d.path().join("cb_cs_b5.bin")).unwrap();
    let e = replay_err(&mut ren, d.path());
    assert!(e.contains("cb_cs_b5"), "{e}");

    let d = tempfile::tempdir().unwrap();
    write_fixture_composite(d.path());
    edit_json(d.path(), "composite.json", &|v| {
        v["srvs"].as_array_mut().unwrap().pop();
    });
    let e = replay_err(&mut ren, d.path());
    assert!(e.contains("composite reflects"), "{e}");

    let d = tempfile::tempdir().unwrap();
    write_fixture_composite(d.path());
    edit_json(d.path(), "composite.json", &|v| {
        v["mainrw"]["crop"] = json!([0, 0, 4, 4])
    });
    let e = replay_err(&mut ren, d.path());
    assert!(e.contains("composite.json"), "{e}");

    // Only NotFound means "no composite": any other read error names composite.json.
    let d = tempfile::tempdir().unwrap();
    write_fixture_composite(d.path());
    fs::remove_file(d.path().join("composite.json")).unwrap();
    fs::create_dir(d.path().join("composite.json")).unwrap();
    let e = replay_err(&mut ren, d.path());
    assert!(e.contains("composite.json"), "{e}");

    // D3's fixture has no composite.json: no main.
    let d = tempfile::tempdir().unwrap();
    write_fixture_deferred(d.path());
    let r = load_fixture(d.path()).unwrap();
    assert!(r.composite.is_none());
    assert!(ren.render_replay(&r, true).unwrap().main.is_none());
}

/// D6c part 3: a debug view compiles the injected source with SDC_DEBUG, so RT0 is the view's value (the stand-in's
/// Roughness view writes 1 − glossiness = 0.75); Lit draws the material colour as before.
#[test]
fn replay_honours_the_debug_view() {
    let shaders = tempfile::tempdir().unwrap();
    let src = fs::read_to_string(common::standin_root().join("Lighting.hlsl")).unwrap();
    // The stand-in has no debug anchor on purpose (tests/deferred.rs pattern).
    fs::write(
        shaders.path().join("Lighting.hlsl"),
        src.replace("    return psout ;", "    return psout;"),
    )
    .unwrap();
    let d = tempfile::tempdir().unwrap();
    write_fixture_deferred(d.path());
    let r = load_fixture(d.path()).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer_at(shaders.path(), cache.path(), &[]);
    ren.set_debug_output(render3d::DebugOutput::Roughness)
        .unwrap();
    let out = ren.render_replay(&r, false).unwrap();
    for px in out.mrt[0].chunks(4) {
        assert_eq!(px, [0.75, 0.75, 0.75, 1.0], "Roughness view");
    }
    ren.set_debug_output(render3d::DebugOutput::Lit).unwrap();
    let out = ren.render_replay(&r, false).unwrap();
    for px in out.mrt[0].chunks(4) {
        assert_eq!(px, [0.75, 0.25, 0.5, 1.0], "Lit");
    }
}

#[test]
#[ignore]
fn replay_real_fixture_smoke() {
    let (Ok(root), Ok(shaders)) = (std::env::var("SDC_GOLDEN"), std::env::var("SDC_CS_SHADERS"))
    else {
        println!("skipped: set SDC_GOLDEN and SDC_CS_SHADERS");
        return;
    };
    let root = Path::new(&root);
    let cache = tempfile::tempdir().unwrap();
    let fp = render3d::locate::fingerprint(Path::new(&shaders), None).unwrap();
    let mut ren = Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    let noise = Noise::load(
        &root.join("noise_Skyrim_AE_1.6.1170_frame207657_Skyrim_AE_1.6.1170_frame207658.json"),
    )
    .unwrap();
    for (s, deferred) in SUBJECTS.iter().flat_map(|s| [(s, false), (s, true)]) {
        let dir = root.join(s);
        let r = load_fixture(&dir).unwrap();
        let out = ren.render_replay(&r, deferred).unwrap();
        let n = (r.crop[2] * r.crop[3]) as usize;
        let mask = golden::read_mask(&dir.join("mask.bin"), n).unwrap();
        let name = dir.file_name().unwrap().to_string_lossy();
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        if deferred {
            assert_eq!(out.mrt.len(), 8, "{s}");
            for rt in &out.mrt {
                assert_eq!(rt.len(), n * 4, "{s}");
                assert!(rt.iter().all(|v| v.is_finite()), "{s}");
            }
            let fwd = ren.render_replay(&r, false).unwrap();
            assert_eq!(
                bits(&out.mrt[0]),
                bits(&fwd.mrt[0]),
                "{s}: RT0 deferred vs forward"
            );
            let masked = mask.iter().filter(|m| **m).count();
            for (i, rt) in out.mrt.iter().enumerate() {
                let file = format!("mrt{i}.f32");
                let after = golden::read_f32(&dir.join(&file), n * 4).unwrap();
                let (p99, max) = noise.limit(&name, &file).unwrap();
                let d = golden::diff_tol(
                    rt,
                    &after,
                    &mask,
                    golden::format_tol(&r.formats[i]).unwrap(),
                    max,
                );
                let identical = (0..n)
                    .filter(|&p| {
                        mask[p] && bits(&rt[4 * p..4 * p + 4]) == bits(&after[4 * p..4 * p + 4])
                    })
                    .count();
                // A zero diff where the draw changed nothing would only compare `before` with itself.
                let changed = (0..n)
                    .filter(|&p| mask[p] && rt[4 * p..4 * p + 4] != r.before[i][4 * p..4 * p + 4])
                    .count();
                println!(
                    "{s} RT{i} {}: {d:?} (noise p99 {p99}, max {max}); bit-identical {identical} of {masked}; changed {changed}",
                    r.formats[i]
                );
            }
            println!("{s} deferred: warnings {:?}", out.warnings);
            // D4: the composite's MainRW against mainrw_after, over the mask and over the part of it where RT0
            // at subject time still equals mainrw_before (later overdraw excluded).
            let rc = r.composite.as_ref().expect("composite.json");
            let main = out.main.as_ref().expect("composite ran");
            assert_eq!(main.len(), n * 4, "{s}");
            assert!(main.iter().all(|v| v.is_finite()), "{s}");
            let after = golden::read_f32(&dir.join("mainrw_after.f32"), n * 4).unwrap();
            let (_, max) = noise.limit(&name, "mainrw_after.f32").unwrap();
            let mrt0 = golden::read_f32(&dir.join("mrt0.f32"), n * 4).unwrap();
            let clean: Vec<bool> = (0..n)
                .map(|p| {
                    mask[p] && bits(&mrt0[4 * p..4 * p + 4]) == bits(&rc.before[4 * p..4 * p + 4])
                })
                .collect();
            for (label, m) in [("mask", &mask), ("mask & mrt0 == mainrw_before", &clean)] {
                let d = golden::diff_tol(
                    main,
                    &after,
                    m,
                    golden::format_tol("R16G16B16A16_FLOAT").unwrap(),
                    max,
                );
                let count = m.iter().filter(|x| **x).count();
                let identical = (0..n)
                    .filter(|&p| {
                        m[p] && bits(&main[4 * p..4 * p + 4]) == bits(&after[4 * p..4 * p + 4])
                    })
                    .count();
                let changed = (0..n)
                    .filter(|&p| m[p] && main[4 * p..4 * p + 4] != rc.before[4 * p..4 * p + 4])
                    .count();
                println!(
                    "{s} composite [{label}]: {d:?} (noise max {max}); bit-identical {identical} of {count}; changed from before {changed}"
                );
            }
            continue;
        }
        assert_eq!(out.mrt[0].len(), n * 4, "{s}");
        assert!(out.mrt[0].iter().all(|v| v.is_finite()), "{s}");
        let masked = mask.iter().filter(|m| **m).count();
        assert!(masked > 0, "{s}: empty mask");
        let changed = (0..n)
            .filter(|&p| mask[p] && out.mrt[0][4 * p..4 * p + 4] != r.before[0][4 * p..4 * p + 4])
            .count();
        assert!(
            changed * 2 >= masked,
            "{s}: {changed} of {masked} masked pixels changed"
        );
        let after = golden::read_f32(&dir.join("mrt0.f32"), n * 4).unwrap();
        let (p99, max) = noise.limit(&name, "mrt0.f32").unwrap();
        let d = golden::diff_tol(
            &out.mrt[0],
            &after,
            &mask,
            golden::format_tol("R16G16B16A16_FLOAT").unwrap(),
            max,
        );
        let identical = (0..n)
            .filter(|&p| {
                mask[p] && bits(&out.mrt[0][4 * p..4 * p + 4]) == bits(&after[4 * p..4 * p + 4])
            })
            .count();
        println!("{s}: {d:?} (noise p99 {p99}, max {max} used); changed {changed}/{masked}");
        println!("{s}: bit-identical: {identical} of {masked}");
        println!("{s}: warnings {:?}", out.warnings);
    }
}

/// A DX10 cube DDS: `size`², `mips`, dxgi `format`, misc flags and arraySize as given, `data` after the header.
fn cube_dds(size: u32, mips: u32, format: u32, misc: u32, array: u32, data: &[u8]) -> Vec<u8> {
    let mut h = vec![0u8; 148];
    h[..4].copy_from_slice(b"DDS ");
    for (o, v) in [
        (4, 124),
        (8, 0x1007 | 0x20000),
        (12, size),
        (16, size),
        (28, mips),
        (76, 32),
        (80, 4),
        (128, format),
        (132, 3),
        (136, misc),
        (140, array),
    ] {
        h[o..o + 4].copy_from_slice(&u32::to_le_bytes(v));
    }
    h[84..88].copy_from_slice(b"DX10");
    h.extend_from_slice(data);
    h
}

#[test]
fn read_cube_dds_accepts_cubes_and_rejects_others() {
    use render3d::textures::read_cube_dds;
    // R11G11B10, 4², 3 mips: (16 + 4 + 1) texels × 4 bytes per face.
    let data = vec![7u8; 6 * 21 * 4];
    let file = cube_dds(4, 3, 26, 4, 1, &data);
    let (size, mips, f, d) = read_cube_dds(&file).unwrap();
    assert_eq!((size, mips, f.0, d.len()), (4, 3, 26, data.len()));
    // arraySize 6 without the cube flag is 6 faces too.
    assert!(read_cube_dds(&cube_dds(4, 3, 26, 0, 6, &data)).is_ok());
    // RGBA16F: 8 bytes per texel.
    assert!(read_cube_dds(&cube_dds(4, 3, 10, 4, 1, &vec![0u8; 6 * 21 * 8])).is_ok());
    let bad = [
        ("2D", cube_dds(4, 3, 26, 0, 1, &data[..21 * 4])),
        ("6 cubes", cube_dds(4, 3, 26, 4, 6, &data)),
        ("unknown format", cube_dds(4, 3, 28, 4, 1, &data)),
        (
            "truncated",
            cube_dds(4, 3, 26, 4, 1, &data[..data.len() - 1]),
        ),
        ("header only", cube_dds(4, 3, 26, 4, 1, &[])[..100].to_vec()),
        ("not DDS", b"nope".to_vec()),
        // Review I1: 16384² × 16 B = 2^32 overflows u32 face sizes; an empty body must be Err, not a panic.
        (
            "hostile 16384² RGBA32F, empty body",
            cube_dds(16384, 1, 2, 4, 1, &[]),
        ),
        (
            "hostile 16384² RGBA32F, 15 mips",
            cube_dds(16384, 15, 2, 4, 1, &[]),
        ),
        ("over the 16384 cap", cube_dds(32768, 1, 26, 4, 1, &[])),
        (
            "146-byte header",
            cube_dds(4, 3, 26, 4, 1, &[])[..146].to_vec(),
        ),
    ];
    for (what, b) in bad {
        assert!(read_cube_dds(&b).is_err(), "{what}");
    }
    let mut non_square = cube_dds(4, 3, 26, 4, 1, &data);
    non_square[12..16].copy_from_slice(&8u32.to_le_bytes());
    assert!(read_cube_dds(&non_square).is_err(), "non-square");
}

/// An RGBA16F 2² cube, 2 mips: mip 0 of face f is `mip0[f]` (all 4 texels), mip 1 is `mip1[f]`.
fn rgba16f_cube(mip0: [[f32; 3]; 6], mip1: [[f32; 3]; 6]) -> Vec<u8> {
    let h = |c: [f32; 3]| -> Vec<u8> {
        [c[0], c[1], c[2], 1.0]
            .iter()
            .flat_map(|v| half::f16::from_f32(*v).to_le_bytes())
            .collect()
    };
    let mut data = vec![];
    for f in 0..6 {
        for _ in 0..4 {
            data.extend(h(mip0[f]));
        }
        data.extend(h(mip1[f]));
    }
    cube_dds(2, 2, 10, 4, 1, &data)
}

/// The golden layout of a DC capture: `root/composite/` (capture level: t7.dds, extras.json with the composite's
/// sampler) and the subject `root/ev1_subject` ([`write_fixture_deferred`] + a composite.json that compiles the
/// stand-in with DYNAMIC_CUBEMAPS: t4 depth, t5 reflectance, t7 cube). `sampler_max_lod` is the captured maxLOD.
fn write_fixture_dc(
    root: &Path,
    t7_file: Option<&str>,
    sampler_max_lod: f32,
) -> std::path::PathBuf {
    let dir = root.join("ev1_subject");
    let cap = root.join("composite");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&cap).unwrap();
    write_fixture_deferred(&dir);
    let mut t7 = json!({"name": "EnvReflectionsTexture", "bind": 7, "type": "TextureCubeArray", "cube": true,
                        "format": "R16G16B16A16_FLOAT"});
    if let Some(f) = t7_file {
        t7["file"] = json!(f);
        t7["saved"] = json!(true);
    }
    put(
        &dir,
        "composite.json",
        &json!({
            "threadGroups": [1, 1, 1],
            "srvs": [{"name": "DepthTexture", "bind": 4, "type": "Texture2D", "cube": false},
                     {"name": "ReflectanceTexture", "bind": 5, "type": "Texture2D", "cube": false}, t7],
            "uavs": [{"name": "MainRW", "bind": 0}],
            "cbuffers": [{"name": "SharedData", "bind": 5, "file": "../composite/cb_cs_b5.bin"}],
            "definesInferred": ["DYNAMIC_CUBEMAPS"],
            "mainrw": {"crop": [2, 2, 4, 4], "files": ["mainrw_before.f32", "mainrw_after.f32"]},
        }),
    );
    fs::write(cap.join("cb_cs_b5.bin"), f32s(&[8.0, 8.0, 0.125, 0.125])).unwrap();
    put(
        &cap,
        "extras.json",
        &json!({"samplers": {"cs": [{
            "bind": 0, "name": "LinearSampler", "minify": "Linear", "magnify": "Linear", "mip": "Linear",
            "func": "Normal", "addressU": "ClampEdge", "addressV": "ClampEdge", "addressW": "ClampEdge",
            "border": [0.0, 0.0, 0.0, 0.0], "compare": "Never", "minLOD": 0.0, "maxLOD": sampler_max_lod,
            "mipBias": 0.0, "maxAniso": 0.0}]}}),
    );
    // Face +Z (4): mip 0 (2, 4, 1), mip 1 (1, 2, 0.5); every other face 0.
    let (mut m0, mut m1) = ([[0.0; 3]; 6], [[0.0; 3]; 6]);
    m0[4] = [2.0, 4.0, 1.0];
    m1[4] = [1.0, 2.0, 0.5];
    fs::write(cap.join("t7.dds"), rgba16f_cube(m0, m1)).unwrap();
    let before = f32s(&[0.25, 0.5, 0.75, 0.5].repeat(16));
    fs::write(dir.join("mainrw_before.f32"), &before).unwrap();
    fs::write(dir.join("mainrw_after.f32"), &before).unwrap();
    dir
}

/// The captured cube at t7 and the captured sampler: the stand-in adds RT5 (0.25, 0.125, 0.5) × the level-15 +Z
/// sample. The captured maxLOD 0 clamps level 15 to mip 0 (2, 4, 1): with the composite's own sampler it would be mip 1.
#[test]
fn replay_binds_the_captured_env_cube_and_sampler() {
    let root = tempfile::tempdir().unwrap();
    let dir = write_fixture_dc(root.path(), Some("../composite/t7.dds"), 0.0);
    let r = load_fixture(&dir).unwrap();
    let rc = r.composite.as_ref().unwrap();
    assert_eq!(rc.missing_env(), None);
    assert!(rc.sampler.is_some());
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let out = ren.render_replay(&r, true).unwrap();
    let main = out.main.unwrap();
    for p in main.chunks(4) {
        assert_eq!(p, [0.75, 1.0, 1.25, 1.0], "{:?}", out.warnings);
    }
    // maxLOD = max: level 15 reaches mip 1 (1, 2, 0.5).
    let dir = write_fixture_dc(root.path(), Some("../composite/t7.dds"), 3.4e38);
    let out = ren
        .render_replay(&load_fixture(&dir).unwrap(), true)
        .unwrap();
    for p in out.main.unwrap().chunks(4) {
        assert_eq!(p, [0.5, 0.75, 1.0, 1.0]);
    }
    assert_eq!(ren.bound_cs_views(), 0);
}

#[test]
fn replay_without_env_cube_file_reports_missing_env() {
    let root = tempfile::tempdir().unwrap();
    let dir = write_fixture_dc(root.path(), None, 0.0);
    let r = load_fixture(&dir).unwrap();
    let why = r.composite.as_ref().unwrap().missing_env();
    assert!(
        why.as_deref()
            .is_some_and(|w| w.contains("EnvReflectionsTexture")),
        "{why:?}"
    );
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let out = ren.render_replay(&r, true).unwrap();
    assert!(out.main.is_none());
    assert_eq!(out.mrt.len(), 8);
    assert!(
        out.warnings
            .iter()
            .any(|w| w.contains("EnvReflectionsTexture")),
        "{:?}",
        out.warnings
    );
    // A named file that does not read is a load error, not a missing cube.
    let dir = write_fixture_dc(root.path(), Some("../composite/nope.dds"), 0.0);
    let e = load_fixture(&dir).err().expect("unreadable cube file");
    assert!(e.contains("nope.dds"), "{e}");
}

/// Review I1: `cube_srv` sizes in checked u64; an oversized cube or a short body is Err before any D3D call.
#[test]
fn cube_srv_rejects_oversized_and_short_data() {
    use render3d::textures::cube_srv;
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_R11G11B10_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT,
    };
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    assert!(cube_srv(&gpu, 16384, 1, DXGI_FORMAT_R32G32B32A32_FLOAT, 16, &[]).is_err());
    assert!(cube_srv(&gpu, 65536, 17, DXGI_FORMAT_R32G32B32A32_FLOAT, 16, &[]).is_err());
    assert!(cube_srv(&gpu, 32768, 1, DXGI_FORMAT_R11G11B10_FLOAT, 4, &[]).is_err());
    assert!(cube_srv(
        &gpu,
        2,
        2,
        DXGI_FORMAT_R11G11B10_FLOAT,
        4,
        &[0; 6 * 5 * 4 - 1]
    )
    .is_err());
    assert!(cube_srv(&gpu, 2, 3, DXGI_FORMAT_R11G11B10_FLOAT, 4, &[0; 6 * 5 * 4]).is_err());
    assert!(cube_srv(&gpu, 2, 2, DXGI_FORMAT_R11G11B10_FLOAT, 4, &[0; 6 * 5 * 4]).is_ok());
}

/// Review M1: every env cube the composite reflects must be in the capture under its own name and bind. A composite
/// that also reads t6 EnvTexture (stand-in STANDIN_COMPOSITE_ENV6) with only t7 saved is an error, never t7 bound
/// in its place.
#[test]
fn replay_env_cube_binds_by_exact_name() {
    let root = tempfile::tempdir().unwrap();
    let dir = write_fixture_dc(root.path(), Some("../composite/t7.dds"), 0.0);
    edit_json(&dir, "composite.json", &|v| {
        v["definesInferred"] = json!(["DYNAMIC_CUBEMAPS", "STANDIN_COMPOSITE_ENV6"]);
        v["srvs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "EnvTexture", "bind": 6, "type": "null", "cube": false}));
    });
    let r = load_fixture(&dir).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut ren = common::renderer(cache.path(), &[]);
    let e = ren.render_replay(&r, true).expect_err("t6 has no cube");
    assert!(e.contains("EnvTexture"), "{e}");
    assert_eq!(ren.bound_cs_views(), 0);
}
