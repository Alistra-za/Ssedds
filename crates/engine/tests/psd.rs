//! Photoshop PSD/PSB input (`engine::psd` through `load`): the merged image, its channels, depths and refusals.
use engine::load::*;
use std::path::{Path, PathBuf};

/// A PSD (or PSB) built by hand: header, no colour data, `res` image resources, a layer section holding only the
/// global tagged blocks `blocks`, then the merged image `planes` (one per channel, big-endian samples) raw (0) or
/// PackBits (1) as `compression`.
struct Psd {
    big: bool,
    w: u32,
    h: u32,
    depth: u16,
    mode: u16,
    res: Vec<(u16, Vec<u8>)>,
    blocks: Vec<(&'static [u8; 4], Vec<u8>)>,
    compression: u16,
    planes: Vec<Vec<u8>>,
}

impl Psd {
    fn rgb(w: u32, h: u32, depth: u16, planes: Vec<Vec<u8>>) -> Self {
        Psd {
            big: false,
            w,
            h,
            depth,
            mode: 3,
            res: Vec::new(),
            blocks: Vec::new(),
            compression: 0,
            planes,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"8BPS");
        b.extend_from_slice(&(if self.big { 2u16 } else { 1 }).to_be_bytes());
        b.extend_from_slice(&[0; 6]);
        b.extend_from_slice(&(self.planes.len() as u16).to_be_bytes());
        b.extend_from_slice(&self.h.to_be_bytes());
        b.extend_from_slice(&self.w.to_be_bytes());
        b.extend_from_slice(&self.depth.to_be_bytes());
        b.extend_from_slice(&self.mode.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());

        let mut res = Vec::new();
        for (id, data) in &self.res {
            res.extend_from_slice(b"8BIM");
            res.extend_from_slice(&id.to_be_bytes());
            res.extend_from_slice(&[0, 0]); // empty name, padded to even
            res.extend_from_slice(&(data.len() as u32).to_be_bytes());
            res.extend_from_slice(data);
            if data.len() % 2 == 1 {
                res.push(0);
            }
        }
        b.extend_from_slice(&(res.len() as u32).to_be_bytes());
        b.extend_from_slice(&res);

        let len = |v: &mut Vec<u8>, n: u64, long: bool| {
            if long {
                v.extend_from_slice(&n.to_be_bytes());
            } else {
                v.extend_from_slice(&(n as u32).to_be_bytes());
            }
        };
        let mut lm = Vec::new();
        len(&mut lm, 0, self.big); // no layer records
        lm.extend_from_slice(&0u32.to_be_bytes()); // no global layer mask
        for (key, data) in &self.blocks {
            lm.extend_from_slice(b"8BIM");
            lm.extend_from_slice(*key);
            len(&mut lm, data.len() as u64, self.big);
            lm.extend_from_slice(data);
        }
        len(&mut b, lm.len() as u64, self.big);
        b.extend_from_slice(&lm);

        b.extend_from_slice(&self.compression.to_be_bytes());
        let row = (self.w * u32::from(self.depth / 8)) as usize;
        if self.compression == 1 {
            // Literal-only PackBits: runs of at most 128 bytes.
            let rows: Vec<Vec<u8>> = self
                .planes
                .iter()
                .flat_map(|p| p.chunks(row))
                .map(|r| {
                    let mut o = Vec::new();
                    for c in r.chunks(128) {
                        o.push((c.len() - 1) as u8);
                        o.extend_from_slice(c);
                    }
                    o
                })
                .collect();
            for r in &rows {
                if self.big {
                    b.extend_from_slice(&(r.len() as u32).to_be_bytes());
                } else {
                    b.extend_from_slice(&(r.len() as u16).to_be_bytes());
                }
            }
            rows.iter().for_each(|r| b.extend_from_slice(r));
        } else {
            self.planes.iter().for_each(|p| b.extend_from_slice(p));
        }
        b
    }

    fn write(&self, dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, self.bytes()).unwrap();
        p
    }
}

fn u16s(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

#[test]
fn rgb8_raw_reads_exactly() {
    let d = tempfile::tempdir().unwrap();
    let p = Psd::rgb(
        2,
        2,
        8,
        vec![
            vec![0, 51, 102, 255],
            vec![10, 20, 30, 40],
            vec![255, 0, 7, 9],
        ],
    )
    .write(d.path(), "a.psd");
    let (img, info, meta) = load_image_meta(&p).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.has_alpha, info.is_float),
        (8, 3, false, false)
    );
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(img.at(1, 0), [51.0 / 255.0, 20.0 / 255.0, 0.0, 1.0]);
    assert_eq!(img.at(1, 1), [1.0, 40.0 / 255.0, 9.0 / 255.0, 1.0]);
    assert!(!meta.icc_profile);
    assert_eq!(meta.dds_dxgi, None);
    assert_eq!(image_size(&p).unwrap(), (2, 2));
}

