//! Key data blocks (NiFloatData, NiPosData, NiBoolData): KeyGroup decode and byte-exact encode (nif.xml 1991-2012).
use crate::ctrl::{body, exact, geo};
use crate::cursor::Cursor;
use crate::{header::Header, NifError};

/// Named KeyType values. `KeyGroup::key_type` stays a raw u32 so any stored value round-trips (F9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum KeyType {
    Linear = 1,
    Quadratic = 2,
    Tbc = 3,
    Xyz = 4,
    Const = 5,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key<T> {
    pub time: f32,
    pub value: T,
    /// Some only for QUADRATIC (2).
    pub forward: Option<T>,
    pub backward: Option<T>,
    /// [t, b, c], Some only for TBC (3).
    pub tbc: Option<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyGroup<T> {
    /// None exactly when there are 0 keys (no Interpolation field in the file).
    pub key_type: Option<u32>,
    pub keys: Vec<Key<T>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyData {
    Float(KeyGroup<f32>),
    Pos(KeyGroup<[f32; 3]>),
    Bool(KeyGroup<u8>),
}

/// A key value type: its size, and exact (bit-preserving) read and write.
trait Val: Copy + Default {
    const SIZE: usize;
    fn read(c: &mut Cursor) -> Result<Self, NifError>;
    fn write(&self, out: &mut Vec<u8>);
}

impl Val for f32 {
    const SIZE: usize = 4;
    fn read(c: &mut Cursor) -> Result<Self, NifError> {
        c.f32()
    }
    fn write(&self, out: &mut Vec<u8>) {
        out.extend(self.to_le_bytes());
    }
}

impl Val for [f32; 3] {
    const SIZE: usize = 12;
    fn read(c: &mut Cursor) -> Result<Self, NifError> {
        c.f32s()
    }
    fn write(&self, out: &mut Vec<u8>) {
        self.iter().for_each(|v| v.write(out));
    }
}

impl Val for u8 {
    const SIZE: usize = 1;
    fn read(c: &mut Cursor) -> Result<Self, NifError> {
        c.u8()
    }
    fn write(&self, out: &mut Vec<u8>) {
        out.push(*self);
    }
}

const QUAD: u32 = KeyType::Quadratic as u32;
const TBC: u32 = KeyType::Tbc as u32;

fn group<T: Val>(h: &Header, block: usize, d: &[u8]) -> Result<KeyGroup<T>, NifError> {
    let past = || geo(block, "key count runs past the block");
    let mut c = Cursor::new(d, 0);
    let n = c.u32().map_err(|_| past())? as usize;
    if n == 0 {
        exact(h, block, 4, d.len())?;
        return Ok(KeyGroup {
            key_type: None,
            keys: vec![],
        });
    }
    let t = c.u32().map_err(|_| past())?;
    let size = 4
        + T::SIZE
        + match t {
            QUAD => 2 * T::SIZE,
            TBC => 12,
            _ => 0,
        };
    if n > (d.len() - 8) / size {
        return Err(past());
    }
    let mut keys = Vec::with_capacity(n);
    for _ in 0..n {
        let time = c.f32()?;
        let value = T::read(&mut c)?;
        let (forward, backward) = match t {
            QUAD => (Some(T::read(&mut c)?), Some(T::read(&mut c)?)),
            _ => (None, None),
        };
        let tbc = if t == TBC { Some(c.f32s()?) } else { None };
        keys.push(Key {
            time,
            value,
            forward,
            backward,
            tbc,
        });
    }
    exact(h, block, c.pos(), d.len())?;
    Ok(KeyGroup {
        key_type: Some(t),
        keys,
    })
}

/// NiFloatData, NiPosData or NiBoolData; any other class is an Err (F8). Consumes exactly the block size.
pub fn read_data(h: &Header, b: &[u8], block: usize) -> Result<KeyData, NifError> {
    let d = body(h, b, block)?;
    match h.kind(block) {
        b"NiFloatData" => group(h, block, d).map(KeyData::Float),
        b"NiPosData" => group(h, block, d).map(KeyData::Pos),
        b"NiBoolData" => group(h, block, d).map(KeyData::Bool),
        _ => Err(geo(block, "not a key data block")),
    }
}

fn encode_group<T: Val>(g: &KeyGroup<T>, out: &mut Vec<u8>) {
    out.extend((g.keys.len() as u32).to_le_bytes());
    if g.keys.is_empty() {
        return;
    }
    let t = g.key_type.unwrap_or(KeyType::Linear as u32);
    out.extend(t.to_le_bytes());
    for k in &g.keys {
        k.time.write(out);
        k.value.write(out);
        if t == QUAD {
            k.forward.unwrap_or_default().write(out);
            k.backward.unwrap_or_default().write(out);
        }
        if t == TBC {
            k.tbc.unwrap_or_default().write(out);
        }
    }
}

/// The block body. Count = keys.len(); Interpolation is written only when count > 0, as
/// `key_type.unwrap_or(KeyType::Linear as u32)`; per key the fields the type calls for. A missing `forward`,
/// `backward` or `tbc` on a type that needs it is written as zeros (callers never build that).
pub fn encode_data(d: &KeyData) -> Vec<u8> {
    let mut out = vec![];
    match d {
        KeyData::Float(g) => encode_group(g, &mut out),
        KeyData::Pos(g) => encode_group(g, &mut out),
        KeyData::Bool(g) => encode_group(g, &mut out),
    }
    out
}
