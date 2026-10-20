//! D3DCompile of the include-expanded CS source, reflection into plain data, and an on-disk cache keyed by
//! bytes that were compiled (expanded source hash, entry, target, defines, flags, reflection version, compiler
//! version): see [`program_key`].
use crate::permutation::Defines;
use crate::source::ExpandedSource;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;
use std::sync::OnceLock;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DReflect, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D_CT_CBUFFER, D3D_SHADER_MACRO, D3D_SIT_CBUFFER, D3D_SIT_SAMPLER,
    D3D_SIT_STRUCTURED, D3D_SIT_TEXTURE, D3D_SIT_UAV_RWTYPED, D3D_SRV_DIMENSION_BUFFER,
    D3D_SRV_DIMENSION_TEXTURE2DARRAY, D3D_SRV_DIMENSION_TEXTURECUBE, D3D_SVC_STRUCT, D3D_SVF_USED,
    D3D_SVT_BOOL, D3D_SVT_FLOAT, D3D_SVT_INT, D3D_SVT_UINT,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ShaderReflection, ID3D11ShaderReflectionType, D3D11_SHADER_BUFFER_DESC,
    D3D11_SHADER_DESC, D3D11_SHADER_INPUT_BIND_DESC, D3D11_SHADER_TYPE_DESC,
    D3D11_SHADER_VARIABLE_DESC, D3D11_SIGNATURE_PARAMETER_DESC,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CbLayout {
    pub name: String,
    pub slot: u32,
    pub size: u32,
    /// Every variable: name, absolute offset and size in bytes, scalar type. Struct members are listed after
    /// their struct as `struct.member` (nested: `a.b.c`), sized up to the next member, so `CbWriter` can set them.
    pub vars: Vec<CbVar>,
}

