//! Photoshop `.psd` / `.psb` input: the merged (composite) image Photoshop stores after the layers. Layers are never
//! read or composited; the file reads as Photoshop last flattened it on save.
//!
//! Supported: RGB and Grayscale documents, 8, 16 and 32 bits per channel, raw or RLE (PackBits) image data. RGB
//! reads channels 0–2 as colour and channel 3, when present, as alpha (Grayscale: 0 and 1): that is the document's
//! transparency, or its first alpha channel when it has a Background layer, as the Photoshop DDS plugins read it.
//! Merged transparency (a negative layer count, or the `Mtrn`/`Mt16`/`Mt32` blocks) means Photoshop matted the colour against white, so the
//! colour is un-matted (`(c - (1 - a)) / a`, kept as stored where a = 0). A file saved without a real composite
//! (Maximize Compatibility off) is refused. Embedded ICC profiles are detected and ignored (DECISIONS S8).
use crate::image::{Rgba32F, SourceInfo};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub(crate) enum PsdError {
    TooLarge(u32),
    Bad(String),
}

impl From<std::io::Error> for PsdError {
    fn from(e: std::io::Error) -> Self {
        PsdError::Bad(if e.kind() == std::io::ErrorKind::UnexpectedEof {
            "truncated PSD".into()
        } else {
            e.to_string()
        })
    }
}

fn bad<T>(msg: impl Into<String>) -> Result<T, PsdError> {
    Err(PsdError::Bad(msg.into()))
}

/// True when `path` starts with the PSD/PSB signature. `false` when it cannot be read.
pub(crate) fn sniff(path: &Path) -> bool {
    let mut b = [0u8; 4];
    File::open(path)
        .and_then(|mut f| f.read_exact(&mut b))
        .is_ok_and(|()| &b == b"8BPS")
}

struct Header {
    /// PSB (version 2): 64-bit section lengths and 32-bit RLE row counts.
    big: bool,
    channels: u16,
    width: u32,
    height: u32,
    depth: u16,
    mode: u16,
}

struct In(BufReader<File>);

impl In {
    fn u8(&mut self) -> Result<u8, PsdError> {
        let mut b = [0u8; 1];
        self.0.read_exact(&mut b)?;
        Ok(b[0])
    }
    fn u16(&mut self) -> Result<u16, PsdError> {
        let mut b = [0u8; 2];
        self.0.read_exact(&mut b)?;
        Ok(u16::from_be_bytes(b))
    }
    fn u32(&mut self) -> Result<u32, PsdError> {
        let mut b = [0u8; 4];
        self.0.read_exact(&mut b)?;
        Ok(u32::from_be_bytes(b))
    }
    fn u64(&mut self) -> Result<u64, PsdError> {
        let mut b = [0u8; 8];
        self.0.read_exact(&mut b)?;
        Ok(u64::from_be_bytes(b))
    }
    fn tag(&mut self) -> Result<[u8; 4], PsdError> {
        let mut b = [0u8; 4];
        self.0.read_exact(&mut b)?;
        Ok(b)
    }
    /// A section length: 32-bit, or 64-bit in a PSB where `long`.
    fn len(&mut self, long: bool) -> Result<u64, PsdError> {
        if long {
            self.u64()
        } else {
            self.u32().map(u64::from)
        }
    }
    fn pos(&mut self) -> Result<u64, PsdError> {
        Ok(self.0.stream_position()?)
    }
    /// Skip `n` bytes forward.
    fn skip(&mut self, n: u64) -> Result<(), PsdError> {
        let to = self.pos()?.saturating_add(n);
        self.seek(to)
    }
    fn seek(&mut self, to: u64) -> Result<(), PsdError> {
        self.0.seek(SeekFrom::Start(to))?;
        Ok(())
    }
}

fn header(r: &mut In) -> Result<Header, PsdError> {
    if &r.tag()? != b"8BPS" {
        return bad("not a PSD file");
    }
    let big = match r.u16()? {
        1 => false,
        2 => true,
        v => return bad(format!("unknown PSD version {v}")),
    };
    r.0.seek_relative(6)?;
    let channels = r.u16()?;
    let height = r.u32()?;
    let width = r.u32()?;
    let depth = r.u16()?;
    let mode = r.u16()?;
    if channels == 0 || width == 0 || height == 0 {
        return bad("empty PSD (no channels or a zero size)");
    }
    Ok(Header {
        big,
        channels,
        width,
        height,
        depth,
        mode,
    })
}

/// Width and height from the header alone. Unchecked against MAX_DIM.
pub(crate) fn size(path: &Path) -> Result<(u32, u32), PsdError> {
    let h = header(&mut In(BufReader::new(File::open(path)?)))?;
    Ok((h.width, h.height))
}

