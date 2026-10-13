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
    let cell = grup(b"QUST", 0, &[vec![0xFF; 100]]);
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
    // The reader stops at declared + 1 bytes: this pins the `take` bound.
    assert!(
        f.errors[0].contains("inflates to 11 bytes or more"),
        "{:?}",
        f.errors
    );
    assert_eq!(f.records.len(), 1);
    assert_eq!(f.records[0].edid.as_deref(), Some("After"));
}

#[test]
fn a_stream_inflating_short_of_its_declared_size_reports_the_exact_size() {
    let mut liar = compressed(&edid("abc"));
    let real = u32::from_le_bytes(liar[0..4].try_into().unwrap());
    liar[0..4].copy_from_slice(&(real + 5).to_le_bytes());
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[top(b"STAT", &[rec_raw(b"STAT", 1, FLAG_COMPRESSED, &liar)])],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    let want = format!("inflates to {real} bytes, header says {}", real + 5);
    assert!(f.errors[0].contains(&want), "{:?}", f.errors);
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

fn placed(sig: &[u8; 4], id: u32, base: u32) -> Vec<u8> {
    rec(
        sig,
        id,
        &[edid("R"), form(b"NAME", base), sub(b"DATA", &[0u8; 24])],
    )
}

/// Filtering spec §3; group types per xEdit dev-4.1.6 `wbImplementation.pas` `TwbGroupRecord.GetName` (1-10). The base
/// FormID of every REFR/ACHR under CELL and WRLD, once each, ascending; other records there are skipped unread.
#[test]
fn placed_references_in_interior_and_exterior_cells() {
    let cell = 0x0000_1000u32.to_le_bytes();
    let interior = grup(
        b"CELL",
        0,
        &[grup(
            &0u32.to_le_bytes(),
            2,
            &[grup(
                &0u32.to_le_bytes(),
                3,
                &[
                    rec(b"CELL", 0x1000, &[edid("Room")]),
                    grup(
                        &cell,
                        6,
                        &[
                            grup(
                                &cell,
                                8,
                                &[
                                    placed(b"REFR", 0x2001, 0x0100_0800),
                                    placed(b"ACHR", 0x2002, 0x7777),
                                ],
                            ),
                            grup(
                                &cell,
                                9,
                                &[
                                    placed(b"REFR", 0x2003, 0x0100_0800),
                                    rec(b"NAVM", 0x2004, &[vec![0xFF; 32]]),
                                ],
                            ),
                        ],
                    ),
                ],
            )],
        )],
    );
    let (wrld, ext) = (0x0000_3000u32.to_le_bytes(), 0x0000_3001u32.to_le_bytes());
    let packed = [form(b"NAME", 0x0ABC), sub(b"DATA", &[0u8; 24])].concat();
    let exterior = grup(
        b"WRLD",
        0,
        &[
            rec(b"WRLD", 0x3000, &[edid("Tamriel")]),
            grup(
                &wrld,
                1,
                &[grup(
                    &[0u8; 4],
                    4,
                    &[grup(
                        &[0u8; 4],
                        5,
                        &[
                            rec(b"CELL", 0x3001, &[]),
                            grup(
                                &ext,
                                6,
                                &[grup(
                                    &ext,
                                    9,
                                    &[rec_raw(
                                        b"REFR",
                                        0x3002,
                                        FLAG_COMPRESSED,
                                        &compressed(&packed),
                                    )],
                                )],
                            ),
                        ],
                    )],
                )],
            ),
        ],
    );
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[
                interior,
                exterior,
                top(b"STAT", &[rec(b"STAT", 0x0100_0800, &[edid("Rock")])]),
            ],
        ),
    );
    assert!(f.errors.is_empty(), "{:?}", f.errors);
    assert_eq!(f.placed, vec![0x0ABC, 0x7777, 0x0100_0800]);
    assert_eq!(f.records.len(), 1, "placed references are not records");
}

/// Review focus 4: a bad nested group is reported and ends only its own group.
#[test]
fn a_bad_nested_cell_group_is_reported_and_later_groups_are_read() {
    let mut bad = grup(&[0u8; 4], 6, &[placed(b"REFR", 1, 0x10)]);
    bad[4..8].copy_from_slice(&10_000u32.to_le_bytes()); // runs past its parent
    let f = parse(
        "x.esp",
        &plugin(
            0,
            &[],
            &[
                grup(b"CELL", 0, &[bad]),
                top(b"STAT", &[rec(b"STAT", 0x0100_0800, &[edid("Rock")])]),
            ],
        ),
    );
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("nested group size"), "{:?}", f.errors);
    assert_eq!(f.records.len(), 1);
}

/// Review focus 4: nesting is capped, so a crafted file cannot overflow the stack.
#[test]
fn groups_nested_too_deep_are_an_error_not_a_stack_overflow() {
    let mut g = grup(&[0u8; 4], 9, &[placed(b"REFR", 1, 0x10)]);
    for _ in 0..20 {
        g = grup(&[0u8; 4], 6, &[g]);
    }
    let f = parse("x.esp", &plugin(0, &[], &[grup(b"CELL", 0, &[g])]));
    assert_eq!(f.errors.len(), 1, "{:?}", f.errors);
    assert!(f.errors[0].contains("nested deeper"), "{:?}", f.errors);
    assert!(f.placed.is_empty());
}
