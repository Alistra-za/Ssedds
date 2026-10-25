//! The host rules every plugin shares: the size limit, "can this DDS be opened", Open's and Save's alpha rules and
//! the largest DDS file. Pure functions; none starts the engine's thread pool, so none needs `pin_self`.

/// The largest side Skyrim (D3D11) loads.
const MAX_SIDE: u32 = 16384;
const TOO_LARGE: &str =
    "This image is larger than 16384 pixels on a side, the most Skyrim supports.";

/// The one size rule (DC12): the pixel count of a `w` x `h` image the library will encode or open. A zero side or a
/// side over 16384 is refused with the text a host shows as is. The `isize` bound is `from_raw_parts`' precondition
/// for the 16-byte pixels of `psdds_encode` (already implied by the side limit on 64-bit).
pub fn check_size(w: u32, h: u32) -> Result<usize, String> {
    if w == 0 || h == 0 {
        return Err("This image is empty".into());
    }
    (w <= MAX_SIDE && h <= MAX_SIDE)
        .then(|| (w as usize).checked_mul(h as usize))
        .flatten()
        .filter(|n| n.checked_mul(16).is_some_and(|b| b <= isize::MAX as usize))
        .ok_or_else(|| TOO_LARGE.into())
}

fn le32(h: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]])
}

/// True when the first bytes of a file (magic, header, DX10 header; up to 148) show a DDS `psdds_decode` can open.
/// No magic, BC6H or float DXGI formats, float D3DFMT FourCCs 111-117, cube maps and arrays are not. A file shorter
/// than the headers is openable, so the reader can say why it fails.
pub fn dds_openable(head: &[u8]) -> bool {
    openable(head, false)
}

/// [`dds_openable`] for `psdds_decode_depth` (DE4): also true for the float formats `read_dds` maps to RGBA32F,
/// RGBA16F and BC6H: DXGI 2, 10, 95, 96 and D3DFMT 113, 116. Other float formats stay false.
pub fn dds_openable_float(head: &[u8]) -> bool {
    openable(head, true)
}

fn openable(head: &[u8], float: bool) -> bool {
    if !head.starts_with(b"DDS ") {
        return false;
    }
    if head.len() < 128 {
        return true;
    }
    if le32(head, 112) & 0x200 != 0 {
        return false; // caps2 DDSCAPS2_CUBEMAP
    }
    let fourcc = le32(head, 84);
    if (111..=117).contains(&fourcc) && !(float && matches!(fourcc, 113 | 116)) {
        return false; // D3DFMT float formats (R16F .. A32B32G32R32F)
    }
    if fourcc != 0x3031_5844 /* "DX10" */ || head.len() < 148 {
        return true;
    }
    // DXGI float formats and BC6H: R32G32B32A32 2, R32G32B32 6, R16G16B16A16 10, R32G32 16, R11G11B10 26,
    // R16G16 34, R32 41, R16 54, R9G9B9E5 67, BC6H typeless/UF16/SF16 94..96.
    if matches!(
        le32(head, 128),
        2 | 6 | 10 | 16 | 26 | 34 | 41 | 54 | 67 | 94..=96
    ) && !(float && matches!(le32(head, 128), 2 | 10 | 95 | 96))
    {
        return false;
    }
    if le32(head, 136) & 0x4 != 0 {
        return false; // miscFlag D3D11_RESOURCE_MISC_TEXTURECUBE
    }
    le32(head, 140) <= 1 // arraySize
}

/// Open's alpha rule (DC23): transparency only when the file stores alpha and some alpha byte (byte 3 of each
/// 4-byte pixel, RGBA8 or BGRA8) is below 254.
pub fn opens_with_alpha(px8: &[u8], has_alpha: bool) -> bool {
    has_alpha && px8.as_chunks::<4>().0.iter().any(|p| p[3] < 254)
}

/// Save's alpha rule (DC31): some alpha is below 254/255. The limit sits just under, so the float of byte 254
/// (254 * (1/255)) never counts as transparent through rounding. NaN does not count.
pub fn alpha_used(rgba: &[[f32; 4]]) -> bool {
    let limit = 254.0f32 / 255.0 - 1e-6;
    rgba.iter().any(|p| p[3] < limit)
}

/// The largest DDS file `psdds_decode` accepts by policy: 16384^2 RGBA8 with a full chain, plus magic, header and
/// DX10 header.
pub const MAX_DDS_FILE_BYTES: u64 = {
    let mut bytes = 4 + 124 + 20;
    let mut side = MAX_SIDE as u64;
    while side >= 1 {
        bytes += side * side * 4;
        side /= 2;
    }
    bytes
};
