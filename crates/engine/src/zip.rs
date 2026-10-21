//! Track A Task 8 (DECISIONS Note CM): an in-house ZIP writer for the Also export's folder zips (C8). Method 8 (raw
//! Deflate), entries sorted by name, the DOS time fixed at 1980-01-01, so the same files give the same bytes.
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ZipError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("over the ZIP limits (4 GiB or 65,535 files); ZIP64 is not written")]
    TooLarge,
}

/// The archive of exactly `files` (paths under `folder`, TA23), entries named `<folder name>/<rel>` with `/`.
// shortcut: the whole zip is buffered in RAM, so TA17's 4 GiB ceiling is also a RAM ceiling; upgrade to a temp file
// plus rename (same flush) if a real mod gets near it.
// shortcut: no ZIP64 (TA17); an archive over 4 GiB - 1 or 65,535 entries is `TooLarge`. Upgrade if a real mod hits it.
pub fn write_zip(folder: &Path, files: &[PathBuf]) -> Result<Vec<u8>, ZipError> {
    let top = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut named: Vec<(String, &PathBuf)> = files
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(folder).unwrap_or(p);
            let parts = rel.components().map(|c| c.as_os_str().to_string_lossy());
            let name = std::iter::once(top.clone().into())
                .chain(parts)
                .collect::<Vec<_>>()
                .join("/");
            (name, p)
        })
        .collect();
    named.sort();
    if named.len() > 0xFFFF {
        return Err(ZipError::TooLarge);
    }
    let fit = |n: usize| u32::try_from(n).ok().filter(|&n| n != u32::MAX);
    let (mut out, mut cd) = (vec![], vec![]);
    for (name, path) in &named {
        let data = std::fs::read(path)?;
        let mut crc = flate2::Crc::new();
        crc.update(&data);
        let mut enc = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::default());
        enc.write_all(&data)?;
        let packed = enc.finish()?;
        let (Some(csize), Some(usize_), Some(offset), Ok(nlen)) = (
            fit(packed.len()),
            fit(data.len()),
            fit(out.len()),
            u16::try_from(name.len()),
        ) else {
            return Err(ZipError::TooLarge);
        };
        // version 20, flags: UTF-8 names, method 8, DOS time 0:00, date 1980-01-01, crc, sizes, name length.
        let common = |b: &mut Vec<u8>| {
            for v in [0x0800u16, 8, 0, 0x0021] {
                b.extend(v.to_le_bytes());
            }
            for v in [crc.sum(), csize, usize_] {
                b.extend(v.to_le_bytes());
            }
            b.extend(nlen.to_le_bytes());
            b.extend(0u16.to_le_bytes()); // extra field length
        };
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(20u16.to_le_bytes());
        common(&mut out);
        out.extend(name.as_bytes());
        out.extend(&packed);

        cd.extend(0x0201_4b50u32.to_le_bytes());
        cd.extend(20u16.to_le_bytes()); // made by
        cd.extend(20u16.to_le_bytes()); // needed
        common(&mut cd);
        // comment length, disk, internal attributes, external attributes, local header offset.
        for v in [0u16, 0, 0] {
            cd.extend(v.to_le_bytes());
        }
        cd.extend(0u32.to_le_bytes());
        cd.extend(offset.to_le_bytes());
        cd.extend(name.as_bytes());
    }
    let (Some(cd_off), Some(cd_len), Some(_)) = (
        fit(out.len()),
        fit(cd.len()),
        fit(out.len() + cd.len() + 22),
    ) else {
        return Err(ZipError::TooLarge);
    };
    out.extend(cd);
    let n = (named.len() as u16).to_le_bytes();
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // disk numbers
    out.extend(n);
    out.extend(n);
    out.extend(cd_len.to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // comment length
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    fn u16le(b: &[u8], at: usize) -> usize {
        u16::from_le_bytes([b[at], b[at + 1]]) as usize
    }
    fn u32le(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    /// (name, bytes) of every entry, by EOCD → central directory → local header → inflate; CRC and sizes checked.
    fn read_back(z: &[u8]) -> Vec<(String, Vec<u8>)> {
        let eocd = z.len() - 22;
        assert_eq!(u32le(z, eocd), 0x0605_4b50);
        let n = u16le(z, eocd + 10);
        let mut at = u32le(z, eocd + 16) as usize;
        let mut out = vec![];
        for _ in 0..n {
            assert_eq!(u32le(z, at), 0x0201_4b50);
            assert_eq!(u16le(z, at + 10), 8, "method");
            let crc = u32le(z, at + 16);
            let (csize, usize_) = (u32le(z, at + 20) as usize, u32le(z, at + 24) as usize);
            let (nlen, xlen, clen) = (u16le(z, at + 28), u16le(z, at + 30), u16le(z, at + 32));
            let local = u32le(z, at + 42) as usize;
            let name = String::from_utf8(z[at + 46..at + 46 + nlen].to_vec()).unwrap();
            at += 46 + nlen + xlen + clen;

            assert_eq!(u32le(z, local), 0x0403_4b50);
            assert_eq!(u32le(z, local + 14), crc);
            assert_eq!(u32le(z, local + 18) as usize, csize);
            assert_eq!(u32le(z, local + 22) as usize, usize_);
            let (ln, lx) = (u16le(z, local + 26), u16le(z, local + 28));
            assert_eq!(&z[local + 30..local + 30 + ln], name.as_bytes());
            let data = local + 30 + ln + lx;
            let mut bytes = vec![];
            DeflateDecoder::new(&z[data..data + csize])
                .read_to_end(&mut bytes)
                .unwrap();
            assert_eq!(bytes.len(), usize_, "{name}");
            let mut c = flate2::Crc::new();
            c.update(&bytes);
            assert_eq!(c.sum(), crc, "{name}");
            out.push((name, bytes));
        }
        out
    }

    fn noise(n: usize) -> Vec<u8> {
        let mut x = 0x2545_f491_u32;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    fn put(p: &Path, b: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b).unwrap();
    }

    #[test]
    fn a_written_zip_reads_back_byte_for_byte() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("Mod PBR");
        let a = noise(10 * 1024);
        let b = b"not really a png".repeat(40);
        put(&f.join("a.bin"), &a);
        put(&f.join("sub/b.png"), &b);
        let z = write_zip(&f, &[f.join("sub/b.png"), f.join("a.bin")]).unwrap();
        assert_eq!(
            read_back(&z),
            vec![
                ("Mod PBR/a.bin".to_string(), a),
                ("Mod PBR/sub/b.png".to_string(), b)
            ]
        );
    }

    #[test]
    fn the_same_folder_zips_to_the_same_bytes() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("Mod");
        put(&f.join("x.dds"), &noise(3000));
        put(&f.join("y/z.dds"), b"z");
        let files = [f.join("x.dds"), f.join("y/z.dds")];
        let one = write_zip(&f, &files).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(one, write_zip(&f, &files).unwrap());
    }

    #[test]
    fn only_the_listed_files_are_zipped_with_forward_slash_names() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("Mod CM");
        put(&f.join("_sdc/manifest.jsonl"), b"{}");
        put(&f.join(".sdc-1-2-3.tmp"), b"tmp");
        put(&f.join("x.dds"), b"x");
        put(&f.join("sub/y.dds"), b"y");
        let z = write_zip(&f, &[f.join("x.dds"), f.join("sub").join("y.dds")]).unwrap();
        let names: Vec<String> = read_back(&z).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["Mod CM/sub/y.dds", "Mod CM/x.dds"]);
    }

    #[test]
    fn an_empty_list_is_an_empty_archive() {
        let d = tempfile::tempdir().unwrap();
        let z = write_zip(d.path(), &[]).unwrap();
        assert_eq!(z.len(), 22);
        assert!(read_back(&z).is_empty());
    }
}
