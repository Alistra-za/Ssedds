//! Note GG: the set preview's material-object (MATO) layer, built from the set's `cs_mato` block and its record's
//! DATA. Model level: the layer is read back from the viewport (`Viewport3d::projected`); one test runs the real
//! render thread on WARP with the repo's stand-in shader.
#[path = "../../esp/tests/common/mod.rs"]
mod b;
#[path = "../../modscan/tests/common/mod.rs"]
mod scan_fixture;
mod support;
use b::{edid, plugin, rec, sub, top};
use render3d::{Glint, ProjectedLayer};
use sdc_gui::model::cs_pbr::Kind;
use sdc_gui::model::set_preview::*;
use sdc_gui::model::viewport3d::Content;
use sdc_gui::model::AppModel;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ROCK: &str = "landscape/rock01";

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn default_layer(base: [f32; 3], roughness: f32, specular: f32) -> ProjectedLayer {
    ProjectedLayer {
        falloff_scale: 0.35,
        falloff_bias: 0.4,
        noise_uv_scale: 48.0,
        colour: [1.0; 3],
        base_color_scale: base,
        roughness,
        specular_level: specular,
        glint: None,
    }
}

#[test]
fn layer_defaults() {
    assert_eq!(
        layer_from_block(&Map::new(), None),
        default_layer([1.0; 3], 1.0, 1.0)
    );
}

#[test]
fn layer_from_values() {
    let v = obj(json!({
        "baseColorScale": [0.66, 0.63, 0.63],
        "roughness": 0.9,
        "specularLevel": 0.04,
        "glintParameters": {"enabled": true, "logMicrofacetDensity": 13, "screenSpaceScale": 1.0},
    }));
    let data = esp::MatoData {
        falloff_scale: 0.5,
        falloff_bias: 0.2,
        noise_uv_scale: 30.0,
        material_uv_scale: 1.0,
        projection: [0.0, 0.0, 1.0],
        normal_dampener: 1.0,
        colour: [0.9, 0.8, 0.7],
        flags: 1,
    };
    let l = layer_from_block(&v, Some(&data));
    assert_eq!(
        l,
        ProjectedLayer {
            falloff_scale: 0.5,
            falloff_bias: 0.2,
            noise_uv_scale: 30.0,
            colour: [0.9, 0.8, 0.7],
            base_color_scale: [0.66, 0.63, 0.63],
            roughness: 0.9,
            specular_level: 0.04,
            // The unset glint fields take CS's defaults.
            glint: Some(Glint {
                screen_space_scale: 1.0,
                log_microfacet_density: 13.0,
                microfacet_roughness: 0.015,
                density_randomization: 2.0,
            }),
        }
    );
    // A disabled glint is no glint.
    let off = obj(json!({"glintParameters": {"enabled": false, "logMicrofacetDensity": 13}}));
    assert_eq!(layer_from_block(&off, None).glint, None);
}

/// Review Focus 2: a wrong-typed or wrongly shaped value takes that field's CS default; nothing panics.
#[test]
fn bad_values_fall_back() {
    let v = obj(json!({
        "roughness": "x",
        "baseColorScale": [0.5, 0.5],
        "specularLevel": null,
        "glintParameters": {"enabled": "yes"},
    }));
    assert_eq!(
        layer_from_block(&v, None),
        default_layer([1.0; 3], 1.0, 1.0)
    );
    for v in [
        json!({"baseColorScale": [0.5, "a", 0.5]}),
        json!({"baseColorScale": "white"}),
        json!({"baseColorScale": [0.5, 0.5, 0.5, 0.5]}),
        json!({"baseColorScale": [1e39, 0.5, 0.5]}),
        json!({"glintParameters": "on"}),
        json!({"glintParameters": {"enabled": true, "screenSpaceScale": "big"}}),
        json!({"roughness": 1e39, "specularLevel": [1]}),
    ] {
        let l = layer_from_block(&obj(v.clone()), None);
        assert_eq!(l.base_color_scale, [1.0; 3], "{v}");
        assert_eq!((l.roughness, l.specular_level), (1.0, 1.0), "{v}");
        if let Some(g) = l.glint {
            assert_eq!(g.screen_space_scale, 1.5, "{v}");
        }
    }
}

