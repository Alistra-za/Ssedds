//! Shared cube-test fixtures: the arrow cube and the raw layouts built from it with literal cell offsets (not
//! `layout::cells`, so a wrong table cannot agree with itself).
// Each test binary uses a different part of this module.
#![allow(dead_code)]
use engine::analyse::code8;
use engine::image::Rgba32F;
use image::DynamicImage;

/// `img` as an 8-bit PNG-ready image: RGB when every alpha is 1, else RGBA.
pub fn png8(img: &Rgba32F) -> DynamicImage {
    let rgba = image::RgbaImage::from_fn(img.width, img.height, |x, y| {
        image::Rgba(img.at(x, y).map(code8))
    });
    if img.px.iter().all(|p| p[3] == 1.0) {
        DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(rgba).to_rgb8())
    } else {
        DynamicImage::ImageRgba8(rgba)
    }
}

/// The 8-bit texels of `img`, as `png8` stores them (alpha 255 where it is 1).
pub fn texels8(img: &Rgba32F) -> Vec<[u8; 4]> {
    img.px.iter().map(|p| p.map(code8)).collect()
}

const GREY: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

/// Six `n` x `n` faces (+X red, -X cyan, +Y green, -Y magenta, +Z blue, -Z yellow), a white band on rows `0..n/8`
/// and a black band on columns `0..n/8`.
pub fn arrow_faces(n: u32) -> [Rgba32F; 6] {
    let colours: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
    ];
    colours.map(|c| {
        let mut f = Rgba32F::new(n, n, [c[0], c[1], c[2], 1.0]);
        for y in 0..n {
            for x in 0..n {
                if y < n / 8 {
                    *f.at_mut(x, y) = [1.0; 4];
                } else if x < n / 8 {
                    *f.at_mut(x, y) = [0.0, 0.0, 0.0, 1.0];
                }
            }
        }
        f
    })
}

fn paste(img: &mut Rgba32F, face: &Rgba32F, col: u32, row: u32) {
    let n = face.width;
    for y in 0..n {
        for x in 0..n {
            *img.at_mut(col * n + x, row * n + y) = face.at(x, y);
        }
    }
}

pub fn turn180(f: &Rgba32F) -> Rgba32F {
    let n = f.width;
    let mut o = f.clone();
    for y in 0..n {
        for x in 0..n {
            *o.at_mut(x, y) = f.at(n - 1 - x, n - 1 - y);
        }
    }
    o
}

pub fn hcross(faces: &[Rgba32F; 6]) -> Rgba32F {
    let n = faces[0].width;
    let mut img = Rgba32F::new(4 * n, 3 * n, GREY);
    for (f, (c, r)) in faces
        .iter()
        .zip([(2, 1), (0, 1), (1, 0), (1, 2), (1, 1), (3, 1)])
    {
        paste(&mut img, f, c, r);
    }
    img
}

/// A vertical cross; `negz_rotated` stores -Z turned 180 degrees, as most tools write it.
pub fn vcross(faces: &[Rgba32F; 6], negz_rotated: bool) -> Rgba32F {
    let n = faces[0].width;
    let mut img = Rgba32F::new(3 * n, 4 * n, GREY);
    for (k, (f, (c, r))) in faces
        .iter()
        .zip([(2, 1), (0, 1), (1, 0), (1, 2), (1, 1), (1, 3)])
        .enumerate()
    {
        let f = if k == 5 && negz_rotated {
            turn180(f)
        } else {
            f.clone()
        };
        paste(&mut img, &f, c, r);
    }
    img
}

pub fn hstrip(faces: &[Rgba32F; 6]) -> Rgba32F {
    let n = faces[0].width;
    let mut img = Rgba32F::new(6 * n, n, GREY);
    for (k, f) in faces.iter().enumerate() {
        paste(&mut img, f, k as u32, 0);
    }
    img
}

pub fn vstrip(faces: &[Rgba32F; 6]) -> Rgba32F {
    let n = faces[0].width;
    let mut img = Rgba32F::new(n, 6 * n, GREY);
    for (k, f) in faces.iter().enumerate() {
        paste(&mut img, f, 0, k as u32);
    }
    img
}
