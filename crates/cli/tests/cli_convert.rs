//! End-to-end checks of `sdc convert` through the real binary: spec §8 precedence (built-in, preset, user
//! config, project config, rules, accepted suggestions, `--set`), scoped overrides, suffix aliases, the batch
//! options (`--on-existing`, `--dry-run`, `--jobs`) and the DECISIONS Note I exit codes.
//!
//! Every test points `APPDATA` at its own temporary folder, so the developer's real `%APPDATA%\sdc\config.toml`
//! is never read or written.
use engine::dds::{read_dds, DdsPixelFormat};
use image::{ImageBuffer, Rgb};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// One test's sandbox: `in/textures/a/` (inputs), `out/` (outputs) and `appdata/` (the user config home).
struct Fx {
    root: tempfile::TempDir,
}

impl Fx {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("in/textures/a")).unwrap();
        Fx { root }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    /// A 32 px RGB gradient (no alpha, so `alpha.policy` resolves to `remove` by analysis) at `in/textures/a/<name>`.
    fn rgb(&self, name: &str) {
        self.rgb_sized(name, 32, 90);
    }

    fn rgb_sized(&self, name: &str, size: u32, blue: u8) {
        let step = (255 / size) as u8;
        ImageBuffer::from_fn(size, size, |x, y| {
            Rgb([x as u8 * step, y as u8 * step, blue])
        })
        .save(self.p(&format!("in/textures/a/{name}")))
        .unwrap();
    }

    /// A 16 px tangent-space-looking map; `_n` makes it a high-confidence `normal`.
    fn normal(&self, name: &str) {
        ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 16, y as u8 * 16, 128]))
            .save(self.p(&format!("in/textures/a/{name}")))
            .unwrap();
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.p(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn user_config(&self, text: &str) {
        self.write("appdata/sdc/config.toml", text);
    }

    fn project_config(&self, text: &str) {
        self.write("in/sdc.toml", text);
    }

    fn sdc(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_sdc"));
        c.env("APPDATA", self.p("appdata"));
        c
    }

    /// `sdc convert --json --recursive <extra> -o out in`.
    fn convert(&self, extra: &[&str]) -> Output {
        self.convert_to("out", extra)
    }

    fn convert_to(&self, out: &str, extra: &[&str]) -> Output {
        self.sdc()
            .args(["convert", "--json", "--recursive"])
            .args(extra)
            .arg("-o")
            .arg(self.p(out))
            .arg(self.p("in"))
            .output()
            .unwrap()
    }

    fn dds(&self, rel: &str) -> PathBuf {
        self.p("out").join(rel)
    }

    fn format(&self, rel: &str) -> DdsPixelFormat {
        read_dds(&std::fs::read(self.dds(rel)).unwrap())
            .unwrap()
            .format
    }

    fn mip_count(&self, rel: &str) -> u32 {
        read_dds(&std::fs::read(self.dds(rel)).unwrap())
            .unwrap()
            .mip_count
    }
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("sdc was killed by a signal")
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

fn events(o: &Output) -> Vec<Value> {
    text(&o.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l}: {e}")))
        .collect()
}

fn file_name(ev: &Value) -> Option<&str> {
    ev["path"].as_str().and_then(|p| p.rsplit('/').next())
}

/// The `job` of every `file_planned` event, keyed by input file name.
fn planned(o: &Output) -> BTreeMap<String, Value> {
    events(o)
        .into_iter()
        .filter(|e| e["kind"] == "file_planned")
        .map(|e| (file_name(&e).unwrap().to_string(), e["job"].clone()))
        .collect()
}

fn batch_started(o: &Output) -> Value {
    events(o).remove(0)
}

fn batch_done(o: &Output) -> Value {
    let ev = events(o).pop().unwrap();
    assert_eq!(ev["kind"], "batch_done");
    ev
}

/// `file_skipped` events as `(file name, code)`.
fn skips(o: &Output) -> Vec<(String, Option<String>)> {
    events(o)
        .iter()
        .filter(|e| e["kind"] == "file_skipped")
        .map(|e| {
            (
                file_name(e).unwrap().to_string(),
                e["code"].as_str().map(str::to_string),
            )
        })
        .collect()
}

fn prov<'a>(job: &'a Value, field: &str) -> &'a str {
    job["provenance"][field]
        .as_str()
        .unwrap_or_else(|| panic!("no provenance for {field}: {}", job["provenance"]))
}

/// `profile.<dotted path>` as f64.
fn num(job: &Value, path: &str) -> f64 {
    let mut v = &job["profile"];
    for part in path.split('.') {
        v = &v[part];
    }
    v.as_f64()
        .unwrap_or_else(|| panic!("profile.{path} is not a number: {v}"))
}

fn read_all(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, into);
            } else if p.extension().is_some_and(|x| x == "dds") {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
                into.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(dir, dir, &mut m);
    m
}

const DIFFUSE: [&str; 2] = ["--set", "subtype=diffuse"];

