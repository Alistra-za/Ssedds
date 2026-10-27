//! The CS/ENB dynamic-cubemap marker: a 1x1 six-face RGBA8 cube DDS (152 bytes) whose texel carries the F0 colour
//! in RGB and the roughness in alpha. Only `sdc cubemap-marker` writes a 1x1 cube.
use crate::config::Format;
use crate::dds::{header_bytes_with, may_replace, read_dds, write_atomic, DdsDesc, ResolvedHeader};
use std::path::Path;

/// Header plus six BGRA texels (face 0 carries the colour; the other five are zero).
pub fn marker_bytes(rgb: [u8; 3], alpha: u8) -> Vec<u8> {
    let desc = DdsDesc {
        width: 1,
        height: 1,
        format: Format::Rgba8,
        srgb: false,
        header: ResolvedHeader::Legacy,
    };
    let mut b = header_bytes_with(&desc, 1, true).expect("a 1x1 RGBA8 legacy cube header is valid");
    b.extend_from_slice(&[rgb[2], rgb[1], rgb[0], alpha]);
    b.resize(b.len() + 5 * 4, 0);
    b
}

/// Alpha for a roughness: `None` is 255 (CS's defaults, the ParallaxGen marker); `Some(r)` is
/// `min(round(r * 255), 254)` so a given roughness never reads as the default.
pub fn alpha_of(roughness: Option<f64>) -> Result<u8, String> {
    match roughness {
        None => Ok(255),
        Some(r) if (0.0..=1.0).contains(&r) => Ok(((r * 255.0).round() as u32).min(254) as u8),
        Some(r) => Err(format!("--roughness {r}: must be between 0 and 1")),
    }
}

/// Writes the marker, keeping a file Tessalith did not write; the error text is the reason.
pub fn write_marker(path: &Path, rgb: [u8; 3], alpha: u8) -> Result<(), String> {
    let bytes = marker_bytes(rgb, alpha);
    may_replace(path, &bytes, |b| {
        b.len() == 152 && read_dds(b).is_ok_and(|f| f.is_cubemap && f.width == 1)
    })
    .map_err(|k| k.note(path))?;
    write_atomic(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn marker_is_exactly_152_bytes() {
        let b = marker_bytes([255, 197, 78], 26);
        assert_eq!(b.len(), 152);
        assert_eq!(u(&b, 8), 0x100F);
        assert_eq!((u(&b, 12), u(&b, 16), u(&b, 20), u(&b, 28)), (1, 1, 4, 1));
        assert_eq!((u(&b, 80), u(&b, 88)), (0x41, 32));
        assert_eq!(
            [u(&b, 92), u(&b, 96), u(&b, 100), u(&b, 104)],
            [0xff0000, 0xff00, 0xff, 0xff000000]
        );
        assert_eq!((u(&b, 108), u(&b, 112)), (0x1008, 0xFE00));
        assert_eq!(&b[128..132], &[78, 197, 255, 26]);
        assert!(b[132..].iter().all(|&x| x == 0));
    }

    #[test]
    fn roughness_mapping() {
        assert_eq!(alpha_of(None), Ok(255));
        assert_eq!(alpha_of(Some(0.0)), Ok(0));
        assert_eq!(alpha_of(Some(1.0)), Ok(254));
        assert_eq!(alpha_of(Some(0.1)), Ok(26));
        assert!(alpha_of(Some(1.5)).is_err());
    }
}
