//! esp spec §6: the reader core on plugins built in code (`tests/common`).
mod common;
use common::*;
use esp::{parse, FLAG_COMPRESSED, FLAG_ESL, FLAG_ESM, FLAG_LOCALIZED};
use std::io::Write;

/// A compressed record body as stored: u32 decompressed size, then the zlib stream.
fn compressed(body: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(body).unwrap();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(e.finish().unwrap());
    out
}

#[test]
fn tes4_header_flags_and_masters() {
    let f = parse(
        "X.esp",
        &plugin(
            FLAG_ESM | FLAG_ESL | FLAG_LOCALIZED,
            &["Skyrim.esm", "Update.esm"],
            &[],
        ),
    );
    assert_eq!(f.name, "X.esp");
    assert_eq!(f.masters, ["Skyrim.esm", "Update.esm"]);
    assert!(f.esm() && f.esl() && f.localized());
    assert!(
        f.records.is_empty() && f.errors.is_empty(),
        "{:?}",
        f.errors
    );
    let plain = parse("Y.esp", &plugin(0, &[], &[]));
    assert!(!plain.esm() && !plain.esl() && !plain.localized());
}

#[test]
fn not_a_plugin_is_an_error_not_a_panic() {
    let f = parse("x.esp", b"hello");
    assert_eq!(f.errors, ["offset 0x0: not a plugin (no TES4 header)"]);
    assert!(f.records.is_empty());
}

/// A top-level group of an unwanted type, and a nested group inside a wanted one, are skipped by their sizes: the
/// garbage inside them is never read.
#[test]
fn unwanted_groups_are_skipped_without_reading_them() {
    let cell = grup(b"CELL", 0, &[vec![0xFF; 100]]);
    let nested = grup(&[0u8; 4], 1, &[vec![0xEE; 40]]);
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[
                cell,
                top(
                    b"STAT",
                    &[rec(b"STAT", 0x0100_0800, &[edid("Rock")]), nested],
                ),
            ],
        ),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.records.len(), 1);
    assert_eq!(&f.records[0].sig, b"STAT");
    assert_eq!(
        f.records[0].local_formid, 0x0100_0800,
        "the FormID as the plugin stores it"
    );
    assert_eq!(f.records[0].edid.as_deref(), Some("Rock"));
}

#[test]
fn compressed_records_are_inflated() {
    let body = edid("Packed");
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(
                b"STAT",
                &[rec_raw(
                    b"STAT",
                    0x0100_0801,
                    FLAG_COMPRESSED,
                    &compressed(&body),
                )],
            )],
        ),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.records[0].edid.as_deref(), Some("Packed"));
}

#[test]
fn a_bad_zlib_stream_is_reported_and_the_next_record_is_read() {
    let mut bad = 100u32.to_le_bytes().to_vec();
    bad.extend_from_slice(b"not zlib at all");
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(
                b"STAT",
                &[
                    rec_raw(b"STAT", 0x0100_0801, FLAG_COMPRESSED, &bad),
                    rec(b"STAT", 0x0100_0802, &[edid("After")]),
                ],
            )],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(
        f.errors[0].contains("STAT 01000801: bad zlib stream"),
        "{:?}",
        f.errors
    );
    assert_eq!(f.records.len(), 1);
    assert_eq!(f.records[0].edid.as_deref(), Some("After"));
}

#[test]
fn xxxx_sets_the_next_subrecord_size() {
    let long = "a".repeat(70_000);
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(
                b"STAT",
                &[
                    rec(b"STAT", 1, &[edid(&long)]),
                    rec(b"STAT", 2, &[edid("Next")]),
                ],
            )],
        ),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.records[0].edid.as_deref().map(str::len), Some(70_000));
    assert_eq!(f.records[1].edid.as_deref(), Some("Next"));
}

