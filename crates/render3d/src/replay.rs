//! Replay mode (v2 Phase D, D2): draws a golden fixture's captured geometry with its exact cbuffers, textures and
//! samplers, so the CS shader math is compared in isolation from render3d's own extraction and cbuffer writers.
//! Ruling D-P2: the captured IA + mesh.vb/ib are drawn as they are; the VS defines are derived from the PS list and
//! the IA ([`vs_defines`]) and checked against the compiled VS input signature.
use crate::compile::compile;
use crate::composite::Composite;
use crate::device::{Gpu, Target, MRT_FORMATS};
use crate::draw::create_program;
use crate::golden;
use crate::permutation::Defines;
use crate::textures::load_dds;
use crate::Renderer;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::path::Path;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11DepthStencilState, ID3D11DepthStencilView, ID3D11SamplerState,
    ID3D11Texture2D, D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_DEPTH_STENCIL, D3D11_BIND_FLAG,
    D3D11_BIND_INDEX_BUFFER, D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC, D3D11_BUFFER_DESC,
    D3D11_COMPARISON_FUNC, D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE, D3D11_DEPTH_STENCIL_DESC,
    D3D11_DEPTH_STENCIL_VIEW_DESC, D3D11_DEPTH_WRITE_MASK_ALL, D3D11_DEPTH_WRITE_MASK_ZERO,
    D3D11_DSV_DIMENSION_TEXTURE2D, D3D11_FILTER, D3D11_FILTER_ANISOTROPIC,
    D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_VERTEX_DATA, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SAMPLER_DESC, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
    D3D11_TEXTURE_ADDRESS_MODE, D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE, D3D11_USAGE_STAGING,
    D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R16G16B16A16_FLOAT,
    DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R24G8_TYPELESS,
    DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};

/// One golden subject, read by [`load_fixture`].
pub struct Replay {
    /// "vs_b2" / "ps_b1" → bytes of cb_<stage>_b<N>.bin. Keyed by file stem: PerTechnique(b0) and PerMaterial(b1)
    /// exist on both stages with different bytes (extras.json).
    pub cbuffers: HashMap<String, Vec<u8>>,
    /// state.json World; None for skinned draws.
    pub world: Option<[[f32; 4]; 3]>,
    /// state.json ViewProj, row-major 16 floats.
    pub view_proj: [[f32; 4]; 4],
    /// = cbuffers["vs_b10"]; kept for D6 (already uploaded through cbuffers).
    pub bones: Option<Vec<u8>>,
    /// tex.json entries with saved: true → tex\t<N>.dds bytes.
    pub textures: HashMap<u32, Vec<u8>>,
    /// tex.json type "null": the game bound nothing (reads 0).
    pub null_srvs: Vec<u32>,
    /// mrt.json crop [x, y, w, h].
    pub crop: [u32; 4],
    /// mrt.json size.
    pub size: (u32, u32),
    /// state.json defines (PS list variants, "K=V" or "K").
    pub defines: Vec<Vec<String>>,
    pub mesh: ReplayMesh,
    /// extras.json samplers.ps, by bind.
    pub samplers: Vec<(u32, Sampler)>,
    /// extras.json rasterizer cull == Back (front faces CCW).
    pub cull_back: bool,
    /// extras.json viewports[0]: x, y, w, h, minDepth, maxDepth.
    pub viewport: [f32; 6],
    /// state.json stageB.blend[i].writeMask.
    pub write_masks: Vec<u8>,
    /// Any stageB.blend[i].enabled.
    pub blended: bool,
    /// mrt<i>_before.f32 of every mrt.json rt, index order.
    pub before: Vec<Vec<f32>>,
    /// mrt.json rt formats, index order (e.g. "R16G16B16A16_FLOAT").
    pub formats: Vec<String>,
    /// The subject's composite.json (D4), if it has one.
    pub composite: Option<ReplayComposite>,
    /// Load-time notes (e.g. a define added from the capture); render_replay passes them on.
    pub warnings: Vec<String>,
    /// depth.json + extras.json depthStencil; None = no captured depth (LessEqual + write from a 1.0 clear, as today).
    pub depth: Option<ReplayDepth>,
}

/// The captured prepass depth (X3d, D6c) and the draw's depth state.
pub struct ReplayDepth {
    /// depth_before.d24s8 over the crop, row-major: depth = v & 0xFFFFFF, stencil = v >> 24.
    pub d24s8: Vec<u32>,
    /// D3D11_COMPARISON_FUNC.
    pub func: i32,
    pub write: bool,
}

/// depth.json of `dir` and extras.json depthStencil, None when there is no depth.json.
fn load_depth(dir: &Path, crop: [u32; 4], extras: &Value) -> Result<Option<ReplayDepth>, String> {
    let dp = dir.join("depth.json");
    // Only NotFound means "no captured depth"; any other error must not fall back to LessEqual silently.
    let s = match std::fs::read_to_string(&dp) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        r => r.map_err(|e| format!("{}: {e}", dp.display()))?,
    };
    let d: Value = serde_json::from_str(&nonfinite_to_null(&s))
        .map_err(|e| format!("{}: {e}", dp.display()))?;
    let derr = |m: &str| format!("{}: {m}", dp.display());
    let format = d.get("format").and_then(Value::as_str).unwrap_or("");
    if !matches!(format, "R24G8_TYPELESS" | "D24_UNORM_S8_UINT") {
        return Err(derr(&format!("format {format:?} is not D24S8")));
    }
    let dcrop = d
        .get("crop")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().map(u32_of).collect::<Option<Vec<_>>>());
    if dcrop.as_deref() != Some(&crop[..]) {
        return Err(derr(&format!(
            "crop {dcrop:?} differs from mrt.json's {crop:?}"
        )));
    }
    let fp = dir.join(
        d.get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| derr("no file"))?,
    );
    let raw = read(&fp)?;
    if raw.len() != crop[2] as usize * crop[3] as usize * 4 {
        return Err(format!(
            "{}: {} bytes, the crop needs {}",
            fp.display(),
            raw.len(),
            crop[2] as usize * crop[3] as usize * 4
        ));
    }
    let d24s8 = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let eerr = |m: &str| format!("{}: depthStencil {m}", dir.join("extras.json").display());
    let ds = extras.get("depthStencil").ok_or_else(|| eerr("missing"))?;
    let flag = |k: &str| {
        ds.get(k)
            .and_then(Value::as_bool)
            .ok_or_else(|| eerr(&format!("{k} must be a bool")))
    };
    if flag("stencilEnable")? {
        return Err(eerr("stencilEnable: stencil test not replayable"));
    }
    let enable = flag("depthEnable")?;
    let write = flag("depthWrite")?;
    let f = ds.get("depthFunc").and_then(Value::as_str).unwrap_or("");
    // D3D11_COMPARISON_FUNC values; RenderDoc names Never/Always AlwaysFalse/AlwaysTrue.
    let func = match f {
        "AlwaysFalse" | "Never" => 1,
        "Less" => 2,
        "Equal" => 3,
        "LessEqual" => 4,
        "Greater" => 5,
        "NotEqual" => 6,
        "GreaterEqual" => 7,
        "AlwaysTrue" | "Always" => 8,
        _ => return Err(eerr(&format!("depthFunc {f:?} unknown"))),
    };
    // Validated first, then a disabled depth test passes everything and writes nothing.
    Ok(Some(if enable {
        ReplayDepth { d24s8, func, write }
    } else {
        ReplayDepth {
            d24s8,
            func: 8,
            write: false,
        }
    }))
}

