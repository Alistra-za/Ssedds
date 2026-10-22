mod builder;
use builder::scenes;
use nif::header::Header;
use nif::write::{block_diff, NifDoc, WriteError};

const FIXTURES: [&str; 5] = [
    "TestNifFile_Skinned_SE.nif",
    "TestNifFile_Furniture_Col_SE.nif",
    "TestNifFile_DeepGraph_SE.nif",
    "TestNifFile_Skinned_Dynamic_SE.nif",
    "TestNifFile_Optimize_LE_to_SE.nif",
];

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn block_bytes<'a>(h: &Header, b: &'a [u8], k: usize) -> &'a [u8] {
    &b[h.offsets()[k]..][..h.sizes[k] as usize]
}

fn appended(b: &[u8]) -> (Vec<u8>, u32) {
    let mut d = NifDoc::open(b).unwrap();
    let s = d.string(b"NewName").unwrap();
    let body = [nif::net::encode_net(s, &[], -1), vec![0xEC, 0x12, 128]].concat();
    let i = d.append(b"NiAlphaProperty", body).unwrap();
    (d.bytes(), i)
}

#[test]
fn identity_without_edits() {
    for f in FIXTURES
        .iter()
        .copied()
        .chain(["TestNifFile_Animated_LE.nif"])
    {
        let b = read(f);
        assert_eq!(NifDoc::open(&b).unwrap().bytes(), b, "{f}");
    }
    for b in scenes::all() {
        assert_eq!(NifDoc::open(&b).unwrap().bytes(), b);
    }
}

#[test]
fn forced_header_re_emit_is_identical() {
    let mut all: Vec<Vec<u8>> = FIXTURES.iter().map(|f| read(f)).collect();
    all.push(read("TestNifFile_Animated_LE.nif"));
    all.extend(scenes::all());
    for b in all {
        let mut d = NifDoc::open(&b).unwrap();
        d.rebuild_header();
        assert_eq!(d.bytes(), b);
    }
}

#[test]
fn one_patch_changes_exactly_those_bytes() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let mut d = NifDoc::open(&b).unwrap();
    let blk = scenes::block_of(d.header(), "BSLightingShaderProperty");
    let at = d.header().offsets()[blk] + 20;
    d.patch(blk as u32, 20, &1.5f32.to_le_bytes()).unwrap();
    let out = d.bytes();
    assert_eq!(out.len(), b.len());
    let diff: Vec<usize> = (0..b.len()).filter(|&i| b[i] != out[i]).collect();
    assert!(diff.iter().all(|&i| (at..at + 4).contains(&i)) && !diff.is_empty());
}

#[test]
fn append_and_string_keep_old_blocks_and_strings() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let h0 = Header::parse(&b).unwrap();
    let (out, i) = appended(&b);
    let h = Header::parse(&out).unwrap();
    h.check_file_length(&out).unwrap();
    assert_eq!(i as usize, h0.sizes.len());
    assert_eq!(&h.strings[..h0.strings.len()], &h0.strings[..]);
    for k in 0..h0.sizes.len() {
        assert_eq!(block_bytes(&h, &out, k), block_bytes(&h0, &b, k));
    }
    assert_eq!(h.roots(&out).unwrap(), h0.roots(&b).unwrap());
    assert_eq!(
        h.max_string_len as usize,
        h.strings.iter().map(Vec::len).max().unwrap()
    );
}

#[test]
fn patch_outside_block_is_refused() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let mut d = NifDoc::open(&b).unwrap();
    let size = d.block(0).len();
    let e = d.patch(0, size - 2, &[0; 4]).unwrap_err();
    assert_eq!(
        e,
        WriteError::Patch {
            block: 0,
            at: size - 2,
            len: 4
        }
    );
    assert!(d.patch(0, usize::MAX, &[0; 4]).is_err());
    assert!(d.patch(u32::MAX, 0, &[0]).is_err());
    assert!(d.kind(u32::MAX).is_empty() && d.block(u32::MAX).is_empty());
    assert_eq!(d.bytes(), b);
}

#[test]
fn block_diff_names_changed_and_added_blocks() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let (out, i) = appended(&b);
    assert_eq!(
        block_diff(&b, &out).unwrap(),
        vec![
            format!("block {i} NiAlphaProperty: added"),
            "strings: +1".to_string()
        ]
    );
    let mut d = NifDoc::open(&b).unwrap();
    d.patch(0, 0, &[0xFF]).unwrap();
    let n = d.block(1).len();
    d.set_block(1, vec![0; n + 1]);
    let k = String::from_utf8_lossy(d.kind(1)).into_owned();
    let lines = block_diff(&b, &d.bytes()).unwrap();
    assert!(
        lines.contains(&format!("block 1 {k}: {n} → {} bytes", n + 1)),
        "{lines:?}"
    );
    assert!(lines
        .iter()
        .any(|l| l.ends_with(": same size, bytes differ")));
}

#[test]
fn quirk_fixture_round_trips_and_survives_an_unrelated_edit() {
    let b = scenes::quirks();
    assert_eq!(NifDoc::open(&b).unwrap().bytes(), b);
    let mut d = NifDoc::open(&b).unwrap();
    d.string(b"Extra").unwrap(); // forces a header re-emit
    let out = d.bytes();
    let (h0, h) = (Header::parse(&b).unwrap(), Header::parse(&out).unwrap());
    assert_eq!(h.roots(&out).unwrap(), vec![2, 0]);
    assert_ne!(h0.type_index_raw[4] & 0x8000, 0);
    assert!(nif::model::read_model(&b).unwrap().problems.is_empty());
    assert_eq!(h.block_types, h0.block_types); // unused type kept
    assert_eq!(h.type_index_raw, h0.type_index_raw);
    assert_eq!(&h.strings[..h0.strings.len()], &h0.strings[..]);
    for k in 0..h0.sizes.len() {
        assert_eq!(block_bytes(&h, &out, k), block_bytes(&h0, &b, k));
    }
}

/// Note CY2: a NIF listing `n` block types, with one block (0, NiNode) of type 0.
fn many_types(n: usize) -> Vec<u8> {
    let mut b = builder::Nif::new(100);
    b.block("NiNode", vec![]);
    for k in 1..n {
        b.add_type(&format!("T{k}"));
    }
    b.bytes()
}

#[test]
fn append_never_writes_a_type_index_the_parser_masks() {
    let b = many_types(0x8000);
    let mut d = NifDoc::open(&b).unwrap();
    let r = d.append(b"NiAlphaProperty", vec![0; 15]);
    assert!(matches!(r, Err(WriteError::Refused(_))), "{r:?}");
    assert_eq!(d.bytes(), b);
    // a type already listed below 0x8000 still appends, and reads back as itself
    let i = d.append(b"NiNode", vec![]).unwrap();
    assert_eq!(
        Header::parse(&d.bytes()).unwrap().kind(i as usize),
        b"NiNode"
    );
}

#[test]
fn string_never_writes_past_the_parsers_string_cap() {
    let mut n = builder::Nif::new(100);
    n.block("NiNode", vec![]);
    n.strings = (0..1u32 << 20).map(|k| k.to_string()).collect();
    let b = n.bytes();
    let mut d = NifDoc::open(&b).unwrap();
    assert_eq!(d.string(b"7"), Ok(7));
    assert!(matches!(d.string(b"new"), Err(WriteError::Refused(_))));
    assert_eq!(d.bytes(), b);
}
