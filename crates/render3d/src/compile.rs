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
    D3D_SRV_DIMENSION_TEXTURE2DARRAY, D3D_SRV_DIMENSION_TEXTURECUBE,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ShaderReflection, D3D11_SHADER_BUFFER_DESC, D3D11_SHADER_DESC,
    D3D11_SHADER_INPUT_BIND_DESC, D3D11_SHADER_VARIABLE_DESC, D3D11_SIGNATURE_PARAMETER_DESC,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CbLayout {
    pub name: String,
    pub slot: u32,
    pub size: u32,
    /// (name, offset, size) in bytes.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

fn key(file: &Path, d: &Defines, target: &str, root_hash: &str) -> String {
    let mut sorted = d.0.clone();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(root_hash);
    h.update([0]);
    h.update(
        file.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    h.update([0]);
    for (k, v) in &sorted {
        h.update(format!("{k}={v};"));
    }
    h.update([0]);
    h.update(target);
    h.update(FLAGS.to_le_bytes());
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
    let k = key(file, d, target, root_hash);
    let (cso, js) = (
        cache_dir.join(format!("{k}.cso")),
        cache_dir.join(format!("{k}.json")),
    );
    if let (Ok(b), Ok(j)) = (std::fs::read(&cso), std::fs::read_to_string(&js)) {
        if let Ok(r) = serde_json::from_str(&j) {
            return Ok(Compiled {
                bytecode: b,
                reflection: r,
            });
        }
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
    if std::fs::create_dir_all(cache_dir).is_ok() {
        if let Ok(j) = serde_json::to_string(&reflection) {
            let _ = std::fs::write(&cso, &bytecode);
            let _ = std::fs::write(&js, j);
        }
    }
    Ok(Compiled {
        bytecode,
        reflection,
    })
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
                vars.push((pcstr(vd.Name), vd.StartOffset, vd.Size));
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