#[test]
fn rgb16_rle_reads_alpha_and_ignores_extra_channels() {
    let d = tempfile::tempdir().unwrap();
    let mut psd = Psd::rgb(
        3,
        1,
        16,
        vec![
            u16s(&[0, 65535, 32768]),
            u16s(&[1, 2, 3]),
            u16s(&[4, 5, 6]),
            u16s(&[65535, 0, 1000]),
            u16s(&[9, 9, 9]), // a second alpha channel: not read
        ],
    );
    psd.compression = 1;
    let p = psd.write(d.path(), "b.psd");
    let (img, info) = load_image(&p).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.has_alpha, info.is_float),
        (16, 4, true, false)
    );
    assert_eq!(img.at(0, 0), [0.0, 1.0 / 65535.0, 4.0 / 65535.0, 1.0]);
    assert_eq!(img.at(1, 0)[3], 0.0);
    assert!((img.at(2, 0)[0] - 32768.0 / 65535.0).abs() < 1e-7);
    assert!((img.at(2, 0)[3] - 1000.0 / 65535.0).abs() < 1e-7);
}

#[test]
fn grayscale_with_alpha() {
    let d = tempfile::tempdir().unwrap();
    let mut psd = Psd::rgb(2, 1, 8, vec![vec![255, 51], vec![0, 255]]);
    psd.mode = 1;
    psd.compression = 1;
    let p = psd.write(d.path(), "g.psd");
    let (img, info) = load_image(&p).unwrap();
    assert_eq!((info.channels, info.has_alpha), (2, true));
    assert_eq!(img.at(0, 0), [1.0, 1.0, 1.0, 0.0]);
    assert_eq!(img.at(1, 0), [0.2, 0.2, 0.2, 1.0]);
}

#[test]
fn float32_is_linear_and_unclamped() {
    let d = tempfile::tempdir().unwrap();
    let p = Psd::rgb(1, 1, 32, vec![f32s(&[2.5]), f32s(&[0.25]), f32s(&[-0.0])])
        .write(d.path(), "h.psd");
    let (img, info) = load_image(&p).unwrap();
    assert!(info.is_float);
    assert_eq!(info.bit_depth, 32);
    assert_eq!(img.at(0, 0), [2.5, 0.25, 0.0, 1.0]);
}

/// Merged transparency: Photoshop matted the colour against white, so it is un-matted.
#[test]
fn merged_transparency_is_unmatted() {
    let d = tempfile::tempdir().unwrap();
    // c = 0.2 at a = 0.5 is stored as 0.2·0.5 + (1 − 0.5) = 0.6; a = 0 keeps the stored white.
    let mut psd = Psd::rgb(
        2,
        1,
        32,
        vec![
            f32s(&[0.6, 1.0]),
            f32s(&[1.0, 1.0]),
            f32s(&[0.5, 1.0]),
            f32s(&[0.5, 0.0]),
        ],
    );
    psd.blocks = vec![(b"Mt32", vec![0; 4])];
    let p = psd.write(d.path(), "m.psd");
    let (img, _) = load_image(&p).unwrap();
    let px = img.at(0, 0);
    assert!((px[0] - 0.2).abs() < 1e-6 && (px[1] - 1.0).abs() < 1e-6 && px[2].abs() < 1e-6);
    assert_eq!(px[3], 0.5);
    assert_eq!(img.at(1, 0), [1.0, 1.0, 1.0, 0.0]);

    // Without the block the fourth channel is a plain alpha channel and the colour is read as stored.
    psd.blocks = vec![(b"Patt", vec![0; 6])];
    let p = psd.write(d.path(), "n.psd");
    assert_eq!(load_image(&p).unwrap().0.at(0, 0)[0], 0.6);
}