/// Spec §2.1: "the plugin's remaining groups are still walked when the group sizes allow it".
#[test]
fn a_record_running_past_its_group_is_reported_and_the_next_group_is_read() {
    let mut broken = rec(b"STAT", 0x0100_0802, &[edid("Broken")]);
    broken[4..8].copy_from_slice(&1000u32.to_le_bytes());
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[
                top(
                    b"STAT",
                    &[rec(b"STAT", 0x0100_0801, &[edid("Before")]), broken],
                ),
                top(b"TXST", &[rec(b"TXST", 0x0100_0900, &[edid("NextGroup")])]),
            ],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(
        f.errors[0].contains("runs past its group"),
        "{:?}",
        f.errors
    );
    let edids: Vec<&str> = f
        .records
        .iter()
        .map(|r| r.edid.as_deref().unwrap())
        .collect();
    assert_eq!(edids, ["Before", "NextGroup"]);
}

/// Spec §5: a truncated plugin contributes the records read before the fault.
#[test]
fn a_truncated_file_keeps_the_records_before_the_fault() {
    let mut bytes = plugin(
        0,
        &[],
        &[top(
            b"STAT",
            &[
                rec(b"STAT", 1, &[edid("First")]),
                rec(b"STAT", 2, &[edid("Second")]),
            ],
        )],
    );
    bytes.truncate(bytes.len() - 3);
    let f = parse("x.esp", &bytes);
    assert_eq!(f.records.len(), 1);
    assert_eq!(f.records[0].edid.as_deref(), Some("First"));
    assert!(
        f.errors
            .iter()
            .any(|e| e.contains("runs past the end of the file")),
        "{:?}",
        f.errors
    );
}

#[test]
fn a_subrecord_running_past_its_record_keeps_what_came_before() {
    let mut body = edid("Kept"); // 6 + 5 bytes: the next subrecord starts at +0xb
    body.extend_from_slice(b"MODL");
    body.extend_from_slice(&50u16.to_le_bytes());
    body.extend_from_slice(b"short");
    let f = parse(
        "x.esp",
        &plugin(0, &[], &[top(b"STAT", &[rec_raw(b"STAT", 7, 0, &body)])]),
    );
    assert_eq!(f.records[0].edid.as_deref(), Some("Kept"));
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(
        f.errors[0].contains("subrecord at +0xb runs past the record"),
        "{:?}",
        f.errors
    );
}

#[test]
fn a_stream_inflating_past_its_declared_size_is_an_error_and_the_next_record_is_read() {
    let mut liar = compressed(&edid(&"b".repeat(5000)));
    liar[0..4].copy_from_slice(&10u32.to_le_bytes());
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(
                b"STAT",
                &[
                    rec_raw(b"STAT", 0x0100_0801, FLAG_COMPRESSED, &liar),
                    rec(b"STAT", 0x0100_0802, &[edid("After")]),
                ],
            )],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(
        f.errors[0].contains("STAT 01000801: bad zlib stream"),
        "{:?}",
        f.errors
    );
    assert_eq!(f.records.len(), 1);
    assert_eq!(f.records[0].edid.as_deref(), Some("After"));
}

#[test]
fn a_declared_size_over_the_cap_is_an_error() {
    let mut huge = compressed(&edid("x"));
    huge[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(b"STAT", &[rec_raw(b"STAT", 1, FLAG_COMPRESSED, &huge)])],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("over the"), "{:?}", f.errors);
}

#[test]
fn a_trailing_xxxx_is_an_incomplete_subrecord_and_keeps_earlier_facts() {
    let mut body = edid("Kept");
    body.extend_from_slice(b"XXXX");
    body.extend_from_slice(&4u16.to_le_bytes());
    body.extend_from_slice(&9u32.to_le_bytes());
    let f = parse(
        "x.esp",
        &plugin(0, &[], &[top(b"STAT", &[rec_raw(b"STAT", 7, 0, &body)])]),
    );
    assert_eq!(f.records[0].edid.as_deref(), Some("Kept"));
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(
        f.errors[0].contains("subrecord at +0xb runs past the record"),
        "{:?}",
        f.errors
    );
}
