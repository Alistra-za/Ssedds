//! Which groups are read, and what each record type contributes (spec §3).
use crate::read::{each_sub, u32_at};
use crate::{AltTex, ModelRef, Payload, Rec, Txst};
use nif::path::{canon_model, canon_texture};

/// Top-level groups read (spec §3); every other group is skipped by its size.
const WANTED: [[u8; 4]; 29] = [
    *b"TXST", *b"STAT", *b"MSTT", *b"ACTI", *b"FURN", *b"DOOR", *b"CONT", *b"LIGH", *b"MISC",
    *b"TREE", *b"FLOR", *b"GRAS", *b"ARMA", *b"ARMO", *b"WEAP", *b"AMMO", *b"BOOK", *b"INGR",
    *b"ALCH", *b"KEYM", *b"SLGM", *b"SCRL", *b"LTEX", *b"HDPT", *b"NPC_", *b"RACE", *b"EFSH",
    *b"WTHR", *b"IPCT",
];

pub(crate) fn wanted(label: &[u8; 4]) -> bool {
    WANTED.contains(label)
}

/// The bytes up to the first NUL.
pub(crate) fn until_nul(d: &[u8]) -> &[u8] {
    &d[..d.iter().position(|&c| c == 0).unwrap_or(d.len())]
}

/// A zstring, Windows-1252 (EDID and paths are never localized); `None` when empty.
pub(crate) fn zstring(d: &[u8]) -> Option<String> {
    let s = until_nul(d);
    (!s.is_empty()).then(|| nif::text::lossy(s))
}

/// Model subrecord, its alternate-texture subrecord, and the slot both use (xEdit `wbGenericModel`, `wbMODS`,
/// `wbMO2S`..`wbMO5S`, `wbDMDSs`; DMDL → slot 6, ruling R5).
const MODELS: [([u8; 4], [u8; 4], u8); 6] = [
    (*b"MODL", *b"MODS", 0),
    (*b"MOD2", *b"MO2S", 2),
    (*b"MOD3", *b"MO3S", 3),
    (*b"MOD4", *b"MO4S", 4),
    (*b"MOD5", *b"MO5S", 5),
    (*b"DMDL", *b"DMDS", 6),
];

/// Fills `rec` from its subrecords; subrecords not listed in spec §3 are skipped without allocating.
pub(crate) fn read(rec: &mut Rec, data: &[u8]) -> Result<(), usize> {
    let kind = rec.sig;
    let edid = &mut rec.edid;
    let p = &mut rec.payload;
    each_sub(data, |sig, d| {
        if &sig == b"EDID" {
            *edid = zstring(d);
            return;
        }
        match &kind {
            b"TXST" => txst(p, &sig, d),
            b"LTEX" | b"HDPT" if &sig == b"TNAM" => form_ref(p, sig, d),
            b"NPC_" if &sig == b"FTST" => form_ref(p, sig, d),
            b"ARMA" if matches!(&sig, b"NAM0" | b"NAM1") => form_ref(p, sig, d),
            b"RACE" => race(p, sig, d),
            // Skyrim ARMO MODL is an ARMA FormID (one per armature), not a path; world models are MOD2/MOD4 (ruling F-2).
            b"ARMO" if &sig == b"MODL" => form_ref(p, sig, d),
            b"HDPT" if &sig == b"DATA" => p.hdpt_flags = d.first().copied(),
            b"HDPT" if &sig == b"HNAM" => form_ref(p, sig, d),
            b"LTEX" if &sig == b"GNAM" => form_ref(p, sig, d),
            b"IPCT" if matches!(&sig, b"DNAM" | b"ENAM") => form_ref(p, sig, d),
            b"EFSH" if matches!(&sig, b"ICON" | b"ICO2" | b"NAM7" | b"NAM8" | b"NAM9") => {
                texture(p, sig, d)
            }
            b"WTHR" if is_cloud_layer(&sig) => texture(p, sig, d),
            _ => {}
        }
        if &kind != b"RACE" && !(&kind == b"ARMO" && &sig == b"MODL") {
            model(p, &sig, d);
        }
    })
}

fn tex_path(d: &[u8]) -> Option<String> {
    let t = canon_texture(until_nul(d));
    (!t.norm.is_empty()).then_some(t.norm)
}

