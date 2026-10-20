//! NiExtraData blocks the editor can change (spec §5.2); every other extra-data type is a read-only row.
use crate::{cursor::Cursor, header::Header, text, NifError};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ExtraValue {
    Bsx(u32),
    Str(String),
    Int(u32),
    Bool(u8),
    /// f32 bits.
    Float(u32),
    /// `zoom` holds f32 bits.
    Inv {
        rot: [u16; 3],
        zoom: u32,
    },
}

pub const EDITABLE: [&str; 6] = [
    "BSXFlags",
    "NiStringExtraData",
    "NiIntegerExtraData",
    "NiBooleanExtraData",
    "NiFloatExtraData",
    "BSInvMarker",
];

pub const BSX_NAMES: [&str; 14] = [
    "Animated",
    "Havok",
    "Ragdoll",
    "Complex",
    "Addon",
    "EditorMarker",
    "Dynamic",
    "Articulated",
    "NeedsTransformUpdates",
    "ExternalEmit",
    "MagicShaderParticles",
    "Lights",
    "Breakable",
    "SearchedBreakable",
];

pub const INV_DEFAULT: ([u16; 3], f32) = ([4712, 6283, 0], 1.0);

#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    pub block: u32,
    pub kind: String,
    pub name: Option<Vec<u8>>,
    /// `None` for every type outside [`EDITABLE`].
    pub value: Option<ExtraValue>,
    pub size: u32,
}

pub fn read_extra(h: &Header, b: &[u8], block: usize) -> Result<Extra, NifError> {
    let geo = |msg: &str| NifError::Geometry {
        block: block as u32,
        msg: msg.into(),
    };
    if block >= h.sizes.len() {
        return Err(geo("no such block"));
    }
    let kind = text::lossy(h.kind(block));
    let start = h.offsets()[block];
    let size = h.sizes[block];
    let end = start + size as usize;
    if end > b.len() {
        return Err(geo("block runs past the file"));
    }
    let mut c = Cursor::new(&b[..end], start);
    let (name, value) = if EDITABLE.contains(&kind.as_str()) {
        let name = c.i32()?;
        let v = match kind.as_str() {
            "BSXFlags" => ExtraValue::Bsx(c.u32()?),
            "NiStringExtraData" => {
                let i = c.i32()?;
                // -1 is the null string; any other unresolvable index is a damaged file, not an empty string.
                let s = match (i, h.string(i)) {
                    (-1, _) => &[][..],
                    (_, Some(s)) => s,
                    _ => return Err(geo("string index outside the string table")),
                };
                ExtraValue::Str(text::lossy(s))
            }
            "NiIntegerExtraData" => ExtraValue::Int(c.u32()?),
            "NiBooleanExtraData" => ExtraValue::Bool(c.u8()?),
            "NiFloatExtraData" => ExtraValue::Float(c.f32()?.to_bits()),
            _ => ExtraValue::Inv {
                rot: [c.u16()?, c.u16()?, c.u16()?],
                zoom: c.f32()?.to_bits(),
            },
        };
        if c.pos() != end {
            return Err(NifError::BlockSize {
                block: block as u32,
                kind,
                declared: size,
                consumed: (c.pos() - start) as u32,
            });
        }
        (name, Some(v))
    } else {
        // Other types are shown as kind + size only; the name read is best effort.
        (c.i32().unwrap_or(-1), None)
    };
    Ok(Extra {
        block: block as u32,
        kind,
        name: h.string(name).map(<[u8]>::to_vec),
        value,
        size,
    })
}

/// Block type name and body. `str_index` is the string-table index, used only for `Str`.
pub fn encode_extra(name: i32, v: &ExtraValue, str_index: i32) -> (&'static str, Vec<u8>) {
    let mut o = name.to_le_bytes().to_vec();
    let kind = match v {
        ExtraValue::Bsx(x) => {
            o.extend(x.to_le_bytes());
            EDITABLE[0]
        }
        ExtraValue::Str(_) => {
            o.extend(str_index.to_le_bytes());
            EDITABLE[1]
        }
        ExtraValue::Int(x) => {
            o.extend(x.to_le_bytes());
            EDITABLE[2]
        }
        ExtraValue::Bool(x) => {
            o.push(*x);
            EDITABLE[3]
        }
        ExtraValue::Float(x) => {
            o.extend(x.to_le_bytes());
            EDITABLE[4]
        }
        ExtraValue::Inv { rot, zoom } => {
            for r in rot {
                o.extend(r.to_le_bytes());
            }
            o.extend(zoom.to_le_bytes());
            EDITABLE[5]
        }
    };
    (kind, o)
}
