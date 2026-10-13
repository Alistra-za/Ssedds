use image::{ImageBuffer, Luma, Rgb};
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
    assert_eq!(v["schema"], 1);
    // The object is exactly what `schema/dds-info.schema.json` describes.
    let info: engine::reports::DdsInfo = serde_json::from_value(v).unwrap();
    assert!(matches!(info, engine::reports::DdsInfo::Info(_)));
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
    assert_eq!(code(&o), 1, "{}", text(&o.stderr)); // bad.png cannot be read
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
    assert_eq!(code(&o), 1, "{}", text(&o.stderr)); // junk.dds cannot be read
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
        let err = text(&o.stdout);
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
    assert!(text(&o.stdout).contains("cannot be read"));
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
    assert_eq!(v["schema"], 1);
    let rec: engine::reports::DdsValidation = serde_json::from_value(v).unwrap();
    assert!(matches!(rec, engine::reports::DdsValidation::Validated(_)));

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
    assert!(text(&o.stdout).contains("(normal"), "{}", text(&o.stdout));
    for args in [
        &["--subtype", "nonsense"][..],
        &["--subtype", "pbr_albedo", "--target", "vanilla"],
    ] {
        let o = sdc().arg("validate").args(args).arg(&dds).output().unwrap();
        assert_eq!(code(&o), 2, "{args:?}");
    }
}

/// `<root>/in/<name>.png` (16x16 RGBA, alpha from `alpha(x, y)`) converted with `extra`; returns the DDS.
fn converted_rgba(
    root: &Path,
    name: &str,
    alpha: impl Fn(u32, u32) -> u8,
    extra: &[&str],
) -> PathBuf {
    let src = root.join("in");
    std::fs::create_dir_all(&src).unwrap();
    let png = src.join(format!("{name}.png"));
    ImageBuffer::from_fn(16, 16, |x, y| {
        image::Rgba([x as u8 * 12, y as u8 * 12, 90, alpha(x, y)])
    })
    .save(&png)
    .unwrap();
    let out = root.join("out");
    let o = sdc()
        .args(["convert", "--accept-suggestions", "high", "-o"])
        .arg(&out)
        .arg(&png)
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    out.join(format!("{name}.dds"))
}

fn validate(args: &[&str], dds: &Path) -> Output {
    sdc().arg("validate").args(args).arg(dds).output().unwrap()
}

#[test]
fn validate_judges_a_file_that_stores_alpha_by_the_alpha_it_stores() {
    // Review I1: any file with real alpha used to fail VAL-ALPHA-GUARANTEE under the subtype's `remove`.
    let d = tempfile::tempdir().unwrap();
    let blend = converted_rgba(
        d.path(),
        "leaf_d",
        |x, y| (x * 16 + y) as u8 % 250,
        &["--set", "subtype:diffuse.alpha.policy=blend"],
    );
    for target in ["cs", "vanilla"] {
        let o = validate(&["--target", target], &blend);
        let out = text(&o.stdout);
        assert_eq!(code(&o), 0, "{target}: {out}");
        assert!(!out.contains("VAL-ALPHA-GUARANTEE"), "{out}");
    }
    // The same pixels under the names of other sub-types that default to `remove`: `_p` (height) and `_m`.
    for name in ["foo_p", "rock_m"] {
        let copy = d.path().join(format!("{name}.dds"));
        std::fs::copy(&blend, &copy).unwrap();
        for target in ["cs", "vanilla"] {
            let o = validate(&["--target", target], &copy);
            let out = text(&o.stdout);
            assert!(
                !out.contains("VAL-ALPHA-GUARANTEE"),
                "{name} {target}: {out}"
            );
            assert_eq!(code(&o), 0, "{name} {target}: {out}");
        }
    }
}

#[test]
fn validate_applies_the_remove_guarantee_only_when_the_policy_is_remove() {
    // Mostly opaque with a few texels at 239: the file stores alpha, so by default it is judged as `data`;
    // `--alpha-policy remove` holds it to the >= 254 guarantee and fails it.
    let d = tempfile::tempdir().unwrap();
    let dds = converted_rgba(
        d.path(),
        "plate_d",
        |x, y| if x < 2 && y < 2 { 239 } else { 255 },
        &["--set", "subtype:diffuse.alpha.policy=data"],
    );
    let o = validate(&[], &dds);
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    assert!(
        text(&o.stdout).contains("alpha data"),
        "{}",
        text(&o.stdout)
    );

    let o = validate(&["--alpha-policy", "remove", "--json"], &dds);
    assert_eq!(code(&o), 4, "{}", text(&o.stdout));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(v["alpha_policy"], "remove");
    assert!(
        v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "VAL-ALPHA-GUARANTEE"),
        "{v}"
    );
    let o = validate(&["--alpha-policy", "bogus"], &dds);
    assert_eq!(code(&o), 2);
}

