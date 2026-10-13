//! esp spec §2.1: plugin model paths join the NIF scan's `path` rows (`meshes/…`, lower case, `/`).
use nif::path::canon_model;

#[test]
fn plugin_model_paths_get_the_meshes_prefix() {
    assert_eq!(canon_model(b"Clutter\\Book.NIF"), "meshes/clutter/book.nif");
    assert_eq!(
        canon_model(b"meshes\\clutter\\book.nif"),
        "meshes/clutter/book.nif"
    );
    assert_eq!(
        canon_model(b"C:\\Games\\Skyrim\\Data\\Meshes\\x.nif"),
        "meshes/x.nif"
    );
    assert_eq!(canon_model(b"  \\Clutter\\x.nif"), "meshes/clutter/x.nif");
    assert_eq!(canon_model(b""), "");
}
