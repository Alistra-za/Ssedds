//! One MATO record's DATA (material object: the snow/ash projected layer). Reuses the record helpers in `read.rs`;
//! xEdit `wbMATO`: DATA is 11 f32 (falloff scale/bias, noise UV scale, material UV scale, projection xyz, normal
//! dampener, single-pass colour rgb) then a u32 flags field.
use crate::read::{each_sub, inflate, sig_at, u32_at, HEADER};
use crate::FLAG_COMPRESSED;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatoData {
    pub falloff_scale: f32,
    pub falloff_bias: f32,
    pub noise_uv_scale: f32,
    pub material_uv_scale: f32,
    pub projection: [f32; 3],
    pub normal_dampener: f32,
    pub colour: [f32; 3],
    pub flags: u32,
}

/// Nested groups entered under the MATO group (valid files have none; the cap stops a hostile file).
const MAX_DEPTH: usize = 4;

/// The DATA of the MATO record whose EDID equals `edid` (ASCII case-insensitive); `None` when there is no such record,
/// its DATA is shorter than 48 bytes, or the plugin is malformed. The first match wins.
pub fn mato_data(plugin: &[u8], edid: &str) -> Option<MatoData> {
    if plugin.len() < HEADER || &plugin[0..4] != b"TES4" {
        return None;
    }
    let mut off = HEADER.checked_add(u32_at(plugin, 4) as usize)?;
    while off.checked_add(HEADER)? <= plugin.len() {
        let size = u32_at(plugin, off + 4) as usize;
        if &plugin[off..off + 4] != b"GRUP" || size < HEADER {
            return None;
        }
        let end = off.checked_add(size)?.min(plugin.len());
        if &sig_at(plugin, off + 8) == b"MATO" && u32_at(plugin, off + 12) == 0 {
            return find(plugin, off + HEADER, end, 0, edid);
        }
        off = end;
    }
    None
}

fn find(b: &[u8], start: usize, end: usize, depth: usize, edid: &str) -> Option<MatoData> {
    let mut p = start;
    while p + HEADER <= end {
        let size = u32_at(b, p + 4) as usize;
        if &b[p..p + 4] == b"GRUP" {
            let gend = p
                .checked_add(size)
                .filter(|&g| size >= HEADER && g <= end)?;
            if depth < MAX_DEPTH {
                if let Some(d) = find(b, p + HEADER, gend, depth + 1, edid) {
                    return Some(d);
                }
            }
            p = gend;
            continue;
        }
        let rend = p
            .checked_add(HEADER)?
            .checked_add(size)
            .filter(|&r| r <= end)?;
        if &b[p..p + 4] == b"MATO" {
            if let Some(d) = record(&b[p + HEADER..rend], u32_at(b, p + 8), edid) {
                return Some(d);
            }
        }
        p = rend;
    }
    None
}

fn record(raw: &[u8], flags: u32, edid: &str) -> Option<MatoData> {
    let inflated;
    let data = if flags & FLAG_COMPRESSED != 0 {
        inflated = inflate(raw).ok()?;
        &inflated[..]
    } else {
        raw
    };
    let (mut name, mut dat) = (None, None);
    // A truncated tail is ignored: the subrecords before it were delivered.
    let _ = each_sub(data, |sig, d| match &sig {
        b"EDID" if name.is_none() => name = Some(d),
        b"DATA" if dat.is_none() => dat = Some(d),
        _ => {}
    });
    let name = name?;
    let name = name.strip_suffix(&[0]).unwrap_or(name);
    if !name.eq_ignore_ascii_case(edid.as_bytes()) {
        return None;
    }
    let d = dat.filter(|d| d.len() >= 48)?;
    let f = |o: usize| f32::from_bits(u32_at(d, o));
    Some(MatoData {
        falloff_scale: f(0),
        falloff_bias: f(4),
        noise_uv_scale: f(8),
        material_uv_scale: f(12),
        projection: [f(16), f(20), f(24)],
        normal_dampener: f(28),
        colour: [f(32), f(36), f(40)],
        flags: u32_at(d, 44),
    })
}