/// Final review M2: a non-finite DATA value (or a noise scale whose reciprocal overflows) takes that field's
/// default, so no cbuffer value is NaN or infinite; non-finite JSON values already do.
#[test]
fn non_finite_data_falls_back() {
    let mut d = esp::MatoData {
        falloff_scale: f32::NAN,
        falloff_bias: f32::NEG_INFINITY,
        noise_uv_scale: 1e-45,
        material_uv_scale: 1.0,
        projection: [0.0, 0.0, 1.0],
        normal_dampener: 1.0,
        colour: [1.0, f32::INFINITY, 0.5],
        flags: 1,
    };
    let l = layer_from_block(&Map::new(), Some(&d));
    assert_eq!(l.falloff_scale, 0.35);
    assert_eq!(l.falloff_bias, 0.4);
    assert_eq!(l.noise_uv_scale, 48.0);
    assert_eq!(l.colour, [1.0, 1.0, 0.5]);
    assert!((1.0 / l.noise_uv_scale).is_finite());
    // +inf and a NaN colour channel too; a good field is kept.
    d.noise_uv_scale = f32::INFINITY;
    d.falloff_scale = 0.5;
    d.colour = [f32::NAN, 0.2, 0.3];
    let l = layer_from_block(&Map::new(), Some(&d));
    assert_eq!((l.noise_uv_scale, l.falloff_scale), (48.0, 0.5));
    assert_eq!(l.colour, [1.0, 0.2, 0.3]);
    // The JSON side: a non-finite number is not representable in JSON, but 1e39 overflows f32 to inf.
    let v = obj(json!({"roughness": 1e39, "baseColorScale": [1e39, 1, 1], "specularLevel": -1e39}));
    let l = layer_from_block(&v, None);
    assert_eq!(
        (l.roughness, l.specular_level, l.base_color_scale),
        (1.0, 1.0, [1.0; 3])
    );
}

/// Polls like the app does (panel drawn, input every frame) until nothing is in flight.
fn run(s: &mut SetPreview, m: &AppModel) {
    let t = Instant::now();
    loop {
        s.shown = true;
        s.input = true;
        s.poll(m);
        if !s.busy() {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(90),
            "the preview did not settle"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn rock_key(m: &AppModel) -> String {
    sdc_gui::model::sets::set_views(m)
        .into_iter()
        .find(|v| v.id == ROCK)
        .expect("the fixture's PBR set")
        .key
}

fn other_key(m: &AppModel) -> String {
    sdc_gui::model::sets::set_views(m)
        .into_iter()
        .find(|v| v.id != ROCK)
        .expect("a second set")
        .key
}

/// Review Focus 1: a block with no EDIDs, or EDIDs the scan doesn't know, shows default coverage and the grey line.
#[test]
fn no_record_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    m.add_cs_block(ROCK, Kind::Mato);
    let key = rock_key(&m);
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    assert_eq!(s.v3.projected(), None, "off by default");
    assert!(s.mato_lines().is_empty(), "no lines while off");
    s.set_mato_layer(true);
    run(&mut s, &m);
    assert_eq!(
        s.mato_lines(),
        vec!["No MATO record in the load order; showing default coverage".to_string()]
    );
    assert_eq!(s.v3.projected(), Some(&default_layer([1.0; 3], 1.0, 1.0)));
    m.set_cs_edids(ROCK, Kind::Mato, &["NoSuchMaterial".to_string()]);
    run(&mut s, &m);
    assert_eq!(
        s.mato_lines(),
        vec!["No MATO record in the load order; showing default coverage".to_string()]
    );
    // An unsaved block value reaches the layer.
    m.set_cs_value(ROCK, Kind::Mato, "roughness", Some(json!(0.25)));
    run(&mut s, &m);
    assert_eq!(s.v3.projected().map(|l| l.roughness), Some(0.25));
    // Ruling (d): the simulated PBR built-in carries the entry's specular_level, not builtin_default's 80.
    let mat = &s.rows[0].material;
    assert!(mat.is_pbr());
    assert_eq!(mat.params.glossiness, 0.04);
    s.set_mato_layer(false);
    assert_eq!(s.v3.projected(), None);
    assert!(s.mato_lines().is_empty());
}

/// Final review M5: with the layer on, a block edit reaches the layer on the next frame, with no input event and
/// inside the throttle (a slider drag only produces pointer moves).
#[test]
fn value_edit_reaches_the_layer_next_frame() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    m.add_cs_block(ROCK, Kind::Mato);
    let key = rock_key(&m);
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    s.set_mato_layer(true);
    run(&mut s, &m);
    assert_eq!(s.v3.projected().map(|l| l.roughness), Some(1.0));
    m.set_cs_value(ROCK, Kind::Mato, "roughness", Some(json!(0.25)));
    s.shown = true;
    s.poll(&m);
    assert_eq!(s.v3.projected().map(|l| l.roughness), Some(0.25));
}
/// Review Focus 5: the layer follows the followed set's block.
#[test]
fn switching_sets_follows_block() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    m.add_cs_block(ROCK, Kind::Mato);
    let (a, b) = (rock_key(&m), other_key(&m));
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    s.set_mato_layer(true);
    m.select_set(&a);
    run(&mut s, &m);
    assert!(s.v3.projected().is_some());
    m.select_set(&b);
    run(&mut s, &m);
    assert_eq!(s.v3.projected(), None);
    assert!(s.mato_lines().is_empty());
    m.select_set(&a);
    run(&mut s, &m);
    assert!(s.v3.projected().is_some());
    // The block removed: off.
    m.remove_cs_block(ROCK, Kind::Mato);
    run(&mut s, &m);
    assert_eq!(s.v3.projected(), None);
}

