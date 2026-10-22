mod common;
use common::instance_fixture;
use modscan::mo2::{loose_origins, Instance};

fn put_modlist(root: &std::path::Path, bytes: &[u8]) {
    std::fs::write(root.join("profiles/Default/modlist.txt"), bytes).unwrap();
}

#[test]
fn modlist_with_a_bom_keeps_its_first_line() {
    let d = instance_fixture("");
    put_modlist(d.path(), b"\xEF\xBB\xBF+A\n+C\n");
    let inst = Instance::load(d.path(), None).unwrap();
    let (o, missing) = loose_origins(&inst).unwrap();
    assert!(missing.is_empty(), "{missing:?}");
    assert!(o.iter().any(|x| x.name == "A"));
}

#[test]
fn modlist_decodes_each_line_on_its_own() {
    let d = instance_fixture("");
    std::fs::create_dir_all(d.path().join("mods/caf\u{e9}")).unwrap();
    std::fs::create_dir_all(d.path().join("mods/na\u{ef}ve")).unwrap();
    // First name is one ANSI 0xE9 byte, second is UTF-8 (0xC3 0xAF).
    put_modlist(d.path(), b"+caf\xE9\n+na\xC3\xAFve\n");
    let inst = Instance::load(d.path(), None).unwrap();
    let (o, missing) = loose_origins(&inst).unwrap();
    assert!(missing.is_empty(), "{missing:?}");
    assert!(o.iter().any(|x| x.name == "caf\u{e9}"));
    assert!(o.iter().any(|x| x.name == "na\u{ef}ve"));
}

#[test]
fn modorganizer_ini_with_a_bom_loads() {
    let d = instance_fixture("+A\n");
    let p = d.path().join("ModOrganizer.ini");
    let mut b = b"\xEF\xBB\xBF".to_vec();
    b.extend(std::fs::read(&p).unwrap());
    std::fs::write(&p, b).unwrap();
    assert_eq!(Instance::load(d.path(), None).unwrap().profile, "Default");
}

#[test]
fn missing_modlist_is_still_an_error() {
    let d = instance_fixture("+A\n");
    let inst = Instance::load(d.path(), None).unwrap();
    std::fs::remove_file(d.path().join("profiles/Default/modlist.txt")).unwrap();
    assert!(loose_origins(&inst).is_err());
}
