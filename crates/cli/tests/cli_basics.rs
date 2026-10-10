use image::{ImageBuffer, Rgb};
use std::path::Path;
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

/// `<root>/in/textures/a/wall_n.png`, a non-PBR `_n`: high-confidence `normal`, so
/// `--accept-suggestions high` converts it.
fn normal_fixture(root: &Path) {
    let t = root.join("in/textures/a");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128]))
        .save(t.join("wall_n.png"))
        .unwrap();
}

fn events(stdout: &[u8]) -> Vec<serde_json::Value> {
    text(stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l}: {e}")))
        .collect()
}

#[test]
fn help_and_version_exit_0_bad_flag_exits_2() {
    assert_eq!(code(&sdc().arg("--help").output().unwrap()), 0);
    let v = sdc().arg("--version").output().unwrap();
    assert_eq!(code(&v), 0);
    assert!(text(&v.stdout).contains("sdc"));
    for args in [
        &["convert", "--no-such-flag", "x"][..],
        &["convert", "x", "--set", "colour.tag=srgb"],
        &["convert", "does-not-exist"],
        &["convert", "x", "--allow", "NOT-A-CODE"],
        &["convert", "x", "--target", "pc"],
        &["convert"],
    ] {
        let o = sdc().args(args).output().unwrap();
        assert_eq!(code(&o), 2, "{args:?}: {}", text(&o.stderr));
        assert!(
            o.stdout.is_empty(),
            "{args:?}: usage errors write no stdout"
        );
    }
}

#[test]
fn bad_mem_budget_is_a_usage_error() {
    for bad in ["8Q", "G", "-1", "99999999999999999999G"] {
        let o = sdc()
            .args(["convert", "--mem-budget", bad, "x"])
            .output()
            .unwrap();
        assert_eq!(code(&o), 2, "{bad}");
    }
}

#[test]
fn json_mode_puts_only_json_lines_on_stdout() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let o = sdc()
        .args([
            "convert",
            "--json",
            "--recursive",
            "--accept-suggestions",
            "high",
            "-o",
        ])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let ev = events(&o.stdout);
    assert!(ev.len() >= 4);
    assert!(ev.iter().all(|v| v["schema"] == 2));
    assert_eq!(ev.last().unwrap()["kind"], "batch_done");
    assert_eq!(ev[0]["kind"], "batch_started");
    // The human log is on stderr, never mixed into stdout.
    let err = text(&o.stderr);
    assert!(err.contains("ok    "), "{err}");
    assert!(err.contains("done: 1 converted"), "{err}");
    assert!(d.path().join("out/textures/a/wall_n.dds").exists());
}

#[test]
fn without_json_stdout_stays_empty_and_the_log_is_on_stderr() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let o = sdc()
        .args([
            "convert",
            "--recursive",
            "--accept-suggestions",
            "high",
            "-o",
        ])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(o.stdout.is_empty(), "{}", text(&o.stdout));
    assert!(text(&o.stderr).contains("done: 1 converted"));
}

#[test]
fn unaccepted_suggestion_skips_the_file_and_exits_1() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let o = sdc()
        .args(["convert", "--json", "--recursive", "-o"])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    let ev = events(&o.stdout);
    assert!(ev.iter().any(|v| v["kind"] == "file_skipped"));
    assert!(!d.path().join("out/textures/a/wall_n.dds").exists());
}

#[test]
fn target_is_optional_and_passed_through_only_when_given() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let run = |extra: &[&str]| {
        let o = sdc()
            .args(["convert", "--json", "--dry-run", "--recursive", "-o"])
            .arg(d.path().join("out"))
            .arg(d.path().join("in"))
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(code(&o), 1, "{}", text(&o.stderr)); // unresolved without acceptance
        events(&o.stdout)[0].clone()
    };
    let none = run(&[]);
    assert_eq!(none["kind"], "batch_started");
    assert_eq!(none["target"], "cs"); // the documented fall-back when nothing states a target
    assert_eq!(run(&["--target", "vanilla"])["target"], "vanilla");
}

#[test]
fn project_config_target_is_used_when_no_target_flag_is_given() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let cfg = d.path().join("proj.toml");
    std::fs::write(&cfg, "target = \"cpm\"\n").unwrap();
    let o = sdc()
        .args(["convert", "--json", "--dry-run", "--recursive", "--config"])
        .arg(&cfg)
        .arg("-o")
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    let ev = events(&o.stdout);
    assert_eq!(ev[0]["target"], "cpm", "{}", text(&o.stderr));
}

