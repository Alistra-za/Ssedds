//! `mato_data`: one MATO record's DATA from a plugin built in code (`tests/common`).
mod common;
use common::*;
use esp::{mato_data, MatoData, FLAG_COMPRESSED};
use std::io::Write;

fn data52() -> Vec<u8> {
    let f = [
        0.35f32, 0.4, 48.0, 170.667, 0.0, 0.0, -1.0, 0.4, 1.0, 1.0, 1.0,
    ];
    let mut v: Vec<u8> = f.iter().flat_map(|x| x.to_le_bytes()).collect();
    v.extend_from_slice(&1u32.to_le_bytes()); // flags
    v.extend_from_slice(&0u32.to_le_bytes()); // trailing field of the 52-byte layout
    v
}

fn compressed(body: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(body).unwrap();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(e.finish().unwrap());
    out
}

fn fixture() -> Vec<u8> {
    let a = rec(
        b"MATO",
        0x0100_0801,
        &[edid("SnowMaterialObject1P"), sub(b"DATA", &data52())],
    );
    let mut d48 = data52();
    d48.truncate(48);
    d48[44..48].copy_from_slice(&5u32.to_le_bytes());
    let b_body = [edid("AshMaterialSolstheim1P"), sub(b"DATA", &d48)].concat();
    let b = rec_raw(b"MATO", 0x0100_0802, FLAG_COMPRESSED, &compressed(&b_body));
    // A short-DATA record and a non-MATO group the walker must step over.
    let c = rec(
        b"MATO",
        0x0100_0803,
        &[edid("ShortOne"), sub(b"DATA", &data52()[..40])],
    );
    let stat = top(b"STAT", &[rec(b"STAT", 0x0100_0900, &[edid("Rock")])]);
    plugin(0, &[], &[stat, top(b"MATO", &[a, b, c])])
}

#[test]
fn reads_52_byte_data() {
    assert_eq!(
        mato_data(&fixture(), "snowmaterialobject1p"),
        Some(MatoData {
            falloff_scale: 0.35,
            falloff_bias: 0.4,
            noise_uv_scale: 48.0,
            material_uv_scale: 170.667,
            projection: [0.0, 0.0, -1.0],
            normal_dampener: 0.4,
            colour: [1.0, 1.0, 1.0],
            flags: 1,
        })
    );
}

#[test]
fn reads_compressed_48_byte_data() {
    let d = mato_data(&fixture(), "AshMaterialSolstheim1P").unwrap();
    assert_eq!(d.flags, 5);
    assert_eq!(d.noise_uv_scale, 48.0);
}

#[test]
fn missing_or_short_is_none() {
    let p = fixture();
    assert_eq!(mato_data(&p, "NoSuchMato"), None);
    assert_eq!(mato_data(&p, "ShortOne"), None);
    assert_eq!(mato_data(b"not a plugin", "SnowMaterialObject1P"), None);
}

/// Owner data: set `SDC_ACHERUS` and run with `--ignored`.
#[test]
#[ignore]
fn owner_faultier_values() {
    let Ok(root) = std::env::var("SDC_ACHERUS") else {
        return;
    };
    let path = std::path::Path::new(&root)
        .join("mods")
        .join("Faultier's PBR Landscapes 4k")
        .join("Faultier's PBR landscapes.esp");
    let bytes = std::fs::read(path).unwrap();
    let snow = mato_data(&bytes, "SnowMaterialObject1P").unwrap();
    assert_eq!(
        (snow.falloff_scale, snow.falloff_bias, snow.noise_uv_scale),
        (0.35, 0.4, 48.0)
    );
    let ash = mato_data(&bytes, "AshMaterialSolstheim1P").unwrap();
    // 20.0 is the reciprocal of the captured 0.05.
    assert_eq!(
        (ash.falloff_scale, ash.falloff_bias, ash.noise_uv_scale),
        (0.4, 0.5, 20.0)
    );
}
