use engine::load::*;
use image::{ExtendedColorType, ImageBuffer, ImageEncoder, Luma, LumaA, Rgb, Rgba};

#[test]
fn png8_rgba_loads_exactly() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.png");
    ImageBuffer::from_fn(4, 4, |x, y| Rgba([x as u8 * 60, y as u8 * 60, 7, 200]))
        .save(&p)
        .unwrap();
    let (img, info) = load_image(&p).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.has_alpha, info.is_float),
        (8, 4, true, false)
    );
    assert_eq!(
        img.at(3, 2),
        [180.0 / 255.0, 120.0 / 255.0, 7.0 / 255.0, 200.0 / 255.0]
    );
}

#[test]
fn png16_and_tga_and_tiff() {
    let d = tempfile::tempdir().unwrap();
    let p16 = d.path().join("b.png");
    ImageBuffer::from_fn(2, 2, |_, _| Rgb([65535u16, 0, 32768]))
        .save(&p16)
        .unwrap();
    let (img, info) = load_image(&p16).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.has_alpha),
        (16, 3, false)
    );
    assert_eq!(img.at(0, 0)[3], 1.0);
    assert!((img.at(1, 1)[2] - 32768.0 / 65535.0).abs() < 1e-6);

    let tga = d.path().join("c.tga");
    ImageBuffer::from_fn(2, 2, |_, _| Rgb([10u8, 20, 30]))
        .save(&tga)
        .unwrap();
    assert!(!load_image(&tga).unwrap().1.has_alpha);

    let tif = d.path().join("d.tif");
    ImageBuffer::from_fn(2, 2, |_, _| Luma([1000u16]))
        .save(&tif)
        .unwrap();
    let (img, info) = load_image(&tif).unwrap();
    assert_eq!((info.bit_depth, info.channels), (16, 1));
    assert_eq!(img.at(0, 0)[0], img.at(0, 0)[2]);
    assert!((img.at(0, 0)[0] - 1000.0 / 65535.0).abs() < 1e-6);
}

#[test]
fn rejects_over_16384_and_garbage() {
    let d = tempfile::tempdir().unwrap();
    let big = d.path().join("big.png");
    ImageBuffer::from_fn(16385, 1, |_, _| Luma([0u8]))
        .save(&big)
        .unwrap();
    assert!(matches!(
        load_image(&big),
        Err(LoadError::TooLarge { dim: 16385, .. })
    ));
    let bad = d.path().join("bad.png");
    std::fs::write(&bad, b"not a png").unwrap();
    assert!(matches!(load_image(&bad), Err(LoadError::Decode { .. })));
}

#[test]
fn luma_alpha_png_replicates_grey_and_keeps_alpha() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("la.png");
    ImageBuffer::from_fn(2, 2, |_, _| LumaA([100u8, 50]))
        .save(&p)
        .unwrap();
    let (img, info) = load_image(&p).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.has_alpha, info.is_float),
        (8, 2, true, false)
    );
    let g = 100.0 / 255.0;
    let px = img.at(1, 1);
    assert_eq!(px[0], px[1]);
    assert_eq!(px[1], px[2]);
    assert!((px[0] - g).abs() < 1e-6);
    assert!((px[3] - 50.0 / 255.0).abs() < 1e-6);
}

#[test]
fn accepts_exactly_16384_wide() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("w.png");
    ImageBuffer::from_fn(16384, 4, |_, _| Luma([9u8]))
        .save(&p)
        .unwrap();
    let (img, _) = load_image(&p).unwrap();
    assert_eq!((img.width, img.height), (16384, 4));
}

/// The `image` crate's default 512 MiB decode cap must not apply: a 12000x12000 RGBA8
/// image decodes to ~549 MiB (plus a ~2.1 GiB float buffer), so this is slow and heavy.
/// Run with `cargo test -p engine --test load --release -- --ignored`.
#[test]
#[ignore = "allocates several GiB"]
fn decodes_images_beyond_default_512_mib_limit() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("huge.png");
    ImageBuffer::from_fn(12000, 12000, |_, _| Rgba([10u8, 20, 30, 255]))
        .save(&p)
        .unwrap();
    let (img, info) = load_image(&p).unwrap();
    assert_eq!((img.width, img.height, info.bit_depth), (12000, 12000, 8));
    assert_eq!(img.at(11999, 11999)[1], 20.0 / 255.0);
}

#[test]
fn exr_and_hdr_load_as_float_rgb() {
    let d = tempfile::tempdir().unwrap();
    let img = ImageBuffer::from_fn(4, 4, |x, y| {
        Rgb([if (x, y) == (1, 1) { 2.5f32 } else { 0.25 }, 0.5, 0.75])
    });
    for (name, tol) in [("a.exr", 1e-6f32), ("a.hdr", 0.02)] {
        let p = d.path().join(name);
        image::DynamicImage::ImageRgb32F(img.clone())
            .save(&p)
            .unwrap();
        let (got, info) = load_image(&p).unwrap();
        assert_eq!(
            (info.bit_depth, info.channels, info.has_alpha, info.is_float),
            (32, 3, false, true),
            "{name}"
        );
        assert!(
            (got.at(1, 1)[0] - 2.5).abs() <= 2.5 * tol,
            "{name}: {:?}",
            got.at(1, 1)
        );
        assert_eq!(got.at(0, 0)[3], 1.0, "{name}");
    }
}

