//! Krita `.kra` / OpenRaster `.ora` (their `mergedimage.png`) and WebP inputs through `load`.
use engine::load::*;
use image::{ImageBuffer, Rgba};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A ZIP holding `entries` (name, bytes), each stored (method 0) or deflated (8). CRCs are left 0: the reader does
/// not check them (the PNG carries its own).
fn zip(entries: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    for (name, data) in entries {
        let body = if deflate {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        } else {
            data.to_vec()
        };
        let method: u16 = if deflate { 8 } else { 0 };
        let local = out.len() as u32;
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0; 8]); // time, date, crc
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);

        cd.extend_from_slice(b"PK\x01\x02");
        cd.extend_from_slice(&[20, 0, 20, 0, 0, 0]); // made by, needed, flags
        cd.extend_from_slice(&method.to_le_bytes());
        cd.extend_from_slice(&[0; 8]);
        cd.extend_from_slice(&(body.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
        cd.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
        cd.extend_from_slice(&local.to_le_bytes());
        cd.extend_from_slice(name.as_bytes());
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&cd);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = ImageBuffer::from_fn(w, h, |x, y| Rgba([x as u8 * 50, y as u8 * 50, 9, 128]));
    let mut b = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png)
        .unwrap();
    b
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn kra_and_ora_read_mergedimage_png() {
    let d = tempfile::tempdir().unwrap();
    let merged = png(3, 2);
    // Krita stores `mimetype` first and the layers beside the merged image; only mergedimage.png is read.
    for (name, deflate) in [("a.kra", false), ("b.ora", true)] {
        let p = write(
            d.path(),
            name,
            &zip(
                &[
                    ("mimetype", b"application/x-krita"),
                    ("layers/layer1", &[1, 2, 3]),
                    ("mergedimage.png", &merged),
                    ("preview.png", &png(1, 1)),
                ],
                deflate,
            ),
        );
        let (img, info) = load_image(&p).unwrap();
        assert_eq!((img.width, img.height), (3, 2), "{name}");
        assert_eq!(
            (info.bit_depth, info.channels, info.has_alpha),
            (8, 4, true)
        );
        assert_eq!(
            img.at(2, 1),
            [100.0 / 255.0, 50.0 / 255.0, 9.0 / 255.0, 128.0 / 255.0]
        );
        assert_eq!(image_size(&p).unwrap(), (3, 2), "{name}");
    }
}

#[test]
fn an_archive_without_mergedimage_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        d.path(),
        "old.ora",
        &zip(&[("stack.xml", b"<image/>")], true),
    );
    for r in [load_image(&p).map(|_| ()), image_size(&p).map(|_| ())] {
        match r {
            Err(LoadError::Decode { msg, .. }) => assert!(msg.contains("mergedimage.png"), "{msg}"),
            other => panic!("expected a decode error, got {other:?}"),
        }
    }
    // A merged image that is not a PNG fails to decode, with the PNG decoder's reason.
    let p = write(
        d.path(),
        "bad.kra",
        &zip(&[("mergedimage.png", b"nope")], false),
    );
    assert!(matches!(load_image(&p), Err(LoadError::Decode { .. })));
}

#[test]
fn an_archive_is_size_checked_before_decoding() {
    let d = tempfile::tempdir().unwrap();
    let mut merged = png(1, 1);
    merged[16..20].copy_from_slice(&16385u32.to_be_bytes()); // IHDR width; the PNG is now corrupt anyway
    let p = write(
        d.path(),
        "wide.kra",
        &zip(&[("mergedimage.png", &merged)], true),
    );
    assert!(matches!(
        image_size(&p),
        Err(LoadError::TooLarge { dim: 16385, .. })
    ));
}

#[test]
fn webp_lossless_reads_exactly() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("w.webp");
    let img = ImageBuffer::from_fn(4, 2, |x, y| Rgba([x as u8 * 60, y as u8 * 90, 7, 200]));
    let f = std::fs::File::create(&p).unwrap();
    image::codecs::webp::WebPEncoder::new_lossless(f)
        .encode(img.as_raw(), 4, 2, image::ExtendedColorType::Rgba8)
        .unwrap();
    let (out, info) = load_image(&p).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.has_alpha),
        (8, 4, true)
    );
    assert_eq!(
        out.at(3, 1),
        [180.0 / 255.0, 90.0 / 255.0, 7.0 / 255.0, 200.0 / 255.0]
    );
    assert_eq!(image_size(&p).unwrap(), (4, 2));
}
