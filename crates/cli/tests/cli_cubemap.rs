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