/// What the image resources say: an ICC profile (1039), and whether the composite is real (1057, Version Info:
/// `hasRealMergedData`; absent means real, as writers other than Photoshop omit it).
fn resources(r: &mut In, end: u64) -> Result<(bool, bool), PsdError> {
    let (mut icc, mut merged) = (false, true);
    while r.pos()? + 12 <= end {
        r.tag()?; // "8BIM", or another vendor's signature: the layout is the same.
        let id = r.u16()?;
        let name = u64::from(r.u8()?);
        r.0.seek_relative(((name + 1 + 1) & !1) as i64 - 1)?;
        let size = u64::from(r.u32()?);
        let data = r.pos()?;
        match id {
            1039 => icc = size > 0,
            1057 if size >= 5 => {
                r.u32()?;
                merged = r.u8()? != 0;
            }
            _ => {}
        }
        r.seek(data + ((size + 1) & !1))?;
    }
    Ok((icc, merged))
}

/// A negative layer count (in the layer info, or at the start of an `Lr16`/`Lr32` block in a 16/32-bit document)
/// means "the first alpha channel is the merged result's transparency".
fn negative_count(r: &mut In, len: u64) -> Result<bool, PsdError> {
    Ok(len >= 2 && (r.u16()? as i16) < 0)
}

/// Whether the merged image has transparency: a negative layer count, or the `Mtrn`, `Mt16` or `Mt32` global
/// tagged block after the layer records.
/// A block that does not parse ends the walk with what was found so far: the layers are never needed otherwise.
fn merged_transparency(r: &mut In, h: &Header, end: u64) -> Result<bool, PsdError> {
    // Keys with a 64-bit length in a PSB.
    const LONG: [&[u8; 4]; 13] = [
        b"LMsk", b"Lr16", b"Lr32", b"Layr", b"Mt16", b"Mt32", b"Mtrn", b"Alph", b"FMsk", b"lnk2",
        b"FEid", b"FXid", b"PxSD",
    ];
    if r.pos()? + 4 > end {
        return Ok(false);
    }
    let layers = r.len(h.big)?;
    let start = r.pos()?;
    let mut found = negative_count(r, layers)?;
    r.seek(start.saturating_add(layers))?;
    if r.pos()? + 4 > end {
        return Ok(false);
    }
    let mask = u64::from(r.u32()?);
    r.skip(mask)?;
    while r.pos()? + 12 <= end {
        // Block lengths are padded to 2 or 4 bytes depending on the writer: skip up to 3 bytes of padding.
        let mut sig = r.tag()?;
        for _ in 0..3 {
            if &sig == b"8BIM" || &sig == b"8B64" || r.pos()? + 8 > end {
                break;
            }
            sig = [sig[1], sig[2], sig[3], r.u8()?];
        }
        if &sig != b"8BIM" && &sig != b"8B64" {
            break;
        }
        let key = r.tag()?;
        found |= matches!(&key, b"Mtrn" | b"Mt16" | b"Mt32");
        let len = r.len(h.big && LONG.contains(&&key))?;
        let next = r.pos()?.saturating_add(len);
        if next > end {
            break;
        }
        if matches!(&key, b"Lr16" | b"Lr32") {
            found |= negative_count(r, len)?;
        }
        r.seek(next)?;
    }
    Ok(found)
}

/// PackBits-decode `src` into all of `dst`; `false` when the run does not fill it exactly.
fn unpack_bits(src: &[u8], dst: &mut [u8]) -> bool {
    let (mut i, mut o) = (0, 0);
    while i < src.len() && o < dst.len() {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let k = n as usize + 1;
            if i + k > src.len() || o + k > dst.len() {
                return false;
            }
            dst[o..o + k].copy_from_slice(&src[i..i + k]);
            i += k;
            o += k;
        } else if n != -128 {
            let k = (1 - i32::from(n)) as usize;
            if i >= src.len() || o + k > dst.len() {
                return false;
            }
            dst[o..o + k].fill(src[i]);
            i += 1;
            o += k;
        }
    }
    o == dst.len()
}

