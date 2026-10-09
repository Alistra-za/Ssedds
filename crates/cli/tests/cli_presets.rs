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
    assert!(["sse2", "sse4", "avx", "avx2"].contains(&v["isa"].as_str().unwrap()));
    assert_eq!(v["nvtt"]["found"], false);
    assert_eq!(v["nvtt"]["source"], "--nvtt");
    // Any existing file passed to --nvtt counts as found: detection only looks at the path.
    let exe = env!("CARGO_BIN_EXE_sdc");
    let (_, out, _) = stdout_of(&["encoders", "--json", "--nvtt", exe]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["nvtt"]["found"], true);
    assert!(v["nvtt"]["path"].as_str().unwrap().ends_with("sdc.exe"));
}