/// D6: a non-PBR drawn shape still gets the layer, and the line says the JSON values do not reach it.
#[test]
fn pbr_only_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    let b = other_key(&m);
    let id = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.key == b)
        .unwrap()
        .id;
    m.add_cs_block(&id, Kind::Mato);
    m.select_set(&b);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    s.set_mato_layer(true);
    run(&mut s, &m);
    assert!(!s.rows[0].material.is_pbr());
    assert!(s.v3.projected().is_some());
    assert_eq!(
        s.mato_lines(),
        vec![
            "No MATO record in the load order; showing default coverage".to_string(),
            "The JSON values apply to PBR shapes only.".to_string()
        ]
    );
}

/// 48-byte MATO DATA: falloff scale/bias, noise UV scale, material UV scale, projection, dampener, colour, flags.
fn data(f: [f32; 11], flags: u32) -> Vec<u8> {
    let mut d: Vec<u8> = f.iter().flat_map(|x| x.to_le_bytes()).collect();
    d.extend(flags.to_le_bytes());
    d
}

/// Skyrim.esm with one MATO `SnowMat1P` (single-pass) of DATA `f`.
fn skyrim_esm(f: [f32; 11]) -> Vec<u8> {
    let mato = rec(
        b"MATO",
        0xA00,
        &[edid("SnowMat1P"), sub(b"DATA", &data(f, 1))],
    );
    plugin(esp::FLAG_ESM, &[], &[top(b"MATO", &[mato])])
}

const SNOW: [f32; 11] = [0.5, 0.2, 30.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.9, 0.8, 0.7];

/// A scanned MO2 instance: Skyrim.esm with [`SNOW`], and mod A's loose projected diffuse.
fn scanned(root: &Path) -> PathBuf {
    scanned_with(root, &skyrim_esm(SNOW))
}

/// [`scanned`] with `esm` as Skyrim.esm.
fn scanned_with(root: &Path, esm: &[u8]) -> PathBuf {
    scan_fixture::put(root, "Game/Data/Skyrim.esm", esm);
    scan_fixture::put(
        root,
        "mods/A/textures/effects/projecteddiffuse.dds",
        b"DDS stand-in",
    );
    scan_fixture::put(root, "profiles/Default/plugins.txt", b"");
    let db = root.join("scan.db");
    let mut st = modscan::store::Store::open(&db).unwrap();
    let src = modscan::scan::Source::Mo2(modscan::mo2::Instance::load(root, None).unwrap());
    modscan::scan::run(
        &src,
        &mut st,
        &std::sync::atomic::AtomicBool::new(false),
        &|_| {},
        &|| vec![],
    )
    .unwrap();
    db
}