/// The deferred composite dispatch of a subject's frame (composite.json in the subject dir).
pub struct ReplayComposite {
    /// "cs_b5" → bytes of composite.json cbuffers[].file (relative to the subject dir: "../composite/cb_cs_b5.bin").
    pub cbuffers: HashMap<String, Vec<u8>>,
    /// composite.json srvs (name, bind), "null" ones included.
    pub srvs: Vec<(String, u32)>,
    pub defines_inferred: Vec<String>,
    pub thread_groups: [u32; 3],
    /// mainrw.files [before, after] over the crop (mainrw.crop must equal mrt.json's).
    pub before: Vec<f32>,
    pub after: Vec<f32>,
}

/// composite.json of `dir`, None when there is none.
fn load_composite(
    dir: &Path,
    crop: [u32; 4],
    px: usize,
) -> Result<Option<ReplayComposite>, String> {
    let cp = dir.join("composite.json");
    // Only NotFound means "no composite"; exists() would turn any other error into a silent skip.
    let s = match std::fs::read_to_string(&cp) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        r => r.map_err(|e| format!("{}: {e}", cp.display()))?,
    };
    let c: Value = serde_json::from_str(&nonfinite_to_null(&s))
        .map_err(|e| format!("{}: {e}", cp.display()))?;
    let err = |m: &str| format!("{}: {m}", cp.display());
    let list = |k: &str| {
        c.get(k)
            .and_then(Value::as_array)
            .ok_or_else(|| err(&format!("no {k}")))
    };
    let mut cbuffers = HashMap::new();
    for cb in list("cbuffers")? {
        let (Some(bind), Some(file)) = (
            cb.get("bind").and_then(u32_of),
            cb.get("file").and_then(Value::as_str),
        ) else {
            return Err(err("a cbuffer without bind or file"));
        };
        cbuffers.insert(format!("cs_b{bind}"), read(&dir.join(file))?);
    }
    let srvs = list("srvs")?
        .iter()
        .map(|s| {
            Some((
                s.get("name")?.as_str()?.to_string(),
                u32_of(s.get("bind")?)?,
            ))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| err("an srv without name or bind"))?;
    let defines_inferred = list("definesInferred")?
        .iter()
        .map(|d| d.as_str().map(String::from))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| err("definesInferred must be strings"))?;
    let thread_groups = c
        .get("threadGroups")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 3)
        .and_then(|a| Some([u32_of(&a[0])?, u32_of(&a[1])?, u32_of(&a[2])?]))
        .ok_or_else(|| err("bad threadGroups"))?;
    let m = c.get("mainrw").ok_or_else(|| err("no mainrw"))?;
    let mcrop = m
        .get("crop")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 4)
        .and_then(|a| {
            Some([
                u32_of(&a[0])?,
                u32_of(&a[1])?,
                u32_of(&a[2])?,
                u32_of(&a[3])?,
            ])
        });
    if mcrop != Some(crop) {
        return Err(err(&format!(
            "mainrw crop {mcrop:?} is not mrt.json's {crop:?}"
        )));
    }
    let files = m
        .get("files")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 2)
        .and_then(|a| Some([a[0].as_str()?, a[1].as_str()?]))
        .ok_or_else(|| err("mainrw files must be [before, after]"))?;
    Ok(Some(ReplayComposite {
        cbuffers,
        srvs,
        defines_inferred,
        thread_groups,
        before: golden::read_f32(&dir.join(files[0]), px)?,
        after: golden::read_f32(&dir.join(files[1]), px)?,
    }))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplayMesh {
    /// Semantic, index (trailing digits of "TEXCOORD2"; none = 0), DXGI format, slot, byteOffset.
    pub inputs: Vec<(String, u32, u32, u32, u32)>,
    /// Slot, stride, mesh.vb<slot> bytes (the file is already sliced at srcByteOffset).
    pub vbs: Vec<(u32, u32, Vec<u8>)>,
    /// R16 indices.
    pub ib: Vec<u8>,
    pub index_count: u32,
    pub min_index: u32,
}

/// A captured sampler, already in D3D11 enum values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampler {
    pub filter: i32,
    pub address: [i32; 3],
    pub border: [f32; 4],
    pub compare: i32,
    pub min_lod: f32,
    pub max_lod: f32,
    pub mip_bias: f32,
    pub max_aniso: u32,
}

#[derive(Debug)]
pub struct ReplayOut {
    /// 8 when deferred (D3), 1 forward.
    pub mrt: Vec<Vec<f32>>,
    /// The composite's MainRW (D4).
    pub main: Option<Vec<f32>>,
    pub warnings: Vec<String>,
    /// Depth after the draw over the crop (0..1), only with [`Renderer::set_replay_depth_readback`] (D6c test hook).
    pub depth: Option<Vec<f32>>,
}

fn json(path: &Path) -> Result<Value, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&nonfinite_to_null(&s)).map_err(|e| format!("{}: {e}", path.display()))
}

