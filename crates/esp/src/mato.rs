//! One MATO record's DATA (material object: the snow/ash projected layer). Walks with the shared record/group
//! traversal in `read.rs`; xEdit `wbMATO`: DATA is 11 f32 (falloff scale/bias, noise UV scale, material UV scale,
//! projection xyz, normal dampener, single-pass colour rgb) then a u32 flags field.
use crate::read::{each_sub, group_records, record_data, sig_at, top_groups, u32_at, HEADER};

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

/// The DATA of the MATO record whose EDID equals `edid` (ASCII case-insensitive); the first one with at least 48
/// bytes of DATA wins. `None` when there is no such record or the plugin is malformed anywhere the walk goes: a bad
/// group or record size, a bad compressed record, or a subrecord that runs past its record (even after EDID and DATA).
pub fn mato_data(plugin: &[u8], edid: &str) -> Option<MatoData> {
    if plugin.len() < HEADER || &plugin[0..4] != b"TES4" {
        return None;
    }
    let from = HEADER + u32_at(plugin, 4) as usize;
    if from > plugin.len() {
        return None;
    }
    let (mut found, mut bad) = (None, false);
    top_groups(plugin, from, |g| match g {
        Err(_) => bad = true,
        Ok(g) if g.top && &g.label == b"MATO" => {
            group_records(plugin, g.start, g.end, 0, true, &mut |r| match r {
                Err(_) => bad = true,
                Ok((p, rend)) if &sig_at(plugin, p) == b"MATO" => {
                    match record_data(plugin, p, rend) {
                        Ok(data) => match record(&data, edid) {
                            Some(d) => found = found.or(d),
                            None => bad = true,
                        },
                        Err(_) => bad = true,
                    }
                }
                Ok(_) => {}
            })
        }
        Ok(_) => {}
    });
    if bad {
        None
    } else {
        found
    }
}

/// `None`: the subrecords are malformed. `Some(None)`: a well-formed record that is not the one wanted, or has no
/// usable DATA.
fn record(data: &[u8], edid: &str) -> Option<Option<MatoData>> {
    let (mut name, mut dat) = (None, None);
    each_sub(data, |sig, d| match &sig {
        b"EDID" if name.is_none() => name = Some(d),
        b"DATA" if dat.is_none() => dat = Some(d),
        _ => {}
    })
    .ok()?;
    let name = name.map(|n| n.strip_suffix(&[0]).unwrap_or(n));
    let wanted = name.is_some_and(|n| n.eq_ignore_ascii_case(edid.as_bytes()));
    let Some(d) = dat.filter(|d| wanted && d.len() >= 48) else {
        return Some(None);
    };
    let f = |o: usize| f32::from_bits(u32_at(d, o));
    Some(Some(MatoData {
        falloff_scale: f(0),
        falloff_bias: f(4),
        noise_uv_scale: f(8),
        material_uv_scale: f(12),
        projection: [f(16), f(20), f(24)],
        normal_dampener: f(28),
        colour: [f(32), f(36), f(40)],
        flags: u32_at(d, 44),
    }))
}
