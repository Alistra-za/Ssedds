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

pub fn net(name: i32) -> W {
    W::default().i32(name).u32(0).i32(-1)
}
pub fn avobj(w: W) -> W {
    w.u32(14)
        .fs(&[0.0; 3])
        .fs(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        .f(1.0)
        .i32(-1)
}
pub fn texset(paths: &[&str]) -> Vec<u8> {
    let mut w = W::default().u32(paths.len() as u32);
    for p in paths {
        w = w.sized(p);
    }
    w.0
}
pub fn lsp(st: u32, ts: i32) -> Vec<u8> {
    W::default()
        .u32(st)
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u32(0x8000_0000)
        .u32(0x21)
        .fs(&[0.0, 0.0])
        .fs(&[1.0, 1.0])
        .i32(ts)
        .fs(&[0.0; 3])
        .f(1.0)
        .u32(3)
        .f(1.0)
        .f(0.0)
        .f(80.0)
        .fs(&[1.0; 3])
        .f(1.0)
        .f(0.3)
        .f(2.0)
        .0
}

pub fn bstri(shader: i32, alpha: i32) -> Vec<u8> {
    bstri_named(-1, shader, alpha)
}

pub fn bstri_named(name: i32, shader: i32, alpha: i32) -> Vec<u8> {
    avobj(net(name))
        .fs(&[0.0; 4])
        .i32(-1)
        .i32(shader)
        .i32(alpha)
        .u64(0)
        .u16(0)
        .u16(0)
        .u32(0)
        .0
}

/// NiTimeController base (next, flags, freq, phase, start, stop, target) plus `tail`.
pub fn ctrl_body(next: i32, flags: u16, target: i32, tail: W) -> Vec<u8> {
    W::default()
        .i32(next)
        .u16(flags)
        .fs(&[1.0, 0.0, 0.0, 1.0])
        .i32(target)
        .raw(&tail.0)
        .0
}

/// NiFloatData with two keys of the given KeyType (1 LINEAR, 2 QUADRATIC); raw, not decoded in Phase A.
fn float_data(key_type: u32) -> Vec<u8> {
    let mut w = W::default().u32(2).u32(key_type);
    for (t, v) in [(0.0, 0.0), (1.0, 1.0)] {
        w = w.fs(&[t, v]);
        if key_type == 2 {
            w = w.fs(&[0.0, 0.0]);
        }
    }
    w.0
}

/// BSLSP (block 0) -> Float ctrl var 11 (1) -> next Float ctrl var 20 (2); each has a NiFloatInterpolator
/// (3, 4) over NiFloatData LINEAR (5) / QUADRATIC (6).
pub fn free_chain() -> Vec<u8> {
    let mut n = Nif::new(100);
    n.block("BSLightingShaderProperty", lsp_body(1, &[], 1, &[1.0]));
    for (next, var, interp) in [(2, 11, 3), (-1, 20, 4)] {
        n.block(
            "BSLightingShaderPropertyFloatController",
            ctrl_body(next, 0x48, 0, W::default().i32(interp).u32(var)),
        );
    }
    n.block("NiFloatInterpolator", W::default().f(0.5).i32(5).0);
    n.block("NiFloatInterpolator", W::default().f(0.25).i32(6).0);
    n.block("NiFloatData", float_data(1));
    n.block("NiFloatData", float_data(2));
    n.bytes()
}

/// NiAlphaProperty (0) with a BSNiAlphaPropertyTestRefController (1) -> NiFloatInterpolator (2) -> NiFloatData (3).
pub fn alpha_testref() -> Vec<u8> {
    let mut n = Nif::new(100);
    n.block(
        "NiAlphaProperty",
        W::default()
            .raw(&nif::net::encode_net(-1, &[], 1))
            .u16(0x00ED)
            .u8(128)
            .0,
    );
    n.block(
        "BSNiAlphaPropertyTestRefController",
        ctrl_body(-1, 0x48, 0, W::default().i32(2)),
    );
    n.block("NiFloatInterpolator", W::default().f(1.0).i32(3).0);
    n.block("NiFloatData", float_data(1));
    n.bytes()
}

/// Root NiNode (0, controller -> manager) ; NiControllerManager (1, next -> MTTC) with sequences "A" (2) and "B" (3)
/// and palette (4); BSTriShape (5) -> BSLSP (6) -> Float ctrl var 11, flags 0x68 (Manager Controlled, 7) with a
/// NiBlendFloatInterpolator (8); NiMultiTargetTransformController (9); keyed NiFloatInterpolators (10, 11).
/// A has CBs with Controller ID "11" and null; B has one with the empty string.
pub fn managed_clip() -> Vec<u8> {
    let mut n = Nif::new(100);
    let s = |n: &mut Nif, x: &str| n.string(x);
    let (root, a, bb, shape) = (
        s(&mut n, "Root"),
        s(&mut n, "A"),
        s(&mut n, "B"),
        s(&mut n, "shape"),
    );
    let prop = s(&mut n, "BSLightingShaderProperty");
    let ct = s(&mut n, "BSLightingShaderPropertyFloatController");
    let (id11, empty) = (s(&mut n, "11"), s(&mut n, ""));
    n.block(
        "NiNode",
        avobj(W::default().i32(root).u32(0).i32(1)).u32(0).u32(0).0,
    );
    n.block(
        "NiControllerManager",
        ctrl_body(9, 0x48, 0, W::default().u8(1).u32(2).i32(2).i32(3).i32(4)),
    );
    let seq = |name: i32, cbs: &[(i32, i32, i32)]| {
        let mut w = W::default().i32(name).u32(cbs.len() as u32).u32(0);
        for &(interp, cid, iid) in cbs {
            w = w
                .i32(interp)
                .i32(7)
                .u8(0)
                .i32(shape)
                .i32(prop)
                .i32(ct)
                .i32(cid)
                .i32(iid);
        }
        w.f(1.0) // weight
            .i32(-1) // text keys
            .u32(0) // cycle
            .fs(&[1.0, 0.0, 1.0]) // freq, start, stop
            .i32(1) // manager
            .i32(root) // accum root name
            .u16(0)
            .0
    };
    n.block(
        "NiControllerSequence",
        seq(a, &[(10, id11, -1), (10, -1, -1)]),
    );
    n.block("NiControllerSequence", seq(bb, &[(11, empty, -1)]));
    n.block(
        "NiDefaultAVObjectPalette",
        W::default().i32(0).u32(1).sized("shape").i32(5).0,
    );
    n.block("BSTriShape", bstri(6, -1));
    n.block("BSLightingShaderProperty", lsp_body(1, &[], 7, &[1.0]));
    n.block(
        "BSLightingShaderPropertyFloatController",
        ctrl_body(-1, 0x68, 6, W::default().i32(8).u32(11)),
    );
    // flags (manager controlled), array size, threshold, value
    n.block(
        "NiBlendFloatInterpolator",
        W::default().u8(1).u8(0).f(0.0).f(0.0).0,
    );
    n.block(
        "NiMultiTargetTransformController",
        ctrl_body(-1, 0x48, 0, W::default().u16(0)),
    );
    n.block("NiFloatInterpolator", W::default().f(0.0).i32(-1).0);
    n.block("NiFloatInterpolator", W::default().f(0.0).i32(-1).0);
    n.bytes()
}

/// NiControllerSequence body with one controlled block and every string null.
pub fn sequence_body() -> Vec<u8> {
    W::default()
        .i32(-1)
        .u32(1)
        .u32(0)
        .i32(-1)
        .i32(-1)
        .u8(0)
        .i32(-1)
        .i32(-1)
        .i32(-1)
        .i32(-1)
        .i32(-1)
        .f(1.0)
        .i32(-1)
        .u32(0)
        .fs(&[1.0, 0.0, 1.0])
        .i32(-1)
        .i32(-1)
        .u16(0)
        .0
}

/// Every scene builder that exists so far.
pub fn all() -> Vec<Vec<u8>> {
    vec![
        one_block_nif("BSLightingShaderProperty", lsp_body(1, &[], -1, &[1.0])),
        free_chain(),
        alpha_testref(),
        managed_clip(),
        all_extras(),
        shared_alpha_and_set(),
    ]
}

/// One of each editable extra (BSX 0x83 "BSX", BODYTRI "a\r\nb", int 7, bool 1 "PG_IGNORE", float -0.0, BSInvMarker
/// 4712/6283/0/1.0) plus a NiStringsExtraData (read-only row).
pub fn all_extras() -> Vec<u8> {
    let mut n = Nif::new(100);
    let (bsx, bodytri, val, nm, pg, f, inv, ss) = (
        n.string("BSX"),
        n.string("BODYTRI"),
        n.string("a\r\nb"),
        n.string("Count"),
        n.string("PG_IGNORE"),
        n.string("Scale"),
        n.string("INV"),
        n.string("Strs"),
    );
    n.block("BSXFlags", W::default().i32(bsx).u32(0x83).0);
    n.block("NiStringExtraData", W::default().i32(bodytri).i32(val).0);
    n.block("NiIntegerExtraData", W::default().i32(nm).u32(7).0);
    n.block("NiBooleanExtraData", W::default().i32(pg).u8(1).0);
    n.block("NiFloatExtraData", W::default().i32(f).f(-0.0).0);
    n.block(
        "BSInvMarker",
        W::default().i32(inv).u16(4712).u16(6283).u16(0).f(1.0).0,
    );
    n.block(
        "NiStringsExtraData",
        W::default().i32(ss).u32(1).sized("x").0,
    );
    n.bytes()
}

/// Root NiNode (0) with BSTriShapes "A" (1) and "B" (2) whose BSLSPs (5, 6) share one BSShaderTextureSet (3) and
/// one NiAlphaProperty (4).
pub fn shared_alpha_and_set() -> Vec<u8> {
    let mut n = Nif::new(100);
    let (r, a, b) = (n.string("Root"), n.string("A"), n.string("B"));
    n.block(
        "NiNode",
        avobj(W::default().i32(r).u32(0).i32(-1))
            .u32(2)
            .i32(1)
            .i32(2)
            .u32(0)
            .0,
    );
    n.block("BSTriShape", bstri_named(a, 5, 4));
    n.block("BSTriShape", bstri_named(b, 6, 4));
    n.block("BSShaderTextureSet", texset(&["textures\\a.dds"]));
    n.block(
        "NiAlphaProperty",
        W::default()
            .raw(&nif::net::encode_net(-1, &[], -1))
            .u16(0x00ED)
            .u8(128)
            .0,
    );
    n.block("BSLightingShaderProperty", lsp(1, 3));
    n.block("BSLightingShaderProperty", lsp(1, 3));
    n.bytes()
}