#[test]
fn validate_infers_remove_for_a_flat_opaque_file() {
    let d = tempfile::tempdir().unwrap();
    let dds = converted_rgba(d.path(), "wall_d", |_, _| 255, &[]);
    let o = validate(&["--json"], &dds);
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(v["alpha_policy"], "remove", "{v}");
}

#[test]
fn result_verbs_print_their_report_on_stdout_and_only_json_with_json() {
    // DECISIONS Note J.
    let d = tempfile::tempdir().unwrap();
    let dds = converted_normal(d.path());
    let o = validate(&[], &dds);
    assert_eq!(code(&o), 0);
    let (out, err) = (text(&o.stdout), text(&o.stderr));
    assert!(
        out.contains("wall_n.dds") && out.contains("validated 1"),
        "{out}"
    );
    assert!(err.is_empty(), "{err}");
    let o = validate(&["--json"], &dds);
    for l in text(&o.stdout).lines() {
        serde_json::from_str::<serde_json::Value>(l).unwrap_or_else(|e| panic!("{l}: {e}"));
    }
    let o = validate(&[], &d.path().join("missing.dds"));
    assert_eq!(code(&o), 1);
    assert!(
        text(&o.stdout).contains("cannot be read"),
        "{}",
        text(&o.stdout)
    );
}

#[test]
fn inspect_json_is_deterministic_and_relative_to_the_mapped_root() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/a");
    std::fs::create_dir_all(&dir).unwrap();
    ImageBuffer::from_fn(8, 8, |_, _| Rgb([128u8, 128, 255]))
        .save(dir.join("rock_n.png"))
        .unwrap();
    let run = || {
        let tmp = tempfile::tempdir().unwrap();
        let o = sdc()
            .env("TMP", tmp.path())
            .env("TEMP", tmp.path())
            .args(["inspect", "--json", "--recursive"])
            .args(["--set", "file:**/rock_n.png.alpha.threshold=100"])
            .arg(d.path().join("in"))
            .output()
            .unwrap();
        assert_eq!(code(&o), 0, "{}", text(&o.stderr));
        text(&o.stdout)
            .lines()
            .map(|l| {
                let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["ts"].take();
                v["batch"].take();
                v
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b);
    let all = serde_json::to_string(&a).unwrap();
    assert!(!all.contains("sdc-inspect-"), "{all}");
    let disc = a.iter().find(|v| v["kind"] == "file_discovered").unwrap();
    assert_eq!(disc["output"], "textures/a/rock_n.dds");
    let planned = a.iter().find(|v| v["kind"] == "file_planned").unwrap();
    assert_eq!(planned["job"]["output"], "textures/a/rock_n.dds");
    let set = a.iter().find(|v| v["kind"] == "set_resolved").unwrap();
    assert!(
        set["set"].as_str().unwrap().starts_with("textures/a/"),
        "{set}"
    );
}

#[test]
fn inspect_honours_bc1_decoder_and_exits_1_when_an_input_cannot_be_read() {
    let d = tempfile::tempdir().unwrap();
    let dds = converted_normal(d.path());
    for model in ["ideal", "ideal-round4", "nvidia", "amd"] {
        let o = sdc()
            .args(["inspect", "--bc1-decoder", model])
            .arg(&dds)
            .output()
            .unwrap();
        assert_eq!(code(&o), 0, "{model}: {}", text(&o.stderr));
    }
    let o = sdc()
        .args(["inspect", "--bc1-decoder", "bogus"])
        .arg(&dds)
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);

    let junk = d.path().join("junk.dds");
    std::fs::write(&junk, b"not a dds").unwrap();
    let o = sdc().arg("inspect").arg(&junk).output().unwrap();
    assert_eq!(code(&o), 1);
    assert!(text(&o.stdout).contains("cannot be read"));
    let png = d.path().join("bad.png");
    std::fs::write(&png, b"not a png").unwrap();
    let o = sdc()
        .args(["inspect", "--json"])
        .arg(&png)
        .arg(&dds)
        .output()
        .unwrap();
    assert_eq!(code(&o), 1);
    // A missing input stays a usage error.
    let o = sdc().args(["inspect", "nope.png"]).output().unwrap();
    assert_eq!(code(&o), 2);
}