/// The HLSL scalar type of a cbuffer variable (`D3D_SVT_*` of its type; vectors, matrices and arrays report their
/// element's). Structs and anything else are `Other`. Decides how `cs_settings::write_feature_data` encodes a
/// number (C6): the JSON shape (`1` vs `1.0`) is not trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VarKind {
    Float,
    Uint,
    Int,
    Bool,
    Other,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CbVar {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub kind: VarKind,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Reflection {
    pub cbuffers: Vec<CbLayout>,
    pub srvs: Vec<(String, u32, SrvKind)>,
    pub samplers: Vec<(String, u32)>,
    /// (semantic, index, component mask).
    pub inputs: Vec<(String, u32, u32)>,
    /// Typed UAVs (RWTexture*): (name, register). Compute shaders only (v2 D4).
    // No REFLECT_VERSION bump for `uavs` and `threads`: no cs_5_0 entry was ever cached, and VS/PS have neither
    // (an older entry reads them as empty / [0; 3], which is right).
    #[serde(default)]
    pub uavs: Vec<(String, u32)>,
    /// numthreads; [0; 3] for VS/PS.
    #[serde(default)]
    pub threads: [u32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SrvKind {
    Tex2D,
    Tex2DArray,
    Cube,
    Buffer,
    Structured,
}

#[derive(Debug, Clone)]
pub struct Compiled {
    pub bytecode: Vec<u8>,
    pub reflection: Reflection,
}

/// The vertex and pixel stage of one permutation: plain data, `Send`, what a worker returns.
#[derive(Debug, Clone)]
pub struct CompiledPair {
    pub vs: Compiled,
    pub ps: Compiled,
}

const FLAGS: u32 = D3DCOMPILE_OPTIMIZATION_LEVEL3;
/// Bumped when the cached `Reflection` gains information (2: struct members flattened; 3: variable types), so
/// old entries miss.
const REFLECT_VERSION: u32 = 3;

/// Names one compiled stage by what was compiled. The root path and the saved fingerprint are not part of it:
/// two CS folders with identical sources share entries, and a stale fingerprint cannot file a new compile
/// under an old name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProgramKey(pub String);

impl ProgramKey {
    /// The key of a VS + PS pair (the unit the renderer caches in memory).
    pub fn pair(vs: &ProgramKey, ps: &ProgramKey) -> ProgramKey {
        let mut h = Sha256::new();
        h.update(vs.0.as_bytes());
        h.update([0]);
        h.update(ps.0.as_bytes());
        ProgramKey(hex::encode(h.finalize()))
    }
}

/// `sha256(expanded.sha256 || entry || target || sorted defines || FLAGS || REFLECT_VERSION || compiler_id)`.
pub fn program_key(
    src: &ExpandedSource,
    entry: &str,
    target: &str,
    defines: &[(String, String)],
) -> ProgramKey {
    let mut sorted: Vec<&(String, String)> = defines.iter().collect();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(src.sha256);
    h.update([0]);
    h.update(entry);
    h.update([0]);
    h.update(target);
    h.update([0]);
    for (k, v) in sorted {
        h.update(format!("{k}={v};"));
    }
    h.update([0]);
    h.update(FLAGS.to_le_bytes());
    h.update(REFLECT_VERSION.to_le_bytes());
    h.update(compiler_id());
    ProgramKey(hex::encode(h.finalize()))
}

/// The file version (`10.1.x.y`) of the d3dcompiler_47.dll this process loaded: a Windows update replaces the
/// DLL and its output can change, so it is part of the key. `"unknown"` when the query fails.
pub fn compiler_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| dll_version().unwrap_or_else(|| "unknown".to_string()))
}

fn dll_version() -> Option<String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    use windows::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleW, LoadLibraryW,
    };
    // SAFETY (whole block): `path` is zero-initialised and longer than the returned length, so it is NUL
    // terminated; `buf` is as long as the size the API asked for; `VerQueryValueW` points into `buf`, which
    // outlives the unaligned read of the fixed-size struct.
    unsafe {
        let module = GetModuleHandleW(w!("d3dcompiler_47.dll"))
            .or_else(|_| LoadLibraryW(w!("d3dcompiler_47.dll")))
            .ok()?;
        let mut path = [0u16; 520];
        let n = GetModuleFileNameW(Some(module), &mut path) as usize;
        if n == 0 || n >= path.len() {
            return None;
        }
        let name = PCWSTR(path.as_ptr());
        let size = GetFileVersionInfoSizeW(name, None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(name, None, size, buf.as_mut_ptr().cast()).ok()?;
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let ok = VerQueryValueW(buf.as_ptr().cast(), w!("\\"), &mut p, &mut len);
        if !ok.as_bool() || p.is_null() || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let info = p.cast::<VS_FIXEDFILEINFO>().read_unaligned();
        let (ms, ls) = (info.dwFileVersionMS, info.dwFileVersionLS);
        Some(format!(
            "{}.{}.{}.{}",
            ms >> 16,
            ms & 0xffff,
            ls >> 16,
            ls & 0xffff
        ))
    }
}

fn blob_bytes(b: &ID3DBlob) -> Vec<u8> {
    // SAFETY: the blob owns GetBufferSize() bytes at GetBufferPointer() for its lifetime.
    unsafe {
        std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec()
    }
}

fn cstr(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| format!("NUL byte in {s:?}"))
}

fn pcstr(p: PCSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: reflection names are NUL-terminated and live as long as the reflector.
    unsafe { p.to_string() }.unwrap_or_default()
}

/// The cache entry paths of `k`: (`.cso`, `.json`).
fn entry_paths(cache_dir: &Path, k: &ProgramKey) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        cache_dir.join(format!("{}.cso", k.0)),
        cache_dir.join(format!("{}.json", k.0)),
    )
}

/// A disk-cache hit for this stage, if there is one (a missing, truncated or torn pair is a miss). A hit
/// refreshes the entry's age (`cache::touch`).
pub fn cached(
    src: &ExpandedSource,
    d: &Defines,
    target: &str,
    cache_dir: &Path,
) -> Option<Compiled> {
    let k = program_key(src, "main", target, &d.0);
    let (cso, js) = entry_paths(cache_dir, &k);
    let hit = read_cache(&cso, &js)?;
    crate::cache::touch(&js);
    Some(hit)
}

