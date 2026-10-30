use std::path::PathBuf;
use std::process::Command;

fn sdc() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sdc"));
    c.env(
        "APPDATA",
        std::env::temp_dir().join("sdc-test-appdata-none"),
    );
    c
}

#[test]
fn cli_writes_a_marker_and_keeps_foreign_files() {
    let dir = std::env::temp_dir().join(format!("sdc-cubemap-marker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out: PathBuf = dir.join("x_e.dds");
    let o = sdc()
        .args([
            "cubemap-marker",
            "--color",
            "255,197,78",
            "--roughness",
            "0.1",
            "-o",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let b = std::fs::read(&out).unwrap();
    assert_eq!((b.len(), &b[128..132]), (152, &[78u8, 197, 255, 26][..]));

    let foreign = dir.join("y_e.dds");
    std::fs::write(&foreign, b"0123456789").unwrap();
    let o = sdc()
        .args(["cubemap-marker", "-o"])
        .arg(&foreign)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(all.contains("kept as edited outside Tessalith"), "{all}");
    assert_eq!(std::fs::read(&foreign).unwrap(), b"0123456789");

    let o = sdc()
        .args(["cubemap-marker", "--roughness", "2", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    for bad in ["1,2", "1,2,300"] {
        let o = sdc()
            .args(["cubemap-marker", "--color", bad, "-o"])
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "--color {bad}");
    }

    // write_atomic creates missing folders, so make the parent a plain file to force an I/O failure.
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let o = sdc()
        .args(["cubemap-marker", "-o"])
        .arg(blocker.join("x_e.dds"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sdc-cubechecks-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A legacy RGBA8 DDS: a cube of `size` with `mips` levels per face, or a flat 4x4 when `cube` is false.
fn dds(cube: bool, size: u32, mips: u32) -> Vec<u8> {
    use engine::config::Format;
    use engine::dds::{dds_bytes, dds_bytes_cube, DdsDesc, ResolvedHeader};
    let d = DdsDesc {
        width: size,
        height: size,
        format: Format::Rgba8,
        srgb: false,
        header: ResolvedHeader::Legacy,
    };
    if !cube {
        return dds_bytes(&d, &[vec![9; (size * size * 4) as usize]]).unwrap();
    }
    let mut levels = vec![];
    for _ in 0..6 {
        let mut s = size;
        for _ in 0..mips {
            levels.push(vec![9; (s * s * 4) as usize]);
            s = (s / 2).max(1);
        }
    }
    dds_bytes_cube(&d, &levels).unwrap()
}

fn text(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn cli_inspect_lists_cube_issues() {
    let dir = scratch("inspect").join("textures").join("cubemaps");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("x_e.dds");
    std::fs::write(&p, dds(false, 4, 1)).unwrap();
    let o = sdc().arg("inspect").arg(&p).output().unwrap();
    let all = text(&o);
    assert!(
        all.contains("header issue: 2D texture where a cubemap is expected"),
        "{all}"
    );
    // A plain 2D file outside cubemaps/ and _e gets no cube issue.
    let q = dir.parent().unwrap().join("plain.dds");
    std::fs::write(&q, dds(false, 4, 1)).unwrap();
    let all = text(&sdc().arg("inspect").arg(&q).output().unwrap());
    assert!(!all.contains("cubemap is expected"), "{all}");
}

/// FC7 (supersedes DS12's FAIL-by-default in 1.x): a 2D `_e` or `cubemaps\` file is a warning and exits 0;
/// `--strict-cubemaps` makes it FAIL.
#[test]
fn validate_warns_on_2d_e_files_by_default() {
    let dir = scratch("warn2d").join("textures").join("cubemaps");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["x_e.dds", "plain.dds"] {
        let p = dir.join(name);
        std::fs::write(&p, dds(false, 4, 1)).unwrap();
        let o = sdc().args(["validate", "--json"]).arg(&p).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{name}: {}", text(&o));
        let line = String::from_utf8_lossy(&o.stdout);
        let v: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
        assert_eq!(v["passed"], true, "{line}");
        let w = v["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| {
                w["message"]
                    .as_str()
                    .unwrap()
                    .contains("cubemap is expected")
            })
            .unwrap_or_else(|| panic!("{line}"));
        assert_eq!(w["severity"], "warning", "{line}");
        assert!(
            text(&o).contains("  warning VAL-HEADER 2D texture where"),
            "{}",
            text(&o)
        );
    }
}

#[test]
fn strict_cubemaps_fails() {
    let dir = scratch("strict2d");
    let p = dir.join("x_e.dds");
    std::fs::write(&p, dds(false, 4, 1)).unwrap();
    let o = sdc()
        .args(["validate", "--strict-cubemaps"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(4), "{}", text(&o));
    assert!(text(&o).contains("FAIL "), "{}", text(&o));
}

/// FC7: `sdc convert` says what re-encodes before the run; a fresh output folder is a first export.
#[test]
fn convert_says_what_re_encodes_before_the_run() {
    let dir = scratch("recode");
    let input = dir.join("in");
    std::fs::create_dir_all(&input).unwrap();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([90, 120, 30, 255]))
        .save(input.join("x.png"))
        .unwrap();
    let out = dir.join("out");
    let _ = std::fs::remove_dir_all(&out);
    let o = sdc()
        .args(["convert", "--accept-suggestions", "high"])
        .arg(&input)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let want = format!("First export with {}: 1 file", env!("CARGO_PKG_VERSION"));
    assert!(text(&o).contains(&want), "{}", text(&o));
    // Up to date: nothing to say.
    let o = sdc()
        .args(["convert", "--accept-suggestions", "high"])
        .arg(&input)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!text(&o).contains("re-encode"), "{}", text(&o));
    assert!(!text(&o).contains("First export"), "{}", text(&o));
    // A record an older version wrote with another encode version: the line names the type and the new version.
    let m = out.join("_sdc").join("manifest.jsonl");
    let mut subtype = None;
    let lines: Vec<String> = std::fs::read_to_string(&m)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            subtype = Some(
                serde_json::from_value::<engine::config::Subtype>(v["profile"]["subtype"].clone())
                    .unwrap(),
            );
            v["engine_version"] = "1.0.0".into();
            v["encode_version"] = "0.9.0".into();
            v.to_string()
        })
        .collect();
    std::fs::write(&m, lines.join("\n") + "\n").unwrap();
    let run = |extra: &[&str]| {
        sdc()
            .args(["convert", "--accept-suggestions", "high"])
            .args(extra)
            .arg(&input)
            .arg("-o")
            .arg(&out)
            .output()
            .unwrap()
    };
    // `skip` ignores the record, so it says nothing (and leaves the record as it is).
    let o = run(&["--on-existing", "skip"]);
    assert!(!text(&o).contains("re-encode"), "{}", text(&o));
    let o = run(&["--on-existing", "if-changed"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let t = text(&o);
    let line = t
        .lines()
        .find(|l| l.starts_with("1 file re-encodes: the "))
        .unwrap_or_else(|| panic!("{t}"));
    let to = engine::manifest::encode_version(subtype.unwrap());
    assert!(
        line.ends_with(&format!("encoder changed in {to}")),
        "{line}"
    );
}

#[test]
fn cli_validate_flags_a_mipless_cube() {
    let dir = scratch("validate");
    let p = dir.join("m_e.dds");
    std::fs::write(&p, dds(true, 8, 1)).unwrap();
    let o = sdc().args(["validate", "--json"]).arg(&p).output().unwrap();
    assert_eq!(o.status.code(), Some(4), "{}", text(&o));
    let line = String::from_utf8_lossy(&o.stdout);
    let v: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(v["passed"], false, "{line}");
    assert!(line.contains("cube has no mips"), "{line}");
    let good = dir.join("g_e.dds");
    std::fs::write(&good, dds(true, 8, 4)).unwrap();
    let o = sdc().args(["validate"]).arg(&good).output().unwrap();
    assert!(!text(&o).contains("cube has no mips"), "{}", text(&o));
}
