use image::{ImageBuffer, Rgb};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sdc() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sdc"));
    // Keep the developer's real user config out of the tests.
    c.env(
        "APPDATA",
        std::env::temp_dir().join("sdc-test-appdata-none"),
    );
    c
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("sdc was killed by a signal")
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

/// A bench DDS (BC7, decoded alpha 254 on ~21 % of mip 0), or `None` when the bench is not on this machine.
fn bench_dds(encoder: &str) -> Option<PathBuf> {
    let root = std::env::var("SDC_BENCH_ROOT").unwrap_or(r"F:\ai\dds\bench".into());
    let p = PathBuf::from(root).join(format!("out/{encoder}/rgb_real/Bricks104.dds"));
    if p.exists() {
        Some(p)
    } else {
        eprintln!("skipped: {} not found", p.display());
        None
    }
}

/// `<root>/wall_n.png` converted by `sdc convert`; returns the DDS.
fn converted_normal(root: &Path) -> PathBuf {
    let src = root.join("in");
    std::fs::create_dir_all(&src).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128]))
        .save(src.join("wall_n.png"))
        .unwrap();
    let out = root.join("out");
    let o = sdc()
        .args(["convert", "--accept-suggestions", "high", "-o"])
        .arg(&out)
        .arg(src.join("wall_n.png"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    out.join("wall_n.dds")
}

#[test]
fn inspect_reports_suggestions_and_dds_headers() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
        .save(d.path().join("rock_n.png"))
        .unwrap();
    let o = sdc()
        .arg("inspect")
        .arg(d.path().join("rock_n.png"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("normal") && out.contains("high"), "{out}");

    let Some(bench) = bench_dds("bc7e_u5") else {
        return;
    };
    let o = sdc()
        .args(["inspect", "--json"])
        .arg(&bench)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0);
    let out = text(&o.stdout);
    assert!(
        out.contains("\"kind\":\"dds_info\"") && out.contains("Bc7"),
        "{out}"
    );
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["mip_count"], 1);
    assert_eq!(v["mips"][0]["alpha"]["min"], 254);
}

#[test]
fn inspect_human_output_covers_the_dds_header_and_mip_alpha() {
    let Some(bench) = bench_dds("bc7e_u5") else {
        return;
    };
    let o = sdc().arg("inspect").arg(&bench).output().unwrap();
    assert_eq!(code(&o), 0);
    let out = text(&o.stdout);
    for want in [
        "format Bc7",
        "1024x512",
        "1 mips",
        "Dx10",
        "cubemap false",
        "mip 0",
        "=254",
    ] {
        assert!(out.contains(want), "{want}: {out}");
    }
}

#[test]
fn inspect_json_mode_prints_only_json_lines() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    ImageBuffer::from_fn(8, 8, |_, _| Rgb([128u8, 128, 255]))
        .save(dir.join("rock_n.png"))
        .unwrap();
    std::fs::write(dir.join("bad.png"), b"not a png").unwrap();
    let o = sdc()
        .args(["inspect", "--json"])
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let lines: Vec<serde_json::Value> = text(&o.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l}: {e}")))
        .collect();
    let kinds: Vec<&str> = lines.iter().filter_map(|v| v["kind"].as_str()).collect();
    assert!(kinds.contains(&"file_planned"), "{kinds:?}");
    assert!(kinds.contains(&"file_failed"), "{kinds:?}");
    let planned = lines.iter().find(|v| v["kind"] == "file_planned").unwrap();
    assert_eq!(planned["job"]["suggestions"][0]["confidence"], "high");
}

