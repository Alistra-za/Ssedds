use render3d::scene::Scene;
use render3d::textures::{default_for_register, register_for_slot};

#[test]
fn slot_register_tables() {
    assert_eq!(register_for_slot(0, false, false, 0), Some(0));
    assert_eq!(register_for_slot(2, false, false, 2), Some(6));
    assert_eq!(register_for_slot(5, false, true, 2), Some(12));
    assert_eq!(register_for_slot(5, false, true, 7), Some(2));
    assert_eq!(register_for_slot(1, false, false, 4), Some(4));
    assert_eq!(register_for_slot(11, false, false, 6), Some(8));
    assert_eq!(register_for_slot(0, true, false, 5), Some(5));
    assert_eq!(register_for_slot(0, true, false, 6), Some(9));
    assert_eq!(register_for_slot(0, true, false, 7), Some(12));
    assert_eq!(default_for_register(1), [128, 128, 255, 255]);
    // Capture-confirmed rows (frame11178, see register_for_slot docs).
    assert_eq!(register_for_slot(4, false, false, 2), Some(12)); // Arch_Head _sk
    assert_eq!(register_for_slot(6, false, false, 7), Some(9)); // Lashes lash_flow
    assert_eq!(register_for_slot(0, true, false, 3), Some(4)); // Moisture m_p
    assert_eq!(register_for_slot(0, true, false, 4), None);
}

fn shaders() -> Option<std::path::PathBuf> {
    std::env::var("SDC_CS_SHADERS").ok().map(Into::into)
}

fn lit(px: &[u8]) -> usize {
    px.chunks(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
        .count()
}

/// Needs the installed CS: `SDC_CS_SHADERS=... cargo test -p render3d -- --ignored`
#[test]
#[ignore]
fn renders_fixture_nonblack_with_defaults() {
    let root = shaders().unwrap();
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = render3d::Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Furniture_Col_SE.nif"
    ))
    .unwrap();
    let rep = r.load(&nif, &|_| None);
    assert!(rep.compile_errors.is_empty(), "{:?}", rep.compile_errors);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert!(rep.shapes > 0);
    let px = r.render(&Scene::neutral(), 256, 256).unwrap();
    let lit = lit(&px);
    assert!(
        lit > 256 * 256 / 50,
        "object should cover >2% of the frame and be lit, got {lit} px"
    );
}

#[test]
#[ignore]
fn missing_textures_use_defaults() {
    let root = shaders().unwrap();
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = render3d::Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
    let rep = r.load(&nif, &|_| None);
    assert!(!rep.missing_textures.is_empty());
    assert!(rep.compile_errors.is_empty(), "{:?}", rep.compile_errors);
    assert!(rep.shapes > 0, "{rep:?}");
    let px = r.render(&Scene::neutral(), 128, 128).unwrap();
    assert!(lit(&px) > 0, "skinned shape with default textures renders");
}

#[test]
#[ignore]
fn le_mesh_is_skipped() {
    let root = shaders().unwrap();
    let mut r = render3d::Renderer::new(
        render3d::locate::fingerprint(&root, None).unwrap(),
        tempfile::tempdir().unwrap().keep(),
    )
    .unwrap();
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Animated_LE.nif"
    ))
    .unwrap();
    let rep = r.load(&nif, &|_| None);
    assert!(rep.le_mesh);
    assert_eq!(rep.shapes, 0);
}