/// Compiles `src` (entry `main`) for `target` with `d`, or returns the disk-cache entry.
pub fn compile(
    src: &ExpandedSource,
    d: &Defines,
    target: &str,
    cache_dir: &Path,
) -> Result<Compiled, String> {
    if let Some(hit) = cached(src, d, target, cache_dir) {
        return Ok(hit);
    }
    let k = program_key(src, "main", target, &d.0);
    let (cso, js) = entry_paths(cache_dir, &k);
    let names =
        d.0.iter()
            .map(|(k, v)| Ok((cstr(k)?, cstr(v)?)))
            .collect::<Result<Vec<_>, String>>()?;
    let mut macros: Vec<D3D_SHADER_MACRO> = names
        .iter()
        .map(|(k, v)| D3D_SHADER_MACRO {
            Name: PCSTR(k.as_ptr() as _),
            Definition: PCSTR(v.as_ptr() as _),
        })
        .collect();
    macros.push(D3D_SHADER_MACRO::default());
    let src_name = cstr(
        &src.files
            .first()
            .map_or_else(|| "shader.hlsl".to_string(), |p| p.display().to_string())
            .replace('\\', "/"),
    )?;
    let target_c = cstr(target)?;
    let (mut code, mut errs): (Option<ID3DBlob>, Option<ID3DBlob>) = (None, None);
    // SAFETY: every pointer (source, macros, names) outlives the call; the macro array is NULL-terminated.
    let r = unsafe {
        D3DCompile(
            src.text.as_ptr() as _,
            src.text.len(),
            PCSTR(src_name.as_ptr() as _),
            Some(macros.as_ptr()),
            None,
            PCSTR(c"main".as_ptr() as _),
            PCSTR(target_c.as_ptr() as _),
            FLAGS,
            0,
            &mut code,
            Some(&mut errs),
        )
    };
    if let Err(e) = r {
        return Err(errs
            .map(|b| {
                String::from_utf8_lossy(&blob_bytes(&b))
                    .trim_end_matches('\0')
                    .to_string()
            })
            .unwrap_or_else(|| e.to_string()));
    }
    let bytecode = blob_bytes(&code.ok_or("D3DCompile returned no bytecode")?);
    let reflection = reflect(&bytecode)?;
    write_cache(cache_dir, &cso, &js, &bytecode, &reflection);
    Ok(Compiled {
        bytecode,
        reflection,
    })
}

/// The `.json` half of a cache entry: the reflection plus what the `.cso` must be.
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheMeta {
    len: usize,
    sha256: String,
    reflection: Reflection,
}

/// DXBC container: "DXBC" magic, total size (u32 LE) at bytes 24..28.
fn valid_dxbc(b: &[u8]) -> bool {
    b.len() >= 32
        && &b[..4] == b"DXBC"
        && u32::from_le_bytes([b[24], b[25], b[26], b[27]]) as usize == b.len()
}

/// Any mismatch (missing, truncated, torn or foreign files) is a miss; the caller recompiles.
fn read_cache(cso: &Path, js: &Path) -> Option<Compiled> {
    let meta: CacheMeta = serde_json::from_str(&std::fs::read_to_string(js).ok()?).ok()?;
    let b = std::fs::read(cso).ok()?;
    if b.len() != meta.len || !valid_dxbc(&b) || hex::encode(Sha256::digest(&b)) != meta.sha256 {
        return None;
    }
    Some(Compiled {
        bytecode: b,
        reflection: meta.reflection,
    })
}

/// Best-effort: unique temp file + rename for each half, `.json` last, so a reader never sees a new `.json`
/// next to a partial `.cso`. Failures only cost a recompile next time.
fn write_cache(dir: &Path, cso: &Path, js: &Path, bytecode: &[u8], reflection: &Reflection) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let meta = CacheMeta {
        len: bytecode.len(),
        sha256: hex::encode(Sha256::digest(bytecode)),
        reflection: reflection.clone(),
    };
    let Ok(j) = serde_json::to_vec(&meta) else {
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let put = |dst: &Path, data: &[u8]| -> bool {
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join(format!(".tmp-{}-{n}", std::process::id()));
        if std::fs::write(&tmp, data).is_ok() && std::fs::rename(&tmp, dst).is_ok() {
            return true;
        }
        let _ = std::fs::remove_file(&tmp);
        false
    };
    if put(cso, bytecode) {
        put(js, &j);
    }
}