#[test]
fn inspect_and_dry_run_convert_plan_an_image_next_to_its_same_stem_dds() {
    // Review I2: both used to be IN-DUPLICATE and the image lost its plan.
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("mixed");
    std::fs::create_dir_all(&dir).unwrap();
    ImageBuffer::from_fn(8, 8, |_, _| Rgb([128u8, 128, 255]))
        .save(dir.join("rock_n.png"))
        .unwrap();
    let dds = converted_normal(d.path());
    std::fs::copy(&dds, dir.join("rock_n.dds")).unwrap();

    let o = sdc()
        .args(["inspect", "--json"])
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("\"kind\":\"file_planned\""), "{out}");
    assert!(out.contains("\"kind\":\"dds_info\""), "{out}");
    assert!(!out.contains("IN-DUPLICATE"), "{out}");
    let o = sdc().arg("inspect").arg(&dir).output().unwrap();
    assert!(
        text(&o.stdout).contains("suggested normal"),
        "{}",
        text(&o.stdout)
    );

    let o = sdc()
        .args(["convert", "--dry-run", "--accept-suggestions", "high", "-o"])
        .arg(d.path().join("planned"))
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(
        !text(&o.stderr).contains("IN-DUPLICATE"),
        "{}",
        text(&o.stderr)
    );
}

/// A 4x4 BC2 (DXT3) file, or the same with the LINEARSIZE flag cleared.
fn bc2_file(dir: &Path, name: &str, break_header: bool) -> PathBuf {
    let mut b = vec![0u8; 128 + 16];
    let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    b[0..4].copy_from_slice(b"DDS ");
    put(&mut b, 4, 124);
    // CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE
    put(&mut b, 8, if break_header { 0x1007 } else { 0x8_1007 });
    put(&mut b, 12, 4);
    put(&mut b, 16, 4);
    put(&mut b, 20, 16);
    put(&mut b, 76, 32);
    put(&mut b, 80, 4); // DDPF_FOURCC
    b[84..88].copy_from_slice(b"DXT3");
    put(&mut b, 108, 0x1000); // TEXTURE
    let p = dir.join(name);
    std::fs::write(&p, b).unwrap();
    p
}

#[test]
fn validate_does_not_judge_formats_sdc_never_writes_but_still_checks_their_header() {
    let d = tempfile::tempdir().unwrap();
    let good = bc2_file(d.path(), "ui_d.dds", false);
    let o = validate(&["--json"], &good);
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(v["judged"], false, "{v}");
    assert_eq!(v["passed"], true);
    let note = v["note"].as_str().unwrap();
    assert!(
        note.contains("not judged: format not produced by sdc"),
        "{v}"
    );
    let o = validate(&[], &good);
    assert!(text(&o.stdout).contains("not judged: format not produced by sdc"));

    let bad = bc2_file(d.path(), "ui2_d.dds", true);
    let o = validate(&["--json"], &bad);
    assert_eq!(code(&o), 4, "{}", text(&o.stdout));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(v["warnings"][0]["code"], "VAL-HEADER", "{v}");
}

/// A legacy 4x4 RGBA8 file with `DDSD_PITCH` and a row pitch of `pitch` bytes.
fn rgba8_file(dir: &Path, name: &str, pitch: u32) -> PathBuf {
    let mut b = vec![0u8; 128 + 64];
    let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    b[0..4].copy_from_slice(b"DDS ");
    put(&mut b, 4, 124);
    put(&mut b, 8, 0x100F); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    put(&mut b, 12, 4);
    put(&mut b, 16, 4);
    put(&mut b, 20, pitch);
    put(&mut b, 76, 32);
    put(&mut b, 80, 0x41); // DDPF_RGB | DDPF_ALPHAPIXELS
    put(&mut b, 88, 32);
    for (i, m) in [0xff, 0xff00, 0xff_0000, 0xff00_0000]
        .into_iter()
        .enumerate()
    {
        put(&mut b, 92 + i * 4, m);
    }
    put(&mut b, 108, 0x1000); // TEXTURE
    let p = dir.join(name);
    std::fs::write(&p, b).unwrap();
    p
}

