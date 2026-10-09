use engine::load::*;
use image::{ImageBuffer, Luma, Rgb, Rgba};

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
    assert_eq!((info.bit_depth, info.has_alpha), (16, false));
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
    assert_eq!(info.bit_depth, 16);
    assert_eq!(img.at(0, 0)[0], img.at(0, 0)[2]);
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
