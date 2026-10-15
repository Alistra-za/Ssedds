//! Post-encode fallback for exact opaque alpha (spec §5.6): only valid on blocks whose source alpha is 255 throughout.

/// If `block` is BC7 mode 6, set both alpha endpoints to 127 and both p-bits to 1, so alpha decodes to 255.
/// RGB endpoints with p-bit 0 move up by one LSB (unoptimised). Returns true if the block was rewritten.
pub fn force_opaque_mode6(block: &mut [u8; 16]) -> bool {
    if block[0] & 0x7f != 0x40 {
        return false;
    }
    let mut v = u128::from_le_bytes(*block);
    let alpha_mask: u128 = ((1u128 << 14) - 1) << 49;
    v = (v & !alpha_mask) | (127u128 << 49) | (127u128 << 56);
    v |= (1u128 << 63) | (1u128 << 64);
    *block = v.to_le_bytes();
    true
}

/// If `block` is BC7 mode 4 or 5 with rotation 1-3, set both endpoints of the colour channel that
/// decodes as alpha to all-ones, so alpha decodes to 255. Lossless for RGB: the decoder swaps that
/// channel with the stored alpha field after decoding, so no output RGB value reads the patched bits.
/// Equal endpoints interpolate to a constant, so the indices and the mode-4 index-selection bit do
/// not matter. Returns true if the block is one of those modes (the patch is then applied).
///
/// Layout (bit 0 = LSB of byte 0): mode bits (mode 4: `10000`, bits 0-4; mode 5: `100000`, bits 0-5),
/// rotation (2 bits), mode 4 only: index-selection bit, then one subset of endpoints in the order
/// R0 R1 G0 G1 B0 B1 A0 A1 from bit 8 (colour 5 bits / alpha 6 in mode 4, colour 7 / alpha 8 in
/// mode 5; no p-bits). Rotation r swaps alpha with channel r-1 (1 = R, 2 = G, 3 = B).
pub fn force_opaque_rotated(block: &mut [u8; 16]) -> bool {
    let mut v = u128::from_le_bytes(*block);
    let (rot, bits) = if block[0] & 0x1f == 0x10 {
        ((v >> 5) & 3, 5)
    } else if block[0] & 0x3f == 0x20 {
        ((v >> 6) & 3, 7)
    } else {
        return false;
    };
    if rot == 0 {
        return false;
    }
    let pair_bits = 2 * bits;
    v |= ((1u128 << pair_bits) - 1) << (8 + pair_bits * (rot as u32 - 1));
    *block = v.to_le_bytes();
    true
}

/// Exact alpha 0 (DECISIONS AH1): only valid on blocks whose source alpha is 0 throughout. Sets both alpha endpoints
/// of every subset to 0 (and, in modes 6 and 7, the p-bits they share with RGB to 0), so alpha decodes to exactly 0.
/// RGB endpoints that had p-bit 1 move down (unoptimised): one LSB in mode 6 (7-bit + p, 1/255), one step of the
/// 6-bit value in mode 7 (5-bit + p, about 4/255). Modes 4 and 5 are lossless for RGB. Modes 0-3 store no alpha (it decodes as 255) and are left alone. Returns true if the block was rewritten.
///
/// Layouts (bit 0 = LSB of byte 0): mode 4 alpha 6-bit x2 at 38, colour 5-bit; mode 5 alpha 8-bit x2 at 50, colour
/// 7-bit; with rotation r the alpha sits in colour pair r-1 from bit 8. Mode 6 alpha 7-bit x2 at 49, p-bits 63, 64.
/// Mode 7 (two subsets) alpha 5-bit x4 at 74, p-bits 94..98.
pub fn force_zero_alpha(block: &mut [u8; 16]) -> bool {
    let mut v = u128::from_le_bytes(*block);
    let field = |at: u32, len: u32| ((1u128 << len) - 1) << at;
    let mask = if block[0] & 0x1f == 0x10 {
        match (v >> 5) & 3 {
            0 => field(38, 12),
            r => field(8 + 10 * (r as u32 - 1), 10),
        }
    } else if block[0] & 0x3f == 0x20 {
        match (v >> 6) & 3 {
            0 => field(50, 16),
            r => field(8 + 14 * (r as u32 - 1), 14),
        }
    } else if block[0] & 0x7f == 0x40 {
        field(49, 16)
    } else if block[0] == 0x80 {
        field(74, 24)
    } else {
        return false;
    };
    v &= !mask;
    *block = v.to_le_bytes();
    true
}
