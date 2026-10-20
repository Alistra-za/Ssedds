use render3d::golden::*;
use std::fs;
use std::path::Path;

fn px(v: f32) -> [f32; 4] {
    [v; 4]
}

#[test]
fn diff_identical_and_one_off() {
    let a = vec![0.25f32; 16];
    let m = [true; 4];
    let d = diff(&a, &a, &m, 0.1);
    assert_eq!(
        (d.max_abs, d.mean_abs, d.over, d.pixels),
        (0.0, 0.0, 0.0, 4)
    );
    let mut b = a.clone();
    b[5] += 0.5; // one channel of pixel 1
    let d = diff(&a, &b, &m, 0.1);
    assert_eq!(d.max_abs, 0.5);
    assert!((d.mean_abs - 0.5 / 16.0).abs() < 1e-7);
    assert_eq!(d.over, 0.25);
}

#[test]
fn diff_ignores_unmasked_and_empty_mask() {
    let a = vec![0.0f32; 8];
    let mut b = a.clone();
    b[4..8].copy_from_slice(&px(9.0));
    let d = diff(&a, &b, &[true, false], 0.1);
    assert_eq!((d.max_abs, d.pixels, d.over), (0.0, 1, 0.0));
    let d = diff(&a, &b, &[false, false], 0.1);
    assert_eq!((d.pixels, d.mean_abs, d.over), (0, 0.0, 0.0));
    assert!(!d.mean_abs.is_nan());
}

#[test]
fn format_tol_table() {
    assert_eq!(
        format_tol("R10G10B10A2_UNORM"),
        Some(Tol::Abs(1.0 / 1023.0))
    );
    assert_eq!(
        format_tol("R11G11B10_FLOAT"),
        Some(Tol::Rel([1.0 / 64.0, 1.0 / 64.0, 1.0 / 32.0, 0.0]))
    );
    for f in ["R16G16B16A16_FLOAT", "R16G16_FLOAT", "R16_FLOAT"] {
        assert_eq!(format_tol(f), Some(Tol::Rel([1e-3; 4])));
    }
    assert_eq!(format_tol("R16_UNORM"), Some(Tol::Abs(1.0 / 65535.0)));
    assert_eq!(format_tol("R8G8_UNORM"), Some(Tol::Abs(1.0 / 255.0)));
    assert_eq!(format_tol("R8G8B8A8_UNORM"), Some(Tol::Abs(1.0 / 255.0)));
    assert_eq!(format_tol("D24S8_TYPELESS"), None);
}

#[test]
fn diff_tol_relative_and_noise() {
    let t = format_tol("R16G16B16A16_FLOAT").unwrap();
    let b = px(1000.0);
    assert_eq!(diff_tol(&px(1000.5), &b, &[true], t, 0.0).over, 0.0);
    assert_eq!(diff_tol(&px(1002.0), &b, &[true], t, 0.0).over, 1.0);
    // noise floor raises the limit
    assert_eq!(diff_tol(&px(1002.0), &b, &[true], t, 2.5).over, 0.0);
    // zero capture value compares absolutely, not as 0 * rel
    assert_eq!(diff_tol(&px(0.0), &px(0.0), &[true], t, 0.0).over, 0.0);
    assert_eq!(diff_tol(&px(0.01), &px(0.0), &[true], t, 0.0).over, 1.0);
    // floor: a tiny nonzero difference against 0 must pass
    assert_eq!(diff_tol(&px(5e-7), &px(0.0), &[true], t, 0.0).over, 0.0);
    // Abs
    let t = Tol::Abs(1.0 / 255.0);
    assert_eq!(diff_tol(&px(0.003), &px(0.0), &[true], t, 0.0).over, 0.0);
    assert_eq!(diff_tol(&px(0.01), &px(0.0), &[true], t, 0.0).over, 1.0);
}

fn write_noise(dir: &Path) -> std::path::PathBuf {
    let p = dir.join("noise.json");
    fs::write(
        &p,
        r#"{"tolerances":{"static":{"subjects":2,"files":{"mrt0.f32":{"p99":0.001,"max":0.04}}},
        "Core01":{"subjects":1,"files":{"mrt0.f32":{"p99":0.5,"max":1.5},"mrt2.f32":{"p99":9.0,"max":10.0}}}}}"#,
    )
    .unwrap();
    p
}