/// The extractors write Python's `NaN` / `Infinity` / `-Infinity` (garbage cbuffer vars such as WindTimers); serde
/// rejects them, so outside strings they become `null`. No field replay reads may be non-finite.
#[doc(hidden)]
pub fn nonfinite_to_null(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut in_str, mut esc) = (false, false);
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if !in_str {
            if let Some(t) = ["-Infinity", "Infinity", "NaN"]
                .iter()
                .find(|t| rest.starts_with(**t))
            {
                out.push_str("null");
                rest = &rest[t.len()..];
                continue;
            }
        }
        if in_str && esc {
            esc = false;
        } else if in_str && c == '\\' {
            esc = true;
        } else if c == '"' {
            in_str = !in_str;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn floats<const N: usize>(v: &Value) -> Option<[f32; N]> {
    let a = v.as_array().filter(|a| a.len() == N)?;
    let mut out = [0f32; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64()? as f32;
    }
    Some(out)
}

fn rows<const R: usize>(f: &[f32]) -> [[f32; 4]; R] {
    std::array::from_fn(|r| std::array::from_fn(|c| f[r * 4 + c]))
}

fn u32_of(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// The four IA formats the D-P2 survey found; anything else is an error.
fn ia_format(s: &str) -> Option<DXGI_FORMAT> {
    Some(match s {
        "R32G32B32A32_FLOAT" => DXGI_FORMAT_R32G32B32A32_FLOAT,
        "R16G16_FLOAT" => DXGI_FORMAT_R16G16_FLOAT,
        "R8G8B8A8_UNORM" => DXGI_FORMAT_R8G8B8A8_UNORM,
        "R16G16B16A16_FLOAT" => DXGI_FORMAT_R16G16B16A16_FLOAT,
        _ => return None,
    })
}

/// "TEXCOORD2" → ("TEXCOORD", 2); no trailing digits → index 0.
fn split_semantic(s: &str) -> (String, u32) {
    let head = s.trim_end_matches(|c: char| c.is_ascii_digit());
    (head.to_string(), s[head.len()..].parse().unwrap_or(0))
}

fn sampler(v: &Value) -> Result<(u32, Sampler), String> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
    let f = |k: &str| v.get(k).and_then(Value::as_f64).map(|x| x as f32);
    let bind = v
        .get("bind")
        .and_then(u32_of)
        .ok_or("sampler without bind")?;
    let lin = |k: &str| match s(k) {
        "Point" => Ok(0),
        "Linear" => Ok(1),
        x => Err(format!("sampler {bind}: {k} {x:?}")),
    };
    let comparison = match s("func") {
        "Normal" => 0,
        "Comparison" => 0x80,
        x => return Err(format!("sampler {bind}: func {x:?}")),
    };
    let filter = if s("minify") == "Anisotropic" {
        D3D11_FILTER_ANISOTROPIC.0
    } else {
        (0x10 * lin("minify")?) | (0x4 * lin("magnify")?) | lin("mip")?
    } | comparison;
    let mut address = [0; 3];
    for (o, k) in address.iter_mut().zip(["addressU", "addressV", "addressW"]) {
        *o = match s(k) {
            "Wrap" => 1,
            "Mirror" => 2,
            "ClampEdge" => 3,
            "ClampBorder" => 4,
            "MirrorOnce" => 5,
            x => return Err(format!("sampler {bind}: {k} {x:?}")),
        };
    }
    let compare = match s("compare") {
        "Never" => 1,
        "Less" => 2,
        "Equal" => 3,
        "LessEqual" => 4,
        "Greater" => 5,
        "NotEqual" => 6,
        "GreaterEqual" => 7,
        "AlwaysTrue" => 8,
        x => return Err(format!("sampler {bind}: compare {x:?}")),
    };
    let bad = |k: &str| format!("sampler {bind}: bad {k}");
    Ok((
        bind,
        Sampler {
            filter,
            address,
            border: v
                .get("border")
                .and_then(floats::<4>)
                .ok_or_else(|| bad("border"))?,
            compare,
            min_lod: f("minLOD").ok_or_else(|| bad("minLOD"))?,
            max_lod: f("maxLOD").ok_or_else(|| bad("maxLOD"))?,
            mip_bias: f("mipBias").ok_or_else(|| bad("mipBias"))?,
            max_aniso: (f("maxAniso").ok_or_else(|| bad("maxAniso"))? as u32).clamp(1, 16),
        },
    ))
}

fn load_mesh(dir: &Path) -> Result<ReplayMesh, String> {
    let p = dir.join("mesh.json");
    let err = |m: String| format!("{}: {m}", p.display());
    let v = json(&p)?;
    if v.get("topology").and_then(Value::as_str) != Some("TriangleList")
        || v.get("indexed").and_then(Value::as_bool) != Some(true)
        || v.get("indexStride").and_then(Value::as_u64) != Some(2)
        || v.get("numInstances").and_then(Value::as_u64).unwrap_or(0) > 1
    {
        return Err(err(
            "only indexed, R16, non-instanced TriangleList draws replay".into(),
        ));
    }
    let mut inputs = Vec::new();
    for i in v
        .get("inputs")
        .and_then(Value::as_array)
        .ok_or_else(|| err("no inputs".into()))?
    {
        let name = i
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| err("input without name".into()))?;
        let fmt = i.get("format").and_then(Value::as_str).unwrap_or("");
        let format =
            ia_format(fmt).ok_or_else(|| err(format!("{name}: IA format {fmt} not replayable")))?;
        if i.get("perInstance").and_then(Value::as_bool) == Some(true) {
            return Err(err(format!("{name}: per-instance input")));
        }
        let (sem, idx) = split_semantic(name);
        let n = |k: &str| {
            i.get(k)
                .and_then(u32_of)
                .ok_or_else(|| err(format!("{name}: bad {k}")))
        };
        inputs.push((sem, idx, format.0 as u32, n("slot")?, n("byteOffset")?));
    }
    let mut vbs = Vec::new();
    for b in v
        .get("vbs")
        .and_then(Value::as_array)
        .ok_or_else(|| err("no vbs".into()))?
    {
        let n = |k: &str| {
            b.get(k)
                .and_then(u32_of)
                .ok_or_else(|| err(format!("vb: bad {k}")))
        };
        let file = b
            .get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| err("vb without file".into()))?;
        vbs.push((n("slot")?, n("stride")?, read(&dir.join(file))?));
    }
    let ib_file = v
        .get("ib")
        .and_then(Value::as_str)
        .ok_or_else(|| err("no ib".into()))?;
    let ib = read(&dir.join(ib_file))?;
    let index_count = v
        .get("numIndices")
        .and_then(u32_of)
        .ok_or_else(|| err("bad numIndices".into()))?;
    if ib.len() != 2 * index_count as usize {
        return Err(err(format!(
            "{ib_file} has {} bytes, numIndices {index_count}",
            ib.len()
        )));
    }
    // R16: minIndex fits u16, so -minIndex fits the i32 base vertex; no index may lie below it.
    let min_index = v
        .get("minIndex")
        .and_then(u32_of)
        .filter(|m| *m <= u16::MAX as u32)
        .ok_or_else(|| err("bad minIndex (must fit R16)".into()))?;
    if ib
        .as_chunks::<2>()
        .0
        .iter()
        .any(|c| (u16::from_le_bytes(*c) as u32) < min_index)
    {
        return Err(err(format!(
            "{ib_file}: an index lies below minIndex {min_index}"
        )));
    }
    Ok(ReplayMesh {
        inputs,
        vbs,
        ib,
        index_count,
        min_index,
    })
}

