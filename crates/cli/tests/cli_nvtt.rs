//! M5 (DECISIONS Note NV): `sdc convert --nvtt` and `sdc encoders` against the test-only fake nvcompress.
//!
//! The fake is the engine's `nvcompress` bin (`test-hooks` only); `cargo test --workspace` builds it next to `sdc.exe`.
//! `SDC_FAKE_NVTT_*` are set on the `sdc` child only, which passes them on to the fake.
use image::{ImageBuffer, Rgb};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fake() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_sdc")).with_file_name("nvcompress.exe");
    assert!(
        p.is_file(),
        "{} missing: run `cargo test --workspace`, which builds the engine's fake nvcompress",
        p.display()
    );
    p
}

fn sdc() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sdc"));
    c.env(
        "APPDATA",
        std::env::temp_dir().join("sdc-test-appdata-none"),
    );
    c
}

fn img(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128]))
        .save(p)
        .unwrap();
}

/// `sdc convert --json --set subtype=diffuse <extra> -o <out> <dir>/in` on one colour PNG.
fn convert(dir: &Path, extra: &[&str]) -> Command {
    img(&dir.join("in/a.png"));
    let mut c = sdc();
    c.args(["convert", "--json", "--set", "subtype=diffuse"])
        .args(extra)
        .arg("-o")
        .arg(dir.join("out"))
        .arg(dir.join("in"));
    c
}

fn events(o: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn has_code(o: &Output, code: &str) -> bool {
    events(o).iter().any(|e| e.to_string().contains(code))
}

fn show(o: &Output) -> String {
    format!(
        "exit {:?}\n{}\n{}",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn convert_with_nvtt_writes_a_bc7_dds() {
    let d = tempfile::tempdir().unwrap();
    let fake = fake();
    let o = convert(
        d.path(),
        &[
            "--nvtt",
            fake.to_str().unwrap(),
            "--set",
            "encoder.backend=nvtt",
        ],
    )
    .output()
    .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let dds = std::fs::read(d.path().join("out/a.dds")).unwrap();
    // BC7 (98 UNORM or 99 SRGB): the engine writes the header, never NVTT.
    let dxgi = u32::from_le_bytes(dds[128..132].try_into().unwrap());
    assert!(matches!(dxgi, 98 | 99), "DXGI {dxgi}");
    let manifest = std::fs::read_to_string(d.path().join("out/_sdc/manifest.jsonl")).unwrap();
    assert!(manifest.contains("\"nvtt\":\"3.2.5\""), "{manifest}");
}

#[test]
fn an_untested_version_blocks_until_allowed() {
    let d = tempfile::tempdir().unwrap();
    let fake = fake();
    let args = [
        "--nvtt",
        fake.to_str().unwrap(),
        "--set",
        "encoder.backend=nvtt",
    ];
    let o = convert(d.path(), &args)
        .env("SDC_FAKE_NVTT_VERSION", "3.3.0")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(has_code(&o, "ENC-NVTT-VERSION"), "{}", show(&o));
    assert!(!d.path().join("out/a.dds").exists());
    let mut allowed = args.to_vec();
    allowed.extend(["--allow", "ENC-NVTT-VERSION"]);
    let o = convert(d.path(), &allowed)
        .env("SDC_FAKE_NVTT_VERSION", "3.3.0")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    assert!(d.path().join("out/a.dds").exists());
}

#[test]
fn a_missing_nvtt_blocks_only_textures_on_it() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("no/nvcompress.exe");
    let o = convert(
        d.path(),
        &[
            "--nvtt",
            missing.to_str().unwrap(),
            "--set",
            "encoder.backend=nvtt",
        ],
    )
    .output()
    .unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    assert!(has_code(&o, "ENC-NVTT-MISSING"), "{}", show(&o));
    // Without the backend nothing looks for NVTT, so a missing --nvtt file does not matter.
    let o = convert(d.path(), &["--nvtt", missing.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
}

#[test]
fn a_failed_nvcompress_fails_the_file() {
    let d = tempfile::tempdir().unwrap();
    let fake = fake();
    let o = convert(
        d.path(),
        &[
            "--nvtt",
            fake.to_str().unwrap(),
            "--set",
            "encoder.backend=nvtt",
        ],
    )
    .env("SDC_FAKE_NVTT_FAIL", "exit")
    .output()
    .unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let failed: Vec<Value> = events(&o)
        .into_iter()
        .filter(|e| e["kind"] == "file_failed")
        .collect();
    assert_eq!(failed.len(), 1, "{}", show(&o));
    assert_eq!(failed[0]["stage"], "compress");
    let err = failed[0]["error"].as_str().unwrap();
    assert!(
        err.starts_with("ENC-NVTT-FAILED:") && err.contains("forced failure"),
        "{err}"
    );
    assert!(!d.path().join("out/a.dds").exists());
}

#[test]
fn encoders_reports_the_nvtt_version() {
    let fake = fake();
    let o = sdc()
        .args(["encoders", "--json", "--nvtt"])
        .arg(&fake)
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let n = &v["nvtt"];
    assert_eq!(
        (&n["found"], &n["version"], &n["tested"], &n["source"]),
        (
            &Value::from(true),
            &Value::from("3.2.5"),
            &Value::from(true),
            &Value::from("--nvtt")
        ),
        "{v}"
    );
    let o = sdc()
        .args(["encoders", "--nvtt"])
        .arg(&fake)
        .env("SDC_FAKE_NVTT_VERSION", "3.3.0")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        out.contains("NVTT: 3.3.0 at") && out.contains("acknowledge with --allow ENC-NVTT-VERSION"),
        "{out}"
    );
}