#[test]
fn noise_load_and_limit() {
    let d = tempfile::tempdir().unwrap();
    let n = Noise::load(&write_noise(d.path())).unwrap();
    assert_eq!(n.limit("ev31949_Core01_22", "mrt2.f32"), Some((9.0, 10.0)));
    assert_eq!(n.limit("ev31949_Core01_22", "mrt0.f32"), Some((0.5, 1.5)));
    assert_eq!(n.limit("ev31535_Coffin", "mrt0.f32"), Some((0.001, 0.04)));
    assert_eq!(
        n.limit("ev31572_Wreath04_0", "mrt0.f32"),
        Some((0.001, 0.04))
    );
    assert_eq!(n.limit("ev31535_Coffin", "mrt2.f32"), None);
    assert!(Noise::load(&d.path().join("missing.json")).is_err());
    fs::write(d.path().join("bad.json"), "{").unwrap();
    assert!(Noise::load(&d.path().join("bad.json")).is_err());
}

fn capture(root: &Path, cap: &str, profile: &str) {
    fs::create_dir_all(root.join(cap)).unwrap();
    fs::write(
        root.join(cap).join("manifest.json"),
        format!(r#"{{"profile":"{profile}"}}"#),
    )
    .unwrap();
}

fn subject(root: &Path, cap: &str, name: &str) {
    fs::create_dir_all(root.join(cap).join(name)).unwrap();
}

#[test]
fn subjects_filter_and_sort() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    capture(r, "capB_2", "A_RT");
    capture(r, "capA_1", "A_RT");
    capture(r, "capC_3", "B");
    subject(r, "capB_2", "ev20_Zed");
    subject(r, "capB_2", "ev10_Alpha_3");
    subject(r, "capA_1", "ev99_Coffin");
    subject(r, "capA_1", "composite");
    subject(r, "capC_3", "ev5_Other");
    let s = subjects(r, &["A_RT"]).unwrap();
    let got: Vec<_> = s
        .iter()
        .map(|s| {
            (
                s.capture.as_str(),
                s.event,
                s.shape.as_str(),
                s.profile.as_str(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("capA_1", 99, "Coffin", "A_RT"),
            ("capB_2", 10, "Alpha_3", "A_RT"),
            ("capB_2", 20, "Zed", "A_RT")
        ]
    );
    assert_eq!(subjects(r, &["A_RT", "B"]).unwrap().len(), 4);
    assert!(subjects(&r.join("nope"), &["A_RT"]).is_err());
}

#[test]
fn subjects_profile_comes_from_capture_manifest() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    capture(r, "cap", "A_RT");
    subject(r, "cap", "ev1_X");
    fs::write(r.join("cap/ev1_X/manifest.json"), r#"{"profile":"B"}"#).unwrap();
    assert_eq!(subjects(r, &["A_RT"]).unwrap().len(), 1);
    assert!(subjects(r, &["B"]).unwrap().is_empty());
    // missing or malformed capture manifest is an error naming the path
    fs::create_dir_all(r.join("cap2/ev2_Y")).unwrap();
    let e = subjects(r, &["A_RT"]).err().unwrap();
    assert!(e.contains("cap2"), "{e}");
    fs::write(r.join("cap2/manifest.json"), "{}").unwrap();
    assert!(subjects(r, &["A_RT"]).is_err());
}

#[test]
fn nonfinite_channels_fail_closed() {
    let b = vec![0.5f32; 4];
    let mut a = b.clone();
    a[2] = f32::NAN;
    let d = diff(&a, &b, &[true], 0.1);
    assert_eq!((d.over, d.max_abs), (1.0, f32::INFINITY));
    assert!(d.mean_abs.is_finite());
    let t = Tol::Abs(1.0);
    let d = diff_tol(&a, &b, &[true], t, 0.0);
    assert_eq!((d.over, d.max_abs), (1.0, f32::INFINITY));
    assert!(d.mean_abs.is_finite());
    // bit-identical non-finite values pass
    let (mut a, mut b) = (vec![0.5f32; 4], vec![0.5f32; 4]);
    a[1] = f32::INFINITY;
    b[1] = f32::INFINITY;
    let d = diff(&a, &b, &[true], 0.1);
    assert_eq!((d.over, d.max_abs), (0.0, 0.0));
    a[3] = f32::NAN;
    b[3] = f32::NAN;
    assert_eq!(diff_tol(&a, &b, &[true], t, 0.0).over, 0.0);
}

#[test]
fn noise_rejects_bad_values() {
    let d = tempfile::tempdir().unwrap();
    for (i, bad) in ["1e100", "-1"].iter().enumerate() {
        let p = d.path().join(format!("n{i}.json"));
        let j = format!(
            r#"{{"tolerances":{{"static":{{"files":{{"mrt0.f32":{{"p99":{bad},"max":0.1}}}}}}}}}}"#
        );
        fs::write(&p, j).unwrap();
        let e = Noise::load(&p).err().unwrap();
        assert!(e.contains("mrt0.f32") && e.contains("static"), "{e}");
    }
}

#[test]
fn mrt_info_and_raw_readers() {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join("mrt.json"),
        r#"{"crop":[2,361,558,140],"rts":[{"index":0,"format":"R16G16B16A16_FLOAT"},{"index":1,"format":"R16G16_FLOAT"}]}"#,
    )
    .unwrap();
    let m = read_mrt_info(d.path()).unwrap();
    assert_eq!(m.crop, [2, 361, 558, 140]);
    assert_eq!(
        m.rts,
        vec![
            (0, "R16G16B16A16_FLOAT".to_string()),
            (1, "R16G16_FLOAT".to_string())
        ]
    );
    fs::write(d.path().join("mrt.json"), r#"{"crop":[1,2],"rts":[]}"#).unwrap();
    assert!(read_mrt_info(d.path()).is_err());

    let bytes: Vec<u8> = [1.5f32, -2.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let p = d.path().join("a.f32");
    fs::write(&p, &bytes).unwrap();
    assert_eq!(read_f32(&p, 2).unwrap(), vec![1.5, -2.0]);
    assert!(read_f32(&p, 3).is_err());
    let p = d.path().join("mask.bin");
    fs::write(&p, [0u8, 1, 255]).unwrap();
    assert_eq!(read_mask(&p, 3).unwrap(), vec![false, true, true]);
    assert!(read_mask(&p, 4).is_err());
}

#[test]
#[ignore]
fn real_root_smoke() {
    let Ok(root) = std::env::var("SDC_GOLDEN") else {
        println!("skipped: set SDC_GOLDEN");
        return;
    };
    let root = Path::new(&root);
    let s = subjects(root, &["A_RT"]).unwrap();
    assert!(!s.is_empty());
    let first = &s[0];
    let m = read_mrt_info(&first.dir).unwrap();
    let n = (m.crop[2] * m.crop[3]) as usize;
    let v = read_f32(&first.dir.join("mrt0.f32"), n * 4).unwrap();
    let mask = read_mask(&first.dir.join("mask.bin"), n).unwrap();
    println!(
        "{} subjects; first {}/{} crop {:?} rts {:?} mask {}",
        s.len(),
        first.capture,
        first.shape,
        m.crop,
        m.rts.len(),
        mask.iter().filter(|b| **b).count()
    );
    let d = diff(&v, &v, &mask, 0.0);
    assert_eq!(d.max_abs, 0.0);
    let noise = Noise::load(
        &root.join("noise_Skyrim_AE_1.6.1170_frame207657_Skyrim_AE_1.6.1170_frame207658.json"),
    )
    .unwrap();
    let core = noise.limit("ev31949_Core01_22", "mrt2.f32").unwrap();
    let stat = noise.limit("ev31535_Coffin", "mrt2.f32").unwrap();
    println!("Core01 mrt2 {core:?} static mrt2 {stat:?}");
    assert_ne!(core, stat);
}