/// D3: the record's DATA is read off the UI thread and pushed when it lands; a second pick of the same record is
/// served from the cache at once. The projected textures the load order has are in the scene's texture map.
#[test]
fn record_data_lands_and_is_cached() {
    let inst = scan_fixture::instance_fixture("+A\n");
    let db = scanned(inst.path());
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    m.scan_db = Some(db.clone());
    m.poll();
    m.add_cs_block(ROCK, Kind::Mato);
    // The scan's case differs: still the record (esp::mato_data matches case-insensitively too).
    m.set_cs_edids(
        ROCK,
        Kind::Mato,
        &["Unknown".to_string(), "snowmat1p".to_string()],
    );
    let (a, b) = (rock_key(&m), other_key(&m));
    m.select_set(&a);
    let mut s = SetPreview::new(None, None, Some(db), DEFAULT_CAP);
    s.set_mato_layer(true);
    run(&mut s, &m);
    let want = ProjectedLayer {
        falloff_scale: 0.5,
        falloff_bias: 0.2,
        noise_uv_scale: 30.0,
        colour: [0.9, 0.8, 0.7],
        ..default_layer([1.0; 3], 1.0, 1.0)
    };
    assert_eq!(s.v3.projected(), Some(&want));
    assert!(s.mato_lines().is_empty(), "{:?}", s.mato_lines());
    let Some(Content::Builtin { textures, .. }) = s.v3.content() else {
        panic!("{:?}", s.v3.content())
    };
    assert!(textures.contains_key("textures/effects/projecteddiffuse.dds"));
    assert!(!textures.contains_key("textures/effects/projectednoise.dds"));
    // Away and back: the record is picked again and served from the cache in the same frame, no worker.
    m.select_set(&b);
    run(&mut s, &m);
    assert_eq!(s.v3.projected(), None);
    m.select_set(&a);
    s.shown = true;
    s.input = true;
    s.poll(&m);
    assert_eq!(s.v3.projected(), Some(&want));
}

/// A missing projected texture names its path in the grey lines (the reply the render thread sends, ruling R1).
#[test]
fn missing_texture_lines() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    m.add_cs_block(ROCK, Kind::Mato);
    m.select_set(&rock_key(&m));
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    s.set_mato_layer(true);
    run(&mut s, &m);
    s.v3.handle(sdc_gui::model::viewport3d::V3Reply::ProjectedMissing(vec![
        "textures/effects/projectednoise.dds".into(),
    ]));
    assert_eq!(
        s.mato_lines(),
        vec![
            "No MATO record in the load order; showing default coverage".to_string(),
            "textures/effects/projectednoise.dds is not in the load order; showing a plain fallback"
                .to_string(),
        ]
    );
    // Off: the lines go, and a late reply for the old layer is dropped.
    s.set_mato_layer(false);
    s.v3.handle(sdc_gui::model::viewport3d::V3Reply::ProjectedMissing(vec![
        "textures/effects/projectednoise.dds".into(),
    ]));
    assert!(s.v3.projected_missing.is_empty());
}

/// The render thread (WARP, the repo's stand-in shader): a layer set before the first frame reaches the new thread,
/// resolves its textures through the loaded content's map, and reports the paths the map lacks; the shape draws.
#[test]
fn the_layer_resolves_through_the_content_map() {
    use sdc_gui::model::viewport3d::{FileSource, Viewport3d};
    let standin = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ));
    let f = render3d::locate::fingerprint(&standin, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(f.clone()));
    v.on_located(Ok(Some(f)));
    v.cache_dir = Some(cache.path().to_path_buf());
    let mut mat = nif::material::MaterialState::builtin_default();
    // DO_ALPHA_TEST: the stand-in's pixel permutation compiles.
    mat.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    });
    let white = engine::image::Rgba32F {
        width: 4,
        height: 4,
        px: vec![[1.0; 4]; 16],
    };
    let dds = uncompressed_dds(&[white], false, false).unwrap();
    let textures = std::collections::HashMap::from([(
        "textures/effects/projecteddiffuse.dds".to_string(),
        FileSource::Memory {
            id: 1,
            bytes: std::sync::Arc::new(dds),
        },
    )]);
    v.set_content(
        Some(Content::Builtin {
            key: 1,
            shape: render3d::builtin::Builtin::Sphere,
            material: mat,
            textures: std::sync::Arc::new(textures),
        }),
        false,
    );
    v.set_projected_layer(Some(default_layer([1.0; 3], 1.0, 1.0)));
    let t = Instant::now();
    loop {
        v.poll();
        v.request_frame(64, 64);
        if !v.busy() && !v.shapes.is_empty() && !v.projected_missing.is_empty() {
            break;
        }
        assert!(
            t.elapsed().as_secs() < 60,
            "never went idle: {:?}",
            v.shapes
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        v.projected_missing,
        [
            "textures/effects/projectednormal.dds",
            "textures/effects/projectednormaldetail.dds",
            "textures/effects/projectednoise.dds",
        ]
    );
    assert_eq!(v.shapes[0].status, render3d::ShapeStatus::Drawing);
}

