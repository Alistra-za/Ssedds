//! A cube drawn as a horizontal cross (cubemap GUI step, Note DT16): the six decoded faces, in the engine's own cell
//! order (`engine::cube::layout::HCROSS`, the layout the `hcross` import reads), so a screenshot re-imports correctly.
use engine::cube::layout::HCROSS;

/// The face names, in face order (+X -X +Y -Y +Z -Z), as drawn on the cross.
pub const FACE_LABELS: [&str; 6] = [
    "+X east",
    "\u{2212}X west",
    "+Y north",
    "\u{2212}Y south",
    "+Z up",
    "\u{2212}Z down",
];

/// The cross of six `size`-square faces: a `4*size` by `3*size` image, face `i` in cell `HCROSS[i]`, the empty cells
/// transparent black.
pub fn cross_image(faces: [&[[f32; 4]]; 6], size: u32) -> (u32, u32, Vec<[f32; 4]>) {
    let (s, w) = (size as usize, 4 * size as usize);
    let mut px = vec![[0.0; 4]; w * 3 * s];
    for (face, &(col, row)) in faces.iter().zip(&HCROSS) {
        for y in 0..s {
            let at = (row as usize * s + y) * w + col as usize * s;
            px[at..at + s].copy_from_slice(&face[y * s..(y + 1) * s]);
        }
    }
    (4 * size, 3 * size, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_puts_each_face_in_its_cell() {
        let size = 4u32;
        let colours: [[f32; 4]; 6] =
            std::array::from_fn(|i| [i as f32 / 8.0 + 0.1, 0.5, 0.25, 1.0]);
        let faces: Vec<Vec<[f32; 4]>> = colours
            .iter()
            .map(|c| vec![*c; (size * size) as usize])
            .collect();
        let refs: [&[[f32; 4]]; 6] = std::array::from_fn(|i| faces[i].as_slice());
        let (w, h, px) = cross_image(refs, size);
        assert_eq!((w, h), (16, 12));
        let centre = |col: u32, row: u32| px[((row * size + 2) * w + col * size + 2) as usize];
        for (i, &(col, row)) in HCROSS.iter().enumerate() {
            assert_eq!(centre(col, row), colours[i], "face {i}");
        }
        for (col, row) in [(0, 0), (2, 0), (3, 0), (0, 2), (2, 2), (3, 2)] {
            assert_eq!(centre(col, row), [0.0; 4], "cell ({col}, {row}) is empty");
        }
    }

    #[test]
    fn a_face_keeps_its_own_orientation() {
        // One face whose texel value is its index: copied row by row, not transposed.
        let size = 3u32;
        let ramp: Vec<[f32; 4]> = (0..9).map(|i| [i as f32, 0.0, 0.0, 1.0]).collect();
        let blank = vec![[0.0; 4]; 9];
        let refs: [&[[f32; 4]]; 6] = std::array::from_fn(|i| {
            if i == 0 {
                ramp.as_slice()
            } else {
                blank.as_slice()
            }
        });
        let (w, _, px) = cross_image(refs, size);
        let (col, row) = HCROSS[0];
        for y in 0..3u32 {
            for x in 0..3u32 {
                assert_eq!(
                    px[((row * size + y) * w + col * size + x) as usize][0],
                    (y * 3 + x) as f32
                );
            }
        }
    }
}
