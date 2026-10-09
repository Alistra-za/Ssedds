use engine::load::*;
use image::{ImageBuffer, Luma, LumaA, Rgb, Rgba};

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