/// The merged image of `path` as float RGBA, its `SourceInfo` and whether it embeds an ICC profile. The size is
/// checked against `max_dim` before anything is allocated.
pub(crate) fn load(path: &Path, max_dim: u32) -> Result<(Rgba32F, SourceInfo, bool), PsdError> {
    let mut r = In(BufReader::new(File::open(path)?));
    let h = header(&mut r)?;
    if h.width.max(h.height) > max_dim {
        return Err(PsdError::TooLarge(h.width.max(h.height)));
    }
    let colours: u16 = match h.mode {
        1 => 1,
        3 => 3,
        m => {
            let name = match m {
                0 => "Bitmap",
                2 => "Indexed",
                4 => "CMYK",
                7 => "Multichannel",
                8 => "Duotone",
                9 => "Lab",
                _ => "unknown",
            };
            return bad(format!(
                "{name} colour mode is not supported; convert the document to RGB or Grayscale"
            ));
        }
    };
    if !matches!(h.depth, 8 | 16 | 32) {
        return bad(format!("{} bits per channel is not supported", h.depth));
    }
    if h.channels < colours {
        return bad(format!(
            "{} channels is too few for its colour mode",
            h.channels
        ));
    }
    let used = h.channels.min(colours + 1);
    let alpha = used > colours;

    let colour_data = u64::from(r.u32()?);
    r.skip(colour_data)?;
    let res_len = u64::from(r.u32()?);
    let res_end = r.pos()? + res_len;
    let (icc, real) = resources(&mut r, res_end)?;
    if !real {
        return bad(
            "saved without a composite image; re-save it in Photoshop with Maximize Compatibility on",
        );
    }
    r.seek(res_end)?;
    let lm_len = r.len(h.big)?;
    let lm_end = r.pos()?.saturating_add(lm_len);
    let matted = alpha && merged_transparency(&mut r, &h, lm_end)?;
    r.seek(lm_end)?;

    let (w, ht) = (h.width as usize, h.height as usize);
    let bps = usize::from(h.depth / 8);
    let row = w * bps;
    let compression = r.u16()?;
    // Rows of every channel, in file order; only the first `used` channels are read.
    let counts: Vec<u64> = match compression {
        0 => Vec::new(),
        1 => {
            let n = usize::from(h.channels) * ht;
            let mut c = Vec::with_capacity(usize::from(used) * ht);
            for i in 0..n {
                let v = if h.big { r.u32()? } else { u32::from(r.u16()?) };
                if i < usize::from(used) * ht {
                    c.push(u64::from(v));
                }
            }
            c
        }
        2 | 3 => return bad("a ZIP-compressed composite image is not supported"),
        c => return bad(format!("unknown image data compression {c}")),
    };

    // Trace #26: the pixel data must exist before the float buffer (up to ~4 GiB) is filled. Raw needs every row;
    // RLE needs every counted byte.
    let need = if compression == 0 {
        (row as u64) * (ht as u64) * u64::from(used)
    } else {
        counts.iter().sum()
    };
    if need > r.0.get_ref().metadata()?.len().saturating_sub(r.pos()?) {
        return bad("truncated PSD");
    }
    let mut px = vec![[0.0, 0.0, 0.0, 1.0]; w * ht];
    let mut line = vec![0u8; row];
    let mut packed = Vec::new();
    for ch in 0..usize::from(used) {
        for y in 0..ht {
            if compression == 0 {
                r.0.read_exact(&mut line)?;
            } else {
                // PackBits never needs more than one header byte per byte of row: a larger count is corrupt, and
                // is refused before it sizes the buffer.
                let n = counts[ch * ht + y];
                if n > 2 * row as u64 {
                    return bad(format!("corrupt RLE data (channel {ch}, row {y})"));
                }
                packed.resize(n as usize, 0);
                r.0.read_exact(&mut packed)?;
                if !unpack_bits(&packed, &mut line) {
                    return bad(format!("corrupt RLE data (channel {ch}, row {y})"));
                }
            }
            let out = &mut px[y * w..(y + 1) * w];
            for (x, p) in out.iter_mut().enumerate() {
                let s = &line[x * bps..(x + 1) * bps];
                let v = match bps {
                    1 => f32::from(s[0]) / 255.0,
                    2 => f32::from(u16::from_be_bytes([s[0], s[1]])) / 65535.0,
                    _ => f32::from_be_bytes([s[0], s[1], s[2], s[3]]),
                };
                if ch < usize::from(colours) && colours == 1 {
                    p[..3].fill(v);
                } else if ch < usize::from(colours) {
                    p[ch] = v;
                } else {
                    p[3] = v;
                }
            }
        }
    }
    if matted {
        let float = h.depth == 32;
        for p in &mut px {
            let a = p[3];
            if a > 0.0 {
                for c in &mut p[..3] {
                    let v = (*c - (1.0 - a)) / a;
                    *c = if float { v } else { v.clamp(0.0, 1.0) };
                }
            }
        }
    }
    let info = SourceInfo {
        bit_depth: h.depth as u8,
        channels: used as u8,
        has_alpha: alpha,
        is_float: h.depth == 32,
    };
    Ok((
        Rgba32F {
            width: h.width,
            height: h.height,
            px,
        },
        info,
        icc,
    ))
}

#[cfg(test)]
mod tests {
    use super::unpack_bits;

    #[test]
    fn packbits_literal_repeat_and_noop() {
        let mut d = [0u8; 6];
        // 2 literals, 3 repeats of 9, a -128 no-op, 1 literal.
        assert!(unpack_bits(&[1, 4, 5, 0xFE, 9, 0x80, 0, 7], &mut d));
        assert_eq!(d, [4, 5, 9, 9, 9, 7]);
        assert!(!unpack_bits(&[1, 4, 5], &mut d));
        assert!(!unpack_bits(&[0xF0, 1], &mut d));
    }
}