/// A model with ROCK's block naming `SnowMat1P` on the scanned instance, ROCK followed; and its preview.
fn snow_preview(db: &Path, dir: &Path) -> (AppModel, SetPreview) {
    let mut m = support::pbr_opened_plain(dir);
    m.scan_db = Some(db.to_path_buf());
    m.poll();
    m.add_cs_block(ROCK, Kind::Mato);
    m.set_cs_edids(ROCK, Kind::Mato, &["SnowMat1P".to_string()]);
    let a = rock_key(&m);
    m.select_set(&a);
    let s = SetPreview::new(None, None, Some(db.to_path_buf()), DEFAULT_CAP);
    (m, s)
}

/// Fix round 1, I1: turning the layer on picks the record again, so a plugin whose DATA changed (same winning record,
/// new mtime) is read again.
#[test]
fn turning_on_rereads_a_changed_plugin() {
    let inst = scan_fixture::instance_fixture("+A\n");
    let db = scanned(inst.path());
    let dir = tempfile::tempdir().unwrap();
    let (m, mut s) = snow_preview(&db, dir.path());
    s.set_mato_layer(true);
    run(&mut s, &m);
    assert_eq!(s.v3.projected().map(|l| l.falloff_scale), Some(0.5));
    s.set_mato_layer(false);
    run(&mut s, &m);
    let esm = inst.path().join("Game/Data/Skyrim.esm");
    let mut f = SNOW;
    f[0] = 0.7;
    std::fs::write(&esm, skyrim_esm(f)).unwrap();
    let later = std::time::SystemTime::now() + Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&esm)
        .unwrap()
        .set_modified(later)
        .unwrap();
    s.set_mato_layer(true);
    run(&mut s, &m);
    assert_eq!(s.v3.projected().map(|l| l.falloff_scale), Some(0.7));
}

/// A plugin reader that counts its calls and blocks each one until the test sends on the returned gate.
fn gated_reader() -> (
    PluginReader,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::mpsc::Sender<()>,
) {
    let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = std::sync::Mutex::new(rx);
    let calls = n.clone();
    let r: PluginReader = std::sync::Arc::new(move |p: &Path| {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = rx.lock().unwrap().recv();
        std::fs::read(p)
    });
    (r, n, tx)
}

/// One shown frame with input (no waiting for work in flight).
fn frame(s: &mut SetPreview, m: &AppModel) {
    s.shown = true;
    s.input = true;
    s.poll(m);
}

/// Spins (no sleeps) until `n` reads have started.
fn reads_started(n: &std::sync::atomic::AtomicUsize, want: usize) {
    let t = Instant::now();
    while n.load(std::sync::atomic::Ordering::SeqCst) < want {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "the read did not start"
        );
        std::thread::yield_now();
    }
}

