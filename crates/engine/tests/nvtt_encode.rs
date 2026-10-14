//! M5 (DECISIONS Note NV4-NV8): one level through nvcompress, against the test-only fake.
use engine::config::*;
use engine::nvtt::*;
use engine::presets::PresetRegistry;
use engine::quantize::EncoderInput;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The fake reads SDC_FAKE_NVTT_* from the environment it inherits; every test here holds this lock so a failure
/// mode set by one test never reaches another's child.
static ENV: Mutex<()> = Mutex::new(());

fn fake() -> NvttTool {
    NvttTool {
        path: env!("CARGO_BIN_EXE_nvcompress").into(),
        version: NVTT_TESTED.into(),
    }
}

fn profile(st: Subtype, format: Format, alpha: AlphaPolicy) -> Profile {
    let mut p = PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, st)
        .unwrap();
    p.format = format;
    p.alpha.policy = alpha;
    p.encoder.backend = Backend::Nvtt;
    p
}

fn ldr(w: u32, h: u32) -> EncoderInput {
    EncoderInput::Ldr((0..w * h).map(|i| [i as u8, 40, 200, 255]).collect())
}

fn args(tmp: &TempDir, level: usize) -> Vec<String> {
    std::fs::read_to_string(tmp.path().join(format!("l{level}.out.dds.args")))
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

fn never() -> bool {
    false
}

#[test]
fn bc7_levels_have_the_right_size_and_flags() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    let p = profile(Subtype::Diffuse, Format::Bc7, AlphaPolicy::Remove);
    let out = encode_level_nvtt(&fake(), &ldr(16, 16), 16, 16, &p, &tmp, 0, &never).unwrap();
    assert_eq!(out.len(), 16 * 16);
    let out = encode_level_nvtt(&fake(), &ldr(1, 1), 1, 1, &p, &tmp, 1, &never).unwrap();
    assert_eq!(out.len(), 16);
    let a = args(&tmp, 0);
    for f in [
        "-silent",
        "-nomips",
        "-production",
        "-bc7",
        "-nocuda",
        "-noalpha",
    ] {
        assert!(a.iter().any(|x| x == f), "{f} not in {a:?}");
    }
    // The feed is the level as a legacy-header B8G8R8A8 DDS (Spike S7b).
    let feed = std::fs::read(&a[a.len() - 2]).unwrap();
    assert_ne!(&feed[84..88], b"DX10");
    assert_eq!(u32::from_le_bytes(feed[88..92].try_into().unwrap()), 32);
    assert_eq!(&feed[128..132], &[200, 40, 0, 255]); // B, G, R, A of texel 0

    let p = profile(Subtype::Diffuse, Format::Bc7, AlphaPolicy::Blend);
    encode_level_nvtt(&fake(), &ldr(4, 4), 4, 4, &p, &tmp, 2, &never).unwrap();
    let a = args(&tmp, 2);
    assert!(
        a.iter().any(|x| x == "-alpha") && !a.iter().any(|x| x == "-noalpha"),
        "{a:?}"
    );
}

#[test]
fn bc6h_level_is_fed_as_legacy_rgba16f() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    let p = profile(Subtype::Diffuse, Format::Bc6hUf16, AlphaPolicy::Remove);
    let one = half::f16::from_f32(1.0).to_bits();
    let input = EncoderInput::Hdr(vec![[one, one, one, one]; 6 * 5]);
    let out = encode_level_nvtt(&fake(), &input, 6, 5, &p, &tmp, 0, &never).unwrap();
    assert_eq!(out.len(), 2 * 2 * 16);
    let a = args(&tmp, 0);
    assert!(
        a.iter().any(|x| x == "-bc6") && a.iter().any(|x| x == "-nocuda"),
        "{a:?}"
    );
    assert!(!a.iter().any(|x| x == "-bc7" || x == "-noalpha"), "{a:?}");
    let feed = std::fs::read(&a[a.len() - 2]).unwrap();
    assert_eq!(u32::from_le_bytes(feed[84..88].try_into().unwrap()), 113);
    assert_eq!(feed.len(), 128 + 6 * 5 * 8);
}