fn srv_kind(b: &D3D11_SHADER_INPUT_BIND_DESC) -> Option<SrvKind> {
    if b.Type == D3D_SIT_STRUCTURED {
        return Some(SrvKind::Structured);
    }
    if b.Type != D3D_SIT_TEXTURE {
        return None;
    }
    Some(match b.Dimension {
        D3D_SRV_DIMENSION_TEXTURECUBE => SrvKind::Cube,
        D3D_SRV_DIMENSION_TEXTURE2DARRAY => SrvKind::Tex2DArray,
        D3D_SRV_DIMENSION_BUFFER => SrvKind::Buffer,
        _ => SrvKind::Tex2D,
    })
}

/// Appends `prefix.member` entries for a struct-typed variable (recursively). A member's size is the distance
/// to the next member (the last one runs to the end of the parent), which is all `CbWriter` needs.
///
/// # Safety
/// `t` must be a live reflection type of the reflector being walked.
unsafe fn struct_members(
    t: &ID3D11ShaderReflectionType,
    prefix: &str,
    base: u32,
    size: u32,
    out: &mut Vec<CbVar>,
) {
    let mut td = D3D11_SHADER_TYPE_DESC::default();
    // SAFETY: (caller) live type; `td` is a local out-param; indices are bounded by `Members`.
    unsafe {
        if t.GetDesc(&mut td).is_err() || td.Class != D3D_SVC_STRUCT || td.Elements > 0 {
            return;
        }
        let mut members = vec![];
        for i in 0..td.Members {
            let (Some(mt), name) = (t.GetMemberTypeByIndex(i), pcstr(t.GetMemberTypeName(i)))
            else {
                continue;
            };
            let mut md = D3D11_SHADER_TYPE_DESC::default();
            if mt.GetDesc(&mut md).is_ok() {
                members.push((name, md.Offset, var_kind(&md), mt));
            }
        }
        for (i, (name, off, kind, mt)) in members.iter().enumerate() {
            let end = members.get(i + 1).map_or(size, |m| m.1);
            let full = format!("{prefix}.{name}");
            let msize = end.saturating_sub(*off);
            out.push(CbVar {
                name: full.clone(),
                offset: base + off,
                size: msize,
                kind: *kind,
            });
            struct_members(mt, &full, base + off, msize, out);
        }
    }
}

/// The scalar type of a reflected type (see [`VarKind`]).
fn var_kind(t: &D3D11_SHADER_TYPE_DESC) -> VarKind {
    match t.Type {
        D3D_SVT_FLOAT => VarKind::Float,
        D3D_SVT_UINT => VarKind::Uint,
        D3D_SVT_INT => VarKind::Int,
        D3D_SVT_BOOL => VarKind::Bool,
        _ => VarKind::Other,
    }
}

fn reflector(bytecode: &[u8]) -> Result<ID3D11ShaderReflection, String> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: bytecode is a valid slice; D3DReflect writes an AddRef'd ID3D11ShaderReflection into `raw`.
    unsafe {
        D3DReflect(
            bytecode.as_ptr() as _,
            bytecode.len(),
            &ID3D11ShaderReflection::IID,
            &mut raw,
        )
        .map_err(|e| e.to_string())?;
        Ok(ID3D11ShaderReflection::from_raw(raw))
    }
}

/// (cbuffer, variable) for every top-level cbuffer variable the shader reads (D3D_SVF_USED), in reflection order.
/// On demand (v2 D6c): not part of [`Reflection`], so cached entries stay valid.
pub fn used_vars(bytecode: &[u8]) -> Result<Vec<(String, String)>, String> {
    let e = |e: windows::core::Error| e.to_string();
    let r = reflector(bytecode)?;
    let mut out = vec![];
    // SAFETY (whole block): out-params are valid locals; indices are bounded by the shader desc counts.
    unsafe {
        let mut sd = D3D11_SHADER_DESC::default();
        r.GetDesc(&mut sd).map_err(e)?;
        for i in 0..sd.ConstantBuffers {
            let Some(cb) = r.GetConstantBufferByIndex(i) else {
                continue;
            };
            let mut bd = D3D11_SHADER_BUFFER_DESC::default();
            cb.GetDesc(&mut bd).map_err(e)?;
            if bd.Type != D3D_CT_CBUFFER {
                continue;
            }
            for j in 0..bd.Variables {
                let Some(v) = cb.GetVariableByIndex(j) else {
                    continue;
                };
                let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
                v.GetDesc(&mut vd).map_err(e)?;
                if vd.uFlags & D3D_SVF_USED.0 as u32 != 0 {
                    out.push((pcstr(bd.Name), pcstr(vd.Name)));
                }
            }
        }
    }
    Ok(out)
}

