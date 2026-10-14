//! Smoke test of the test-only fake `nvcompress` (src/bin/nvcompress.rs) the NVTT backend tests run against.
use engine::config::Format;
use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
use std::process::Command;

const FAKE: &str = env!("CARGO_BIN_EXE_nvcompress");

#[test]
fn banner_and_usage() {
    let o = Command::new(FAKE).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(
        text.lines()
            .next()
            .unwrap()
            .starts_with("NVIDIA Texture Tools 3.2.5"),
        "{text}"
    );
    for f in [
        "-nomips",
        "-bc7",
        "-bc6",
        "-production",
        "-noalpha",
        "-nocuda",
        "-silent",
    ] {
        assert!(
            text.lines().any(|l| l.split_whitespace().next() == Some(f)),
            "{f} missing: {text}"
        );
    }
    let o = Command::new(FAKE)
        .env("SDC_FAKE_NVTT_VERSION", "3.3.0")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("NVIDIA Texture Tools 3.3.0"));
}

#[test]
fn one_bc7_level() {
    let dir = tempfile::tempdir().unwrap();
    let (input, output) = (dir.path().join("l0.in.dds"), dir.path().join("l0.out.dds"));
    // 5x3 B8G8R8A8, legacy header (DECISIONS Spike S7b): two blocks across, one down.
    let texels: Vec<u8> = (0..15).flat_map(|_| [10u8, 20, 200, 255]).collect();
    let desc = DdsDesc {
        width: 5,
        height: 3,
        format: Format::Rgba8,
        srgb: false,
        header: ResolvedHeader::Legacy,
    };
    std::fs::write(&input, dds_bytes(&desc, &[texels]).unwrap()).unwrap();
    let st = Command::new(FAKE)
        .args(["-silent", "-nomips", "-bc7", "-production", "-nocuda"])
        .arg(&input)
        .arg(&output)
        .status()
        .unwrap();
    assert!(st.success());
    let b = std::fs::read(&output).unwrap();
    assert_eq!(&b[84..88], b"DX10");
    assert_eq!(u32::from_le_bytes(b[128..132].try_into().unwrap()), 98);
    let payload = &b[148..];
    assert_eq!(payload.len(), 2 * 16);
    let mut px = [0u8; 64];
    bcdec_rs::bc7(&payload[..16], &mut px, 16);
    // Mode 6, endpoints colour >> 1 with p-bit 1: (200, 20, 10) -> (201, 21, 11), alpha 255.
    assert_eq!(&px[..4], &[201, 21, 11, 255]);
    let args = std::fs::read_to_string(dir.path().join("l0.out.dds.args")).unwrap();
    assert!(args.lines().any(|l| l == "-bc7"), "{args}");

    let st = Command::new(FAKE)
        .env("SDC_FAKE_NVTT_FAIL", "short")
        .args(["-bc7"])
        .arg(&input)
        .arg(&output)
        .status()
        .unwrap();
    assert!(st.success());
    assert_eq!(std::fs::read(&output).unwrap().len(), 148 + 16);
    let o = Command::new(FAKE)
        .env("SDC_FAKE_NVTT_FAIL", "exit")
        .args(["-bc7"])
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&o.stderr).contains("forced failure"));
}