/// Codex-B I2 / Note J: a clean uncompressed header (PITCH and a row pitch) is "not judged", exit 0; a wrong
/// pitch is a header failure.
#[test]
fn validate_does_not_judge_a_clean_uncompressed_file() {
    let d = tempfile::tempdir().unwrap();
    let good = rgba8_file(d.path(), "ui_d.dds", 16);
    let o = validate(&["--json"], &good);
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(v["judged"], false, "{v}");
    assert_eq!(v["passed"], true, "{v}");
    assert!(
        v["note"]
            .as_str()
            .unwrap()
            .contains("not judged: format not produced by sdc"),
        "{v}"
    );

    let bad = rgba8_file(d.path(), "ui2_d.dds", 64);
    let o = validate(&["--json"], &bad);
    assert_eq!(code(&o), 4, "{}", text(&o.stdout));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(v["warnings"][0]["code"], "VAL-HEADER", "{v}");
}

#[test]
fn validate_falls_back_when_the_inferred_subtype_is_unavailable_but_not_an_explicit_one() {
    let d = tempfile::tempdir().unwrap();
    let dds = converted_normal(d.path());
    let rmaos = d.path().join("foo_rmaos.dds");
    std::fs::copy(&dds, &rmaos).unwrap();
    let o = validate(&["--target", "vanilla"], &rmaos);
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    let out = text(&o.stdout);
    assert!(
        out.contains("rmaos is unavailable") && out.contains("generic_data"),
        "{out}"
    );
    let o = validate(&["--target", "vanilla", "--subtype", "rmaos"], &rmaos);
    assert_eq!(code(&o), 2);
}

#[test]
fn validate_target_help_states_the_default() {
    let o = sdc().args(["validate", "--help"]).output().unwrap();
    assert!(
        text(&o.stdout).contains("default cs"),
        "{}",
        text(&o.stdout)
    );
}

/// Codex-CLI 2: `inspect` validates the shared batch options (`--set`, `--allow`, `--preset`, `--preset-file`,
/// `--config`, the user config and a folder's `sdc.toml`) before any output, even when there is no image to
/// plan: a DDS-only input and an empty folder exit 2 like an image input would.
#[test]
fn inspect_rejects_invalid_options_and_configs_without_images() {
    let d = tempfile::tempdir().unwrap();
    let dds_dir = d.path().join("dds");
    std::fs::create_dir_all(&dds_dir).unwrap();
    bc2_file(&dds_dir, "ui_d.dds", false);
    let empty = d.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let bad_toml = d.path().join("bad.toml");
    std::fs::write(&bad_toml, "this is = = not toml").unwrap();
    let bad_cfg = d.path().join("bad_cfg.toml");
    std::fs::write(&bad_cfg, "target = \"nope\"\n").unwrap();
    let appdata = d.path().join("appdata");
    std::fs::create_dir_all(appdata.join("sdc")).unwrap();
    std::fs::write(appdata.join("sdc/config.toml"), "target = \"nope\"\n").unwrap();
    let bad_toml_s = bad_toml.to_str().unwrap();
    let bad_cfg_s = bad_cfg.to_str().unwrap();

    for input in [&dds_dir, &empty] {
        // Valid options: exit 0.
        let o = sdc().arg("inspect").arg(input).output().unwrap();
        assert_eq!(code(&o), 0, "{}", text(&o.stderr));
        let cases: [(&str, Vec<&str>); 7] = [
            ("bad --set value", vec!["--set", "format=bc9"]),
            ("unparsable --set", vec!["--set", "garbage"]),
            ("unknown --allow code", vec!["--allow", "NOT-A-CODE"]),
            ("unknown --preset", vec!["--preset", "no-such-preset"]),
            ("invalid --preset-file", vec!["--preset-file", bad_toml_s]),
            ("invalid --config", vec!["--config", bad_cfg_s]),
            ("invalid --preset file", vec!["--preset", bad_toml_s]),
        ];
        for (what, args) in &cases {
            let o = sdc().arg("inspect").args(args).arg(input).output().unwrap();
            assert_eq!(
                code(&o),
                2,
                "{what} {}: {}",
                input.display(),
                text(&o.stderr)
            );
            assert!(o.stdout.is_empty(), "{what}: {}", text(&o.stdout));
        }
        let o = sdc()
            .env("APPDATA", &appdata)
            .arg("inspect")
            .arg(input)
            .output()
            .unwrap();
        assert_eq!(
            code(&o),
            2,
            "user config {}: {}",
            input.display(),
            text(&o.stderr)
        );
        assert!(o.stdout.is_empty(), "{}", text(&o.stdout));
    }
    // A folder's own sdc.toml is its project config.
    std::fs::write(dds_dir.join("sdc.toml"), "target = \"nope\"\n").unwrap();
    let o = sdc().arg("inspect").arg(&dds_dir).output().unwrap();
    assert_eq!(code(&o), 2, "{}", text(&o.stderr));
    assert!(o.stdout.is_empty(), "{}", text(&o.stdout));
}