#[test]
fn inspect_lists_dds_files_found_in_a_folder_and_reports_unreadable_ones() {
    let d = tempfile::tempdir().unwrap();
    let dds = converted_normal(d.path());
    std::fs::write(d.path().join("out/junk.dds"), b"not a dds").unwrap();
    let o = sdc()
        .arg("inspect")
        .arg(dds.parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(
        out.contains("wall_n.dds") && out.contains("format Bc"),
        "{out}"
    );
    assert!(out.contains("cannot be read"), "{out}");
    assert!(!out.contains("IN-DDS-SKIPPED"), "{out}");
}

#[test]
fn inspect_missing_input_is_a_usage_error() {
    let o = sdc()
        .args(["inspect", "no-such-file.png"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
}

#[test]
fn inspect_cleans_up_and_writes_nothing_into_its_temp_root() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_fn(8, 8, |_, _| Rgb([128u8, 128, 255]))
        .save(d.path().join("rock_n.png"))
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let o = sdc()
        .env("TMP", tmp.path())
        .env("TEMP", tmp.path())
        .arg("inspect")
        .arg(d.path().join("rock_n.png"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let left: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
    assert!(left.is_empty(), "left in the temp folder: {left:?}");
}

#[test]
fn validate_flags_254_as_info_and_passes_bench_bc7() {
    let Some(bench) = bench_dds("bc7e_u6") else {
        return;
    };
    for model in ["ideal", "nvidia"] {
        let o = sdc()
            .args(["validate", "--subtype", "diffuse", "--target", "vanilla"])
            .args(["--bc1-decoder", model])
            .arg(&bench)
            .output()
            .unwrap();
        assert_eq!(code(&o), 0, "{}", text(&o.stderr));
        let err = text(&o.stderr);
        assert!(err.contains("ALPHA-254") && err.contains("ok"), "{err}");
    }
}

#[test]
fn validate_unreadable_file_exits_1_even_next_to_a_good_one() {
    let d = tempfile::tempdir().unwrap();
    let junk = d.path().join("junk.dds");
    std::fs::write(&junk, b"not a dds").unwrap();
    let o = sdc().arg("validate").arg(&junk).output().unwrap();
    assert_eq!(code(&o), 1);
    assert!(text(&o.stderr).contains("cannot be read"));
    let o = sdc()
        .arg("validate")
        .arg(d.path().join("missing.dds"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 1);
    if let Some(bench) = bench_dds("bc7e_u6") {
        let o = sdc()
            .args(["validate", "--subtype", "diffuse"])
            .arg(&bench)
            .arg(&junk)
            .output()
            .unwrap();
        assert_eq!(code(&o), 1);
    }
}

#[test]
fn validate_accepts_an_sdc_output_and_fails_one_with_a_bad_header() {
    let d = tempfile::tempdir().unwrap();
    let dds = converted_normal(d.path());
    let o = sdc()
        .args(["validate", "--subtype", "normal", "--json"])
        .arg(&dds)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert_eq!(out.lines().count(), 1, "{out}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["kind"], "dds_validation");
    assert_eq!(v["passed"], true);
    assert_eq!(v["subtype"], "normal");

    // Clear the LINEARSIZE flag: a header rule (spec §5.7) the file now breaks.
    let mut bytes = std::fs::read(&dds).unwrap();
    let flags = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) & !0x8_0000;
    bytes[8..12].copy_from_slice(&flags.to_le_bytes());
    let bad = d.path().join("bad.dds");
    std::fs::write(&bad, bytes).unwrap();
    let o = sdc()
        .args(["validate", "--subtype", "normal", "--json"])
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(code(&o), 4, "{}", text(&o.stderr));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(v["passed"], false);
    assert_eq!(v["warnings"][0]["code"], "VAL-HEADER");
    assert!(text(&o.stderr).contains("FAIL"));
}

#[test]
fn validate_infers_the_subtype_from_the_name_and_rejects_bad_flags() {
    let d = tempfile::tempdir().unwrap();
    let dds = converted_normal(d.path());
    let o = sdc().arg("validate").arg(&dds).output().unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains("(normal)"), "{}", text(&o.stderr));
    for args in [
        &["--subtype", "nonsense"][..],
        &["--subtype", "pbr_albedo", "--target", "vanilla"],
    ] {
        let o = sdc().arg("validate").args(args).arg(&dds).output().unwrap();
        assert_eq!(code(&o), 2, "{args:?}");
    }
}