#[test]
fn exr_with_alpha_loads_rgba_float() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("a.exr");
    image::DynamicImage::ImageRgba32F(ImageBuffer::from_fn(2, 2, |_, _| {
        Rgba([4.0f32, 0.5, 0.25, 0.5])
    }))
    .save(&p)
    .unwrap();
    let (img, info) = load_image(&p).unwrap();
    assert_eq!(
        (info.channels, info.has_alpha, info.is_float),
        (4, true, true)
    );
    assert_eq!(img.at(1, 1), [4.0, 0.5, 0.25, 0.5]);
}

#[test]
fn bmp_loads_exactly_and_jpeg_within_its_loss() {
    let d = tempfile::tempdir().unwrap();
    let src = ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 90u8]));
    let bmp = d.path().join("a.bmp");
    src.save(&bmp).unwrap();
    let (img, info) = load_image(&bmp).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.has_alpha, info.is_float),
        (8, 3, false, false)
    );
    assert_eq!(
        img.at(3, 5),
        [48.0 / 255.0, 80.0 / 255.0, 90.0 / 255.0, 1.0]
    );
    let jpg = d.path().join("a.jpg");
    src.save(&jpg).unwrap();
    let (img, info) = load_image(&jpg).unwrap();
    assert_eq!(
        (info.bit_depth, info.channels, info.is_float),
        (8, 3, false)
    );
    for (x, y) in [(0, 0), (7, 9), (15, 15)] {
        let want = src.get_pixel(x, y).0;
        for (c, &w) in want.iter().enumerate() {
            let got = (img.at(x, y)[c] * 255.0).round() as i32;
            assert!(
                (got - w as i32).abs() <= 12,
                "({x},{y}) c{c}: {got} vs {}",
                w
            );
        }
    }
}

/// A minimal ICC header: size (big-endian) then the `acsp` signature at byte 36.
fn icc_blob() -> Vec<u8> {
    let mut v = vec![0u8; 132];
    v[..4].copy_from_slice(&132u32.to_be_bytes());
    v[36..40].copy_from_slice(b"acsp");
    v
}

#[test]
fn an_embedded_icc_profile_is_reported_and_ignored() {
    let d = tempfile::tempdir().unwrap();
    let px: Vec<u8> = (0..2 * 2 * 3).map(|i| (i * 20) as u8).collect();
    let with = d.path().join("icc.png");
    let mut f = std::fs::File::create(&with).unwrap();
    let mut enc = image::codecs::png::PngEncoder::new(&mut f);
    enc.set_icc_profile(icc_blob()).unwrap();
    enc.write_image(&px, 2, 2, ExtendedColorType::Rgb8).unwrap();
    drop(f);
    let without = d.path().join("plain.png");
    image::codecs::png::PngEncoder::new(std::fs::File::create(&without).unwrap())
        .write_image(&px, 2, 2, ExtendedColorType::Rgb8)
        .unwrap();
    let (a, _, meta) = load_image_meta(&with).unwrap();
    let (b, _, plain) = load_image_meta(&without).unwrap();
    assert!(meta.icc_profile && !plain.icc_profile);
    assert_eq!(a, b, "the profile changes no value");
    assert_eq!(meta.dds_dxgi, None);
}

/// Audit 2026-10-15 item 1 (DECISIONS AV1): a PNG's iCCP inflates under a bound, so a small file cannot make the
/// decoder allocate gigabytes before any size check. 96 MiB of zeros compresses to ~100 KB. The png crate drops a
/// profile over the bound and decodes on, so the image loads with no profile reported.
#[test]
fn an_iccp_bomb_is_dropped_not_inflated() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("bomb.png");
    let mut f = std::fs::File::create(&p).unwrap();
    let mut enc = image::codecs::png::PngEncoder::new(&mut f);
    let mut icc = icc_blob();
    icc.resize(96 << 20, 0);
    enc.set_icc_profile(icc).unwrap();
    enc.write_image(&[1, 2, 3], 1, 1, ExtendedColorType::Rgb8)
        .unwrap();
    drop(f);
    assert!(std::fs::metadata(&p).unwrap().len() < 1 << 20);
    assert_eq!(image_size(&p).unwrap(), (1, 1));
    let (img, _, meta) = load_image_meta(&p).unwrap();
    assert_eq!((img.width, img.height), (1, 1));
    assert!(!meta.icc_profile, "the profile was inflated past the bound");
}

/// The iCCP bound is not a cap on the image: a PNG whose decoded pixels outgrow it still loads.
#[test]
fn a_png_larger_than_the_metadata_bound_loads() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("big16.png");
    // 3000 x 3000 RGBA16 = 72 MiB of decoded pixels.
    ImageBuffer::from_fn(3000, 3000, |x, _| Rgba([x as u16, 7, 9, 65535]))
        .save(&p)
        .unwrap();
    let (img, info) = load_image(&p).unwrap();
    assert_eq!((img.width, img.height, info.bit_depth), (3000, 3000, 16));
}