/// A 4x4 BC7 (DX10 `BC7_UNORM`) file of one mode-6 block whose every texel decodes to RGB 128 (or 129) and alpha
/// `(a7 << 1) | p`.
fn bc7_flat_alpha_file(dir: &Path, name: &str, a7: u128, p: u128) -> PathBuf {
    let mut b = vec![0u8; 128 + 20 + 16];
    let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    b[0..4].copy_from_slice(b"DDS ");
    put(&mut b, 4, 124);
    put(&mut b, 8, 0x8_1007); // CAPS | HEIGHT | WIDTH | PIXELFORMAT | LINEARSIZE
    put(&mut b, 12, 4);
    put(&mut b, 16, 4);
    put(&mut b, 20, 16);
    put(&mut b, 76, 32);
    put(&mut b, 80, 4); // DDPF_FOURCC
    b[84..88].copy_from_slice(b"DX10");
    put(&mut b, 108, 0x1000); // TEXTURE
    put(&mut b, 128, 98); // DXGI_FORMAT_BC7_UNORM
    put(&mut b, 132, 3); // TEXTURE2D
    put(&mut b, 140, 1); // array size
                         // Mode 6: endpoints R0 R1 G0 G1 B0 B1 A0 A1 (7 bits each) from bit 7, p-bits at 63 and 64, indices 0.
    let mut block: u128 = 1 << 6;
    for (i, v) in [64u128, 64, 64, 64, 64, 64, a7, a7].into_iter().enumerate() {
        block |= v << (7 + 7 * i);
    }
    block |= (p << 63) | (p << 64);
    b[148..164].copy_from_slice(&block.to_le_bytes());
    let path = dir.join(name);
    std::fs::write(&path, b).unwrap();
    path
}
/// Codex-CLI 3: standalone `validate` does not hold a third-party file to `alpha.exact_opaque`, which only
/// sdc's own encoder promises (the `envmask` and `complex_material` presets turn it on). A clean BC7 `_m` whose
/// alpha decodes to 254 passes with the ALPHA-254 info; `--exact-opaque` asks for the 255 guarantee.
#[test]
fn validate_does_not_inherit_exact_opaque_unless_asked() {
    let d = tempfile::tempdir().unwrap();
    let dds = bc7_flat_alpha_file(d.path(), "rock_m.dds", 127, 0);
    for target in ["cs", "vanilla"] {
        let o = validate(&["--target", target, "--json"], &dds);
        assert_eq!(code(&o), 0, "{target}: {}", text(&o.stdout));
        let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
        assert_eq!(v["subtype"], "envmask", "{v}");
        assert_eq!(v["alpha_policy"], "remove", "{v}");
        let codes: Vec<&str> = v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["code"].as_str().unwrap())
            .collect();
        assert!(codes.contains(&"ALPHA-254"), "{v}");
        assert!(!codes.contains(&"VAL-ALPHA-GUARANTEE"), "{v}");

        let o = validate(&["--target", target, "--exact-opaque", "--json"], &dds);
        assert_eq!(code(&o), 4, "{target}: {}", text(&o.stdout));
        let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
        assert!(
            v["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w["code"] == "VAL-ALPHA-GUARANTEE"
                    && w["message"].as_str().unwrap().contains("guarantee 255")),
            "{v}"
        );
    }
}

