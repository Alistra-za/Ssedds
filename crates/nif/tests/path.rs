use nif::path::{canon_mesh, canon_texture, is_facegen};

#[test]
fn odd_paths_table() {
    // (raw, norm, empty, rebased, prefixed, non_dds)
    type Case = (&'static [u8], &'static str, bool, bool, bool, bool);
    let cases: &[Case] = &[
        (
            b"textures\\armor\\iron\\iron_d.dds",
            "textures/armor/iron/iron_d.dds",
            false,
            false,
            false,
            false,
        ),
        (
            b"Textures\\Armor\\Iron_N.DDS",
            "textures/armor/iron_n.dds",
            false,
            false,
            false,
            false,
        ),
        (
            b"Data\\Textures\\x\\a.dds",
            "textures/x/a.dds",
            false,
            true,
            false,
            false,
        ),
        (
            b"C:\\build\\Skyrim\\Data\\textures\\x\\a.dds",
            "textures/x/a.dds",
            false,
            true,
            false,
            false,
        ),
        (
            b"armor\\iron\\iron_d.dds",
            "textures/armor/iron/iron_d.dds",
            false,
            false,
            true,
            false,
        ),
        (
            b"textures/x/a.dds  ",
            "textures/x/a.dds",
            false,
            false,
            false,
            false,
        ),
        (
            b"textures\\x\\a.tga",
            "textures/x/a.tga",
            false,
            false,
            false,
            true,
        ),
        (b"", "", true, false, false, false),
        (b"   ", "", true, false, false, false),
    ];
    for &(raw, norm, empty, rebased, prefixed, non_dds) in cases {
        let t = canon_texture(raw);
        assert_eq!(t.norm, norm, "{:?}", String::from_utf8_lossy(raw));
        assert_eq!(
            (
                t.flags.empty,
                t.flags.rebased,
                t.flags.prefixed,
                t.flags.non_dds
            ),
            (empty, rebased, prefixed, non_dds),
            "{:?}",
            String::from_utf8_lossy(raw)
        );
        assert_eq!(t.raw, nif::text::lossy(raw));
    }
}

#[test]
fn mesh_paths_and_facegen() {
    assert_eq!(
        canon_mesh("Meshes\\Armor\\Iron\\x.NIF"),
        "meshes/armor/iron/x.nif"
    );
    assert!(is_facegen(
        "meshes/actors/character/facegendata/facegeom/skyrim.esm/00013bbd.nif"
    ));
    assert!(is_facegen(
        "textures/actors/character/facegendata/facetint/skyrim.esm/00013bbd.dds"
    ));
    assert!(!is_facegen(
        "meshes/actors/character/character assets/malehead.nif"
    ));
}