#[test]
fn failures_are_reported() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    let p = profile(Subtype::Diffuse, Format::Bc7, AlphaPolicy::Remove);
    std::env::set_var("SDC_FAKE_NVTT_FAIL", "exit");
    let e = encode_level_nvtt(&fake(), &ldr(8, 8), 8, 8, &p, &tmp, 0, &never);
    std::env::set_var("SDC_FAKE_NVTT_FAIL", "short");
    let s = encode_level_nvtt(&fake(), &ldr(8, 8), 8, 8, &p, &tmp, 1, &never);
    std::env::remove_var("SDC_FAKE_NVTT_FAIL");
    match e {
        Err(NvttError::Failed(m)) => {
            assert!(m.contains("forced failure") && m.contains("3"), "{m}")
        }
        r => panic!("{r:?}"),
    }
    match s {
        Err(NvttError::Failed(m)) => assert!(m.contains("wrong payload size"), "{m}"),
        r => panic!("{r:?}"),
    }
    let missing = NvttTool {
        path: "Z:/no/such/nvcompress.exe".into(),
        version: NVTT_TESTED.into(),
    };
    match encode_level_nvtt(&missing, &ldr(4, 4), 4, 4, &p, &tmp, 2, &never) {
        Err(NvttError::Failed(m)) => assert!(m.contains("could not start"), "{m}"),
        r => panic!("{r:?}"),
    }
}

#[test]
fn cancel_kills_the_process_and_the_folder_goes() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let p = profile(Subtype::Diffuse, Format::Bc7, AlphaPolicy::Remove);
    std::env::set_var("SDC_FAKE_NVTT_FAIL", "sleep");
    let t = Instant::now();
    let cancel = || t.elapsed() > Duration::from_millis(200);
    let r = encode_level_nvtt(&fake(), &ldr(8, 8), 8, 8, &p, &tmp, 0, &cancel);
    std::env::remove_var("SDC_FAKE_NVTT_FAIL");
    assert_eq!(r, Err(NvttError::Cancelled));
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    assert!(!dir.join("l0.out.dds").exists());
    drop(tmp);
    assert!(!Path::new(&dir).exists());
}

/// A complex-grass BC7 level runs the same post steps as bc7e (DECISIONS Note NV6): when NVTT's bottom-left texel
/// is not a unit normal, the guard re-encodes that block with bc7e.
#[test]
fn grass_guard_runs_on_nvtt_output() {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    let p = profile(Subtype::ComplexGrass, Format::Bc7, AlphaPolicy::Remove);
    // Unit normal (0, 0, 1) everywhere, except the first texel of the corner block, which the fake copies into the
    // whole block: the fake's corner then decodes far from unit length.
    let (w, h) = (8u32, 16u32);
    let mut px = vec![[128u8, 128, 255, 255]; (w * h) as usize];
    px[(12 * w) as usize] = [0, 0, 0, 255];
    let out =
        encode_level_nvtt(&fake(), &EncoderInput::Ldr(px), w, h, &p, &tmp, 0, &never).unwrap();
    let corner = (h / 4 - 1) as usize * (w / 4) as usize * 16;
    let mut d = [0u8; 64];
    bcdec_rs::bc7(&out[corner..corner + 16], &mut d, 16);
    // Texel 12 is (x 0, y 3) of the block: the source's unit normal again, within a few steps.
    let t = &d[12 * 4..12 * 4 + 4];
    assert!(
        t[0].abs_diff(128) <= 4 && t[1].abs_diff(128) <= 4 && t[2] >= 250,
        "{t:?}"
    );
    assert_eq!(t[3], 255);
}

/// DECISIONS AH1: the NVTT path runs the exact-0 fix too (`bc7_post_fix`). The fake writes every block as mode 6 with
/// alpha 255 (p-bits 1), so without the fix a complex `_m` with parallax off would decode 255.
#[test]
fn a_parallax_off_complex_m_decodes_alpha_0_through_nvtt() {
    use engine::dds::DdsPixelFormat;
    use engine::decode::{decode_level, Decoded};
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    let input = EncoderInput::Ldr((0..64).map(|i| [i as u8 * 3, 90, 40, 0]).collect());
    let decoded_alpha = |parallax: bool, level: usize| {
        let mut p = profile(Subtype::ComplexMaterial, Format::Bc7, AlphaPolicy::Data);
        p.alpha.parallax = Some(parallax);
        let out = encode_level_nvtt(&fake(), &input, 8, 8, &p, &tmp, level, &never).unwrap();
        match decode_level(DdsPixelFormat::Bc7 { srgb: false }, 8, 8, &out, None).unwrap() {
            Decoded::Ldr(px) => px.iter().map(|q| q[3]).collect::<Vec<_>>(),
            Decoded::Hdr(_) => unreachable!(),
        }
    };
    assert!(decoded_alpha(false, 0).iter().all(|&a| a == 0));
    // Parallax on keeps the encoder's alpha untouched (the fake's 255 shows the fix did not run).
    assert!(decoded_alpha(true, 1).iter().all(|&a| a == 255));
}
