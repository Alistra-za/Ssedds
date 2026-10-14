//! Test-only stand-in for NVIDIA's `nvcompress` (DECISIONS Note NV), built only with `test-hooks`. It speaks the same
//! command line the NVTT backend uses and writes deterministic, correctly sized BC7/BC6H payloads.
//!
//! - No arguments: when the executable's file name contains `hang`, sleep 60 s first (a program that never answers
//!   the probe). Then the banner `NVIDIA Texture Tools <SDC_FAKE_NVTT_VERSION or 3.2.5>` and a usage list; exit 0.
//! - `<flags> <in.dds> <out.dds>`: every BC7 block is mode 6, alpha 255, colour from the block's first input texel;
//!   every BC6H block is zeros. The argument list is written to `<out>.args`, one per line.
//! - `SDC_FAKE_NVTT_FAIL=exit|short|sleep`: exit 3 with a stderr line; write a payload one block short; sleep 30 s
//!   before writing.
use engine::config::Format;
use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
use std::process::ExitCode;

const USAGE_FLAGS: [&str; 8] = [
    "-nomips",
    "-bc7",
    "-bc6",
    "-production",
    "-noalpha",
    "-alpha",
    "-nocuda",
    "-silent",
];

/// A BC7 mode 6 block whose two endpoints are both `c` (7-bit endpoints, p-bits 1) with all indices 0.
fn bc7_mode6(c: [u8; 3]) -> [u8; 16] {
    let mut v: u128 = 1 << 6;
    let mut at = 7;
    for e in [c[0], c[0], c[1], c[1], c[2], c[2], 255, 255] {
        v |= ((e >> 1) as u128) << at;
        at += 7;
    }
    v |= 0b11 << at;
    v.to_le_bytes()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        if std::env::current_exe().is_ok_and(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().contains("hang"))
        }) {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        let v = std::env::var("SDC_FAKE_NVTT_VERSION").unwrap_or_else(|_| "3.2.5".into());
        println!("NVIDIA Texture Tools {v} - fake for sdc tests");
        println!("usage: nvcompress [options] infile [outfile]");
        for f in USAGE_FLAGS {
            println!("  {f}  (fake)");
        }
        return ExitCode::SUCCESS;
    }
    let fail = std::env::var("SDC_FAKE_NVTT_FAIL").unwrap_or_default();
    if fail == "exit" {
        eprintln!("fake nvcompress: forced failure");
        return ExitCode::from(3);
    }
    let [.., input, output] = args.as_slice() else {
        eprintln!("fake nvcompress: need <in> <out>");
        return ExitCode::from(2);
    };
    if let Err(e) = std::fs::write(format!("{output}.args"), args.join("\n")) {
        eprintln!("fake nvcompress: {e}");
        return ExitCode::from(2);
    }
    let Ok(src) = std::fs::read(input) else {
        eprintln!("fake nvcompress: cannot read {input}");
        return ExitCode::from(2);
    };
    if src.len() < 128 || &src[..4] != b"DDS " {
        eprintln!("fake nvcompress: {input} is not a DDS file");
        return ExitCode::from(2);
    }
    let u = |i: usize| u32::from_le_bytes(src[i..i + 4].try_into().unwrap());
    let (w, h) = (u(16), u(12));
    let texels = &src[if &src[84..88] == b"DX10" { 148 } else { 128 }..];
    let bc7 = args.iter().any(|a| a == "-bc7");
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut payload = Vec::with_capacity((bw * bh * 16) as usize);
    for by in 0..bh {
        for bx in 0..bw {
            if bc7 {
                // Input is B8G8R8A8.
                let o = ((by * 4 * w + bx * 4) * 4) as usize;
                let p = texels.get(o..o + 4).unwrap_or(&[0; 4]);
                payload.extend_from_slice(&bc7_mode6([p[2], p[1], p[0]]));
            } else {
                payload.extend_from_slice(&[0; 16]);
            }
        }
    }
    let desc = DdsDesc {
        width: w,
        height: h,
        format: if bc7 { Format::Bc7 } else { Format::Bc6hUf16 },
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    let mut bytes = dds_bytes(&desc, &[payload]).expect("fake payload has the exact size");
    match fail.as_str() {
        "short" => bytes.truncate(bytes.len() - 16),
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(30)),
        _ => {}
    }
    if let Err(e) = std::fs::write(output, bytes) {
        eprintln!("fake nvcompress: {e}");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}
