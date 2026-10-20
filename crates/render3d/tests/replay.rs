//! Replay mode (v2 Phase D, D2): a golden fixture's captured geometry, cbuffers, textures and samplers.
mod common;

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
    assert!(e.contains("D3"), "{e}");
    fs::remove_file(d.path().join("cb_ps_b2.bin")).unwrap();
    let r = load_fixture(d.path()).unwrap();
    let e = ren.render_replay(&r, false).err().unwrap();
    assert!(e.contains("cb_ps_b2"), "{e}");
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
    for s in SUBJECTS {
        let dir = root.join(s);
        let r = load_fixture(&dir).unwrap();
        let out = ren.render_replay(&r, false).unwrap();
        let n = (r.crop[2] * r.crop[3]) as usize;
        assert_eq!(out.mrt[0].len(), n * 4, "{s}");
        assert!(out.mrt[0].iter().all(|v| v.is_finite()), "{s}");
        let mask = golden::read_mask(&dir.join("mask.bin"), n).unwrap();
        let masked = mask.iter().filter(|m| **m).count();
        let changed = (0..n)
            .filter(|&p| mask[p] && out.mrt[0][4 * p..4 * p + 4] != r.before[0][4 * p..4 * p + 4])
            .count();
        assert!(
            changed * 2 >= masked,
            "{s}: {changed} of {masked} masked pixels changed"
        );
        let after = golden::read_f32(&dir.join("mrt0.f32"), n * 4).unwrap();
        let name = dir.file_name().unwrap().to_string_lossy();
        let (p99, max) = noise.limit(&name, "mrt0.f32").unwrap();
        let d = golden::diff_tol(
            &out.mrt[0],
            &after,
            &mask,
            golden::format_tol("R16G16B16A16_FLOAT").unwrap(),
            max,
        );
        println!("{s}: {d:?} (noise p99 {p99}, max {max} used); changed {changed}/{masked}");
        println!("{s}: warnings {:?}", out.warnings);
    }
}