impl Replay {
    /// D-P19/D-P22: no culling and DO_ALPHA_TEST in the variant the replay renders (the first). Without the game's
    /// prepass depth, front/back face order on such draws differs from the game.
    pub fn two_sided_alpha_tested(&self) -> bool {
        !self.cull_back
            && self
                .defines
                .first()
                .is_some_and(|v| v.iter().any(|d| d == "DO_ALPHA_TEST"))
    }
}

/// Reads one golden subject folder. Every error names the file.
pub fn load_fixture(dir: &Path) -> Result<Replay, String> {
    let sp = dir.join("state.json");
    let serr = |m: &str| format!("{}: {m}", sp.display());
    let state = json(&sp)?;
    let world = match state.get("World") {
        None | Some(Value::Null) => None,
        Some(w) => Some(rows::<3>(
            &floats::<12>(w).ok_or_else(|| serr("bad World"))?,
        )),
    };
    let view_proj = rows::<4>(
        &state
            .get("ViewProj")
            .and_then(floats::<16>)
            .ok_or_else(|| serr("bad ViewProj"))?,
    );
    // Strict: a dropped entry would compile a different permutation.
    let mut defines: Vec<Vec<String>> = state
        .get("defines")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| serr("defines null or empty: no PS define list to compile"))?
        .iter()
        .map(|l| {
            l.as_array()
                .and_then(|l| l.iter().map(|d| d.as_str().map(String::from)).collect())
        })
        .collect::<Option<_>>()
        .ok_or_else(|| serr("defines: every variant must be a list of strings"))?;
    let mut warnings = Vec::new();
    // D-P15: CS's logged lists omit DEFERRED; the capture's 8 PS outputs prove the deferred variant ran.
    let outputs = state.pointer("/identity/outputs").and_then(Value::as_u64);
    if outputs == Some(8) && defines.iter().any(|v| !v.iter().any(|d| d == "DEFERRED")) {
        for v in defines
            .iter_mut()
            .filter(|v| !v.iter().any(|d| d == "DEFERRED"))
        {
            v.push("DEFERRED".into());
        }
        warnings.push("DEFERRED added from the capture's 8 outputs".to_string());
    }
    let blend = state
        .pointer("/stageB/blend")
        .and_then(Value::as_array)
        .ok_or_else(|| serr("no stageB.blend"))?;
    let write_masks = blend
        .iter()
        .map(|b| {
            b.get("writeMask")
                .and_then(Value::as_u64)
                .filter(|m| *m <= 15)
                .map(|m| m as u8)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| serr("stageB.blend writeMask must be 0..=15"))?;
    // Strict: a non-bool would replay a blended draw as opaque.
    let enabled = blend
        .iter()
        .map(|b| b.get("enabled").and_then(Value::as_bool))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| serr("stageB.blend enabled must be a bool"))?;
    let blended = enabled.contains(&true);

    let mut cbuffers = HashMap::new();
    for ent in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let ent = ent.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = ent.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name
            .strip_prefix("cb_")
            .and_then(|n| n.strip_suffix(".bin"))
        {
            cbuffers.insert(stem.to_string(), read(&ent.path())?);
        }
    }
    let bones = cbuffers.get("vs_b10").cloned();

    let tp = dir.join("tex.json");
    let mut textures = HashMap::new();
    let mut null_srvs = Vec::new();
    for t in json(&tp)?
        .as_array()
        .ok_or_else(|| format!("{}: not a list", tp.display()))?
    {
        let bind = t
            .get("bind")
            .and_then(u32_of)
            .ok_or_else(|| format!("{}: entry without bind", tp.display()))?;
        if t.get("type").and_then(Value::as_str) == Some("null") {
            null_srvs.push(bind);
        } else if t.get("saved").and_then(Value::as_bool) == Some(true) {
            let file = t
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{}: t{bind} saved without file", tp.display()))?;
            textures.insert(bind, read(&dir.join(file))?);
        }
    }
    null_srvs.sort_unstable();

    let mp = dir.join("mrt.json");
    let mrt = json(&mp)?;
    let size = mrt
        .get("size")
        .and_then(Value::as_array)
        .filter(|s| s.len() == 2)
        .and_then(|s| Some((u32_of(&s[0])?, u32_of(&s[1])?)))
        .ok_or_else(|| format!("{}: bad size", mp.display()))?;
    let crop = golden::read_mrt_info(dir)?.crop;
    let inside = |o: u32, l: u32, s: u32| o.checked_add(l).is_some_and(|e| e <= s);
    if !inside(crop[0], crop[2], size.0) || !inside(crop[1], crop[3], size.1) {
        return Err(format!(
            "{}: crop {crop:?} outside size {size:?}",
            mp.display()
        ));
    }
    let px = (crop[2] as usize)
        .checked_mul(crop[3] as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| format!("{}: crop {crop:?} too large", mp.display()))?;
    let mut rts: Vec<(u64, String, String)> = mrt
        .get("rts")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{}: no rts", mp.display()))?
        .iter()
        .map(|r| {
            let s = |p: &str| r.pointer(p).and_then(Value::as_str).map(String::from);
            Some((
                r.get("index")?.as_u64()?,
                s("/format")?,
                s("/files/before")?,
            ))
        })
        .collect::<Option<_>>()
        .ok_or_else(|| {
            format!(
                "{}: an rt without index, format or before file",
                mp.display()
            )
        })?;
    rts.sort_by_key(|r| r.0);
    if rts.is_empty() || rts.iter().enumerate().any(|(i, r)| r.0 != i as u64) {
        return Err(format!(
            "{}: rt indices {:?} are not 0..n",
            mp.display(),
            rts.iter().map(|r| r.0).collect::<Vec<_>>()
        ));
    }
    let before = rts
        .iter()
        .map(|r| golden::read_f32(&dir.join(&r.2), px))
        .collect::<Result<Vec<_>, _>>()?;
    let formats = rts.into_iter().map(|r| r.1).collect();

    let ep = dir.join("extras.json");
    let eerr = |m: &str| format!("{}: {m}", ep.display());
    let extras = json(&ep)?;
    let samplers = extras
        .pointer("/samplers/ps")
        .and_then(Value::as_array)
        .ok_or_else(|| eerr("no samplers.ps"))?
        .iter()
        .map(sampler)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| eerr(&e))?;
    let r = extras
        .get("rasterizer")
        .ok_or_else(|| eerr("no rasterizer"))?;
    let cull = r.get("cull").and_then(Value::as_str).unwrap_or("");
    if !matches!(cull, "Back" | "None" | "NoCull")
        || r.get("frontCCW").and_then(Value::as_bool) != Some(true)
    {
        return Err(eerr(&format!(
            "rasterizer cull {cull:?} / frontCCW {:?} not replayable",
            r.get("frontCCW")
        )));
    }
    let vp = extras
        .pointer("/viewports/0")
        .ok_or_else(|| eerr("no viewport"))?;
    let mut viewport = [0f32; 6];
    for (o, k) in viewport
        .iter_mut()
        .zip(["x", "y", "w", "h", "minDepth", "maxDepth"])
    {
        *o = vp
            .get(k)
            .and_then(Value::as_f64)
            .ok_or_else(|| eerr(&format!("viewport: bad {k}")))? as f32;
    }

    Ok(Replay {
        cbuffers,
        world,
        view_proj,
        bones,
        textures,
        null_srvs,
        crop,
        size,
        defines,
        mesh: load_mesh(dir)?,
        samplers,
        cull_back: cull == "Back",
        viewport,
        write_masks,
        blended,
        before,
        formats,
        composite: load_composite(dir, crop, px)?,
        warnings,
        depth: load_depth(dir, crop, &extras)?,
    })
}

