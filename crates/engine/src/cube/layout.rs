//! Raw cubemap source layouts: crosses and strips in one image (cubemap support v1, DECISIONS DN7).
//! Face order is +X -X +Y -Y +Z -Z; the cross cell offsets are texassemble's.
use crate::config::{CubeLayout, CubemapSpec, FaceFlip, Profile};
use crate::image::Rgba32F;

/// (col, row) of each face's cell, +X..-Z. Hcross is 4x3 cells, Vcross 3x4.
const HCROSS: [(u32, u32); 6] = [(2, 1), (0, 1), (1, 0), (1, 2), (1, 1), (3, 1)];
const VCROSS: [(u32, u32); 6] = [(2, 1), (0, 1), (1, 0), (1, 2), (1, 1), (1, 3)];

/// The layout an image's exact aspect ratio says it has; square and anything else `None`.
pub fn detect(w: u32, h: u32) -> Option<CubeLayout> {
    let (w, h) = (u64::from(w), u64::from(h));
    if h == 0 || w == 0 {
        None
    } else if w * 3 == h * 4 {
        Some(CubeLayout::Hcross)
    } else if w * 4 == h * 3 {
        Some(CubeLayout::Vcross)
    } else if w == 6 * h {
        Some(CubeLayout::Hstrip)
    } else if h == 6 * w {
        Some(CubeLayout::Vstrip)
    } else {
        None
    }
}

/// Cell edge and each face's cell for a `w` x `h` image read as `layout`; `None` when the size is not that layout
/// (or the layout is `Faces`, which is six files, not one image).
pub fn cells(layout: CubeLayout, w: u32, h: u32) -> Option<(u32, [(u32, u32); 6])> {
    if w == 0 || h == 0 || detect(w, h) != Some(layout) {
        return None;
    }
    Some(match layout {
        CubeLayout::Hcross => (w / 4, HCROSS),
        CubeLayout::Vcross => (w / 3, VCROSS),
        CubeLayout::Hstrip => (h, [0, 1, 2, 3, 4, 5].map(|k| (k, 0))),
        CubeLayout::Vstrip => (w, [0, 1, 2, 3, 4, 5].map(|k| (0, k))),
        CubeLayout::Faces => return None,
    })
}

fn crop(img: &Rgba32F, x0: u32, y0: u32, n: u32) -> Rgba32F {
    let mut out = Rgba32F::new(n, n, [0.0; 4]);
    for y in 0..n {
        for x in 0..n {
            *out.at_mut(x, y) = img.at(x0 + x, y0 + y);
        }
    }
    out
}

/// `src` turned clockwise by `deg` (0, 90, 180, 270).
fn rotate(src: &Rgba32F, deg: u16) -> Rgba32F {
    let n = src.width;
    let mut out = Rgba32F::new(n, n, [0.0; 4]);
    for y in 0..n {
        for x in 0..n {
            let (sx, sy) = match deg {
                90 => (y, n - 1 - x),
                180 => (n - 1 - x, n - 1 - y),
                270 => (n - 1 - y, x),
                _ => (x, y),
            };
            *out.at_mut(x, y) = src.at(sx, sy);
        }
    }
    out
}

fn flip(src: &Rgba32F, how: FaceFlip) -> Rgba32F {
    let n = src.width;
    let mut out = src.clone();
    for y in 0..n {
        for x in 0..n {
            let (sx, sy) = match how {
                FaceFlip::H => (n - 1 - x, y),
                FaceFlip::V => (x, n - 1 - y),
                FaceFlip::None => (x, y),
            };
            *out.at_mut(x, y) = src.at(sx, sy);
        }
    }
    out
}

/// True when every channel of the `n` x `n` cell at (`x0`, `y0`) varies by at most 2/255 (an unused cross corner).
fn flat(img: &Rgba32F, x0: u32, y0: u32, n: u32) -> bool {
    let (mut lo, mut hi) = ([f32::MAX; 4], [f32::MIN; 4]);
    for y in y0..y0 + n {
        for x in x0..x0 + n {
            for (c, v) in img.at(x, y).into_iter().enumerate() {
                lo[c] = lo[c].min(v);
                hi[c] = hi[c].max(v);
            }
        }
    }
    (0..4).all(|c| hi[c] - lo[c] <= 2.0 / 255.0)
}

/// Cut the six faces out of a cross or strip image. `auto` (the layout was guessed from the aspect ratio) refuses a
/// cross whose unused cells are not flat: a 4:3 photo is not a cubemap.
pub fn import_raw(
    img: &Rgba32F,
    layout: CubeLayout,
    spec: &CubemapSpec,
    auto: bool,
) -> Result<[Rgba32F; 6], String> {
    let (w, h) = (img.width, img.height);
    let Some((n, at)) = cells(layout, w, h) else {
        return Err(if layout == CubeLayout::Faces {
            "cubemap.layout faces is six separate files, not one image".to_string()
        } else {
            format!("{w}x{h} is not a {layout:?} layout: set cubemap.layout for this file")
        });
    };
    if auto && matches!(layout, CubeLayout::Hcross | CubeLayout::Vcross) {
        let (cols, rows) = if layout == CubeLayout::Hcross {
            (4, 3)
        } else {
            (3, 4)
        };
        let unused = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| (c, r)))
            .filter(|cell| !at.contains(cell));
        if unused.into_iter().any(|(c, r)| !flat(img, c * n, r * n, n)) {
            return Err(format!(
                "{w}x{h} looks like a picture, not a cross: set cubemap.layout for this file if it is one"
            ));
        }
    }
    Ok(std::array::from_fn(|k| {
        let (c, r) = at[k];
        let mut face = crop(img, c * n, r * n, n);
        if layout == CubeLayout::Vcross && k == 5 && spec.vcross_negz_rotate {
            face = rotate(&face, 180);
        }
        if spec.face_rotate[k] != 0 {
            face = rotate(&face, spec.face_rotate[k]);
        }
        flip(&face, spec.face_flip[k])
    }))
}

/// The face edge written: `cubemap.face_size`, else the largest power of two within the source cell. `src` is the
/// source image size (one face's size for `Faces`).
pub fn face_size(p: &Profile, layout: CubeLayout, src: (u32, u32)) -> u32 {
    p.cubemap.face_size.unwrap_or_else(|| {
        let cell = cells(layout, src.0, src.1).map_or(src.0.min(src.1), |c| c.0);
        1 << cell.max(1).ilog2()
    })
}

/// A trailing face tag (`_posx` .. `_negz`, `_px` .. `_nz`, ASCII case-insensitive): the stem without it and the
/// face index. Task 7 moves this to `cube::plan`.
pub fn face_tag(stem: &str) -> Option<(&str, usize)> {
    const TAGS: [(&str, &str); 6] = [
        ("_posx", "_px"),
        ("_negx", "_nx"),
        ("_posy", "_py"),
        ("_negy", "_ny"),
        ("_posz", "_pz"),
        ("_negz", "_nz"),
    ];
    let lower = stem.to_ascii_lowercase();
    TAGS.iter().enumerate().find_map(|(k, (long, short))| {
        [long, short]
            .into_iter()
            .find(|t| lower.len() > t.len() && lower.ends_with(*t))
            .map(|t| (&stem[..stem.len() - t.len()], k))
    })
}