#[test]
fn user_config_then_project_config_then_set_decide_the_format() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.rgb("floor.png");
    // User config: every diffuse is BC1 (rgbcx). Project config: a rule decides the sub-type of every file.
    fx.user_config("[subtypes.diffuse]\nformat = \"bc1\"\nencoder = { backend = \"rgbcx\" }\n");
    fx.project_config("[[rules]]\nmatch = { glob = \"**/*.png\" }\nsubtype = \"diffuse\"\n");
    let o = fx.convert(&["--target", "vanilla"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert_eq!(
        fx.format("textures/a/wall.dds"),
        DdsPixelFormat::Bc1 { srgb: false }
    );
    assert_eq!(
        fx.format("textures/a/floor.dds"),
        DdsPixelFormat::Bc1 { srgb: false }
    );
    let jobs = planned(&o);
    for name in ["wall.png", "floor.png"] {
        assert_eq!(prov(&jobs[name], "format"), "user_config", "{name}");
        assert_eq!(
            prov(&jobs[name], "encoder.backend"),
            "user_config",
            "{name}"
        );
        assert_eq!(prov(&jobs[name], "subtype"), "rule:project#0", "{name}");
    }

    // `--set` beats the user config, for one file only.
    let o = fx.convert(&[
        "--target",
        "vanilla",
        "--on-existing",
        "overwrite",
        "--set",
        "file:**/floor.png.format=bc7",
        "--set",
        "file:**/floor.png.encoder.backend=bc7e",
    ]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert_eq!(
        fx.format("textures/a/floor.dds"),
        DdsPixelFormat::Bc7 { srgb: false }
    );
    assert_eq!(
        fx.format("textures/a/wall.dds"),
        DdsPixelFormat::Bc1 { srgb: false }
    );
    let jobs = planned(&o);
    assert_eq!(prov(&jobs["floor.png"], "format"), "user");
    assert_eq!(prov(&jobs["floor.png"], "encoder.backend"), "user");
    assert_eq!(prov(&jobs["wall.png"], "format"), "user_config");
}

/// Spec §8, lowest to highest: built-in, preset, user config, project config, rule, `--set` unscoped,
/// `--set subtype:`, `--set file:`. Each level adds one layer; the winner and its provenance are read from the
/// `file_planned` job. Every layer also sets one field nobody above it touches.
#[test]
fn every_layer_overrides_the_one_below_with_provenance() {
    const F: &str = "validation.min_mip_psnr_db";
    // (levels in use, wall value, wall provenance, floor value, floor provenance)
    let steps: [(u8, f64, &str, f64, &str); 8] = [
        (0, 30.0, "builtin", 30.0, "builtin"),
        (1, 31.0, "preset:chainp", 31.0, "preset:chainp"),
        (2, 32.0, "user_config", 32.0, "user_config"),
        (3, 33.0, "project_config", 33.0, "project_config"),
        (4, 34.0, "rule:project#0", 34.0, "rule:project#0"),
        (5, 35.0, "user", 35.0, "user"),
        (6, 36.0, "user", 36.0, "user"),
        (7, 37.0, "user", 36.0, "user"),
    ];
    for (level, wall, wall_prov, floor, floor_prov) in steps {
        let fx = Fx::new();
        fx.rgb("wall.png");
        fx.rgb("floor.png");
        let preset = fx.write(
            "chainp.toml",
            "schema_version = 1\nid = \"chainp\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\
             [subtypes.diffuse]\nvalidation = { min_mip_psnr_db = 31.0, bc6h_min_peak_ratio = 0.7 }\n",
        );
        if level >= 2 {
            fx.user_config(
                "[subtypes.diffuse]\nvalidation = { min_mip_psnr_db = 32.0, coverage_tolerance_pts = 3.0 }\n",
            );
        }
        if level >= 3 {
            let mut cfg = String::from(
                "[subtypes.diffuse]\nvalidation = { min_mip_psnr_db = 33.0, pgpatcher_non_white_share = 0.6 }\n",
            );
            if level >= 4 {
                cfg.push_str(
                    "[[rules]]\nmatch = { glob = \"**/*.png\" }\n\
                     overrides = { validation = { min_mip_psnr_db = 34.0, complex_m_warn_alpha = 240 } }\n",
                );
            }
            fx.project_config(&cfg);
        }
        let preset = preset.to_string_lossy().to_string();
        let mut args: Vec<&str> = vec!["--dry-run", "--set", "subtype=diffuse"];
        if level >= 1 {
            args.extend(["--preset", &preset]);
        }
        let unscoped = format!("{F}=35.0");
        let typed = format!("subtype:diffuse.{F}=36.0");
        let file = format!("file:**/wall.png.{F}=37.0");
        for (min, set) in [(5, &unscoped), (6, &typed), (7, &file)] {
            if level >= min {
                args.extend(["--set", set]);
            }
        }
        let o = fx.convert(&args);
        assert_eq!(code(&o), 0, "level {level}: {}", text(&o.stderr));
        let jobs = planned(&o);
        let (w, f) = (&jobs["wall.png"], &jobs["floor.png"]);
        assert_eq!(
            (num(w, F), prov(w, F)),
            (wall, wall_prov),
            "level {level} wall"
        );
        assert_eq!(
            (num(f, F), prov(f, F)),
            (floor, floor_prov),
            "level {level} floor"
        );

        // The distinct fields: each layer's own value survives where nothing above it sets the field.
        let distinct: [(u8, &str, f64, &str); 4] = [
            (1, "validation.bc6h_min_peak_ratio", 0.7, "preset:chainp"),
            (2, "validation.coverage_tolerance_pts", 3.0, "user_config"),
            (
                3,
                "validation.pgpatcher_non_white_share",
                0.6,
                "project_config",
            ),
            (
                4,
                "validation.complex_m_warn_alpha",
                240.0,
                "rule:project#0",
            ),
        ];
        let builtin = [0.8, 2.0, 0.5, 247.0];
        for (i, (min, field, value, source)) in distinct.into_iter().enumerate() {
            let (want, want_prov) = if level >= min {
                (value, source)
            } else {
                (builtin[i], "builtin")
            };
            assert_eq!(
                (num(w, field), prov(w, field)),
                (want, want_prov),
                "level {level} {field}"
            );
        }
        assert_eq!(prov(w, "subtype"), "user", "level {level}");
        assert!(
            w["status"]["state"] == "ready",
            "level {level}: {}",
            w["status"]
        );
    }
}

/// Spec §8: `--set` is ranked by scope (unscoped < `subtype:` < `file:`), not by its place on the command line;
/// within one scope the later flag wins.
#[test]
fn set_scopes_rank_by_specificity_not_command_line_order() {
    const F: &str = "validation.min_mip_psnr_db";
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.rgb("floor.png");
    let file_wall = format!("file:**/wall.png.{F}=47.0");
    let file_all_first = "file:**/*.png.alpha.threshold=60";
    let file_floor_later = "file:**/floor.png.alpha.threshold=80";
    let typed = format!("subtype:diffuse.{F}=46.0");
    let other_type = format!("subtype:normal.{F}=99.0");
    let unscoped_a = format!("{F}=44.0");
    let unscoped_b = format!("{F}=45.0");
    let o = fx.convert(&[
        "--dry-run",
        "--set",
        &file_wall,
        "--set",
        &typed,
        "--set",
        &unscoped_b,
        "--set",
        &unscoped_a, // same scope as the line before: the later flag wins among unscoped values
        "--set",
        &other_type, // another sub-type: never applies to diffuse files
        "--set",
        "subtype=diffuse",
        "--set",
        file_all_first,
        "--set",
        file_floor_later,
    ]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let jobs = planned(&o);
    let (w, f) = (&jobs["wall.png"], &jobs["floor.png"]);
    assert_eq!(num(w, F), 47.0); // file: beats subtype: and unscoped although it came first
    assert_eq!(num(f, F), 46.0); // subtype: beats unscoped although it came first
    assert_eq!(prov(w, F), "user");
    assert_eq!(prov(f, F), "user");
    // Two file-scoped values on the same field: command-line order decides where both match.
    assert_eq!(num(w, "alpha.threshold"), 60.0);
    assert_eq!(num(f, "alpha.threshold"), 80.0);

    // Same scope, opposite order: the later unscoped flag wins.
    let o = fx.convert(&[
        "--dry-run",
        "--set",
        "subtype=diffuse",
        "--set",
        &unscoped_a,
        "--set",
        &unscoped_b,
    ]);
    assert_eq!(num(&planned(&o)["wall.png"], F), 45.0);
}

#[test]
fn accepted_suggestion_loses_to_rules_which_lose_to_set() {
    let fx = Fx::new();
    fx.normal("wall_n.png");
    fx.rgb("floor.png");
    let accept = ["--dry-run", "--accept-suggestions", "high"];

    // An accepted high-confidence suggestion decides `wall_n.png`; the medium one for `floor.png` stays open.
    let o = fx.convert(&accept);
    assert_eq!(code(&o), 1, "{}", text(&o.stderr)); // floor.png is unresolved
    let jobs = planned(&o);
    let wall = &jobs["wall_n.png"];
    assert_eq!(wall["profile"]["subtype"], "normal");
    assert_eq!(prov(wall, "subtype"), "suggestion");
    assert_eq!(wall["status"]["state"], "ready");
    assert_eq!(jobs["floor.png"]["status"]["state"], "unresolved");

    // Without acceptance nothing is decided.
    let o = fx.convert(&["--dry-run"]);
    assert_eq!(jobs_state(&o, "wall_n.png"), "unresolved");

    // Project rules beat the suggestion; within one source the first matching rule wins.
    fx.project_config(
        "[[rules]]\nmatch = { glob = \"**/wall_n.png\" }\nsubtype = \"height\"\n\
         [[rules]]\nmatch = { glob = \"**/*.png\" }\nsubtype = \"glow\"\n",
    );
    let o = fx.convert(&accept);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let jobs = planned(&o);
    assert_eq!(jobs["wall_n.png"]["profile"]["subtype"], "height");
    assert_eq!(prov(&jobs["wall_n.png"], "subtype"), "rule:project#0");
    assert_eq!(jobs["floor.png"]["profile"]["subtype"], "glow");
    assert_eq!(prov(&jobs["floor.png"], "subtype"), "rule:project#1");
    // A rule decides on its own, without `--accept-suggestions`.
    assert_eq!(
        jobs_state(&fx.convert(&["--dry-run"]), "floor.png"),
        "ready"
    );

    // A user-config rule is below the project config's rules, both for the sub-type and for field overrides.
    fx.user_config(
        "[[rules]]\nmatch = { glob = \"**/wall_n.png\" }\nsubtype = \"glow\"\n\
         overrides = { alpha = { threshold = 60 }, validation = { complex_m_warn_alpha = 200 } }\n",
    );
    fx.project_config(
        "[[rules]]\nmatch = { glob = \"**/wall_n.png\" }\nsubtype = \"height\"\n\
         overrides = { alpha = { threshold = 70 } }\n",
    );
    let jobs = planned(&fx.convert(&accept));
    let wall = &jobs["wall_n.png"];
    assert_eq!(wall["profile"]["subtype"], "height");
    assert_eq!(prov(wall, "subtype"), "rule:project#0");
    assert_eq!(num(wall, "alpha.threshold"), 70.0);
    assert_eq!(prov(wall, "alpha.threshold"), "rule:project#0");
    // The user rule still supplies what the project rule does not say.
    assert_eq!(num(wall, "validation.complex_m_warn_alpha"), 200.0);
    assert_eq!(prov(wall, "validation.complex_m_warn_alpha"), "rule:user#0");

    // `--set` beats the rules, scoped to one file or for everything.
    let o = fx.convert(&[
        "--dry-run",
        "--accept-suggestions",
        "high",
        "--set",
        "file:**/wall_n.png.subtype=msn",
        "--set",
        "file:**/wall_n.png.alpha.threshold=90",
        "--set",
        "subtype=pbr_normal",
    ]);
    let jobs = planned(&o);
    // File-scoped sub-type beats the unscoped one; the unscoped one still decides floor.png.
    assert_eq!(jobs["wall_n.png"]["profile"]["subtype"], "msn");
    assert_eq!(prov(&jobs["wall_n.png"], "subtype"), "user");
    assert_eq!(num(&jobs["wall_n.png"], "alpha.threshold"), 90.0);
    assert_eq!(prov(&jobs["wall_n.png"], "alpha.threshold"), "user");
    assert_eq!(jobs["floor.png"]["profile"]["subtype"], "pbr_normal");
    assert_eq!(prov(&jobs["floor.png"], "subtype"), "user");
}

fn jobs_state(o: &Output, name: &str) -> String {
    planned(o)[name]["status"]["state"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn target_and_preset_come_from_flag_then_project_then_user_then_preset() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    let run = |extra: &[&str]| {
        let mut args = vec!["--dry-run", "--set", "subtype=diffuse"];
        args.extend(extra);
        let o = fx.convert(&args);
        assert_eq!(code(&o), 0, "{}", text(&o.stderr));
        (batch_started(&o), planned(&o).remove("wall.png").unwrap())
    };
    let (b, job) = run(&[]);
    assert_eq!(
        (b["target"].as_str(), b["preset"].as_str()),
        (Some("cs"), Some("cs-quality"))
    );
    assert_eq!(prov(&job, "target"), "builtin"); // nothing states a target: the documented fall-back

    // A selected preset states its own target.
    let (b, job) = run(&["--preset", "cpm-quality"]);
    assert_eq!(
        (b["target"].as_str(), b["preset"].as_str()),
        (Some("cpm"), Some("cpm-quality"))
    );
    assert_eq!(prov(&job, "target"), "preset:cpm-quality");

    fx.user_config("target = \"cpm\"\npreset = \"cpm-quality\"\n");
    let (b, job) = run(&[]);
    assert_eq!(
        (b["target"].as_str(), b["preset"].as_str()),
        (Some("cpm"), Some("cpm-quality"))
    );
    assert_eq!(prov(&job, "target"), "user_config");

    fx.project_config("target = \"vanilla\"\npreset = \"vanilla-quality\"\n");
    let (b, job) = run(&[]);
    assert_eq!(
        (b["target"].as_str(), b["preset"].as_str()),
        (Some("vanilla"), Some("vanilla-quality"))
    );
    assert_eq!(prov(&job, "target"), "project_config");

    // `--preset` beats both configs' presets; a configured target still outranks the preset's own.
    let (b, job) = run(&["--preset", "cpm-quality"]);
    assert_eq!(b["preset"], "cpm-quality");
    assert_eq!(b["target"], "vanilla");
    assert_eq!(prov(&job, "target"), "project_config");

    let (b, job) = run(&["--target", "cs", "--preset", "cpm-quality"]);
    assert_eq!(
        (b["target"].as_str(), b["preset"].as_str()),
        (Some("cs"), Some("cpm-quality"))
    );
    assert_eq!(prov(&job, "target"), "user");
}

#[test]
fn explicit_config_flag_replaces_the_default_sdc_toml() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.project_config("target = \"cpm\"\n");
    let other = fx.write("other.toml", "target = \"vanilla\"\n");
    let other = other.to_string_lossy().to_string();
    let o = fx.convert(&["--dry-run", "--set", "subtype=diffuse"]);
    assert_eq!(batch_started(&o)["target"], "cpm");
    let o = fx.convert(&["--dry-run", "--set", "subtype=diffuse", "--config", &other]);
    assert_eq!(batch_started(&o)["target"], "vanilla");
}

#[test]
fn suffix_alias_names_the_output_after_the_canonical_suffix() {
    let fx = Fx::new();
    fx.normal("wall_wallultra.png");
    fx.normal("floor_n.png");
    // Outside `in/`, so it is loaded with `--preset-file` and not discovered as an input.
    let ultra = fx.write(
        "ultra.toml",
        "schema_version = 1\nid = \"ultra\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\
         [suffix_aliases]\nwallultra = \"n\"\n[subtypes.normal]\nmips = { levels = 1 }\n",
    );
    let ultra = ultra.to_string_lossy().to_string();
    let o = fx.convert(&["--accept-suggestions", "high", "--preset-file", &ultra]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(fx.dds("textures/a/wall_n.dds").exists());
    assert!(!fx.dds("textures/a/wall_wallultra.dds").exists());
    // Loaded, not selected.
    assert_eq!(batch_started(&o)["preset"], "cs-quality");
    let jobs = planned(&o);
    let aliased = &jobs["wall_wallultra.png"];
    assert_eq!(
        aliased["suffix_alias"],
        serde_json::json!({ "alias": "wallultra", "canonical": "n", "preset": "ultra" })
    );
    assert_eq!(aliased["profile"]["subtype"], "normal");
    assert_eq!(prov(aliased, "subtype"), "suggestion");
    // The alias preset's settings reach only the aliased file.
    assert_eq!(prov(aliased, "mips.levels"), "preset:ultra");
    assert_eq!(fx.mip_count("textures/a/wall_n.dds"), 1);
    assert!(jobs["floor_n.png"].get("suffix_alias").is_none());
    assert_eq!(prov(&jobs["floor_n.png"], "mips.levels"), "builtin");
    assert!(fx.mip_count("textures/a/floor_n.dds") > 1);
}

#[test]
fn dry_run_writes_nothing() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.rgb("floor.png");
    // Unresolved files: exit 1, nothing created, not even the output root.
    let o = fx
        .sdc()
        .args(["convert", "--recursive", "--dry-run", "-o"])
        .arg(fx.p("out"))
        .arg(fx.p("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    assert!(!fx.p("out").exists());
    // Resolved files: exit 0, still nothing created (no DDS, no manifest folder, no sidecar).
    let o = fx.convert(&["--dry-run", "--sidecar", "--set", "subtype=diffuse"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(!fx.p("out").exists());
    assert_eq!(batch_done(&o)["converted"], 0);
    assert_eq!(planned(&o).len(), 2);
    // The real run, for contrast.
    let o = fx.convert(&["--sidecar", "--set", "subtype=diffuse"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(fx.dds("textures/a/wall.dds").exists());
    assert!(fx.dds("textures/a/wall.dds.json").exists());
    assert!(fx.dds("_sdc/manifest.jsonl").exists());
}

#[test]
fn unresolved_files_are_skipped_with_exit_1() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.rgb("floor.png");
    // `diffuse` is only a medium-confidence suggestion, so even `--accept-suggestions high` leaves it open.
    let o = fx.convert(&["--accept-suggestions", "high"]);
    assert_eq!(code(&o), 1);
    assert!(
        text(&o.stderr).contains("SUG-LOW-CONFIDENCE"),
        "{}",
        text(&o.stderr)
    );
    let s = skips(&o);
    assert_eq!(s.len(), 2);
    assert!(s
        .iter()
        .all(|(_, c)| c.as_deref() == Some("SUG-LOW-CONFIDENCE")));
    assert!(!fx.p("out/textures").exists());
    // `--allow SUG-LOW-CONFIDENCE` is the explicit acknowledgement.
    let o = fx.convert(&["--allow", "SUG-LOW-CONFIDENCE"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(fx.dds("textures/a/wall.dds").exists());
}

#[test]
fn if_changed_skip_and_unknown_existing() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.rgb("floor.png");
    let run = |extra: &[&str]| {
        let mut args = DIFFUSE.to_vec();
        args.extend(extra);
        fx.convert(&args)
    };
    let o = run(&[]);
    assert_eq!(code(&o), 0);
    assert_eq!(batch_done(&o)["converted"], 2);

    // Same sources and profile: both are up to date.
    let o = run(&[]);
    assert_eq!(code(&o), 0);
    assert_eq!(batch_done(&o)["up_to_date"], 2);
    assert_eq!(batch_done(&o)["converted"], 0);
    assert!(
        text(&o.stderr).contains("2 up to date"),
        "{}",
        text(&o.stderr)
    );

    // A changed source re-encodes only that file.
    fx.rgb_sized("wall.png", 32, 200);
    let o = run(&[]);
    assert_eq!(code(&o), 0);
    let done = batch_done(&o);
    assert_eq!(
        (done["converted"].as_u64(), done["up_to_date"].as_u64()),
        (Some(1), Some(1))
    );

    // A changed profile re-encodes everything.
    let o = run(&["--set", "alpha.threshold=100"]);
    assert_eq!(code(&o), 0);
    let done = batch_done(&o);
    assert_eq!(
        (done["converted"].as_u64(), done["up_to_date"].as_u64()),
        (Some(2), Some(0))
    );

    // Without the manifest the existing outputs are unknown: kept, reported, exit 1.
    std::fs::remove_file(fx.dds("_sdc/manifest.jsonl")).unwrap();
    let before = std::fs::read(fx.dds("textures/a/wall.dds")).unwrap();
    let o = run(&[]);
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains("OUT-UNKNOWN-EXISTING"));
    assert_eq!(
        skips(&o)
            .iter()
            .filter(|(_, c)| c.as_deref() == Some("OUT-UNKNOWN-EXISTING"))
            .count(),
        2
    );
    let done = batch_done(&o);
    assert_eq!(
        (
            done["converted"].as_u64(),
            done["skipped_intentional"].as_u64()
        ),
        (Some(0), Some(0))
    );
    assert_eq!(
        std::fs::read(fx.dds("textures/a/wall.dds")).unwrap(),
        before
    );

    // `--on-existing overwrite` replaces them and records them again.
    let o = run(&["--on-existing", "overwrite"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert_eq!(batch_done(&o)["converted"], 2);
    assert_eq!(batch_done(&o)["up_to_date"], 0);
    let o = run(&[]);
    assert_eq!(batch_done(&o)["up_to_date"], 2);
}

#[test]
fn on_existing_skip_keeps_outputs_without_failing_the_batch() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.rgb("floor.png");
    assert_eq!(code(&fx.convert(&DIFFUSE)), 0);
    let wall = std::fs::read(fx.dds("textures/a/wall.dds")).unwrap();
    // A changed source would normally be re-encoded; `skip` keeps the old output whatever it says.
    fx.rgb_sized("wall.png", 32, 200);
    fx.rgb("new.png");
    let o = fx.convert(&["--set", "subtype=diffuse", "--on-existing", "skip"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let done = batch_done(&o);
    assert_eq!(done["converted"], 1);
    assert_eq!(done["skipped"], 2);
    assert_eq!(done["skipped_intentional"], 2);
    assert_eq!(std::fs::read(fx.dds("textures/a/wall.dds")).unwrap(), wall);
    assert!(fx.dds("textures/a/new.dds").exists());

    // `overwrite` re-encodes regardless of the manifest.
    let o = fx.convert(&["--set", "subtype=diffuse", "--on-existing", "overwrite"]);
    assert_eq!(code(&o), 0);
    assert_eq!(batch_done(&o)["converted"], 3);
    assert_ne!(std::fs::read(fx.dds("textures/a/wall.dds")).unwrap(), wall);
}

#[test]
fn one_job_and_four_jobs_write_identical_files() {
    let fx = Fx::new();
    for (i, name) in ["a.png", "b.png", "c.png", "d.png"].into_iter().enumerate() {
        fx.rgb_sized(name, if i % 2 == 0 { 64 } else { 32 }, 40 + i as u8 * 50);
    }
    fx.normal("e_n.png");
    let one = fx.convert_to(
        "out1",
        &[
            "--jobs",
            "1",
            "--accept-suggestions",
            "high",
            "--set",
            "file:**/[a-d].png.subtype=diffuse",
        ],
    );
    assert_eq!(code(&one), 0, "{}", text(&one.stderr));
    let four = fx.convert_to(
        "out4",
        &[
            "--jobs",
            "4",
            "--accept-suggestions",
            "high",
            "--set",
            "file:**/[a-d].png.subtype=diffuse",
        ],
    );
    assert_eq!(code(&four), 0, "{}", text(&four.stderr));
    let (a, b) = (read_all(&fx.p("out1")), read_all(&fx.p("out4")));
    assert_eq!(a.len(), 5, "{:?}", a.keys().collect::<Vec<_>>());
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (name, bytes) in &a {
        assert!(
            bytes == &b[name],
            "{name} differs between --jobs 1 and --jobs 4"
        );
    }
}

#[test]
fn intentional_skips_exit_0_and_unintended_ones_exit_1() {
    // DECISIONS Note I.
    // (a) A lone DDS next to the sources is skipped on purpose.
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.write("in/textures/a/other.dds", "not a real dds");
    let o = fx.convert(&DIFFUSE);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(skips(&o).contains(&("other.dds".into(), Some("IN-DDS-SKIPPED".into()))));
    let done = batch_done(&o);
    assert_eq!(
        (
            done["converted"].as_u64(),
            done["skipped"].as_u64(),
            done["skipped_intentional"].as_u64()
        ),
        (Some(1), Some(1), Some(1))
    );

    // (b) Two images for the same output are a duplicate to resolve: exit 1, nothing is intentional.
    fx.rgb("wall.tga");
    let o = fx.convert_to("out2", &DIFFUSE);
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    let s = skips(&o);
    assert!(s.contains(&("wall.png".into(), Some("IN-DUPLICATE".into()))));
    assert!(s.contains(&("wall.tga".into(), Some("IN-DUPLICATE".into()))));
    assert_eq!(batch_done(&o)["skipped_intentional"], 1); // only other.dds
    assert!(!fx.p("out2/textures/a/wall.dds").exists());

    // (c) A DDS next to the image for the same output has no output of its own: the image converts, the DDS
    // is skipped on purpose, exit 0.
    std::fs::remove_file(fx.p("in/textures/a/wall.tga")).unwrap();
    fx.write("in/textures/a/wall.dds", "not a real dds");
    let o = fx.convert_to("out3", &DIFFUSE);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let s = skips(&o);
    assert!(s.contains(&("wall.dds".into(), Some("IN-DDS-SKIPPED".into()))));
    assert!(!s.iter().any(|(_, c)| c.as_deref() == Some("IN-DUPLICATE")));
    let done = batch_done(&o);
    assert_eq!(
        (
            done["converted"].as_u64(),
            done["skipped_intentional"].as_u64()
        ),
        (Some(1), Some(2))
    );
    assert!(fx.p("out3/textures/a/wall.dds").exists());
}

#[test]
fn raw_packing_inputs_are_reported_but_not_a_failure() {
    let fx = Fx::new();
    fx.normal("wall_n.png");
    fx.rgb("tile_roughness.png");
    let o = fx.convert(&["--accept-suggestions", "high"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(skips(&o).contains(&("tile_roughness.png".into(), Some("IN-PACKING-INPUT".into()))));
    assert!(fx.dds("textures/a/wall_n.dds").exists());
    assert!(!fx.dds("textures/a/tile_roughness.dds").exists());
}

#[test]
fn a_file_that_fails_to_resolve_exits_1_and_the_rest_are_converted() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.rgb("floor.png");
    // alpha.threshold 3 is outside 5..=200: a per-file CFG-INVALID error, not a usage error.
    let o = fx.convert(&[
        "--set",
        "subtype=diffuse",
        "--set",
        "file:**/wall.png.alpha.threshold=3",
    ]);
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains("CFG-INVALID"));
    let failed: Vec<Value> = events(&o)
        .into_iter()
        .filter(|e| e["kind"] == "file_failed")
        .collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(file_name(&failed[0]), Some("wall.png"));
    assert_eq!(failed[0]["stage"], "resolve");
    assert_eq!(batch_done(&o)["failed"], 1);
    assert!(fx.dds("textures/a/floor.dds").exists());
    assert!(!fx.dds("textures/a/wall.dds").exists());
}

#[test]
fn invalid_configs_are_exit_2_and_valid_ones_are_really_read() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    let args = ["--dry-run", "--set", "subtype=diffuse"];
    assert_eq!(code(&fx.convert(&args)), 0);

    // The user config under APPDATA is read: an unknown key is rejected.
    fx.user_config("no_such_key = 1\n");
    let o = fx.convert(&args);
    assert_eq!(code(&o), 2, "{}", text(&o.stderr));
    assert!(o.stdout.is_empty());
    fx.user_config("[defaults\n"); // not TOML
    assert_eq!(code(&fx.convert(&args)), 2);
    std::fs::remove_file(fx.p("appdata/sdc/config.toml")).unwrap();
    assert_eq!(code(&fx.convert(&args)), 0);

    // The same for the project config, found next to the input folder's root.
    fx.project_config(
        "[[rules]]\nmatch = { glob = \"**/*.png\" }\nsubtype = \"no_such_subtype\"\n",
    );
    assert_eq!(code(&fx.convert(&args)), 2);
    fx.project_config("[subtypes.diffuse]\nformat = \"bc9\"\n");
    assert_eq!(code(&fx.convert(&args)), 2);
    let missing = fx.p("missing.toml").to_string_lossy().to_string();
    let mut with_missing = args.to_vec();
    with_missing.extend(["--config", &missing]);
    assert_eq!(code(&fx.convert(&with_missing)), 2);

    // An invalid layer value that parses is a per-file CFG-INVALID (exit 1), not a config error.
    fx.project_config("[subtypes.diffuse]\nalpha = { threshold = 250 }\n");
    let o = fx.convert(&args);
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    assert_eq!(planned(&o)["wall.png"]["status"]["state"], "error");
    assert_eq!(
        prov(&planned(&o)["wall.png"], "alpha.threshold"),
        "project_config"
    );
}

/// Deterministic white noise (an LCG) at `in/textures/a/<name>`.
fn noise(fx: &Fx, name: &str) {
    let mut state = 12345u32;
    let mut next = move || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state >> 24) as u8
    };
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([next(), next(), next()]))
        .save(fx.p(&format!("in/textures/a/{name}")))
        .unwrap();
}

/// A layered value is not only reported: the engine acts on it. A noisy source misses the default 30 dB mip
/// threshold (`VAL-MIP-ERROR`), and each layer can move that threshold.
#[test]
fn a_layered_validation_threshold_changes_what_validation_reports() {
    let fx = Fx::new();
    noise(&fx, "noise.png");
    let mip_errors = |o: &Output| -> Vec<String> {
        events(o)
            .iter()
            .filter(|e| e["kind"] == "warning" && e["code"] == "VAL-MIP-ERROR")
            .map(|e| e["message"].as_str().unwrap().to_string())
            .collect()
    };
    // The defaults (`alpha.exact_opaque` off, alpha removed) hold the decoded-alpha guarantee on this noise (fix
    // batch 41), so every run exits 0 and only the warning moves.
    let base = DIFFUSE;
    let o = fx.convert(&base);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(
        mip_errors(&o).iter().any(|m| m.contains("< 30.0 dB")),
        "{:?}",
        mip_errors(&o)
    );

    fx.user_config(
        "[subtypes.diffuse]
validation = { min_mip_psnr_db = 1.0 }
",
    );
    let o = fx.convert_to("out2", &base);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(mip_errors(&o).is_empty());

    let mut args = base.to_vec();
    args.extend(["--set", "validation.min_mip_psnr_db=99.0"]);
    let o = fx.convert_to("out3", &args);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(
        mip_errors(&o).iter().any(|m| m.contains("< 99.0 dB")),
        "{:?}",
        mip_errors(&o)
    );
}

/// A 16 px checker of pure black and orange at `in/textures/a/<name>` (RGB, so the alpha is removed).
fn black_checker(fx: &Fx, name: &str) {
    ImageBuffer::from_fn(16, 16, |x, y| {
        if (x + y) % 2 == 0 {
            Rgb([0u8, 0, 0])
        } else {
            Rgb([230, 120, 20])
        }
    })
    .save(fx.p(&format!("in/textures/a/{name}")))
    .unwrap();
}

/// Settings under which a written output fails validation deterministically: BC1 with transparent-black texels
/// switched on (off by default, DECISIONS Note F) and the alpha removed. rgbcx writes the black texels with BC1's
/// transparent index, they decode alpha 0, and the `remove` guarantee for BC1 is 255 (spec §5.8), so validation
/// reports `VAL-ALPHA-GUARANTEE`, an error. (`VAL-MIP-ERROR` is only a warning, and `VAL-HEADER` cannot be reached
/// through settings.)
const FAILS_VALIDATION: [&str; 8] = [
    "--set",
    "subtype=diffuse",
    "--set",
    "format=bc1",
    "--set",
    "encoder.backend=rgbcx",
    "--set",
    "encoder.bc1_transparent_black=true",
];

/// Spec §8: a written output that fails validation exits 4, and a partial batch (1) outranks it.
#[test]
fn failed_validation_exits_4_unless_the_batch_is_also_partial() {
    let fx = Fx::new();
    black_checker(&fx, "checker.png");
    let o = fx.convert(&FAILS_VALIDATION);
    assert_eq!(code(&o), 4, "{}", text(&o.stderr));
    let done = batch_done(&o);
    assert_eq!(
        (
            done["converted"].as_u64(),
            done["validation_failed"].as_u64()
        ),
        (Some(1), Some(1))
    );
    assert!(text(&o.stderr).contains("[VALIDATION FAILED]"));
    assert!(text(&o.stderr).contains("VAL-ALPHA-GUARANTEE"));
    // The output is still written (the report says so; the file is not deleted).
    assert!(fx.dds("textures/a/checker.dds").exists());
    // A failed validation is not up to date: the next run encodes it again and reports it again.
    let o = fx.convert(&FAILS_VALIDATION);
    assert_eq!(code(&o), 4, "{}", text(&o.stderr));
    assert_eq!(batch_done(&o)["converted"], 1);

    // Partial (exit 1) outranks validation failed (exit 4).
    fx.rgb("bad.png");
    let mut args = FAILS_VALIDATION.to_vec();
    args.extend(["--set", "file:**/bad.png.alpha.threshold=3"]);
    let o = fx.convert(&args);
    assert_eq!(code(&o), 1, "{}", text(&o.stderr));
    assert_eq!(batch_done(&o)["validation_failed"], 1);
}