#[test]
fn a_garbage_exr_is_a_decode_error_not_a_panic() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("bad.exr");
    std::fs::write(&p, [0x76, 0x2f, 0x31, 0x01, 2, 0, 0, 0, b'x', b'y']).unwrap();
    assert!(matches!(load_image(&p), Err(LoadError::Decode { .. })));
}

/// A one-level DX10 DDS with DXGI format `dxgi` and the given level bytes (tight rows).
fn dx10(w: u32, h: u32, dxgi: u32, pitch: u32, data: &[u8]) -> Vec<u8> {
    let mut hd = [0u32; 31];
    hd[0] = 124;
    hd[1] = 0x1 | 0x2 | 0x4 | 0x8 | 0x1000;
    hd[2] = h;
    hd[3] = w;
    hd[4] = pitch;
    hd[5] = 1;
    hd[6] = 1;
    hd[18] = 32;
    hd[19] = 0x4;
    hd[20] = u32::from_le_bytes(*b"DX10");
    hd[26] = 0x1000;
    let mut b = b"DDS ".to_vec();
    for v in hd {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for v in [dxgi, 3, 0, 1, 0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(data);
    b
}

/// Review Focus 2: every uncompressed layout of spec §1 loads mip 0 as float RGBA, with its DXGI in `SourceMeta`.
#[test]
fn third_party_uncompressed_layouts_load_as_float_rgba() {
    let d = tempfile::tempdir().unwrap();
    let half = |v: f32| half::f16::from_f32(v).to_le_bytes();
    let rgba16: Vec<u8> = [2.0f32, 0.5, 0.25, 0.75]
        .iter()
        .flat_map(|v| half(*v))
        .collect();
    let rgba32: Vec<u8> = [3.0f32, 0.5, 0.25, 0.5]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    type Case<'a> = (u32, u32, &'a [u8], [f32; 4], (u8, u8, bool, bool));
    let cases: [Case; 7] = [
        (
            28,
            4,
            &[10, 20, 30, 40],
            [10.0, 20.0, 30.0, 40.0].map(|v| v / 255.0),
            (8, 4, true, false),
        ),
        (
            87,
            4,
            &[30, 20, 10, 40],
            [10.0, 20.0, 30.0, 40.0].map(|v| v / 255.0),
            (8, 4, true, false),
        ),
        (
            88,
            4,
            &[30, 20, 10, 7],
            [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0],
            (8, 3, false, false),
        ),
        (61, 1, &[51], [0.2, 0.2, 0.2, 1.0], (8, 1, false, false)),
        (
            49,
            2,
            &[51, 102],
            [0.2, 0.4, 0.0, 1.0],
            (8, 2, false, false),
        ),
        (10, 8, &rgba16, [2.0, 0.5, 0.25, 0.75], (16, 4, true, true)),
        (2, 16, &rgba32, [3.0, 0.5, 0.25, 0.5], (32, 4, true, true)),
    ];
    for (dxgi, bpp, px, want, (bits, ch, alpha, float)) in cases {
        let p = d.path().join(format!("f{dxgi}.dds"));
        std::fs::write(&p, dx10(1, 1, dxgi, bpp, px)).unwrap();
        let (img, info, meta) = load_image_meta(&p).unwrap_or_else(|e| panic!("{dxgi}: {e}"));
        assert_eq!(
            (info.bit_depth, info.channels, info.has_alpha, info.is_float),
            (bits, ch, alpha, float),
            "{dxgi}"
        );
        for (c, (got, want)) in img.at(0, 0).into_iter().zip(want).enumerate() {
            assert!((got - want).abs() < 1e-3, "{dxgi} c{c}: {:?}", img.at(0, 0));
        }
        assert_eq!(meta.dds_dxgi, Some(dxgi));
    }
}

#[test]
fn a_cubemap_dds_is_a_decode_error() {
    let d = tempfile::tempdir().unwrap();
    let mut b = dx10(1, 1, 28, 4, &[0; 4 * 6]);
    b[136..140].copy_from_slice(&4u32.to_le_bytes()); // DX10 misc: TEXTURECUBE
    let p = d.path().join("cube.dds");
    std::fs::write(&p, b).unwrap();
    let e = load_image(&p).unwrap_err();
    assert!(e.to_string().contains("cubemap or array"), "{e}");
}

/// Fix wave (final review I3): the dimension limit is read from the decoder chosen by the file's content, the one
/// that allocates, not from the decoder its extension names. A 16385-wide PNG named `.bmp` is `TooLarge`.
#[test]
fn dimension_limit_follows_the_content_not_the_extension() {
    let d = tempfile::tempdir().unwrap();
    let png = d.path().join("wide.png");
    ImageBuffer::from_fn(16385, 1, |_, _| Luma([0u8]))
        .save(&png)
        .unwrap();
    let lying = d.path().join("wide.bmp");
    std::fs::rename(&png, &lying).unwrap();
    assert!(matches!(
        load_image(&lying),
        Err(LoadError::TooLarge { dim: 16385, .. })
    ));
}
