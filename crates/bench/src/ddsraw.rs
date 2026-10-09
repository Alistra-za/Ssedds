use anyhow::{bail, Result};

/// Block payload of a single-level DDS (mip 0 only, as all bench outputs are).
pub fn payload(bytes: &[u8]) -> Result<&[u8]> {
    if bytes.len() < 128 || &bytes[..4] != b"DDS " {
        bail!("not a DDS file");
    }
    let off = if &bytes[84..88] == b"DX10" { 148 } else { 128 };
    if bytes.len() < off {
        bail!("truncated DDS header");
    }
    Ok(&bytes[off..])
}
