//! DDS header patch (DECISIONS S7, owner idea 2026-10-11): rebuild a DDS header that is wrong where no pixel needs to
//! change, and copy everything after the old header byte for byte. Faults that need re-encoding are refused.
use crate::codes::{Code, Warning};
use crate::config::Format;
use crate::dds::*;
use crate::decode::{decode_level, Decoded};
use serde_json::json;

/// What the caller states about the file. `Default` asks for structural fixes only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PatchRequest {
    /// Tag to write (`Some(true)` = `_SRGB`); `None` keeps the file's. Only an explicit choice sets it, never a guess.
    pub srgb: Option<bool>,
    /// The block format the file must be (`sdc patch-header --format`); another one is refused.
    pub expect_format: Option<Format>,
    /// The profile wants a full mip chain: fewer stored levels are refused ("missing mips").
    pub require_full_mips: bool,
    /// The sub-type is complex_grass: a w x 2w chain ending at 1x2 (one level short of full, R6) is complete.
    pub complex_grass: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderPatch {
    /// One line per fix, e.g. "mip count 1 → 7".
    pub fixes: Vec<String>,
    /// The patched file: the new header, then `old bytes[old_header_len..]` unchanged.
    pub bytes: Vec<u8>,
    pub old_header_len: usize,
    pub new_header_len: usize,
}

/// Why a file cannot be fixed by its header alone.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Unpatchable(pub String);

impl HeaderPatch {
    pub fn warning(&self) -> Warning {
        Warning::new(
            Code::DdsHeaderPatched,
            format!(
                "header patched, pixels copied byte for byte: {}",
                self.fixes.join("; ")
            ),
        )
        .with_data(json!({ "fixes": self.fixes }))
    }
}

impl Unpatchable {
    pub fn warning(&self) -> Warning {
        Warning::new(
            Code::DdsHeaderUnpatchable,
            format!("not patched: {}", self.0),
        )
    }
}

fn tag_name(srgb: bool) -> &'static str {
    if srgb {
        "sRGB"
    } else {
        "UNORM"
    }
}

