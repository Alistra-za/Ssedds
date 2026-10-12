use std::io::Write;
use vfs::bsa::Bsa;

/// Builds a v105 BSA with the given (folder, file, bytes, compress) entries; flags 0x1|0x2 (+0x4 if all compressed).
fn build(entries: &[(&str, &str, &[u8], bool)], embed_names: bool) -> Vec<u8> {
    let mut folders: Vec<&str> = entries.iter().map(|e| e.0).collect();
    folders.dedup();
    let aflags: u32 = 0x1 | 0x2 | if embed_names { 0x100 } else { 0 };
    let total_folder_len: usize = folders.iter().map(|f| f.len() + 1).sum();
    let total_file_len: usize = entries.iter().map(|e| e.1.len() + 1).sum();
    let header = 36usize;
    let folder_recs = 24 * folders.len();
    let file_rec_block: usize =
        folders.iter().map(|f| 1 + f.len() + 1).sum::<usize>() + 16 * entries.len();
    let names_start = header + folder_recs + file_rec_block;
    let mut data_off = names_start + total_file_len;
    let mut out = vec![];
    out.extend(b"BSA\0");
    for v in [
        105u32,
        36,
        aflags,
        folders.len() as u32,
        entries.len() as u32,
        total_folder_len as u32,
        total_file_len as u32,
    ] {
        out.extend(v.to_le_bytes());
    }
    out.extend(2u16.to_le_bytes()); // file flags: meshes
    out.extend(0u16.to_le_bytes());
    let mut file_recs_off = header + folder_recs;
    for f in &folders {
        let count = entries.iter().filter(|e| e.0 == *f).count() as u32;
        out.extend(0u64.to_le_bytes());
        out.extend(count.to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(((file_recs_off + total_file_len) as u64).to_le_bytes());
        file_recs_off += 1 + f.len() + 1 + 16 * count as usize;
    }
    let mut blobs = vec![];
    for f in &folders {
        out.push((f.len() + 1) as u8);
        out.extend(f.as_bytes());
        out.push(0);
        for (_, name, bytes, comp) in entries.iter().filter(|e| e.0 == *f) {
            let mut blob = vec![];
            if embed_names {
                let full = format!("{f}\\{name}");
                blob.push(full.len() as u8);
                blob.extend(full.as_bytes());
            }
            if *comp {
                blob.extend((bytes.len() as u32).to_le_bytes());
                let mut enc = lz4_flex::frame::FrameEncoder::new(vec![]);
                enc.write_all(bytes).unwrap();
                blob.extend(enc.finish().unwrap());
            } else {
                blob.extend(*bytes);
            }
            let size = blob.len() as u32 | if *comp { 0x4000_0000 } else { 0 };
            out.extend(0u64.to_le_bytes());
            out.extend(size.to_le_bytes());
            out.extend((data_off as u32).to_le_bytes());
            data_off += blob.len();
            blobs.push(blob);
        }
    }
    for e in entries {
        out.extend(e.1.as_bytes());
        out.push(0);
    }
    for b in blobs {
        out.extend(b);
    }
    out
}

#[test]
fn reads_names_and_both_compression_states() {
    let dir = tempfile::tempdir().unwrap();
    for embed in [false, true] {
        let p = dir.path().join(format!("t{embed}.bsa"));
        let bytes = build(
            &[
                ("meshes\\armor\\iron", "cuirass.nif", b"NIFDATA-1", true),
                ("meshes\\armor\\iron", "boots.nif", b"plain", false),
                ("textures\\armor\\iron", "cuirass_d.dds", b"DDS ", true),
            ],
            embed,
        );
        std::fs::write(&p, bytes).unwrap();
        let bsa = Bsa::open(&p).unwrap();
        assert_eq!(bsa.version, 105);
        let names: Vec<_> = bsa.entries.iter().map(|e| e.norm.as_str()).collect();
        assert_eq!(
            names,
            [
                "meshes/armor/iron/cuirass.nif",
                "meshes/armor/iron/boots.nif",
                "textures/armor/iron/cuirass_d.dds"
            ]
        );
        assert_eq!(bsa.read(&bsa.entries[0]).unwrap(), b"NIFDATA-1");
        assert_eq!(bsa.read(&bsa.entries[1]).unwrap(), b"plain");
    }
}

#[test]
fn rejects_non_105_and_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("old.bsa");
    let mut b = build(&[("meshes", "a.nif", b"x", false)], false);
    b[4..8].copy_from_slice(&104u32.to_le_bytes());
    std::fs::write(&p, &b).unwrap();
    assert!(matches!(
        Bsa::open(&p),
        Err(vfs::VfsError::UnsupportedBsaVersion(104))
    ));
    std::fs::write(&p, b"BTDX....").unwrap();
    assert!(matches!(Bsa::open(&p), Err(vfs::VfsError::NotBsa)));
    std::fs::write(&p, b"BSA\0\x69\0\0\0").unwrap();
    assert!(Bsa::open(&p).is_err());
}

/// Opt-in: `SDC_BSA=C:\...\Skyrim - Meshes0.bsa cargo test -p vfs -- --ignored real_bsa`
#[test]
#[ignore]
fn real_bsa() {
    let bsa = Bsa::open(std::path::Path::new(&std::env::var("SDC_BSA").unwrap())).unwrap();
    let nif = bsa
        .entries
        .iter()
        .find(|e| e.norm.ends_with(".nif"))
        .unwrap();
    let bytes = bsa.read(nif).unwrap();
    assert!(nif::scan::scan(&bytes).is_ok(), "{}", nif.norm);
}

fn hostile(
    entries: &[(&str, &str, &[u8], bool)],
) -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("h.bsa");
    (dir, p, build(entries, false))
}

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn hostile_name_table_length_is_an_error() {
    let (_d, p, mut b) = hostile(&[("meshes", "a.nif", b"x", false)]);
    put(&mut b, 28, 0xFFFF_FFFF);
    std::fs::write(&p, &b).unwrap();
    assert!(matches!(Bsa::open(&p), Err(vfs::VfsError::Corrupt(_))));
}

#[test]
fn entry_past_eof_is_an_error() {
    let (_d, p, mut b) = hostile(&[("meshes", "a.nif", b"x", false)]);
    // single folder "meshes": file record at 36+24+1+7 = 68; size at +8, offset at +12
    put(&mut b, 76, 0x3FFF_FFF0);
    std::fs::write(&p, &b).unwrap();
    let bsa = Bsa::open(&p).unwrap();
    assert!(matches!(
        bsa.read(&bsa.entries[0]),
        Err(vfs::VfsError::Corrupt(_))
    ));
}

#[test]
fn huge_declared_decompressed_size_is_an_error() {
    let (_d, p, mut b) = hostile(&[("meshes", "a.nif", b"hello", true)]);
    let off = u32::from_le_bytes(b[80..84].try_into().unwrap()) as usize;
    put(&mut b, off, 0xFFFF_FFF0);
    std::fs::write(&p, &b).unwrap();
    let bsa = Bsa::open(&p).unwrap();
    assert!(matches!(
        bsa.read(&bsa.entries[0]),
        Err(vfs::VfsError::Corrupt(_))
    ));
}

#[test]
fn fewer_names_than_records_is_an_error() {
    let (_d, p, mut b) = hostile(&[
        ("meshes", "a.nif", b"x", false),
        ("meshes", "b.nif", b"y", false),
    ]);
    put(&mut b, 28, 6); // name table "a.nif\0" only
    std::fs::write(&p, &b).unwrap();
    assert!(matches!(Bsa::open(&p), Err(vfs::VfsError::Corrupt(_))));
}