/// Codex-CLI 5 (DECISIONS Note L): for sub-type inference a stored alpha whose minimum is 251 or more is opaque,
/// one at 250 is varying. Under `cs` that is the difference between `envmask` and `complex_material` for `_m`.
#[test]
fn validate_infers_varying_alpha_below_the_note_l_cutoff() {
    let d = tempfile::tempdir().unwrap();
    for (name, a7, p, want) in [
        ("a250_m.dds", 125, 0, "complex_material"),
        ("a251_m.dds", 125, 1, "envmask"),
    ] {
        let dds = bc7_flat_alpha_file(d.path(), name, a7, p);
        let o = validate(&["--json"], &dds);
        let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
        assert_eq!(v["subtype"], want, "{name}: {v}");
    }
}

#[test]
fn inspect_prints_the_packing_recipes_of_a_set() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_fn(16, 16, |x, _| Luma([x as u8 * 16]))
        .save(d.path().join("iron_roughness.png"))
        .unwrap();
    let o = sdc().arg("inspect").arg(d.path()).output().unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("pack iron_rmaos.dds (rmaos) run"), "{out}");
    assert!(
        out.contains("R source:iron_roughness.png:R")
            && out.contains("G default:0")
            && out.contains("A default:1"),
        "{out}"
    );
}

#[test]
fn inspect_reports_nonfinite_samples_of_an_image() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("hot.tif");
    image::DynamicImage::ImageRgb32F(ImageBuffer::from_fn(4, 4, |x, _| {
        Rgb([if x == 0 { f32::NAN } else { 0.5 }, 0.5, 0.5])
    }))
    .save(&p)
    .unwrap();
    let o = sdc()
        .args(["inspect", "--set", "subtype=generic_data"])
        .arg(&p)
        .output()
        .unwrap();
    let all = format!("{}{}", text(&o.stdout), text(&o.stderr));
    assert!(all.contains("IN-NONFINITE"), "{all}");
}

/// Controller ruling (Task 6): an sdc RGBA16F output is judged by `validate` and passes. This does not guard the
/// `is_hdr()` tag-n/a inference in `verbs.rs` (a revert to BC6H-only would still pass): validate reads the tag only
/// through `srgb_tag`, which is false for both `n/a` and `unorm`.
#[test]
fn validate_judges_an_rgba16f_output_with_tag_na() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    std::fs::create_dir_all(&src).unwrap();
    image::DynamicImage::ImageRgb32F(ImageBuffer::from_fn(16, 16, |x, _| {
        Rgb([x as f32 / 4.0, 0.5, 0.25])
    }))
    .save(src.join("lamp_g.tif"))
    .unwrap();
    let out = d.path().join("out");
    let o = sdc()
        .args([
            "convert",
            "--accept-suggestions",
            "high",
            "--set",
            "format=rgba16f",
            "--set",
            "color.tag=n/a",
            "-o",
        ])
        .arg(&out)
        .arg(src.join("lamp_g.tif"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let o = validate(
        &["--json", "--subtype", "pbr_emissive"],
        &out.join("lamp_g.dds"),
    );
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    let v: serde_json::Value = serde_json::from_str(text(&o.stdout).trim()).unwrap();
    assert_eq!(
        (v["judged"].as_bool(), v["passed"].as_bool()),
        (Some(true), Some(true)),
        "{v}"
    );
}

/// DECISIONS S5: sdc's own uncompressed output is judged fully by `validate`, not header-only.
#[test]
fn validate_judges_an_uncompressed_sdc_output_in_full() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    std::fs::create_dir_all(&src).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128]))
        .save(src.join("wall.png"))
        .unwrap();
    let out = d.path().join("out");
    let o = sdc()
        .args([
            "convert",
            "--set",
            "subtype=diffuse",
            "--set",
            "format=rgba8",
            "-o",
        ])
        .arg(&out)
        .arg(src.join("wall.png"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let o = sdc()
        .args(["validate", "--json"])
        .arg(out.join("wall.dds"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let v: serde_json::Value =
        serde_json::from_str(text(&o.stdout).lines().next().unwrap()).unwrap();
    assert_eq!(
        (v["judged"].as_bool(), v["passed"].as_bool()),
        (Some(true), Some(true)),
        "{v}"
    );
}

/// DECISIONS S6: `inspect --set reencode=true` reports a DDS once, by its header, even next to images.
#[test]
fn inspect_reports_an_opted_in_dds_once() {
    let d = tempfile::tempdir().unwrap();
    let dds = converted_normal(d.path());
    let dir = dds.parent().unwrap();
    ImageBuffer::from_fn(8, 8, |_, _| Rgb([128u8, 128, 255]))
        .save(dir.join("rock_n.png"))
        .unwrap();
    let o = sdc()
        .args(["inspect", "--json", "--set", "reencode=true"])
        .arg(dir)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let lines: Vec<serde_json::Value> = text(&o.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let about_dds = |kind: &str| {
        lines
            .iter()
            .filter(|v| v["kind"] == kind && v.to_string().contains("wall_n.dds"))
            .count()
    };
    assert_eq!((about_dds("dds_info"), about_dds("file_planned")), (1, 0));
    assert_eq!(
        lines.iter().filter(|v| v["kind"] == "file_planned").count(),
        1
    ); // rock_n.png
}

#[test]
fn inspect_prints_the_set_shader_and_its_warnings() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("in/textures/a");
    std::fs::create_dir_all(&a).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([120u8, 90, 60]))
        .save(a.join("wall.png"))
        .unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
        .save(a.join("wall_n.png"))
        .unwrap();
    let o = sdc()
        .args([
            "inspect",
            "--recursive",
            "--target",
            "vanilla",
            "--set",
            "set:wall.shader=glow",
        ])
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("shader glow (set)"), "{out}");
    assert!(out.contains("SHADER-MISSING-MAP"), "{out}");
}

#[test]
fn a_bad_set_scope_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("in")).unwrap();
    let o = sdc()
        .args(["inspect", "--set", "set:wall.shader=nope"])
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
    assert!(
        text(&o.stderr).contains("unknown shader"),
        "{}",
        text(&o.stderr)
    );
}

