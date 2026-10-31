//! `mato_data`: one MATO record's DATA from a plugin built in code (`tests/common`).
mod common;
use common::*;
use esp::{mato_data, MatoData, FLAG_COMPRESSED};
use std::io::Write;

/// Snow example from the brief: 52 bytes (11 f32, flags, one trailing u32).
fn data52() -> Vec<u8> {
    let f = [
        0.35f32, 0.4, 48.0, 170.667, 0.0, 0.0, -1.0, 0.4, 1.0, 1.0, 1.0,
    ];
    let mut v: Vec<u8> = f.iter().flat_map(|x| x.to_le_bytes()).collect();
    v.extend_from_slice(&1u32.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes());
    v
}

/// 48 bytes, every float different and a flags word with several bits set, so a wrong offset shows.
const DISTINCT: [f32; 11] = [
    0.11, 0.22, 0.33, 0.44, 0.55, 0.66, 0.77, 0.88, 0.91, 0.92, 0.93,
];
const DISTINCT_FLAGS: u32 = 0x0A5C_3F01;

fn data48() -> Vec<u8> {
    let mut v: Vec<u8> = DISTINCT.iter().flat_map(|x| x.to_le_bytes()).collect();
    v.extend_from_slice(&DISTINCT_FLAGS.to_le_bytes());
    v
}

fn distinct() -> MatoData {
    let d = DISTINCT;
    MatoData {
        falloff_scale: d[0],
        falloff_bias: d[1],
        noise_uv_scale: d[2],
        material_uv_scale: d[3],
        projection: [d[4], d[5], d[6]],
        normal_dampener: d[7],
        colour: [d[8], d[9], d[10]],
        flags: DISTINCT_FLAGS,
    }
}

fn compressed(body: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(body).unwrap();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(e.finish().unwrap());
    out
}

fn snow() -> Vec<u8> {
    rec(
        b"MATO",
        0x0100_0801,
        &[edid("SnowMaterialObject1P"), sub(b"DATA", &data52())],
    )
}

fn ash() -> Vec<u8> {
    let body = [edid("AshMaterialSolstheim1P"), sub(b"DATA", &data48())].concat();
    rec_raw(b"MATO", 0x0100_0802, FLAG_COMPRESSED, &compressed(&body))
}

/// A plugin with one MATO top group holding `records`.
fn mato_plugin(records: &[Vec<u8>]) -> Vec<u8> {
    plugin(0, &[], &[top(b"MATO", records)])
}

/// `n` nested (non-top) groups around `inner`.
fn nest(n: usize, inner: Vec<u8>) -> Vec<u8> {
    (0..n).fold(inner, |g, _| grup(b"MATO", 1, &[g]))
}

fn put_u32(v: &mut [u8], at: usize, x: u32) {
    v[at..at + 4].copy_from_slice(&x.to_le_bytes());
}

/// Where the MATO top group starts in `mato_plugin` output.
fn group_at() -> usize {
    plugin(0, &[], &[]).len()
}

fn fixture() -> Vec<u8> {
    // A short-DATA record and a non-MATO group the walker must step over.
    let c = rec(
        b"MATO",
        0x0100_0803,
        &[edid("ShortOne"), sub(b"DATA", &data52()[..40])],
    );
    let stat = top(b"STAT", &[rec(b"STAT", 0x0100_0900, &[edid("Rock")])]);
    plugin(0, &[], &[stat, top(b"MATO", &[snow(), ash(), c])])
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
    assert_eq!(
        mato_data(&fixture(), "AshMaterialSolstheim1P"),
        Some(distinct())
    );
}

#[test]
fn missing_or_short_is_none() {
    let p = fixture();
    assert_eq!(mato_data(&p, "NoSuchMato"), None);
    assert_eq!(mato_data(&p, "ShortOne"), None);
    assert_eq!(mato_data(b"not a plugin", "SnowMaterialObject1P"), None);
}

#[test]
fn finds_a_record_in_nested_groups() {
    // Traversal must resume after a nested group, and enter it.
    let other = rec(b"MATO", 1, &[edid("Other"), sub(b"DATA", &data48())]);
    for n in [1, 8] {
        let p = mato_plugin(&[nest(n, snow())]);
        assert!(mato_data(&p, "SnowMaterialObject1P").is_some(), "depth {n}");
    }
    let p = mato_plugin(&[nest(2, other), snow()]);
    assert!(mato_data(&p, "SnowMaterialObject1P").is_some());
}

#[test]
fn nesting_deeper_than_the_walker_allows_is_none() {
    let p = mato_plugin(&[nest(9, snow())]);
    assert_eq!(mato_data(&p, "SnowMaterialObject1P"), None);
}

