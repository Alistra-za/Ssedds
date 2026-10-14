//! M5 (DECISIONS Note NV2/NV3): finding nvcompress and reading its version. Runs against the test-only fake.
use engine::nvtt::*;
use std::path::Path;

const FAKE: &str = env!("CARGO_BIN_EXE_nvcompress");

#[test]
fn banner_parse() {
    assert_eq!(
        parse_banner(
            "NVIDIA Texture Tools 3.2.5 - Copyright NVIDIA Corporation 2007 - 2024\nusage: ..."
        ),
        Ok("3.2.5".into())
    );
    let e = parse_banner("nvcompress version two\n").unwrap_err();
    assert!(e.contains("\"nvcompress version two\""), "{e}");
    assert!(parse_banner("NVIDIA Texture Tools 3.x\n").is_err());
    assert!(parse_banner("").is_err());
}

#[test]
fn locate_order() {
    let d = tempfile::tempdir().unwrap();
    let (a, b, c) = (d.path().join("a"), d.path().join("b"), d.path().join("c"));
    for x in [&a, &b, &c] {
        std::fs::create_dir_all(x).unwrap();
    }
    let on_path = b.join("nvcompress.exe");
    std::fs::write(&on_path, b"").unwrap();
    let default = c.join("nvcompress.exe");
    std::fs::write(&default, b"").unwrap();
    let var = std::env::join_paths([&a, &b]).unwrap();

    // PATH beats the default folder.
    assert_eq!(
        locate(None, Some(&var), &default),
        Ok((on_path.clone(), "PATH"))
    );
    // The default folder is used last.
    assert_eq!(
        locate(None, Some(a.as_os_str()), &default),
        Ok((default.clone(), "default"))
    );
    assert_eq!(
        locate(None, None, &default),
        Ok((default.clone(), "default"))
    );
    // An explicit path wins, and a missing one is not found: no fallback to PATH or the default.
    assert_eq!(
        locate(Some(&default), Some(&var), &default),
        Ok((default.clone(), "explicit"))
    );
    let missing = a.join("nvcompress.exe");
    let e = locate(Some(&missing), Some(&var), &default).unwrap_err();
    assert!(e.contains("does not exist"), "{e}");
    // Nothing anywhere: the message says where it looked.
    let e = locate(None, Some(a.as_os_str()), &a.join("none.exe")).unwrap_err();
    assert!(e.contains("not on PATH") && e.contains("none.exe"), "{e}");
}

#[test]
fn probe_the_fake() {
    let tool = match probe(Some(Path::new(FAKE))) {
        NvttProbe::Found(t) => t,
        m => panic!("{m:?}"),
    };
    assert_eq!(tool.version, "3.2.5");
    assert!(tool.tested());
    assert_eq!(tool.path, Path::new(FAKE));
    match probe(Some(Path::new("Z:/no/such/nvcompress.exe"))) {
        NvttProbe::Missing(why) => assert!(why.contains("does not exist"), "{why}"),
        f => panic!("{f:?}"),
    }
    // Not nvcompress at all: a file that cannot run.
    let d = tempfile::tempdir().unwrap();
    let junk = d.path().join("nvcompress.exe");
    std::fs::write(&junk, b"not an executable").unwrap();
    assert!(matches!(probe_path(&junk), NvttProbe::Missing(_)));
}

#[test]
fn probe_untested_version() {
    // The fake reads SDC_FAKE_NVTT_VERSION from its own environment, so the probe is reproduced on a child here.
    let out = std::process::Command::new(FAKE)
        .env("SDC_FAKE_NVTT_VERSION", "3.3.0")
        .output()
        .unwrap();
    let v = parse_banner(&String::from_utf8_lossy(&out.stdout)).unwrap();
    assert_eq!(v, "3.3.0");
    let t = NvttTool {
        path: FAKE.into(),
        version: v,
    };
    assert!(!t.tested());
}

#[test]
fn cell_probes_once_on_first_use() {
    let cell = NvttCell::new(Some(FAKE.into()));
    assert!(matches!(cell.get(), NvttProbe::Found(_)));
    assert!(matches!(cell.into_probed(), Some(NvttProbe::Found(_))));
    assert_eq!(NvttCell::new(Some(FAKE.into())).into_probed(), None);
}