/// Reflects compiled DXBC into plain data (cbuffer layouts, SRV/sampler slots, input signature).
pub fn reflect(bytecode: &[u8]) -> Result<Reflection, String> {
    let e = |e: windows::core::Error| e.to_string();
    let r = reflector(bytecode)?;
    let mut out = Reflection::default();
    // SAFETY (whole block): out-params are valid locals; indices are bounded by the shader desc counts.
    unsafe {
        let mut sd = D3D11_SHADER_DESC::default();
        r.GetDesc(&mut sd).map_err(e)?;
        let mut cb_slots = HashMap::new();
        for i in 0..sd.BoundResources {
            let mut b = D3D11_SHADER_INPUT_BIND_DESC::default();
            r.GetResourceBindingDesc(i, &mut b).map_err(e)?;
            let name = pcstr(b.Name);
            if b.Type == D3D_SIT_CBUFFER {
                cb_slots.insert(name, b.BindPoint);
            } else if b.Type == D3D_SIT_SAMPLER {
                out.samplers.push((name, b.BindPoint));
            } else if b.Type == D3D_SIT_UAV_RWTYPED {
                out.uavs.push((name, b.BindPoint));
            } else if let Some(kind) = srv_kind(&b) {
                out.srvs.push((name, b.BindPoint, kind));
            }
        }
        for i in 0..sd.ConstantBuffers {
            let Some(cb) = r.GetConstantBufferByIndex(i) else {
                continue;
            };
            let mut bd = D3D11_SHADER_BUFFER_DESC::default();
            cb.GetDesc(&mut bd).map_err(e)?;
            if bd.Type != D3D_CT_CBUFFER {
                continue; // structured-buffer layouts (RESOURCE_BIND_INFO) are not cbuffers
            }
            let name = pcstr(bd.Name);
            let Some(&slot) = cb_slots.get(&name) else {
                continue; // declared but not bound in this permutation
            };
            let mut vars = vec![];
            for j in 0..bd.Variables {
                let Some(v) = cb.GetVariableByIndex(j) else {
                    continue;
                };
                let mut vd = D3D11_SHADER_VARIABLE_DESC::default();
                v.GetDesc(&mut vd).map_err(e)?;
                let name = pcstr(vd.Name);
                let t = v.GetType();
                let mut td = D3D11_SHADER_TYPE_DESC::default();
                let mut kind = VarKind::Other;
                if let Some(t) = &t {
                    if t.GetDesc(&mut td).is_ok() {
                        kind = var_kind(&td);
                    }
                }
                vars.push(CbVar {
                    name: name.clone(),
                    offset: vd.StartOffset,
                    size: vd.Size,
                    kind,
                });
                if let Some(t) = t {
                    struct_members(&t, &name, vd.StartOffset, vd.Size, &mut vars);
                }
            }
            out.cbuffers.push(CbLayout {
                name,
                slot,
                size: bd.Size,
                vars,
            });
        }
        for i in 0..sd.InputParameters {
            let mut p = D3D11_SIGNATURE_PARAMETER_DESC::default();
            r.GetInputParameterDesc(i, &mut p).map_err(e)?;
            out.inputs
                .push((pcstr(p.SemanticName), p.SemanticIndex, p.Mask as u32));
        }
        let [x, y, z] = &mut out.threads;
        r.GetThreadGroupSize(Some(x), Some(y), Some(z));
    }
    out.uavs.sort_by_key(|s| s.1);
    out.cbuffers.sort_by_key(|c| c.slot);
    out.srvs.sort_by_key(|s| s.1);
    out.samplers.sort_by_key(|s| s.1);
    Ok(out)
}
