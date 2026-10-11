//! D3DCompile of the include-expanded CS source, reflection into plain data, and an on-disk cache keyed by
//! (root hash, file, defines, target, flags).
use crate::includes::expand;
use crate::permutation::Defines;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DReflect, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D_CT_CBUFFER, D3D_SHADER_MACRO, D3D_SIT_CBUFFER, D3D_SIT_SAMPLER,
    D3D_SIT_STRUCTURED, D3D_SIT_TEXTURE, D3D_SRV_DIMENSION_BUFFER,
    D3D_SRV_DIMENSION_TEXTURE2DARRAY, D3D_SRV_DIMENSION_TEXTURECUBE, D3D_SVC_STRUCT,
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
    /// (name, offset, size) in bytes. Struct members are listed after their struct as `struct.member` (nested:
    /// `a.b.c`), with the absolute offset and the bytes up to the next member, so `CbWriter` can set them.
    pub vars: Vec<(String, u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Reflection {
    pub cbuffers: Vec<CbLayout>,
    pub srvs: Vec<(String, u32, SrvKind)>,
    pub samplers: Vec<(String, u32)>,
    /// (semantic, index, component mask).
    pub inputs: Vec<(String, u32, u32)>,
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

const FLAGS: u32 = D3DCOMPILE_OPTIMIZATION_LEVEL3;
/// Bumped when the cached `Reflection` gains information (2: struct members flattened), so old entries miss.
const REFLECT_VERSION: u32 = 2;

fn key(file: &Path, root: &Path, d: &Defines, target: &str, root_hash: &str) -> String {
    let mut sorted = d.0.clone();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(root_hash);
    h.update([0]);
    // Root-relative path, so `a/x.hlsl` and `b/x.hlsl` never share an entry.
    let rel = file.strip_prefix(root).unwrap_or(file);
    h.update(rel.to_string_lossy().replace('\\', "/"));
    h.update([0]);
    for (k, v) in &sorted {
        h.update(format!("{k}={v};"));
    }
    h.update([0]);
    h.update(target);
    h.update(FLAGS.to_le_bytes());
    h.update(REFLECT_VERSION.to_le_bytes());
    hex::encode(h.finalize())
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

pub fn compile(
    file: &Path,
    root: &Path,
    d: &Defines,
    target: &str,
    cache_dir: &Path,
    root_hash: &str,
) -> Result<Compiled, String> {
    let k = key(file, root, d, target, root_hash);
    let (cso, js) = (
        cache_dir.join(format!("{k}.cso")),
        cache_dir.join(format!("{k}.json")),
    );
    if let Some(hit) = read_cache(&cso, &js) {
        return Ok(hit);
    }
    let src =
        expand(file, &[root.to_path_buf()]).map_err(|e| format!("{}: {e}", file.display()))?;
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
    let src_name = cstr(&file.display().to_string().replace('\\', "/"))?;
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
    out: &mut Vec<(String, u32, u32)>,
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
                members.push((name, md.Offset, mt));
            }
        }
        for (i, (name, off, mt)) in members.iter().enumerate() {
            let end = members.get(i + 1).map_or(size, |m| m.1);
            let full = format!("{prefix}.{name}");
            let msize = end.saturating_sub(*off);
            out.push((full.clone(), base + off, msize));
            struct_members(mt, &full, base + off, msize, out);
        }
    }
}

/// Reflects compiled DXBC into plain data (cbuffer layouts, SRV/sampler slots, input signature).
pub fn reflect(bytecode: &[u8]) -> Result<Reflection, String> {
    let e = |e: windows::core::Error| e.to_string();
    let mut raw = std::ptr::null_mut();
    // SAFETY: bytecode is a valid slice; D3DReflect writes an AddRef'd ID3D11ShaderReflection into `raw`.
    let r: ID3D11ShaderReflection = unsafe {
        D3DReflect(
            bytecode.as_ptr() as _,
            bytecode.len(),
            &ID3D11ShaderReflection::IID,
            &mut raw,
        )
        .map_err(e)?;
        ID3D11ShaderReflection::from_raw(raw)
    };
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
                vars.push((name.clone(), vd.StartOffset, vd.Size));
                if let Some(t) = v.GetType() {
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
    }
    out.cbuffers.sort_by_key(|c| c.slot);
    out.srvs.sort_by_key(|s| s.1);
    out.samplers.sort_by_key(|s| s.1);
    Ok(out)
}
