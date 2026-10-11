#[test]
fn walks_case_insensitively_and_skips_facegen() {
    let d = tempfile::tempdir().unwrap();
    let mk = |p: &str| {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, b"x").unwrap();
    };
    mk("Meshes/Armor/A.NIF");
    mk("meshes/actors/character/FaceGenData/FaceGeom/skyrim.esm/0001.nif");
    mk("meshes/b.txt");
    mk("textures/a_d.dds");
    let got: Vec<_> = vfs::loose::walk(d.path(), &["meshes"], &["nif"])
        .into_iter()
        .map(|f| f.norm)
        .collect();
    assert_eq!(got, ["meshes/armor/a.nif"]);
}
