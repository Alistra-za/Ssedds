//! Trace-debug #17 (Note BQ): nvcompress reads its arguments in the ANSI code page, so a temp folder with
//! characters outside it reaches the tool as `?`. Levels go by bare file names from inside the folder. Its own test
//! binary, because it points TMP and TEMP at such a folder.
use engine::config::*;
use engine::nvtt::*;
use engine::presets::PresetRegistry;
use engine::quantize::EncoderInput;

#[test]
fn encode_level_nvtt_succeeds_when_temp_path_is_not_ansi() {
    let base = tempfile::tempdir().unwrap();
    let temp = base.path().join("田中");
    std::fs::create_dir_all(&temp).unwrap();
    std::env::set_var("TMP", &temp);
    std::env::set_var("TEMP", &temp);
    let tmp = TempDir::new().unwrap();
    assert!(tmp.path().starts_with(&temp), "{}", tmp.path().display());
    let tool = NvttTool {
        path: env!("CARGO_BIN_EXE_nvcompress").into(),
        version: NVTT_TESTED.into(),
    };
    let mut p = PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, Subtype::Diffuse)
        .unwrap();
    p.format = Format::Bc7;
    p.encoder.backend = Backend::Nvtt;
    let input = EncoderInput::Ldr(vec![[10, 20, 30, 255]; 16]);
    let out = encode_level_nvtt(&tool, &input, 4, 4, &p, &tmp, 0, &|| false).unwrap();
    assert_eq!(out.len(), 16);
}