/// Fix round 1, I2: one read per key. A→B→A while A's read is pending reuses it (default coverage until it lands); a
/// result for a key that is no longer current is cached but never reaches the layer.
#[test]
fn reads_are_kept_per_key() {
    let inst = scan_fixture::instance_fixture("+A\n");
    let db = scanned(inst.path());
    let dir = tempfile::tempdir().unwrap();
    let (mut m, mut s) = snow_preview(&db, dir.path());
    let (a, b) = (rock_key(&m), other_key(&m));
    let b_id = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.key == b)
        .unwrap()
        .id;
    m.add_cs_block(&b_id, Kind::Mato); // no EDIDs: default coverage
    let (reader, n, gate) = gated_reader();
    s.set_plugin_reader(reader);
    let default = default_layer([1.0; 3], 1.0, 1.0);
    let snow = |falloff_scale: f32| ProjectedLayer {
        falloff_scale,
        falloff_bias: 0.2,
        noise_uv_scale: 30.0,
        colour: [0.9, 0.8, 0.7],
        ..default.clone()
    };
    s.set_mato_layer(true);
    frame(&mut s, &m);
    reads_started(&n, 1);
    assert_eq!(
        s.v3.projected(),
        Some(&default),
        "pending: default coverage"
    );
    assert!(s.busy());
    m.select_set(&b);
    frame(&mut s, &m);
    m.select_set(&a);
    frame(&mut s, &m);
    assert_eq!(s.v3.projected(), Some(&default));
    gate.send(()).unwrap();
    run(&mut s, &m);
    assert_eq!(s.v3.projected(), Some(&snow(0.5)));
    assert_eq!(
        n.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "A was read once"
    );
    // A new key for A (the plugin changed), pending while the user moves to B: its result is cached, not shown on B.
    let esm = inst.path().join("Game/Data/Skyrim.esm");
    let mut f = SNOW;
    f[0] = 0.7;
    std::fs::write(&esm, skyrim_esm(f)).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&esm)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    s.set_mato_layer(false);
    s.set_mato_layer(true);
    frame(&mut s, &m);
    reads_started(&n, 2);
    m.select_set(&b);
    frame(&mut s, &m);
    gate.send(()).unwrap();
    run(&mut s, &m);
    assert_eq!(s.v3.projected(), Some(&default), "B keeps its own coverage");
    m.select_set(&a);
    frame(&mut s, &m);
    assert_eq!(s.v3.projected(), Some(&snow(0.7)), "served from the cache");
    assert_eq!(n.load(std::sync::atomic::Ordering::SeqCst), 2);
}

/// Fix round 2, I2: one plugin read at a time across keys. A, B and C are picked while A's read is gated: only A runs.
/// The user is on C when A lands, so B (queued, no longer current) is skipped and never read, and C is read next.
#[test]
fn one_plugin_read_at_a_time() {
    use std::sync::atomic::Ordering::SeqCst;
    let inst = scan_fixture::instance_fixture("+A\n");
    let recs: Vec<_> = [("SnowMat1P", 0.5), ("AshMat1P", 0.6), ("MossMat1P", 0.7)]
        .into_iter()
        .zip(0xA00u32..)
        .map(|((e, falloff), id)| {
            let mut f = SNOW;
            f[0] = falloff;
            rec(b"MATO", id, &[edid(e), sub(b"DATA", &data(f, 1))])
        })
        .collect();
    let db = scanned_with(
        inst.path(),
        &plugin(esp::FLAG_ESM, &[], &[top(b"MATO", &recs)]),
    );
    let dir = tempfile::tempdir().unwrap();
    let (mut m, mut s) = snow_preview(&db, dir.path());
    let (reader, n, gate) = gated_reader();
    s.set_plugin_reader(reader);
    s.set_mato_layer(true);
    frame(&mut s, &m);
    reads_started(&n, 1);
    for e in ["AshMat1P", "MossMat1P"] {
        m.set_cs_edids(ROCK, Kind::Mato, &[e.to_string()]);
        frame(&mut s, &m);
    }
    assert!(s.busy());
    assert_eq!(n.load(SeqCst), 1, "one read at a time: B and C wait");
    // Two tokens for A and C; a third so a reader that also starts B does not hang the test.
    for _ in 0..3 {
        gate.send(()).unwrap();
    }
    run(&mut s, &m);
    assert_eq!(n.load(SeqCst), 2, "A and C were read; B was skipped");
    assert_eq!(s.v3.projected().map(|l| l.falloff_scale), Some(0.7));
}

/// Fix round 1, I3 (controller ruling): a finite value outside the cs_mato card's range for that field
/// (`MATO_FIELDS` / `GLINT_FIELDS`) takes the field's CS default; the range ends are inside.
#[test]
fn out_of_range_values_fall_back() {
    let l = layer_from_block(
        &obj(json!({
            "roughness": -1,
            "specularLevel": 2,
            "baseColorScale": [0.5, -0.1, 0.5],
            "glintParameters": {
                "enabled": true,
                "screenSpaceScale": 4,
                "logMicrofacetDensity": 41,
                "microfacetRoughness": -0.1,
                "densityRandomization": 6,
            },
        })),
        None,
    );
    assert_eq!((l.roughness, l.specular_level), (1.0, 1.0));
    assert_eq!(l.base_color_scale, [1.0; 3]);
    assert_eq!(
        l.glint,
        Some(Glint {
            screen_space_scale: 1.5,
            log_microfacet_density: 40.0,
            microfacet_roughness: 0.015,
            density_randomization: 2.0,
        })
    );
    assert_eq!(
        layer_from_block(&obj(json!({"baseColorScale": [11, 1, 1]})), None).base_color_scale,
        [1.0; 3]
    );
    // The ends of each range are kept.
    let l = layer_from_block(
        &obj(json!({
            "roughness": 0,
            "specularLevel": 1,
            "baseColorScale": [0, 10, 0.5],
            "glintParameters": {"enabled": true, "screenSpaceScale": 3, "logMicrofacetDensity": 0},
        })),
        None,
    );
    assert_eq!((l.roughness, l.specular_level), (0.0, 1.0));
    assert_eq!(l.base_color_scale, [0.0, 10.0, 0.5]);
    let g = l.glint.unwrap();
    assert_eq!((g.screen_space_scale, g.log_microfacet_density), (3.0, 0.0));
}