/// Plan a header-only fix of `bytes`. `Ok(None)`: the header is already what sdc would write.
///
/// Memory: the file is copied twice while it is parsed (the reader keeps its own copy of the payload).
pub fn plan_patch(bytes: &[u8], req: &PatchRequest) -> Result<Option<HeaderPatch>, Unpatchable> {
    let refuse = |m: String| Err(Unpatchable(m));
    if bytes.len() < 128 || &bytes[..4] != b"DDS " {
        return refuse("unreadable: not a DDS file".into());
    }
    let header_count = u32::from_le_bytes(bytes[28..32].try_into().unwrap()).max(1);
    // Parse with the mip count forced to 1, so a count the payload cannot hold is diagnosed here, not by the reader.
    let mut probe = bytes.to_vec();
    probe[28..32].copy_from_slice(&1u32.to_le_bytes());
    let f = read_dds(&probe).map_err(|e| Unpatchable(format!("unreadable: {e}")))?;
    drop(probe);
    if f.is_cubemap || f.dx10.is_some_and(|d| d.array_size > 1) {
        return refuse("a cubemap or array: sdc patches 2D textures only".into());
    }
    let Some((format, srgb_now)) = f.format.as_format() else {
        return refuse(format!(
            "{:?} is not a format sdc writes: it can be validated but not patched",
            f.format
        ));
    };
    let fmt_name = format.id().to_uppercase();
    let srgb = req.srgb.unwrap_or(srgb_now);
    if srgb && dxgi_format(format, true) == dxgi_format(format, false) {
        return refuse(format!(
            "{fmt_name} has no _SRGB form: it needs re-encoding as another format"
        ));
    }
    if let Some(e) = req.expect_format.filter(|e| *e != format) {
        return refuse(format!(
            "wrong block format: the file is {fmt_name}, {} expected: it needs re-encoding",
            e.id().to_uppercase()
        ));
    }
    let full = full_chain_len(f.width, f.height);
    let payload = f.data.len();
    let cum = |n: usize| -> Option<usize> {
        (0..n).try_fold(0usize, |s, i| {
            s.checked_add(checked_level_size(
                format,
                (f.width >> i).max(1),
                (f.height >> i).max(1),
            )?)
        })
    };
    // The level count that exactly fills the payload; else the header's count when it fits (trailing bytes kept).
    let stored = (1..=full).find(|&n| cum(n) == Some(payload)).or_else(|| {
        let n = (header_count as usize).min(full);
        cum(n).is_some_and(|c| c <= payload).then_some(n)
    });
    let Some(stored) = stored else {
        return refuse(format!(
            "the payload ({payload} bytes) holds no whole mip chain the header could describe: it needs re-encoding"
        ));
    };
    let grass_chain = req.complex_grass && f.height == 2 * f.width && stored + 1 == full;
    if req.require_full_mips && stored < full && !grass_chain {
        return refuse(format!(
            "missing mips: {stored} of {full} levels stored: it needs re-encoding"
        ));
    }
    if format == Format::Bc1 {
        let (w, h, r) = f.levels[0].clone();
        if let Ok(Decoded::Ldr(px)) = decode_level(f.format, w, h, &f.data[r], None) {
            if px.iter().any(|p| p[3] < 255) {
                return refuse(
                    "BC1 with 1-bit alpha: sdc writes BC1 opaque only; re-encode as BC7 or BC3"
                        .into(),
                );
            }
        }
    }
    let ati = matches!(&f.fourcc, b"ATI1" | b"ATI2");
    let keep_legacy = f.header_kind == HeaderKind::Legacy
        && !ati
        && resolve_header(format, srgb, crate::config::HeaderForm::Legacy)
            == ResolvedHeader::Legacy;
    let desc = DdsDesc {
        width: f.width,
        height: f.height,
        format,
        srgb,
        header: if keep_legacy {
            ResolvedHeader::Legacy
        } else {
            ResolvedHeader::Dx10
        },
    };
    let header = header_bytes(&desc, stored as u32).map_err(|e| Unpatchable(e.to_string()))?;
    let old_header_len = bytes.len() - payload;
    if bytes[..old_header_len] == header[..] {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(header.len() + payload);
    out.extend_from_slice(&header);
    out.extend_from_slice(&bytes[old_header_len..]);
    let new = read_dds(&out)
        .map_err(|e| Unpatchable(format!("internal: the patched file does not read: {e}")))?;
    let issues = check_header(&new);
    if !issues.is_empty() {
        return refuse(format!(
            "the header is still invalid after the patch: {}",
            issues.join("; ")
        ));
    }
    let mut fixes = vec![];
    if header_count as usize != stored {
        fixes.push(format!("mip count {header_count} → {stored}"));
    }
    if f.flags != new.flags {
        fixes.push(format!("flags {:#x} → {:#x}", f.flags, new.flags));
    }
    if f.caps != new.caps {
        fixes.push(format!("caps {:#x} → {:#x}", f.caps, new.caps));
    }
    if f.linear_size != new.linear_size {
        fixes.push(format!(
            "linear size / pitch {} → {}",
            f.linear_size, new.linear_size
        ));
    }
    if f.reserved_nonzero {
        fixes.push("reserved fields cleared".into());
    }
    if f.pf_flags & DDPF_NORMAL != 0 {
        fixes.push("non-standard DDPF_NORMAL cleared".into());
    }
    if f.header_kind != new.header_kind || ati {
        fixes.push(format!(
            "{} header ({}) → DX10 {fmt_name}",
            if f.header_kind == HeaderKind::Legacy {
                "legacy"
            } else {
                "DX10"
            },
            String::from_utf8_lossy(&f.fourcc)
        ));
    } else if let (Some(a), Some(b)) = (f.dx10, new.dx10) {
        if (a.dimension, a.misc, a.array_size, a.misc2)
            != (b.dimension, b.misc, b.array_size, b.misc2)
        {
            fixes.push("DX10 dimension/array/misc fields set to a 2D texture".into());
        }
    }
    if srgb != srgb_now {
        fixes.push(format!(
            "tag {fmt_name} {} → {}",
            tag_name(srgb_now),
            tag_name(srgb)
        ));
    }
    if fixes.is_empty() {
        fixes.push("other header fields normalised (depth, pixel-format size)".into());
    }
    Ok(Some(HeaderPatch {
        fixes,
        bytes: out,
        old_header_len,
        new_header_len: header.len(),
    }))
}