/// A captured cbuffer as uploaded: zero-padded to max(raw, reflected), rounded up to 16 (at least 16). The padding
/// is what D3D11 reads past a short binding (0); a buffer longer than its reflection is kept whole.
pub fn cb_bytes(raw: &[u8], reflected: u32) -> Vec<u8> {
    let mut b = raw.to_vec();
    b.resize(
        (raw.len().max(reflected as usize).max(1)).next_multiple_of(16),
        0,
    );
    b
}

/// PS-only defines render3d's own VS rule leaves out (permutation.rs `defines`), plus the techniques the VS clears.
const PS_ONLY: &[&str] = &[
    "PSHADER",
    "DO_ALPHA_TEST",
    "DEPTH_WRITE_DECALS",
    "TRUE_PBR",
    "GLINT",
    "ANISO_LIGHTING",
    "SPECULAR",
    "SOFT_LIGHTING",
    "RIM_LIGHTING",
    "BACK_LIGHTING",
    "PARALLAX",
    "FACEGEN",
    "FACEGEN_RGB_TINT",
    "HAIR",
];

/// The VS define list for a fixture's PS list: VSHADER for PSHADER, the PS-only defines dropped, VC only when the IA
/// has COLOR and SKINNED only when it has BLENDWEIGHT; everything else (feature defines, DEFERRED, …) kept.
pub fn vs_defines(ps: &[String], mesh: &ReplayMesh) -> Vec<String> {
    let has = |s: &str| mesh.inputs.iter().any(|i| i.0.eq_ignore_ascii_case(s));
    let key = |d: &String| d.split('=').next().unwrap_or("").to_string();
    let mut out = vec!["VSHADER".to_string()];
    out.extend(
        ps.iter()
            .filter(|d| {
                !PS_ONLY.contains(&key(d).as_str())
                    && !matches!(key(d).as_str(), "VSHADER" | "VC" | "SKINNED")
            })
            .cloned(),
    );
    if has("COLOR") {
        out.push("VC".into());
    }
    if has("BLENDWEIGHT") {
        out.push("SKINNED".into());
    }
    out
}

/// "K=V" → (K, V); bare K → (K, "1") as in permutation.rs.
pub(crate) fn to_defines<'a>(list: impl IntoIterator<Item = &'a String>) -> Defines {
    Defines(
        list.into_iter()
            .map(|d| match d.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (d.clone(), "1".to_string()),
            })
            .collect(),
    )
}

fn buffer(gpu: &Gpu, bytes: &[u8], bind: D3D11_BIND_FLAG) -> Result<ID3D11Buffer, String> {
    let d = D3D11_BUFFER_DESC {
        ByteWidth: u32::try_from(bytes.len()).map_err(|_| "buffer over 4 GiB")?,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: bind.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let init = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.as_ptr() as _,
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut b = None;
    // SAFETY: `bytes` outlives the call; descriptors are locals.
    unsafe { gpu.device.CreateBuffer(&d, Some(&init), Some(&mut b)) }
        .map_err(|e| gpu.fail("CreateBuffer", e))?;
    b.ok_or_else(|| "CreateBuffer returned no buffer".into())
}

fn sampler_state(gpu: &Gpu, s: &Sampler) -> Result<ID3D11SamplerState, String> {
    let d = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER(s.filter),
        AddressU: D3D11_TEXTURE_ADDRESS_MODE(s.address[0]),
        AddressV: D3D11_TEXTURE_ADDRESS_MODE(s.address[1]),
        AddressW: D3D11_TEXTURE_ADDRESS_MODE(s.address[2]),
        MipLODBias: s.mip_bias,
        MaxAnisotropy: s.max_aniso,
        ComparisonFunc: D3D11_COMPARISON_FUNC(s.compare),
        BorderColor: s.border,
        MinLOD: s.min_lod,
        MaxLOD: s.max_lod,
    };
    let mut out = None;
    // SAFETY: descriptor and out-pointer are locals.
    unsafe { gpu.device.CreateSamplerState(&d, Some(&mut out)) }
        .map_err(|e| gpu.fail("CreateSamplerState", e))?;
    out.ok_or_else(|| "no sampler".into())
}

/// A full-size D24S8 depth buffer: 0x00FFFFFF (depth 1, stencil 0) with the crop rows from `d`, its DSV and the
/// captured depth state (stencil off). STAGING with initial data, then CopyResource: depth-stencil resources take
/// neither UpdateSubresource nor partial-box copies.
fn seeded_depth(
    gpu: &Gpu,
    size: (u32, u32),
    crop: [u32; 4],
    d: &ReplayDepth,
) -> Result<
    (
        ID3D11DepthStencilView,
        ID3D11DepthStencilState,
        ID3D11Texture2D,
    ),
    String,
> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut px = vec![0x00FF_FFFFu32; w * h];
    let (cx, cy, cw) = (crop[0] as usize, crop[1] as usize, crop[2] as usize);
    for (j, row) in d.d24s8.chunks_exact(cw).enumerate() {
        let o = (cy + j) * w + cx;
        px[o..o + cw].copy_from_slice(row);
    }
    let mut desc = D3D11_TEXTURE2D_DESC {
        Width: size.0,
        Height: size.1,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R24G8_TYPELESS,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
    };
    let init = D3D11_SUBRESOURCE_DATA {
        pSysMem: px.as_ptr() as _,
        SysMemPitch: size.0 * 4,
        SysMemSlicePitch: 0,
    };
    let (mut staging, mut tex, mut dsv, mut state) = (None, None, None, None);
    let depth_tex: ID3D11Texture2D;
    // SAFETY: `px`, the descriptors and out-pointers are live locals; both textures belong to `gpu.device`.
    unsafe {
        gpu.device
            .CreateTexture2D(&desc, Some(&init), Some(&mut staging))
            .map_err(|e| gpu.fail("CreateTexture2D (depth staging)", e))?;
        desc.Usage = D3D11_USAGE_DEFAULT;
        desc.CPUAccessFlags = 0;
        desc.BindFlags = D3D11_BIND_DEPTH_STENCIL.0 as u32;
        gpu.device
            .CreateTexture2D(&desc, None, Some(&mut tex))
            .map_err(|e| gpu.fail("CreateTexture2D (depth)", e))?;
        let (staging, tex) = (
            staging.ok_or("no depth staging texture")?,
            tex.ok_or("no depth texture")?,
        );
        gpu.ctx.CopyResource(&tex, &staging);
        depth_tex = tex.clone();
        let vd = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
            ..Default::default()
        };
        gpu.device
            .CreateDepthStencilView(&tex, Some(&vd), Some(&mut dsv))
            .map_err(|e| gpu.fail("CreateDepthStencilView (depth)", e))?;
        let sd = D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: true.into(),
            DepthWriteMask: if d.write {
                D3D11_DEPTH_WRITE_MASK_ALL
            } else {
                D3D11_DEPTH_WRITE_MASK_ZERO
            },
            DepthFunc: D3D11_COMPARISON_FUNC(d.func),
            ..Default::default()
        };
        gpu.device
            .CreateDepthStencilState(&sd, Some(&mut state))
            .map_err(|e| gpu.fail("CreateDepthStencilState (depth)", e))?;
    }
    Ok((
        dsv.ok_or("no depth view")?,
        state.ok_or("no depth state")?,
        depth_tex,
    ))
}

/// The crop of a depth texture (D32_FLOAT, or R24G8_TYPELESS read as D24) as 0..1 floats. Whole-resource copy:
/// depth-stencil resources take no partial-box copies.
fn read_depth(gpu: &Gpu, tex: &ID3D11Texture2D, crop: [u32; 4]) -> Result<Vec<f32>, String> {
    let mut d = D3D11_TEXTURE2D_DESC::default();
    let mut staging = None;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: descriptors and out-pointers are locals; both textures belong to `gpu.device`.
    unsafe {
        tex.GetDesc(&mut d);
        d.Usage = D3D11_USAGE_STAGING;
        d.BindFlags = 0;
        d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        d.MiscFlags = 0;
        gpu.device
            .CreateTexture2D(&d, None, Some(&mut staging))
            .map_err(|e| gpu.fail("CreateTexture2D (depth readback)", e))?;
    }
    let staging = staging.ok_or("no depth readback texture")?;
    let d24 = d.Format == DXGI_FORMAT_R24G8_TYPELESS;
    let [x, y, w, h] = crop.map(|v| v as usize);
    let mut out = Vec::with_capacity(w * h);
    // SAFETY: `staging` is CPU-readable; while mapped, `pData` holds the rows `RowPitch` apart, 4 bytes per texel,
    // and the crop lies inside the texture (render_replay checked it at load).
    unsafe {
        gpu.ctx.CopyResource(&staging, tex);
        gpu.ctx
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| gpu.fail("Map (depth)", e))?;
        if !mapped.pData.is_null() {
            for row in y..y + h {
                let p = (mapped.pData as *const u8).add(row * mapped.RowPitch as usize + x * 4);
                for b in std::slice::from_raw_parts(p, w * 4).as_chunks::<4>().0 {
                    let v = u32::from_le_bytes(*b);
                    out.push(if d24 {
                        (v & 0x00FF_FFFF) as f32 / 16_777_215.0
                    } else {
                        f32::from_bits(v)
                    });
                }
            }
        }
        gpu.ctx.Unmap(&staging, 0);
    }
    if out.len() != w * h {
        return Err(gpu.fail("Map (depth)", "null pointer returned"));
    }
    Ok(out)
}

