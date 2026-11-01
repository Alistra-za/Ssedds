use std::process::Command;

fn sdc() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sdc"));
    // Keep the developer's real user config out of the tests.
    c.env(
        "APPDATA",
        std::env::temp_dir().join("sdc-test-appdata-none"),
    );
    c
}

#[test]
fn presets_list_show_and_encoders() {
    let (code, out, _) = stdout_of(&["presets", "list"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("cs-quality") && out.contains("vanilla-quality"),
        "{out}"
    );
    let (code, out, _) = stdout_of(&["presets", "show", "cs-quality", "--subtype", "pbr_albedo"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("\"tag\": \"srgb\"") && out.contains("builtin"),
        "{out}"
    );
    assert_eq!(stdout_of(&["presets", "show", "nope"]).0, 2);
    let (code, out, _) = stdout_of(&["encoders"]);
    assert_eq!(code, 0);
    assert!(out.contains("b943862") && out.contains("1.31.0"), "{out}");
}

fn stdout_of(args: &[&str]) -> (i32, String, String) {
    let o = sdc().args(args).output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn presets_list_json_has_one_object_per_preset_with_aliases() {
    let (code, out, _) = stdout_of(&["presets", "list", "--json"]);
    assert_eq!(code, 0);
    let rows: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let ids: Vec<&str> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert!(
        ids.contains(&"cs-quality") && ids.contains(&"vanilla-quality"),
        "{ids:?}"
    );
    assert!(!ids.contains(&"sdc-base"), "hidden presets are not listed");
    assert!(rows.iter().all(|r| r["suffix_aliases"].is_object()));
}

#[test]
fn presets_show_reports_provenance_and_rejects_bad_arguments() {
    let (code, out, _) = stdout_of(&["presets", "show", "cs-quality", "--subtype", "normal"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["subtype"], "normal");
    assert!(
        v["provenance"].as_object().is_some_and(|m| !m.is_empty()),
        "{out}"
    );
    assert_eq!(
        stdout_of(&["presets", "show", "cs-quality", "--subtype", "nope"]).0,
        2
    );
    // Without --subtype every sub-type is shown, one object each.
    let (code, out, _) = stdout_of(&["presets", "show", "vanilla-quality", "--target", "vanilla"]);
    assert_eq!(code, 0);
    assert!(out.matches("\"provenance\"").count() >= 20, "{out}");
}

#[test]
fn presets_schema_prints_one_schema_or_writes_all() {
    let (code, out, _) = stdout_of(&["presets", "schema", "event"]);
    assert_eq!(code, 0);
    let committed = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schema/event.schema.json"
    ))
    .unwrap();
    assert_eq!(out, committed.replace("\r\n", "\n"));
    // `all` without a folder is a usage error; with one it writes every committed file.
    assert_eq!(stdout_of(&["presets", "schema", "all"]).0, 2);
    assert_eq!(stdout_of(&["presets", "schema", "bogus"]).0, 2);
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("out");
    let (code, _, _) = stdout_of(&["presets", "schema", "all", "--out", dir.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 7);
    let (code, out, _) = stdout_of(&[
        "presets",
        "schema",
        "dds-validation",
        "--out",
        dir.to_str().unwrap(),
    ]);
    assert_eq!(
        (code, out.contains("dds-validation.schema.json")),
        (0, true)
    );
}

#[test]
fn encoders_reports_versions_isa_model_and_nvtt() {
    let (code, out, err) = stdout_of(&["encoders"]);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "result verbs print on stdout: {err}");
    for needle in [
        "b943862",
        "79ddbc9",
        "1.31.0",
        "0001-exact-opaque-alpha@sha256:",
        "patch bc4-round-to-nearest+bc4-search@sha256:",
        "dispatched ISA",
        "BC1 decoder model: ideal",
    ] {
        assert!(out.contains(needle), "{needle} not in {out}");
    }
    let (_, out, _) = stdout_of(&["encoders", "--bc1-decoder", "nvidia"]);
    assert!(out.contains("BC1 decoder model: nvidia"), "{out}");
}

#[test]
fn encoders_json_is_one_object_and_nvtt_comes_from_the_flag() {
    let (code, out, _) = stdout_of(&["encoders", "--json", "--nvtt", "no/such/nvcompress.exe"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 1, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["kind"], "encoders");
    assert_eq!(v["ispc"], "1.31.0");
    assert_eq!(v["bc1_model"], "ideal");
    assert!(v["bc7e_patch"].as_str().unwrap().contains("@sha256:"));
    assert!(v["rgbcx_patch"]
        .as_str()
        .unwrap()
        .starts_with("bc4-round-to-nearest+bc4-search@sha256:"));
    assert!(["sse2", "sse4", "avx", "avx2"].contains(&v["isa"].as_str().unwrap()));
    assert_eq!(v["nvtt"]["found"], false);
    assert_eq!(v["nvtt"]["source"], "--nvtt");
    // M5 (DECISIONS Note NV3, AF3): an existing file that is sdc itself is refused, never run.
    let exe = env!("CARGO_BIN_EXE_sdc");
    let (_, out, _) = stdout_of(&["encoders", "--json", "--nvtt", exe]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["nvtt"]["found"], false);
    assert!(v["nvtt"]["path"].as_str().unwrap().ends_with("sdc.exe"));
    assert!(
        v["nvtt"]["error"].as_str().unwrap().contains("sdc itself"),
        "{v}"
    );
}

fn sdc_in(appdata: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_sdc"))
        .env("APPDATA", appdata)
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

const MINE: &str = "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n";

fn appdata_with_mine() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("sdc/presets")).unwrap();
    std::fs::write(d.path().join("sdc/presets/mine.toml"), MINE).unwrap();
    std::fs::write(d.path().join("sdc/presets/broken.toml"), "nope").unwrap();
    d
}

/// Spec §4: `list` shows a folder preset with its source; `--json` has `source` on every row.
#[test]
fn list_shows_user_presets_with_their_source() {
    let d = appdata_with_mine();
    let (code, out, err) = sdc_in(d.path(), &["presets", "list"]);
    assert_eq!(code, 0, "{err}");
    let line = out.lines().find(|l| l.starts_with("mine")).unwrap();
    assert!(line.contains("mine.toml"), "{line}");
    assert!(
        out.contains("cs-compact") && out.contains("fast-preview"),
        "{out}"
    );
    assert!(
        err.contains("CFG-PRESET-SKIPPED") && err.contains("broken.toml"),
        "{err}"
    );
    let (_, out, _) = sdc_in(d.path(), &["presets", "list", "--json"]);
    let rows: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let src = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap()["source"].clone();
    assert_eq!(src("cs-quality"), "builtin");
    assert!(src("mine").as_str().unwrap().ends_with("mine.toml"));
}

/// Spec §3: `--preset-file` works on every verb, and a duplicate id is an error (exit 2).
#[test]
fn preset_file_on_the_verbs_and_duplicates() {
    let d = appdata_with_mine();
    let extra = d.path().join("extra.toml");
    std::fs::write(&extra, MINE.replace("\"mine\"", "\"extra\"")).unwrap();
    let (code, out, _) = sdc_in(
        d.path(),
        &["presets", "list", "--preset-file", extra.to_str().unwrap()],
    );
    assert_eq!(code, 0);
    assert!(out.lines().any(|l| l.starts_with("extra")), "{out}");
    let dup = d.path().join("dup.toml");
    std::fs::write(&dup, MINE).unwrap();
    let (code, _, err) = sdc_in(
        d.path(),
        &[
            "presets",
            "show",
            "mine",
            "--preset-file",
            dup.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("already loaded"), "{err}");
}

/// Spec §4: `diff` prints `<subtype>.<field>: a → b`, `no differences`, or JSON rows.
#[test]
fn diff_prints_only_differences() {
    let d = appdata_with_mine();
    let (code, out, err) = sdc_in(d.path(), &["presets", "diff", "cs-quality", "mine"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out.trim(),
        "diffuse.format: \"bc7\" → \"bc1\"\n(rules are not compared)"
    );
    let (code, out, _) = sdc_in(d.path(), &["presets", "diff", "cs-quality", "cs-quality"]);
    assert_eq!(
        (code, out.trim()),
        (0, "no differences\n(rules are not compared)")
    );
    let (code, out, _) = sdc_in(
        d.path(),
        &[
            "presets",
            "diff",
            "cs-quality",
            "cs-compact",
            "--subtype",
            "normal",
            "--json",
        ],
    );
    assert_eq!(code, 0);
    let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        rows,
        serde_json::json!([
            {"subtype": "normal", "field": "encoder.bc45_search_radius", "a": "default", "b": "fast"},
            {"subtype": "normal", "field": "resize", "a": null, "b": "half"}
        ])
    );
    assert_eq!(
        sdc_in(d.path(), &["presets", "diff", "cs-quality", "nope"]).0,
        2
    );
}

/// Spec §4: export writes files that validate; validate exits 1 on a bad file.
#[test]
fn export_then_validate() {
    let d = appdata_with_mine();
    let out = d.path().join("x.toml");
    let (code, _, err) = sdc_in(
        d.path(),
        &["presets", "export", "mine", "--out", out.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        text.contains("id = \"mine-export\"") && text.contains("base = \"cs-quality\""),
        "{text}"
    );
    let renamed = d.path().join("packed.toml");
    let (code, _, _) = sdc_in(
        d.path(),
        &[
            "presets",
            "export",
            "cs-compact",
            "--id",
            "packed",
            "--out",
            renamed.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0);
    assert!(std::fs::read_to_string(&renamed)
        .unwrap()
        .contains("id = \"packed\""));
    let (code, stdout, _) = sdc_in(d.path(), &["presets", "export", "fast-preview"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("id = \"fast-preview-export\""), "{stdout}");
    let (code, stdout, _) = sdc_in(
        d.path(),
        &[
            "presets",
            "validate",
            out.to_str().unwrap(),
            renamed.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(
        stdout.lines().filter(|l| l.starts_with("ok ")).count(),
        2,
        "{stdout}"
    );
    let bad = d.path().join("bad.toml");
    std::fs::write(
        &bad,
        "schema_version = 1\nid = \"b\"\nversion = \"1\"\nbase = \"nope\"\n",
    )
    .unwrap();
    let (code, stdout, _) = sdc_in(
        d.path(),
        &[
            "presets",
            "validate",
            out.to_str().unwrap(),
            bad.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 1);
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with(bad.to_str().unwrap()) && l.contains("nope")),
        "{stdout}"
    );
}

/// Fix round 1: `export --id <loaded id>` fails (exit 2) and writes no file; neither does the default id when taken.
#[test]
fn export_refuses_a_taken_id_and_writes_nothing() {
    let d = appdata_with_mine();
    let out = d.path().join("clash.toml");
    let (code, _, err) = sdc_in(
        d.path(),
        &[
            "presets",
            "export",
            "cs-compact",
            "--id",
            "cs-quality",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("already loaded"), "{err}");
    assert!(!out.exists());
    let first = d.path().join("first.toml");
    std::fs::write(&first, MINE.replace("\"mine\"", "\"mine-export\"")).unwrap();
    let (code, _, err) = sdc_in(
        d.path(),
        &[
            "presets",
            "export",
            "mine",
            "--preset-file",
            first.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 2, "{err}");
    assert!(!out.exists());
}

/// M6 Track D spec §8: the CLI help names the user presets folder.
#[test]
fn help_names_the_user_presets_folder() {
    for args in [&["presets", "--help"][..], &["convert", "--help"]] {
        let (code, out, _) = stdout_of(args);
        assert_eq!(code, 0);
        assert!(out.contains(r"%APPDATA%\sdc\presets"), "{args:?}: {out}");
    }
}

/// Final review Minor 10: `export --out` refuses to overwrite the source file of a loaded preset.
#[test]
fn export_refuses_to_overwrite_a_loaded_preset_file() {
    let d = appdata_with_mine();
    let src = d.path().join("sdc/presets/mine.toml");
    let (code, _, err) = sdc_in(
        d.path(),
        &[
            "presets",
            "export",
            "cs-compact",
            "--out",
            src.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("is the source of loaded preset 'mine'"),
        "{err}"
    );
    assert_eq!(std::fs::read_to_string(&src).unwrap(), MINE);
}

/// Final review Minors 8 and 13: `diff --help` says rules are not compared; `export --help` warns about aliases.
#[test]
fn diff_and_export_help_state_their_limits() {
    let (code, out, _) = stdout_of(&["presets", "diff", "--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("rules are not compared"), "{out}");
    let (code, out, _) = stdout_of(&["presets", "export", "--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("suffix aliases"), "{out}");
}
