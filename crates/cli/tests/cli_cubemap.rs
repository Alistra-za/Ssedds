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