#[test]
fn dry_run_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let o = sdc()
        .args([
            "convert",
            "--dry-run",
            "--recursive",
            "--accept-suggestions",
            "high",
            "-o",
        ])
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(!d.path().join("out/textures/a/wall_n.dds").exists());
}

#[test]
fn intentional_skips_do_not_make_the_batch_exit_1() {
    // DECISIONS Note I: an output kept by `--on-existing skip` is a deliberate skip.
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let run = |on_existing: &str| {
        sdc()
            .args([
                "convert",
                "--json",
                "--recursive",
                "--accept-suggestions",
                "high",
                "--on-existing",
                on_existing,
                "-o",
            ])
            .arg(d.path().join("out"))
            .arg(d.path().join("in"))
            .output()
            .unwrap()
    };
    assert_eq!(code(&run("if-changed")), 0);
    let second = run("skip");
    assert_eq!(code(&second), 0, "{}", text(&second.stderr));
    let done = events(&second.stdout).pop().unwrap();
    assert_eq!(done["kind"], "batch_done");
    assert_eq!(done["converted"], 0);
    assert_eq!(done["skipped"], 1);
    assert_eq!(done["skipped_intentional"], 1);
}

#[test]
fn preset_file_loads_alias_presets_without_selecting_them() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("in/textures/armor");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([200, x as u8 * 2, 40 + y as u8]))
        .save(t.join("iron_rmaosultrahigh.png"))
        .unwrap();
    let ultra = d.path().join("ultra.toml");
    std::fs::write(
        &ultra,
        "schema_version = 1\nid = \"ultra\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\
         [suffix_aliases]\nrmaosultrahigh = \"rmaos\"\n[subtypes.rmaos]\nmips = { levels = 1 }\n",
    )
    .unwrap();
    let o = sdc()
        .args([
            "convert",
            "--json",
            "--recursive",
            "--accept-suggestions",
            "high",
            "--preset-file",
        ])
        .arg(&ultra)
        .arg("-o")
        .arg(d.path().join("out"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let ev = events(&o.stdout);
    // Loaded, not selected: the batch preset is still the target's default.
    assert_eq!(ev[0]["kind"], "batch_started");
    assert_eq!(ev[0]["preset"], "cs-quality");
    assert!(d.path().join("out/textures/armor/iron_rmaos.dds").exists());

    // `--preset <file.toml>` loads and selects the preset the file declares.
    let o = sdc()
        .args([
            "convert",
            "--json",
            "--dry-run",
            "--recursive",
            "--accept-suggestions",
            "high",
            "--preset",
        ])
        .arg(&ultra)
        .arg("-o")
        .arg(d.path().join("out2"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert_eq!(events(&o.stdout)[0]["preset"], "ultra");
}

#[test]
fn invalid_preset_or_alias_is_a_config_error_exit_2() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    let bad = d.path().join("bad.toml");
    std::fs::write(
        &bad,
        "schema_version = 1\nid = \"bad\"\nversion = \"1.0.0\"\n[suffix_aliases]\nn = \"rmaos\"\n",
    )
    .unwrap();
    for flag in ["--preset-file", "--preset"] {
        let o = sdc()
            .args(["convert", "--recursive", flag])
            .arg(&bad)
            .arg("-o")
            .arg(d.path().join("out"))
            .arg(d.path().join("in"))
            .output()
            .unwrap();
        assert_eq!(code(&o), 2, "{flag}: {}", text(&o.stderr));
    }
    let o = sdc()
        .args(["convert", "--recursive", "--preset-file"])
        .arg(d.path().join("missing.toml"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
    let o = sdc()
        .args(["convert", "--recursive", "--preset", "no-such-preset"])
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 2);
}

#[test]
fn unwritable_output_root_is_fatal_exit_3() {
    let d = tempfile::tempdir().unwrap();
    normal_fixture(d.path());
    // The output root's parent is a regular file, so the root cannot be created.
    let blocker = d.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let o = sdc()
        .args([
            "convert",
            "--recursive",
            "--accept-suggestions",
            "high",
            "-o",
        ])
        .arg(blocker.join("out"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 3, "{}", text(&o.stderr));
}
