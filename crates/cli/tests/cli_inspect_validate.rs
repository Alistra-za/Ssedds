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
