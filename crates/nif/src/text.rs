//! NIF strings are bytes in the author's code page; Skyrim tools write Windows-1252. Decode lossily so a string
//! can never fail a file (plan 06 §2).
pub fn lossy(b: &[u8]) -> String {
    encoding_rs::WINDOWS_1252
        .decode_without_bom_handling(b)
        .0
        .into_owned()
}

/// Most bytes of decoded text one display line holds: shared strings are cheap to keep but not to decode, and a
/// hostile file can name one large string many times (trace 2026-10-28, Note GA1).
pub const DISPLAY_CAP: usize = 64 * 1024;

/// `pieces` decoded and joined, stopping at `cap` bytes with "…" appended. Pieces past the cap are never decoded.
pub fn lossy_capped<I>(pieces: I, cap: usize) -> String
where
    I: IntoIterator,
    I::Item: AsRef<[u8]>,
{
    let mut out = String::new();
    for p in pieces {
        let p = p.as_ref();
        let left = cap - out.len();
        // one byte decodes to at least one byte, so the first `left` bytes always cover what fits
        let more = lossy(&p[..p.len().min(left)]);
        if p.len() <= left && more.len() <= left {
            out.push_str(&more);
            continue;
        }
        let mut end = left.min(more.len());
        while !more.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&more[..end]);
        out.push('…');
        break;
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn lossy_capped_stops_at_the_cap() {
        use super::lossy_capped;
        assert_eq!(lossy_capped(["ab", "cd"], 4), "abcd");
        assert_eq!(lossy_capped(["ab", "cde", "f"], 4), "abcd…");
        // 0x80 decodes to three bytes: a char that would cross the cap is left out
        assert_eq!(lossy_capped([&b"a"[..], &[0x80, 0x80]], 5), "a€…");
        assert_eq!(lossy_capped(["abcd", ""], 4), "abcd");
    }

    #[test]
    fn cp1252_lossy() {
        assert_eq!(super::lossy(b"abc"), "abc");
        assert_eq!(super::lossy(&[0x93, b'x', 0x94]), "\u{201C}x\u{201D}");
        assert_eq!(super::lossy(&[0x0c, 0x90, b')']), "\u{000C}\u{0090})");
    }
}
