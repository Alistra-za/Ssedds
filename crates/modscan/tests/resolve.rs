mod common;
use common::instance_fixture;
use modscan::archives::{active_bsas, ActiveBsa};
use modscan::mo2::{loose_origins, Instance, LooseOrigin, OriginKind};
use modscan::resolve::{winners, Copy, OriginRef};
use std::path::PathBuf;

fn lo(name: &str, prio: i32) -> LooseOrigin {
    LooseOrigin {
        kind: OriginKind::Mod,
        name: name.into(),
        root: PathBuf::from(name),
        priority: prio,
    }
}
fn c(path: &str, o: OriginRef) -> Copy {
    Copy {
        path: path.into(),
        origin: o,
        size: 1,
        mtime: 0,
    }
}

#[test]
fn loose_beats_bsa_and_priority_orders_loose() {
    let loose = vec![
        lo("Data", -1),
        lo("A", 0),
        lo("B", 1),
        lo("overwrite", i32::MAX),
    ];
    let bsas = vec![
        ActiveBsa {
            path: "Skyrim - Meshes0.bsa".into(),
            providing_mod: None,
            rank: 0,
        },
        ActiveBsa {
            path: "B.bsa".into(),
            providing_mod: Some("B".into()),
            rank: 10,
        },
    ];
    let copies = vec![
        c("meshes/x.nif", OriginRef::Bsa(1)),
        c("meshes/x.nif", OriginRef::Loose(1)),
        c("meshes/y.nif", OriginRef::Bsa(0)),
        c("meshes/y.nif", OriginRef::Bsa(1)),
        c("meshes/z.nif", OriginRef::Loose(1)),
        c("meshes/z.nif", OriginRef::Loose(2)),
        c("meshes/z.nif", OriginRef::Loose(3)),
    ];
    let w = winners(&copies, &loose, &bsas, &|_| false);
    assert_eq!(w["meshes/x.nif"], (1, 2));
    assert_eq!(w["meshes/y.nif"], (3, 2));
    assert_eq!(w["meshes/z.nif"], (6, 3));
}

#[test]
fn exclude_gives_source_view() {
    let loose = vec![lo("A", 0), lo("PGP", 5), lo("C", 9)];
    let copies = vec![
        c("meshes/x.nif", OriginRef::Loose(0)),
        c("meshes/x.nif", OriginRef::Loose(1)),
    ];
    let game = winners(&copies, &loose, &[], &|_| false);
    let source = winners(&copies, &loose, &[], &|o| o == OriginRef::Loose(1));
    assert_eq!(game["meshes/x.nif"].0, 1);
    assert_eq!(source["meshes/x.nif"].0, 0);
}

#[test]
fn plugin_bsas_follow_load_order() {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    std::fs::write(
        r.join("profiles/Default/plugins.txt"),
        "# comment\n*Foo.esp\nBar.esp\n*Baz.esp\n",
    )
    .unwrap();
    std::fs::write(r.join("mods/A/Foo.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/A/Bar.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/B/Baz - Textures.bsa"), b"").unwrap();
    let inst = Instance::load(r, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let b = active_bsas(&inst, &loose);
    // plugin-derived names are lowercased (NTFS is case-insensitive); only fixture origins can match, whatever the real skyrim.ini lists
    let names: Vec<String> = b
        .iter()
        .map(|x| {
            x.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase()
        })
        .collect();
    assert_eq!(names, ["foo.bsa", "baz - textures.bsa"]);
    assert_eq!(b[0].providing_mod.as_deref(), Some("A"));
    assert_eq!(b[1].providing_mod.as_deref(), Some("B"));
    assert!(b[0].rank < b[1].rank);
}