fn model_path(d: &[u8]) -> Option<String> {
    let m = canon_model(until_nul(d));
    (!m.is_empty()).then_some(m)
}

/// TXST (xEdit `wbRecord(TXST…)`): TX00..TX07, DODT presence, DNAM itU16 flags.
fn txst(p: &mut Payload, sig: &[u8; 4], d: &[u8]) {
    let t = p.txst.get_or_insert_with(Txst::default);
    match sig {
        [b'T', b'X', b'0', n @ b'0'..=b'7'] => t.slots[usize::from(*n - b'0')] = tex_path(d),
        b"DODT" => t.decal = true,
        b"DNAM" if d.len() >= 2 => t.flags = u16::from_le_bytes([d[0], d[1]]),
        _ => {}
    }
}

/// A u32 FormID subrecord (a TXST; an HDPT for HNAM, a GRAS for GNAM, an ARMA for ARMO MODL); 0 (none) is dropped.
fn form_ref(p: &mut Payload, sig: [u8; 4], d: &[u8]) {
    if d.len() >= 4 {
        let id = u32_at(d, 0);
        if id != 0 {
            p.txst_refs.push((sig, id));
        }
    }
}

fn texture(p: &mut Payload, sig: [u8; 4], d: &[u8]) {
    if let Some(t) = tex_path(d) {
        p.textures.push((sig, t));
    }
}

/// xEdit `wbWeatherCloudTextures`: `_30_0TX`..`_40_0TX`, `A0TX`..`O0TX` = first byte 0x30..=0x4F, then "0TX".
fn is_cloud_layer(sig: &[u8; 4]) -> bool {
    &sig[1..] == b"0TX" && (0x30..=0x4F).contains(&sig[0])
}

fn model(p: &mut Payload, sig: &[u8; 4], d: &[u8]) {
    for (m, s, slot) in MODELS {
        if sig == &m {
            if let Some(path) = model_path(d) {
                p.models.push(ModelRef {
                    slot,
                    path,
                    alt: vec![],
                });
            }
            return;
        }
        if sig == &s {
            let alt = alt_textures(d);
            if let Some(r) = p.models.iter_mut().rev().find(|r| r.slot == slot) {
                r.alt.extend(alt);
            }
            return;
        }
    }
}

/// xEdit `wbAlternateTexture` in a `wbArrayS(…, -1)`: u32 count, then per entry a `wbLenString` 3D name (u32
/// length, no terminator), the TXST FormID (u32) and the 3D index (s32). The prefix widths are not verified against
/// `wbInterface.pas` (plan Task 2); the builder and these tests pin them. A short entry ends the list; TXST 0 is no swap.
fn alt_textures(d: &[u8]) -> Vec<AltTex> {
    let mut out = vec![];
    if d.len() < 4 {
        return out;
    }
    let n = u32_at(d, 0) as usize;
    let mut p = 4;
    for _ in 0..n {
        if p + 4 > d.len() {
            break;
        }
        let name_end = p + 4 + u32_at(d, p) as usize;
        if name_end + 8 > d.len() {
            break;
        }
        let txst = u32_at(d, name_end);
        if txst != 0 {
            out.push(AltTex {
                shape_name: nif::text::lossy(until_nul(&d[p + 4..name_end])),
                txst_local_formid: txst,
                index: u32_at(d, name_end + 4) as i32,
            });
        }
        p = name_end + 8;
    }
    out
}

/// RACE (ruling R4; xEdit `wbRecord(RACE…)`): DFTM/DFTF/FTSM/FTSF → TXST; ANAM (skeletal models) and MODL (body and
/// head data) → texture when `.dds`, else model (slot 0); TINT (tint mask file) → texture.
fn race(p: &mut Payload, sig: [u8; 4], d: &[u8]) {
    match &sig {
        b"DFTM" | b"DFTF" | b"FTSM" | b"FTSF" => form_ref(p, sig, d),
        b"TINT" => texture(p, sig, d),
        b"ANAM" | b"MODL" => {
            if until_nul(d)
                .trim_ascii_end()
                .to_ascii_lowercase()
                .ends_with(b".dds")
            {
                texture(p, sig, d);
            } else if let Some(path) = model_path(d) {
                p.models.push(ModelRef {
                    slot: 0,
                    path,
                    alt: vec![],
                });
            }
        }
        _ => {}
    }
}
