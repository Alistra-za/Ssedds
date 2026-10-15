// real files written by nifly (GPL-3.0 fixtures)
use nif::scan::{scan, Shader};

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn se_fixtures_scan_without_problems() {
    for f in [
        "TestNifFile_Skinned_SE.nif",
        "TestNifFile_Furniture_Col_SE.nif",
        "TestNifFile_DeepGraph_SE.nif",
        "TestNifFile_Skinned_Dynamic_SE.nif",
        "TestNifFile_Optimize_LE_to_SE.nif",
    ] {
        let s = scan(&read(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(s.bsver, 100, "{f}");
        assert!(s.problems.is_empty(), "{f}: {:?}", s.problems);
        assert!(!s.shapes.is_empty(), "{f}");
        assert!(
            s.shapes.iter().any(|r| !matches!(r.shader, Shader::None)),
            "{f}"
        );
    }
}

#[test]
fn le_fixture_is_flagged() {
    let s = scan(&read("TestNifFile_Animated_LE.nif")).unwrap();
    assert!(s.le_mesh);
}

/// Opt-in: walks a real mesh folder. `SDC_NIF_CORPUS=C:\path\to\meshes cargo test -p nif -- --ignored corpus`
#[test]
#[ignore]
fn corpus() {
    let root = std::env::var("SDC_NIF_CORPUS").expect("set SDC_NIF_CORPUS");
    let (mut files, mut bad) = (0, vec![]);
    for e in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if e.path()
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("nif"))
        {
            files += 1;
            match scan(&std::fs::read(e.path()).unwrap()) {
                Ok(s) if s.problems.is_empty() => {}
                Ok(s) => bad.push(format!("{}: {:?}", e.path().display(), s.problems)),
                Err(nif::NifError::NotSkyrim { .. }) => {}
                Err(err) => bad.push(format!("{}: {err}", e.path().display())),
            }
        }
    }
    println!("{files} files, {} with problems", bad.len());
    assert!(bad.is_empty(), "{:#?}", &bad[..bad.len().min(20)]);
}

#[test]
fn every_furniture_shape_has_a_3d_index_numbered_from_zero() {
    let s = scan(&read("TestNifFile_Furniture_Col_SE.nif")).unwrap();
    assert!(
        s.shapes.iter().all(|r| s.index3d.contains_key(&r.block)),
        "{:?} vs {:?}",
        s.index3d,
        s.shapes.iter().map(|r| r.block).collect::<Vec<_>>()
    );
    let mut v: Vec<u32> = s.index3d.values().copied().collect();
    v.sort_unstable();
    assert_eq!(v, (0..v.len() as u32).collect::<Vec<_>>());
}