// ---- Task 4: the toggle and its grey lines in the panel (kittest; the 3D view never idles, so run_steps) ----

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

type H = Harness<'static, (AppModel, SetPreview)>;

fn panel(m: AppModel) -> H {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(520.0, 1000.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, SetPreview)| sdc_gui::ui::set_preview::draw(ui, m, s),
            (m, SetPreview::new(None, None, None, DEFAULT_CAP)),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Draw and poll like the app does until nothing is in flight.
fn settle(h: &mut H) {
    let t = Instant::now();
    loop {
        h.run_steps(4);
        {
            let (m, s) = h.state_mut();
            s.input = true;
            s.poll(&*m);
        }
        if !h.state().1.busy() {
            h.run_steps(4);
            return;
        }
        assert!(t.elapsed() < Duration::from_secs(90), "did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn rock_panel(dir: &Path) -> H {
    let mut m = support::pbr_opened_plain(dir);
    let key = rock_key(&m);
    m.select_set(&key);
    m.settings.set_view_3d = true;
    panel(m)
}

fn toggle(h: &H) -> egui_kittest::Node<'_> {
    h.get_by_role_and_label(
        eframe::egui::accesskit::Role::CheckBox,
        "Material object layer",
    )
}

/// The toggle is a checkbox with the spec's name, and only a set with a `cs_mato` block has it.
#[test]
fn toggle_only_with_block() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = rock_panel(dir.path());
    settle(&mut h);
    assert!(h.query_by_label("Material object layer").is_none());
    h.state_mut().0.add_cs_block(ROCK, Kind::Mato);
    settle(&mut h);
    assert_eq!(
        toggle(&h).accesskit_node().toggled(),
        Some(eframe::egui::accesskit::Toggled::False),
        "off by default"
    );
    h.state_mut().0.remove_cs_block(ROCK, Kind::Mato);
    settle(&mut h);
    assert!(h.query_by_label("Material object layer").is_none());
}

/// A click turns the layer on; with no scan there is no record, so the grey line says so, in the spec's words.
#[test]
fn toggle_turns_layer_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    m.add_cs_block(ROCK, Kind::Mato);
    let key = rock_key(&m);
    m.select_set(&key);
    m.settings.set_view_3d = true;
    let mut h = panel(m);
    settle(&mut h);
    assert!(h
        .query_by_label("No MATO record in the load order; showing default coverage")
        .is_none());
    toggle(&h).click();
    settle(&mut h);
    assert!(h.state().1.mato_layer);
    assert_eq!(
        toggle(&h).accesskit_node().toggled(),
        Some(eframe::egui::accesskit::Toggled::True)
    );
    h.get_by_label("No MATO record in the load order; showing default coverage");
    toggle(&h).click();
    settle(&mut h);
    assert!(!h.state().1.mato_layer);
    assert!(h
        .query_by_label("No MATO record in the load order; showing default coverage")
        .is_none());
}

#[test]
fn toggle_hover_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened_plain(dir.path());
    m.add_cs_block(ROCK, Kind::Mato);
    let key = rock_key(&m);
    m.select_set(&key);
    m.settings.set_view_3d = true;
    let mut h = panel(m);
    settle(&mut h);
    toggle(&h).hover();
    h.run_steps(12);
    h.get_by_label("Shows the snow/ash layer the game draws on statics that use this material object. Applies to PBR shapes; skinned shapes are not shown.");
}
