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

    // D3's fixture has no composite.json: no main.
    let d = tempfile::tempdir().unwrap();
    write_fixture_deferred(d.path());
    let r = load_fixture(d.path()).unwrap();
    assert!(r.composite.is_none());
    assert!(ren.render_replay(&r, true).unwrap().main.is_none());
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
