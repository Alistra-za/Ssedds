use std::fs;

/// An MO2 instance in a temp dir. The profile uses profile-local INIs (`LocalSettings=true`) and has none, so
/// scans never read the real Documents folder.
pub fn instance_fixture(modlist: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let game = r.join("Game");
    fs::create_dir_all(game.join("Data")).unwrap();
    fs::write(
        r.join("ModOrganizer.ini"),
        format!(
            "[General]\ngamePath=@ByteArray({})\nselected_profile=@ByteArray(Default)\n",
            game.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    for m in ["A", "B", "C", "Sep_separator"] {
        fs::create_dir_all(r.join("mods").join(m)).unwrap();
    }
    fs::create_dir_all(r.join("overwrite")).unwrap();
    fs::create_dir_all(r.join("profiles/Default")).unwrap();
    fs::write(r.join("profiles/Default/modlist.txt"), modlist).unwrap();
    fs::write(
        r.join("profiles/Default/settings.ini"),
        "[General]\nLocalSettings=true\n",
    )
    .unwrap();
    d
}

/// Writes `bytes` to `root/rel`, creating parent directories.
#[allow(dead_code)]
pub fn put(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// An uncompressed v105 BSA (archive flags 0x1|0x2) of (folder, file, bytes) entries; the entries of one folder must be
/// adjacent. Uncompressed so modscan needs no lz4 dev-dependency (Cargo.lock unchanged).
#[allow(dead_code)]
pub fn bsa(entries: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let mut folders: Vec<&str> = entries.iter().map(|e| e.0).collect();
    folders.dedup();
    let total_folder_len: usize = folders.iter().map(|f| f.len() + 1).sum();
    let total_file_len: usize = entries.iter().map(|e| e.1.len() + 1).sum();
    let folder_recs = 24 * folders.len();
    let file_rec_block: usize =
        folders.iter().map(|f| f.len() + 2).sum::<usize>() + 16 * entries.len();
    let mut data_off = 36 + folder_recs + file_rec_block + total_file_len;
    let mut out = b"BSA\0".to_vec();
    for v in [
        105u32,
        36,
        0x3,
        folders.len() as u32,
        entries.len() as u32,
        total_folder_len as u32,
        total_file_len as u32,
    ] {
        out.extend(v.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes()); // file flags + padding
    let mut recs_off = 36 + folder_recs;
    for f in &folders {
        let count = entries.iter().filter(|e| e.0 == *f).count();
        out.extend(0u64.to_le_bytes());
        out.extend((count as u32).to_le_bytes());
        out.extend(0u32.to_le_bytes());
        out.extend(((recs_off + total_file_len) as u64).to_le_bytes());
        recs_off += f.len() + 2 + 16 * count;
    }
    for f in &folders {
        out.push((f.len() + 1) as u8);
        out.extend(f.as_bytes());
        out.push(0);
        for (_, _, bytes) in entries.iter().filter(|e| e.0 == *f) {
            out.extend(0u64.to_le_bytes());
            out.extend((bytes.len() as u32).to_le_bytes());
            out.extend((data_off as u32).to_le_bytes());
            data_off += bytes.len();
        }
    }
    for e in entries {
        out.extend(e.1.as_bytes());
        out.push(0);
    }
    for e in entries {
        out.extend(e.2);
    }
    out
}
