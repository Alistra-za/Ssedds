//! Shared fixtures for the Also export tests (Track A Task 7): a PBR set `textures/pbr/arm/steel`, a plain set
//! `textures/clutter/cup`, a PBR landscape set `textures/landscape/dirt` and a complex-material set
//! `textures/clutter/plate` (a raw `_height`: cs packs a `_p`, vanilla a complex `_m`, Note AZ2), in `<tmp>/mods/Mod`.
#![allow(dead_code)]
use engine::also::*;
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::manifest::Manifest;
use engine::policy::AcceptSuggestions;
use engine::presets::{AlsoTarget, ExportConfig};
use engine::session::AnalysisCache;
use engine::shader::Shader;
use image::{ImageBuffer, Rgba};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn png(path: &Path, w: u32, h: u32, px: [u8; 4]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ImageBuffer::from_pixel(w, h, Rgba(px)).save(path).unwrap();
}

/// A 16² gradient, so a BC1 decoder model can change the encoded bytes.
pub fn gradient(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| {
        Rgba([(x * 16) as u8, (y * 16) as u8, ((x + y) * 8) as u8, 255])
    })
    .save(path)
    .unwrap();
}

pub fn steel(m: &Path) {
    let d = m.join("textures/pbr/arm");
    png(&d.join("steel.png"), 16, 16, [200, 200, 200, 255]);
    png(&d.join("steel_n.png"), 16, 16, [128, 128, 255, 255]);
    png(&d.join("steel_rmaos.png"), 16, 16, [128, 255, 200, 255]);
}

pub fn cup(m: &Path) {
    let d = m.join("textures/clutter");
    gradient(&d.join("cup.png"));
    png(&d.join("cup_n.png"), 16, 16, [128, 128, 255, 255]);
}

pub fn dirt(m: &Path) {
    let d = m.join("textures/landscape");
    png(&d.join("dirt.png"), 16, 16, [200, 200, 200, 255]);
    png(&d.join("dirt_n.png"), 16, 16, [128, 128, 255, 255]);
    png(&d.join("dirt_rmaos.png"), 16, 16, [204, 0, 128, 255]);
    png(&d.join("dirt_p.png"), 16, 16, [100, 100, 100, 255]);
}

pub fn plate(m: &Path) {
    let d = m.join("textures/clutter");
    png(&d.join("plate.png"), 16, 16, [180, 170, 160, 255]);
    png(&d.join("plate_height.png"), 16, 16, [90, 90, 90, 255]);
}

/// `<tmp>/mods/Mod` with `build`'s fixtures; the location is `<tmp>/mods`.
pub fn project(build: impl Fn(&Path)) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let loc = d.path().join("mods");
    let m = loc.join("Mod");
    std::fs::create_dir_all(&m).unwrap();
    build(&m);
    (d, m, loc)
}

pub fn request(m: &Path, out: &Path) -> BatchRequest {
    let mut r = BatchRequest::new(vec![m.to_path_buf()], out.to_path_buf(), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.jobs = 2;
    r.set_shaders = BTreeMap::from([
        ("textures/landscape/dirt".to_string(), Shader::PbrLandscape),
        (
            "textures/clutter/plate".to_string(),
            Shader::ComplexMaterial,
        ),
    ]);
    r
}

pub fn spec(loc: &Path, also: &[AlsoTarget]) -> AlsoSpec {
    AlsoSpec {
        cfg: ExportConfig {
            also: also.to_vec(),
            ..Default::default()
        },
        location: loc.to_path_buf(),
        scanned: false,
        inner_layer_sets: BTreeSet::new(),
    }
}

pub const BOTH: &[AlsoTarget] = &[AlsoTarget::Vanilla, AlsoTarget::Cm];

pub fn also_with(
    req: BatchRequest,
    spec: &AlsoSpec,
    sink: Arc<dyn EventSink>,
    cancel: &CancelToken,
) -> Result<AlsoSummary, EngineError> {
    run_also(
        &Engine::new().unwrap(),
        &AnalysisCache::default(),
        req,
        spec,
        sink,
        cancel,
    )
}

pub fn also(
    req: BatchRequest,
    spec: &AlsoSpec,
) -> (Result<AlsoSummary, EngineError>, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let r = also_with(req, spec, sink.clone(), &CancelToken::default());
    (r, sink.take())
}

/// Every `.dds` under `root`, as '/'-separated paths relative to it.
pub fn dds_files(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if !root.exists() {
        return out;
    }
    let mut todo = vec![root.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "dds") {
                out.insert(
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    out
}

pub fn record(folder: &Path, key: &str) -> engine::manifest::ManifestEntry {
    Manifest::at(folder)
        .read()
        .unwrap()
        .remove(key)
        .unwrap_or_else(|| panic!("no record for {key} in {}", folder.display()))
}
