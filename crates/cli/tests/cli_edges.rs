//! `sdc convert` edge cases through the real binary (spec §8, Task 44): exit-code precedence, panic isolation,
//! `--mem-budget`, read-only outputs, an output root that is a file, and non-ASCII paths.
//!
//! Every test points `APPDATA` at a path that does not exist, so the developer's `%APPDATA%\sdc\config.toml`
//! is never read. `SDC_TEST_PANIC_ON`, `SDC_TEST_FAIL_VALIDATION` and `SDC_TEST_CANCEL_ON` are engine test hooks (cargo feature `test-hooks`,
//! which this crate's dev-dependency on `engine` enables for `cargo test`), set on the child only.
use image::{ImageBuffer, Rgb};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// `sdc convert --json --recursive --set subtype=diffuse <extra> -o <out> <input>`.
fn convert(input: &Path, out: &Path, extra: &[&str]) -> Command {
    let mut c = sdc();
    c.args([
        "convert",
        "--json",
        "--recursive",
        "--set",
        "subtype=diffuse",
    ])
    .args(extra)
    .arg("-o")
    .arg(out)
    .arg(input);
    c
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("sdc was killed by a signal")
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn events(o: &Output) -> Vec<Value> {
    String::from_utf8(o.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l}: {e}")))
        .collect()
}

fn batch_done(o: &Output) -> Value {
    let ev = events(o).pop().expect("no events");
    assert_eq!(ev["kind"], "batch_done", "{}", stderr(o));
    ev
}

/// `file_failed` events as `(input file name, stage, error)`.
fn failures(o: &Output) -> Vec<(String, String, String)> {
    events(o)
        .iter()
        .filter(|e| e["kind"] == "file_failed")
        .map(|e| {
            (
                e["path"]
                    .as_str()
                    .unwrap()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_string(),
                e["stage"].as_str().unwrap().to_string(),
                e["error"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// Every file under `dir`, relative with `/` separators.
fn tree(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, into: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, into);
            } else {
                into.push(
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut v = vec![];
    walk(dir, dir, &mut v);
    v.sort();
    v
}

fn no_temp_files(dir: &Path) {
    let leftovers: Vec<_> = tree(dir)
        .into_iter()
        .filter(|f| f.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
}

#[test]
fn a_panicking_file_fails_alone() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/boom.png"));
    img(&d.path().join("in/fine.png"));
    let out = d.path().join("out");
    let o = convert(&d.path().join("in"), &out, &[])
        .env("SDC_TEST_PANIC_ON", "boom.png")
        .output()
        .unwrap();
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert!(stderr(&o).contains("internal error"), "{}", stderr(&o));
    assert!(out.join("fine.dds").exists());
    assert!(!out.join("boom.dds").exists());
    no_temp_files(&out);
    let f = failures(&o);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(f[0].0, "boom.png");
    assert!(f[0].2.starts_with("internal error"), "{f:?}");
    let done = batch_done(&o);
    assert_eq!(
        (done["converted"].as_u64(), done["failed"].as_u64()),
        (Some(1), Some(1))
    );
}

/// Partial (1) outranks a validation failure (4) in the same batch, and fatal (3) outranks partial (1).
#[test]
fn partial_outranks_validation_failed_and_fatal_outranks_partial() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/boom.png"));
    img(&d.path().join("in/checker.png"));
    let out = d.path().join("out");
    let run = |panic_on: Option<&str>| {
        let mut c = convert(&d.path().join("in"), &out, &["--on-existing", "overwrite"]);
        c.env("SDC_TEST_FAIL_VALIDATION", "checker");
        if let Some(p) = panic_on {
            c.env("SDC_TEST_PANIC_ON", p);
        }
        c.output().unwrap()
    };
    let o = run(None);
    assert_eq!(code(&o), 4, "{}", stderr(&o));
    let o = run(Some("boom.png"));
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert_eq!(batch_done(&o)["validation_failed"], 1);

    // An unresolved file is skipped (partial), then the output root cannot be created (fatal).
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    img(&d.path().join("in/lonely_unknown.png"));
    std::fs::write(d.path().join("outfile"), b"x").unwrap();
    let o = sdc()
        .args([
            "convert",
            "--recursive",
            "--set",
            "file:**/a.png.subtype=diffuse",
            "-o",
        ])
        .arg(d.path().join("outfile"))
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 3, "{}", stderr(&o));
    assert!(
        stderr(&o).contains("skip"),
        "the unresolved file is reported first: {}",
        stderr(&o)
    );
}

#[test]
fn output_root_that_is_a_file_is_fatal() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    std::fs::write(d.path().join("outfile"), b"x").unwrap();
    let o = convert(&d.path().join("in"), &d.path().join("outfile"), &[])
        .output()
        .unwrap();
    assert_eq!(code(&o), 3, "{}", stderr(&o));
    assert!(stderr(&o).contains("output root"), "{}", stderr(&o));
    assert_eq!(std::fs::read(d.path().join("outfile")).unwrap(), b"x");
}

/// `0` is not "default": it is a usage error (exit 2) before any work, and nothing is written.
#[test]
fn zero_mem_budget_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    let out = d.path().join("out");
    for zero in ["0", "0G", "0M"] {
        let o = convert(&d.path().join("in"), &out, &["--mem-budget", zero])
            .output()
            .unwrap();
        assert_eq!(code(&o), 2, "{zero}: {}", stderr(&o));
        assert!(stderr(&o).contains("--mem-budget"), "{}", stderr(&o));
        assert!(stderr(&o).contains("greater than 0"), "{}", stderr(&o));
        assert!(o.stdout.is_empty(), "no events before the batch starts");
        assert!(!out.exists());
    }
    // A real budget still works.
    let o = convert(&d.path().join("in"), &out, &["--mem-budget", "512M"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", stderr(&o));
}

/// Spec §8 "Read-only or locked output": `file_failed` at Write. The read-only file is untouched, no temp file or
/// manifest record is left for it, and the other files of the batch are converted.
#[test]
fn read_only_output_file_fails_at_write_alone() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/locked.png"));
    img(&d.path().join("in/free.png"));
    let out = d.path().join("out");
    let first = convert(&d.path().join("in"), &out, &[]).output().unwrap();
    assert_eq!(code(&first), 0, "{}", stderr(&first));
    let locked = out.join("locked.dds");
    let before = std::fs::read(&locked).unwrap();
    let manifest_before = std::fs::read(out.join("_sdc/manifest.jsonl")).unwrap();
    let mut perm = std::fs::metadata(&locked).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&locked, perm.clone()).unwrap();

    let o = convert(&d.path().join("in"), &out, &["--on-existing", "overwrite"])
        .output()
        .unwrap();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&locked, perm).unwrap();

    assert_eq!(code(&o), 1, "{}", stderr(&o));
    assert!(stderr(&o).contains("Write"), "{}", stderr(&o));
    let f = failures(&o);
    assert_eq!(f.len(), 1, "{f:?}");
    assert_eq!(
        (f[0].0.as_str(), f[0].1.as_str()),
        ("locked.png", "write"),
        "{f:?}"
    );
    let done = batch_done(&o);
    assert_eq!(
        (done["converted"].as_u64(), done["failed"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(
        std::fs::read(&locked).unwrap(),
        before,
        "the read-only output must be untouched"
    );
    no_temp_files(&out);
    // Only `free.dds` got a second record.
    let manifest_after =
        String::from_utf8(std::fs::read(out.join("_sdc/manifest.jsonl")).unwrap()).unwrap();
    let new = &manifest_after[manifest_before.len()..];
    assert_eq!(new.lines().count(), 1, "{new}");
    assert!(new.contains("\"free.dds\""), "{new}");
}

/// Denies file creation in a folder with an ACL until dropped (a read-only attribute on a folder does nothing on
/// Windows). `None` when `icacls` cannot set the ACL, e.g. on a filesystem without ACLs.
struct DenyWrite(PathBuf);

const EVERYONE: &str = "*S-1-1-0";

impl DenyWrite {
    fn new(dir: &Path) -> Option<Self> {
        let ok = Command::new("icacls")
            .arg(dir)
            .args(["/deny", &format!("{EVERYONE}:(OI)(CI)(WD,AD)")])
            .output()
            .ok()?
            .status
            .success();
        ok.then(|| DenyWrite(dir.to_path_buf()))
    }
}

impl Drop for DenyWrite {
    fn drop(&mut self) {
        let _ = Command::new("icacls")
            .arg(&self.0)
            .args(["/remove:d", EVERYONE])
            .output();
    }
}

/// A folder that rejects new files: every file in it fails at Write (exit 1), nothing partial is left, and the
/// batch is not fatal because the output root itself exists. Windows only (the ACL is the mechanism). When the
/// ACL cannot be set, or is set but does not stop this process from writing (e.g. a privilege that bypasses
/// deny entries), the test prints `SKIPPED` with the reason and returns; libtest still reports it as passed.
/// Owner ruling (DECISIONS Note Y9): `--nvtt` on convert is a usage error until the NVTT backend exists (M5).
#[test]
fn nvtt_on_convert_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    let out = d.path().join("out");
    let o = convert(&d.path().join("in"), &out, &["--nvtt", "nvcompress.exe"])
        .output()
        .unwrap();
    assert_eq!(code(&o), 2, "{}", stderr(&o));
    assert!(
        stderr(&o).contains("the NVTT backend is not available yet (M5)"),
        "{}",
        stderr(&o)
    );
    assert!(o.stdout.is_empty(), "nothing runs");
    assert!(!out.exists());
}

#[cfg(windows)]
#[test]
fn read_only_output_folder_fails_at_write() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    img(&d.path().join("in/b.png"));
    let out = d.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let Some(_deny) = DenyWrite::new(&out) else {
        eprintln!("SKIPPED read_only_output_folder_fails_at_write: icacls could not deny writes on the temp folder");
        return;
    };
    if std::fs::write(out.join("probe"), b"x").is_ok() {
        eprintln!(
            "SKIPPED read_only_output_folder_fails_at_write: the deny ACL is ineffective for this process,              so the test would prove nothing"
        );
        return;
    }
    let o = convert(&d.path().join("in"), &out, &[]).output().unwrap();
    assert_eq!(code(&o), 1, "{}", stderr(&o));
    let f = failures(&o);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(f.iter().all(|(_, stage, _)| stage == "write"), "{f:?}");
    let done = batch_done(&o);
    assert_eq!(
        (done["converted"].as_u64(), done["failed"].as_u64()),
        (Some(0), Some(2))
    );
    assert!(tree(&out).is_empty(), "left behind: {:?}", tree(&out));
}

/// Non-ASCII file and folder names round-trip: discover, output name, `file_done` output, manifest key, and the
/// next run's up-to-date lookup. The manifest key is case-folded (Unicode lower case); the event keeps the case.
#[test]
fn non_ascii_paths_round_trip() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/textures/Ärmör_n.png"));
    img(&d.path().join("in/textures/木材/鉄の鎧_n.png"));
    img(&d.path().join("in/ïron/Åxe.png"));
    let out = d.path().join("出力");
    let o = convert(&d.path().join("in"), &out, &[]).output().unwrap();
    assert_eq!(code(&o), 0, "{}", stderr(&o));

    let expected = [
        "textures/Ärmör_n.dds",
        "textures/木材/鉄の鎧_n.dds",
        "ïron/Åxe.dds",
    ];
    for rel in expected {
        assert!(out.join(rel).exists(), "{rel}: {:?}", tree(&out));
    }
    // JSON Lines: valid UTF-8 JSON with the names intact.
    let ev = events(&o);
    let of = |kind: &str, field: &str| -> Vec<String> {
        ev.iter()
            .filter(|e| e["kind"] == kind)
            .map(|e| e[field].as_str().unwrap().to_string())
            .collect()
    };
    let done = of("file_done", "output");
    for rel in expected {
        assert!(
            done.iter().any(|o| o.ends_with(rel)),
            "{rel} not in {done:?}"
        );
    }
    let started = of("file_started", "path");
    assert!(
        started.iter().any(|p| p.ends_with("Ärmör_n.png")),
        "{started:?}"
    );
    assert!(
        started.iter().any(|p| p.ends_with("木材/鉄の鎧_n.png")),
        "{started:?}"
    );
    // Manifest keys: root-relative, `/`, lower case.
    let manifest = std::fs::read_to_string(out.join("_sdc/manifest.jsonl")).unwrap();
    let keys: Vec<String> = manifest
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["output"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    for rel in expected {
        assert!(keys.contains(&rel.to_lowercase()), "{rel} not in {keys:?}");
    }
    // The record lookup finds them again: nothing is converted twice.
    let o = convert(&d.path().join("in"), &out, &[]).output().unwrap();
    assert_eq!(code(&o), 0, "{}", stderr(&o));
    let done = batch_done(&o);
    assert_eq!(
        (done["converted"].as_u64(), done["up_to_date"].as_u64()),
        (Some(0), Some(3)),
        "{}",
        stderr(&o)
    );
    // The human log (stderr) names them too.
    assert!(stderr(&o).contains("Ärmör_n.png"), "{}", stderr(&o));
    no_temp_files(&out);
}

/// Codex-CLI 4: a Ctrl-C while planning (simulated by the `SDC_TEST_CANCEL_ON` hook, which cancels just before
/// the named file loads) is not a success: a dry run and a real run both list every pending file as cancelled
/// and exit 1, and nothing is written.
#[test]
fn a_cancel_while_planning_exits_1_and_lists_the_pending_files() {
    let d = tempfile::tempdir().unwrap();
    img(&d.path().join("in/a.png"));
    img(&d.path().join("in/b.png"));
    let out = d.path().join("out");
    for extra in [&["--dry-run"][..], &[]] {
        let o = convert(&d.path().join("in"), &out, extra)
            .env("SDC_TEST_CANCEL_ON", "b.png")
            .output()
            .unwrap();
        assert_eq!(code(&o), 1, "{extra:?}: {}", stderr(&o));
        let done = batch_done(&o);
        let cancelled: Vec<&str> = done["cancelled"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().rsplit('/').next().unwrap())
            .collect();
        assert_eq!(cancelled, ["a.png", "b.png"], "{extra:?}: {done}");
        assert_eq!(done["converted"], 0);
        assert!(stderr(&o).contains("2 cancelled"), "{}", stderr(&o));
        assert!(!out.join("a.dds").exists() && !out.join("b.dds").exists());
    }
}
