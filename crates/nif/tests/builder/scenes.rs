//! Scene builders shared by tests. `all()` lists every scene builder so far.
#![allow(dead_code)]
use super::{Nif, W};
use nif::header::Header;

/// A NIF (BS 100) with a single block of `kind`.
pub fn one_block_nif(kind: &str, body: Vec<u8>) -> Vec<u8> {
    let mut n = Nif::new(100);
    n.block(kind, body);
    n.bytes()
}

/// BSLightingShaderProperty body: shader type, NiObjectNET (name -1), then a plain property tail
/// (flags, uv, texture set, emissive, ..., lighting effects) and `trailer` floats.
pub fn lsp_body(shader_type: u32, extra: &[i32], controller: i32, trailer: &[f32]) -> Vec<u8> {
    let mut w = W::default().u32(shader_type);
    w = w.raw(&nif::net::encode_net(-1, extra, controller));
    w.u32(0x8000_0000) // slsf1
        .u32(1) // slsf2
        .fs(&[0.0, 0.0, 1.0, 1.0]) // uv offset, scale
        .i32(-1) // texture set
        .fs(&[0.0, 0.0, 0.0, 1.0]) // emissive color, multiple
        .u32(3) // clamp
        .fs(&[1.0, 0.0, 80.0]) // alpha, refraction, glossiness
        .fs(&[1.0, 1.0, 1.0, 1.0]) // specular color, strength
        .fs(&[0.0, 0.0]) // lighting effects
        .fs(trailer)
        .0
}

/// Index of the first block of that type.
pub fn block_of(h: &Header, kind: &str) -> usize {
    (0..h.sizes.len())
        .find(|&i| h.kind(i) == kind.as_bytes())
        .unwrap_or_else(|| panic!("no {kind} block"))
}

/// Every scene builder that exists so far.
pub fn all() -> Vec<Vec<u8>> {
    vec![one_block_nif(
        "BSLightingShaderProperty",
        lsp_body(1, &[], -1, &[1.0]),
    )]
}