#[test]
fn truncated_file_is_none() {
    let full = fixture();
    assert!(mato_data(&full, "SnowMaterialObject1P").is_some());
    // Every cut lands in the TES4 header, the STAT group or the MATO group.
    for n in 0..full.len() {
        assert_eq!(
            mato_data(&full[..n], "SnowMaterialObject1P"),
            None,
            "cut {n}"
        );
    }
}

#[test]
fn truncated_record_header_or_body_is_none() {
    // A good record first: the lookup must not return it once the group is damaged.
    let p = mato_plugin(&[snow(), vec![0u8; 10]]);
    assert_eq!(mato_data(&p, "SnowMaterialObject1P"), None);

    let mut long = snow();
    let size = u32::from_le_bytes(long[4..8].try_into().unwrap());
    put_u32(&mut long, 4, size + 10); // body claims 10 bytes the group does not have
    let p = mato_plugin(&[long]);
    assert_eq!(mato_data(&p, "SnowMaterialObject1P"), None);
}

#[test]
fn invalid_group_size_is_none() {
    let at = group_at();
    let good = mato_plugin(&[snow()]);
    // Smaller than its own header.
    let mut p = good.clone();
    put_u32(&mut p, at + 4, 10);
    assert_eq!(mato_data(&p, "SnowMaterialObject1P"), None);
    // Past the end of the file: the record inside is intact, but the file is not.
    let mut p = good.clone();
    let size = u32::from_le_bytes(p[at + 4..at + 8].try_into().unwrap());
    put_u32(&mut p, at + 4, size + 100);
    assert_eq!(mato_data(&p, "SnowMaterialObject1P"), None);
    // A nested group bigger than its parent.
    let mut p = mato_plugin(&[snow(), nest(1, snow())]);
    let nested = at + 24 + snow().len();
    put_u32(&mut p, nested + 4, 10_000);
    assert_eq!(mato_data(&p, "SnowMaterialObject1P"), None);
}

#[test]
fn malformed_compressed_record_is_none() {
    // Not a zlib stream.
    let bad = rec_raw(
        b"MATO",
        2,
        FLAG_COMPRESSED,
        &[&100u32.to_le_bytes()[..], b"junk junk"].concat(),
    );
    assert_eq!(
        mato_data(&mato_plugin(&[snow(), bad]), "SnowMaterialObject1P"),
        None
    );
    // A valid stream whose declared size is wrong.
    let body = [edid("Liar"), sub(b"DATA", &data48())].concat();
    let mut z = compressed(&body);
    let declared = u32::from_le_bytes(z[..4].try_into().unwrap());
    put_u32(&mut z, 0, declared + 1);
    let liar = rec_raw(b"MATO", 3, FLAG_COMPRESSED, &z);
    assert_eq!(
        mato_data(&mato_plugin(&[snow(), liar]), "SnowMaterialObject1P"),
        None
    );
}

#[test]
fn subrecord_faults_are_none() {
    let body = |tail: &[u8]| {
        [
            edid("SnowMaterialObject1P"),
            sub(b"DATA", &data52()),
            tail.to_vec(),
        ]
        .concat()
    };
    let cases: [(&str, Vec<u8>); 3] = [
        // EDID and DATA are intact, then a subrecord header cut short.
        ("stray tail", vec![1, 2, 3]),
        // XXXX announces more bytes than the record has left.
        (
            "xxxx past body",
            [
                &b"XXXX"[..],
                &4u16.to_le_bytes(),
                &1_000_000u32.to_le_bytes(),
                b"ANAM",
                &0u16.to_le_bytes(),
                &[0u8; 8],
            ]
            .concat(),
        ),
        // XXXX with no subrecord after it.
        (
            "dangling xxxx",
            [&b"XXXX"[..], &4u16.to_le_bytes(), &8u32.to_le_bytes()].concat(),
        ),
    ];
    for (name, tail) in cases {
        let p = mato_plugin(&[rec_raw(b"MATO", 1, 0, &body(&tail))]);
        assert_eq!(mato_data(&p, "SnowMaterialObject1P"), None, "{name}");
    }
}

#[test]
fn oversized_subrecord_before_data_is_skipped() {
    // 70000 bytes needs an XXXX; the lookup must step over it and still find DATA.
    let r = rec(
        b"MATO",
        1,
        &[
            edid("Big"),
            sub(b"ANAM", &vec![7u8; 70_000]),
            sub(b"DATA", &data48()),
        ],
    );
    assert_eq!(mato_data(&mato_plugin(&[r]), "Big"), Some(distinct()));
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