impl Renderer {
    /// Draws `r` with its captured geometry, cbuffers, textures and samplers and reads the bound targets back over
    /// the crop. `deferred` chooses which targets are bound, never the permutation (the fixture's define list is
    /// compiled as is): false = RT0 only (the PS writes to unbound slots are discarded); true = the 8 CS targets in
    /// [`MRT_FORMATS`], each seeded from its before file and written through its captured write mask.
    pub fn render_replay(&mut self, r: &Replay, deferred: bool) -> Result<ReplayOut, String> {
        if deferred {
            if r.formats.len() != 8 {
                return Err(format!(
                    "deferred replay needs 8 RTs in mrt.json, the fixture has {}",
                    r.formats.len()
                ));
            }
            for (i, f) in r.formats.iter().enumerate() {
                if f != MRT_FORMATS[i].1 {
                    return Err(format!(
                        "mrt.json rt {i} is {f}, the deferred target is {}",
                        MRT_FORMATS[i].1
                    ));
                }
            }
            if r.write_masks.len() < 8 {
                return Err(format!(
                    "deferred replay needs 8 write masks in state.json stageB.blend, the fixture has {}",
                    r.write_masks.len()
                ));
            }
        }
        if r.blended {
            // shortcut: blended (hint-1 forward) subjects need the captured blend state on top of the seeded RT0; D6b if wanted
            return Err("blended subjects do not replay yet".into());
        }
        let mut warnings = r.warnings.clone();
        if r.defines.len() > 1 {
            warnings.push(format!(
                "{} define variants; replaying the first",
                r.defines.len()
            ));
        }
        let ps_list = r.defines.first().ok_or("no PS define list to compile")?;
        // D6c: a debug view compiles the injected source for both stages (as `wanted`) and SDC_DEBUG on the PS.
        // Never SDC_DEBUG_PRELINEAR: the replay reads raw f32, so the view's data value is what comes back.
        let lighting = Path::new("Lighting.hlsl");
        let debug = self.debug.define();
        let src = match debug {
            Some(_) => self.sources.get_debug(lighting)?,
            None => self.sources.get(lighting)?,
        };
        let vs_d = to_defines(&vs_defines(ps_list, &r.mesh));
        let mut ps_d = to_defines(std::iter::once(&"PSHADER".to_string()).chain(ps_list));
        if let Some(n) = debug {
            ps_d.0.push(("SDC_DEBUG".into(), n.to_string()));
        }
        let vs = compile(&src, &vs_d, "vs_5_0", &self.cache_dir).map_err(|e| format!("VS: {e}"))?;
        let ps = compile(&src, &ps_d, "ps_5_0", &self.cache_dir).map_err(|e| format!("PS: {e}"))?;
        let vs_inputs: BTreeSet<(String, u32)> = vs
            .reflection
            .inputs
            .iter()
            .filter(|i| !i.0.to_ascii_uppercase().starts_with("SV_"))
            .map(|i| (i.0.to_ascii_uppercase(), i.1))
            .collect();
        let ia: BTreeSet<(String, u32)> = r
            .mesh
            .inputs
            .iter()
            .map(|i| (i.0.to_ascii_uppercase(), i.1))
            .collect();
        if vs_inputs != ia {
            return Err(format!(
                "captured IA {ia:?} does not match the VS inputs {vs_inputs:?}"
            ));
        }
        let prog = create_program(&self.gpu, vs, ps)?;
        let gpu = &self.gpu;

        let names = r
            .mesh
            .inputs
            .iter()
            .map(|i| CString::new(i.0.as_str()).map_err(|_| format!("bad semantic {:?}", i.0)))
            .collect::<Result<Vec<_>, _>>()?;
        let elems: Vec<D3D11_INPUT_ELEMENT_DESC> = r
            .mesh
            .inputs
            .iter()
            .zip(&names)
            .map(|(i, n)| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR(n.as_ptr() as _),
                SemanticIndex: i.1,
                Format: DXGI_FORMAT(i.2 as i32),
                InputSlot: i.3,
                AlignedByteOffset: i.4,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            })
            .collect();
        let mut layout = None;
        // SAFETY: `elems` points into `names`, both alive for the call; the bytecode slice is valid.
        unsafe {
            gpu.device
                .CreateInputLayout(&elems, &prog.vs.bytecode, Some(&mut layout))
        }
        .map_err(|e| gpu.fail("CreateInputLayout", e))?;
        let layout = layout.ok_or("CreateInputLayout returned no layout")?;
        let vbs = r
            .mesh
            .vbs
            .iter()
            .map(|(slot, stride, b)| {
                Ok((*slot, *stride, buffer(gpu, b, D3D11_BIND_VERTEX_BUFFER)?))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let ib = buffer(gpu, &r.mesh.ib, D3D11_BIND_INDEX_BUFFER)?;

        // Every reflected cbuffer from its file, verbatim (zero-padded to the reflection).
        let mut cbs = Vec::new();
        for (stage, refl) in [("vs", &prog.vs.reflection), ("ps", &prog.ps.reflection)] {
            for l in &refl.cbuffers {
                let raw = r
                    .cbuffers
                    .get(&format!("{stage}_b{}", l.slot))
                    .ok_or_else(|| {
                        format!(
                            "cb_{stage}_b{}.bin missing ({} is reflected)",
                            l.slot, l.name
                        )
                    })?;
                let b = buffer(gpu, &cb_bytes(raw, l.size), D3D11_BIND_CONSTANT_BUFFER)?;
                cbs.push((stage == "vs", l.slot, b));
            }
        }
        // SRVs at their registers; a "null" register stays unbound like in game (reads 0), not render3d's defaults.
        let mut srvs = Vec::new();
        for (name, reg, kind) in &prog.ps.reflection.srvs {
            if r.null_srvs.contains(reg) {
                continue;
            }
            let Some(bytes) = r.textures.get(reg) else {
                warnings.push(format!("t{reg} ({name}): not in the fixture, left unbound"));
                continue;
            };
            match load_dds(gpu, bytes) {
                Ok(t) if t.kind == *kind => srvs.push((*reg, t.srv)),
                Ok(t) => warnings.push(format!(
                    "t{reg} ({name}): fixture texture is {:?}, the shader reads {kind:?}; left unbound",
                    t.kind
                )),
                Err(e) => warnings.push(format!("t{reg} ({name}): {e}; left unbound")),
            }
        }
        let mut samplers = Vec::new();
        for (name, slot) in &prog.ps.reflection.samplers {
            match r.samplers.iter().find(|s| s.0 == *slot) {
                Some((_, s)) => samplers.push((*slot, sampler_state(gpu, s)?)),
                None => {
                    warnings.push(format!(
                        "s{slot} ({name}): no captured sampler, using the default"
                    ));
                    samplers.push((*slot, self.sampler.clone()));
                }
            }
        }
        let mut bd = D3D11_BLEND_DESC::default();
        bd.RenderTarget[0].BlendEnable = false.into();
        bd.RenderTarget[0].RenderTargetWriteMask = r.write_masks.first().copied().unwrap_or(0xf);
        if deferred {
            // Per-RT masks (Coffin: RT0 and RT3 keep their seeded alpha).
            bd.IndependentBlendEnable = true.into();
            for (t, m) in bd.RenderTarget.iter_mut().zip(&r.write_masks) {
                t.BlendEnable = false.into();
                t.RenderTargetWriteMask = *m;
            }
        }
        let mut blend = None;
        // SAFETY: descriptor and out-pointer are locals.
        unsafe { gpu.device.CreateBlendState(&bd, Some(&mut blend)) }
            .map_err(|e| gpu.fail("CreateBlendState", e))?;

        // Full size: screen-space lookups (t14 shadow mask) use SV_Position.
        let target = if deferred {
            Target::new_deferred(gpu, r.size.0, r.size.1)?
        } else {
            Target::new(gpu, r.size.0, r.size.1)?
        };
        target.clear(gpu, [0.0; 4]);
        let bound = if deferred { 8 } else { 1 };
        for (i, b) in r.before.iter().take(bound).enumerate() {
            target.seed_f32(gpu, i, r.crop, b)?;
        }
        // D6c: the captured depth, seeded through a staging copy (D3D11 has no partial or CPU writes to depth-stencil).
        let seeded = r
            .depth
            .as_ref()
            .map(|d| seeded_depth(gpu, r.size, r.crop, d))
            .transpose()?;
        let dsv = seeded.as_ref().map_or(&target.dsv, |s| &s.0);
        let v = r.viewport;
        // SAFETY (whole block): every bound object belongs to `gpu.device`; slices outlive the calls.
        unsafe {
            let ctx = &gpu.ctx;
            // A frame's SRVs, cbuffers and samplers stay bound: a register the fixture leaves empty must read 0.
            ctx.ClearState();
            ctx.OMSetRenderTargets(Some(&target.rtvs()), dsv);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: v[0],
                TopLeftY: v[1],
                Width: v[2],
                Height: v[3],
                MinDepth: v[4],
                MaxDepth: v[5],
            }]));
            ctx.RSSetState(if r.cull_back {
                &self.raster_back
            } else {
                &self.raster
            });
            match &seeded {
                Some((_, state, _)) => ctx.OMSetDepthStencilState(state, 0),
                // LessEqual + write: the fixture's Equal/no-write is a post-prepass state, and there is no prepass.
                None => ctx.OMSetDepthStencilState(&self.depth_states[3], 0),
            }
            ctx.OMSetBlendState(blend.as_ref(), None, 0xffff_ffff);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetInputLayout(&layout);
            for (slot, stride, b) in &vbs {
                ctx.IASetVertexBuffers(*slot, 1, Some(&Some(b.clone())), Some(stride), Some(&0));
            }
            ctx.IASetIndexBuffer(&ib, DXGI_FORMAT_R16_UINT, 0);
            ctx.VSSetShader(&prog.vs_obj, None);
            ctx.PSSetShader(&prog.ps_obj, None);
            for (vs, slot, b) in &cbs {
                if *vs {
                    ctx.VSSetConstantBuffers(*slot, Some(&[Some(b.clone())]));
                } else {
                    ctx.PSSetConstantBuffers(*slot, Some(&[Some(b.clone())]));
                }
            }
            for (reg, v) in &srvs {
                ctx.PSSetShaderResources(*reg, Some(&[Some(v.clone())]));
            }
            for (slot, s) in &samplers {
                ctx.PSSetSamplers(*slot, Some(&[Some(s.clone())]));
            }
            // Vertex k of mesh.vb is index − minIndex.
            ctx.DrawIndexed(r.mesh.index_count, 0, -(r.mesh.min_index as i32));
        }
        let mrt = (0..bound)
            .map(|i| target.read_f32(gpu, i, r.crop))
            .collect::<Result<Vec<_>, _>>();
        let depth = self
            .replay_depth
            .then(|| read_depth(gpu, seeded.as_ref().map_or(&target.depth, |s| &s.2), r.crop))
            .transpose()?;
        let main = match (&r.composite, deferred, &mrt) {
            (Some(rc), true, Ok(_)) => Some(self.replay_composite(r, rc, &target, &mut warnings)),
            _ => None,
        };
        // SAFETY: plain context call; `render` rebinds everything per frame.
        unsafe { gpu.ctx.ClearState() };
        Ok(ReplayOut {
            mrt: mrt?,
            main: main.transpose()?,
            warnings,
            depth,
        })
    }

    /// The capture's composite on `target` after the draw: compiled with CSHADER, the inferred defines and the PS
    /// list's ENABLE_LL / CS_GAME_GAMMA (not inferable from reflection, and the result depends on them), the captured
    /// cbuffers, RT0's crop re-seeded from mainrw_before. Returns MainRW over the crop.
    fn replay_composite(
        &self,
        r: &Replay,
        rc: &ReplayComposite,
        target: &Target,
        warnings: &mut Vec<String>,
    ) -> Result<Vec<f32>, String> {
        let gpu = &self.gpu;
        let mut defines = vec!["CSHADER".to_string()];
        defines.extend(rc.defines_inferred.iter().cloned());
        defines.extend(
            r.defines
                .first()
                .into_iter()
                .flatten()
                .filter(|d| *d == "ENABLE_LL" || d.starts_with("CS_GAME_GAMMA"))
                .cloned(),
        );
        let mut c = Composite::new(gpu, &self.sources, &defines, &self.cache_dir)
            .map_err(|e| format!("composite: {e}"))?;
        let reflected: BTreeSet<(String, u32)> = c
            .reflection
            .srvs
            .iter()
            .map(|s| (s.0.clone(), s.1))
            .collect();
        let bound: BTreeSet<(String, u32)> = rc.srvs.iter().cloned().collect();
        if reflected != bound {
            return Err(format!(
                "composite reflects {reflected:?}, the capture bound {bound:?}"
            ));
        }
        let layouts = c.reflection.cbuffers.clone();
        for (cb, l) in c.cbs.iter_mut().zip(&layouts) {
            let raw = rc.cbuffers.get(&format!("cs_b{}", l.slot)).ok_or_else(|| {
                format!("cb_cs_b{}.bin missing ({} is reflected)", l.slot, l.name)
            })?;
            cb.buffer = buffer(gpu, &cb_bytes(raw, l.size), D3D11_BIND_CONSTANT_BUFFER)?;
        }
        target.seed_f32(gpu, 0, r.crop, &rc.before)?;
        let g = c.groups(target.width, target.height);
        if g != rc.thread_groups {
            warnings.push(format!(
                "composite: {g:?} thread groups, the capture dispatched {:?}",
                rc.thread_groups
            ));
        }
        c.dispatch(gpu, target)?;
        target.read_f32(gpu, 0, r.crop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(sems: &[&str]) -> ReplayMesh {
        ReplayMesh {
            inputs: sems.iter().map(|s| (s.to_string(), 0, 2, 0, 0)).collect(),
            vbs: vec![],
            ib: vec![],
            index_count: 0,
            min_index: 0,
        }
    }

    #[test]
    fn cb_bytes_pads_to_the_reflection_and_keeps_longer_buffers() {
        let b = cb_bytes(&[1; 20], 32);
        assert_eq!(b.len(), 32);
        assert!(b[..20].iter().all(|&x| x == 1) && b[20..].iter().all(|&x| x == 0));
        assert_eq!(cb_bytes(&[1; 720], 192).len(), 720);
        assert_eq!(cb_bytes(&[], 0).len(), 16);
        assert_eq!(cb_bytes(&[1; 20], 0).len(), 32);
    }

    #[test]
    fn vs_defines_follow_the_ia() {
        let ps: Vec<String> = [
            "VC",
            "SPECULAR",
            "FACEGEN",
            "PROJECTED_UV",
            "CS_GAME_GAMMA=1.6",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let has = |v: &[String], d: &str| v.iter().any(|x| x == d);
        let plain = vs_defines(&ps, &mesh(&["POSITION", "TEXCOORD", "NORMAL"]));
        for d in ["VSHADER", "PROJECTED_UV", "CS_GAME_GAMMA=1.6"] {
            assert!(has(&plain, d), "{d} in {plain:?}");
        }
        for d in ["VC", "SPECULAR", "FACEGEN", "SKINNED"] {
            assert!(!has(&plain, d), "{d} in {plain:?}");
        }
        let full = vs_defines(&ps, &mesh(&["POSITION", "COLOR", "BLENDWEIGHT"]));
        assert!(has(&full, "VC") && has(&full, "SKINNED"), "{full:?}");
    }

    #[test]
    fn nonfinite_tokens_become_null_outside_strings() {
        assert_eq!(
            nonfinite_to_null(r#"[NaN, -Infinity, Infinity, "NaN \"Infinity", 1.0]"#),
            r#"[null, null, null, "NaN \"Infinity", 1.0]"#
        );
    }
}
