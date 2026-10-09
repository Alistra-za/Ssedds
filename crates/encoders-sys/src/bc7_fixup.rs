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