/// Task 5 fix: a bad user-folder preset is reported by `inspect` on every path, exactly once (DDS-only and empty
/// inputs return before any plan; an image input plans and the plan reports it).
#[test]
fn inspect_reports_a_skipped_user_preset_once_on_every_path() {
    let d = tempfile::tempdir().unwrap();
    let appdata = d.path().join("appdata");
    std::fs::create_dir_all(appdata.join("sdc/presets")).unwrap();
    std::fs::write(
        appdata.join("sdc/presets/bad.toml"),
        "schema_version = 1\nid = \"bad\"\nversion = \"1\"\nbase = \"nope\"\n",
    )
    .unwrap();
    let dds_only = d.path().join("dds_only");
    std::fs::create_dir_all(&dds_only).unwrap();
    std::fs::write(dds_only.join("junk.dds"), b"not a dds").unwrap();
    let empty = d.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let image = d.path().join("image");
    std::fs::create_dir_all(&image).unwrap();
    ImageBuffer::from_fn(8, 8, |x, y| Rgb([x as u8, y as u8, 1]))
        .save(image.join("a.png"))
        .unwrap();
    for (input, name) in [
        (&dds_only, "dds-only"),
        (&empty, "empty"),
        (&image, "image"),
    ] {
        for json in [true, false] {
            let mut c = sdc();
            c.env("APPDATA", &appdata).arg("inspect");
            if json {
                c.arg("--json");
            }
            let o = c.arg(input).output().unwrap();
            let out = text(&o.stdout);
            assert_eq!(
                out.matches("CFG-PRESET-SKIPPED").count(),
                1,
                "{name} json={json}: {out}\n{}",
                text(&o.stderr)
            );
        }
    }
}

/// Final review Minor 4: the skipped user presets of one `inspect --json` run share one batch id.
#[test]
fn inspect_json_reports_every_skipped_preset_under_one_batch_id() {
    let d = tempfile::tempdir().unwrap();
    let appdata = d.path().join("appdata");
    std::fs::create_dir_all(appdata.join("sdc/presets")).unwrap();
    for name in ["bad1", "bad2"] {
        std::fs::write(
            appdata.join(format!("sdc/presets/{name}.toml")),
            format!("schema_version = 1\nid = \"{name}\"\nversion = \"1\"\nbase = \"nope\"\n"),
        )
        .unwrap();
    }
    let empty = d.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let o = sdc()
        .env("APPDATA", &appdata)
        .args(["inspect", "--json"])
        .arg(&empty)
        .output()
        .unwrap();
    let skips: Vec<serde_json::Value> = text(&o.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["code"] == "CFG-PRESET-SKIPPED")
        .collect();
    assert_eq!(skips.len(), 2, "{}", text(&o.stdout));
    assert_eq!(skips[0]["batch"], skips[1]["batch"], "{skips:?}");
    assert_ne!(skips[0]["seq"], skips[1]["seq"], "{skips:?}");
}
