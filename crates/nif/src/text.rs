//! NIF strings are bytes in the author's code page; Skyrim tools write Windows-1252. Decode lossily so a string
//! can never fail a file (plan 06 §2).
pub fn lossy(b: &[u8]) -> String {
    encoding_rs::WINDOWS_1252
        .decode_without_bom_handling(b)
        .0
        .into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn cp1252_lossy() {
        assert_eq!(super::lossy(b"abc"), "abc");
        assert_eq!(super::lossy(&[0x93, b'x', 0x94]), "\u{201C}x\u{201D}");
        assert_eq!(super::lossy(&[0x0c, 0x90, b')']), "\u{000C}\u{0090})");
    }
}