/// A PSB: 64-bit section lengths (and `Mt16`'s), 32-bit RLE row counts.
#[test]
fn psb_reads_like_psd() {
    let d = tempfile::tempdir().unwrap();
    let mut psd = Psd::rgb(
        2,
        1,
        16,
        vec![
            u16s(&[65535, 65535]),
            u16s(&[0, 65535]),
            u16s(&[0, 65535]),
            u16s(&[65535, 65535]),
        ],
    );
    psd.big = true;
    psd.compression = 1;
    psd.blocks = vec![(b"Mt16", vec![0; 8])];
    let p = psd.write(d.path(), "big.psb");
    let (img, info) = load_image(&p).unwrap();
    assert_eq!((info.bit_depth, info.channels), (16, 4));
    assert_eq!(img.at(0, 0), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(img.at(1, 0), [1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn icc_profile_is_detected() {
    let d = tempfile::tempdir().unwrap();
    let mut psd = Psd::rgb(1, 1, 8, vec![vec![1], vec![2], vec![3]]);
    psd.res = vec![(1005, vec![0; 16]), (1039, vec![7; 33])];
    let p = psd.write(d.path(), "icc.psd");
    assert!(load_image_meta(&p).unwrap().2.icc_profile);
}

/// Content picks the decoder: a PSD named `.png` still reads, and a PNG named `.psd` reads as a PNG.
#[test]
fn chosen_by_content_not_extension() {
    let d = tempfile::tempdir().unwrap();
    let p = Psd::rgb(1, 1, 8, vec![vec![255], vec![0], vec![0]]).write(d.path(), "x.png");
    assert_eq!(load_image(&p).unwrap().0.at(0, 0), [1.0, 0.0, 0.0, 1.0]);
    let q = d.path().join("y.psd");
    image::RgbImage::from_pixel(1, 1, image::Rgb([0, 255, 0]))
        .save_with_format(&q, image::ImageFormat::Png)
        .unwrap();
    assert_eq!(load_image(&q).unwrap().0.at(0, 0), [0.0, 1.0, 0.0, 1.0]);
}

fn decode_msg(p: &Path) -> String {
    match load_image(p) {
        Err(LoadError::Decode { msg, .. }) => msg,
        other => panic!("expected a decode error, got {:?}", other.map(|(_, i)| i)),
    }
}

#[test]
fn refusals_say_why() {
    let d = tempfile::tempdir().unwrap();
    let rgb = || Psd::rgb(1, 1, 8, vec![vec![1], vec![2], vec![3]]);

    // Version Info (1057) with hasRealMergedData = 0: Maximize Compatibility was off.
    let mut psd = rgb();
    psd.res = vec![(1057, vec![0, 0, 0, 1, 0, 0, 0, 0, 0])];
    assert!(decode_msg(&psd.write(d.path(), "nomerge.psd")).contains("Maximize Compatibility"));
    psd.res = vec![(1057, vec![0, 0, 0, 1, 1, 0, 0, 0, 0])];
    assert!(load_image(&psd.write(d.path(), "merge.psd")).is_ok());

    let mut psd = rgb();
    psd.planes.push(vec![4]);
    psd.mode = 4;
    assert!(decode_msg(&psd.write(d.path(), "cmyk.psd")).contains("CMYK"));

    let mut psd = rgb();
    psd.compression = 3;
    assert!(decode_msg(&psd.write(d.path(), "zip.psd")).contains("ZIP"));

    let mut b = rgb().bytes();
    b.truncate(b.len() - 2);
    let p = d.path().join("short.psd");
    std::fs::write(&p, b).unwrap();
    assert!(decode_msg(&p).contains("truncated"));

    let mut psd = rgb();
    psd.compression = 1;
    let mut b = psd.bytes();
    let n = b.len();
    b[n - 2] = 0x05; // the blue row's literal now claims 6 bytes
    let p = d.path().join("rle.psd");
    std::fs::write(&p, b).unwrap();
    assert!(decode_msg(&p).contains("RLE"));
}

/// The size is checked from the header, before any pixel is read or allocated (the file holds no pixels at all).
#[test]
fn rejects_over_16384_from_the_header() {
    let d = tempfile::tempdir().unwrap();
    let mut b = Psd::rgb(1, 1, 8, vec![vec![0], vec![0], vec![0]]).bytes();
    b[18..22].copy_from_slice(&16385u32.to_be_bytes()); // width
    let p = d.path().join("wide.psd");
    std::fs::write(&p, &b[..26]).unwrap();
    assert!(matches!(
        load_image(&p),
        Err(LoadError::TooLarge { dim: 16385, .. })
    ));
    assert!(matches!(
        image_size(&p),
        Err(LoadError::TooLarge { dim: 16385, .. })
    ));
}
