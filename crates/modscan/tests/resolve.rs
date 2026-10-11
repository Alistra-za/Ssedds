mod common;
use common::instance_fixture;
use modscan::archives::{active_bsas_with, ActiveBsa};
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

// ---- active BSAs (hermetic: the INI folder is a fixture dir, never the real Documents) ----

fn names(b: &[ActiveBsa]) -> Vec<String> {
    b.iter()
        .map(|x| x.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

fn bsas(r: &std::path::Path, ini_dir: Option<&std::path::Path>) -> (Vec<ActiveBsa>, Vec<String>) {
    let inst = Instance::load(r, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    active_bsas_with(&inst, &loose, ini_dir)
}

#[test]
fn plugin_bsas_follow_load_order_and_keep_disk_case() {
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
    let ini = tempfile::tempdir().unwrap();
    let (b, _) = bsas(r, Some(ini.path()));
    assert_eq!(names(&b), ["Foo.bsa", "Baz - Textures.bsa"]);
    assert_eq!(b[0].providing_mod.as_deref(), Some("A"));
    assert_eq!(b[1].providing_mod.as_deref(), Some("B"));
    assert!(b[0].rank < b[1].rank);
}

#[test]
fn higher_priority_origin_provides_the_bsa() {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    std::fs::write(r.join("profiles/Default/plugins.txt"), "*Foo.esp\n").unwrap();
    std::fs::write(r.join("mods/A/foo.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/B/FOO.BSA"), b"").unwrap();
    let (b, _) = bsas(r, None);
    assert_eq!(names(&b), ["FOO.BSA"]);
    assert_eq!(b[0].providing_mod.as_deref(), Some("B"));
}

/// Owner rule: vanilla `Skyrim - *.bsa` in the game Data folder are always active, first, in SSE order,
/// with no INI anywhere; a missing archive list is only a note.
#[test]
fn vanilla_bsas_are_active_without_any_ini() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    let data = r.join("Game/Data");
    for f in [
        "Skyrim - Textures0.bsa",
        "Skyrim - Meshes0.bsa",
        "Skyrim - Zzz.bsa",
        "Skyrim - Aaa.bsa",
        "Other.bsa",
    ] {
        std::fs::write(data.join(f), b"").unwrap();
    }
    std::fs::write(r.join("profiles/Default/plugins.txt"), "*Foo.esp\n").unwrap();
    std::fs::write(r.join("mods/A/Foo.bsa"), b"").unwrap();
    let empty = tempfile::tempdir().unwrap();
    let (b, notes) = bsas(r, Some(empty.path()));
    assert_eq!(
        names(&b),
        [
            "Skyrim - Meshes0.bsa",
            "Skyrim - Textures0.bsa",
            "Skyrim - Aaa.bsa",
            "Skyrim - Zzz.bsa",
            "Foo.bsa"
        ]
    );
    assert!(b.windows(2).all(|w| w[0].rank < w[1].rank));
    assert!(b[0].providing_mod.is_none());
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("sResourceArchiveList"), "{notes:?}");
}

#[test]
fn ini_lists_add_archives_after_vanilla_and_before_plugins() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    std::fs::write(r.join("Game/Data/Skyrim - Meshes0.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/A/Extra.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/A/Foo.bsa"), b"").unwrap();
    std::fs::write(r.join("profiles/Default/plugins.txt"), "*Foo.esp\n").unwrap();
    let ini = tempfile::tempdir().unwrap();
    std::fs::write(
        ini.path().join("Skyrim.ini"),
        "[Archive]\nsResourceArchiveList=Skyrim - Meshes0.bsa, Extra.bsa\n",
    )
    .unwrap();
    let (b, notes) = bsas(r, Some(ini.path()));
    assert_eq!(names(&b), ["Skyrim - Meshes0.bsa", "Extra.bsa", "Foo.bsa"]);
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn skyrim_default_ini_is_the_fallback_list() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    std::fs::write(r.join("mods/A/Extra.bsa"), b"").unwrap();
    std::fs::write(
        r.join("Game/Skyrim_Default.ini"),
        "[Archive]\nsResourceArchiveList2=Extra.bsa\n",
    )
    .unwrap();
    let ini = tempfile::tempdir().unwrap();
    std::fs::write(ini.path().join("skyrim.ini"), "[Display]\nx=1\n").unwrap();
    let (b, notes) = bsas(r, Some(ini.path()));
    assert_eq!(names(&b), ["Extra.bsa"]);
    assert!(notes.is_empty(), "{notes:?}");
}

/// F8: Creation Club plugins named in `<game>\Skyrim.ccc` but absent from plugins.txt load right after the
/// five base masters (MO2 treats them as primary plugins and leaves them out of plugins.txt).
#[test]
fn skyrim_ccc_plugins_follow_the_base_masters() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    std::fs::write(
        r.join("Game/Skyrim.ccc"),
        "ccBGSSSE001-Fish.esm\r\nccQDRSSE001-SurvivalMode.esl\r\n",
    )
    .unwrap();
    std::fs::write(r.join("Game/Data/Dawnguard.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/A/ccBGSSSE001-Fish.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/A/ccQDRSSE001-SurvivalMode.bsa"), b"").unwrap();
    std::fs::write(r.join("mods/A/Foo.bsa"), b"").unwrap();
    std::fs::write(
        r.join("profiles/Default/plugins.txt"),
        "*Foo.esp\n*ccQDRSSE001-SurvivalMode.esl\n",
    )
    .unwrap();
    let (b, _) = bsas(r, None);
    // SurvivalMode is listed in plugins.txt, so it keeps its plugins.txt place (after Foo).
    assert_eq!(
        names(&b),
        [
            "Dawnguard.bsa",
            "ccBGSSSE001-Fish.bsa",
            "Foo.bsa",
            "ccQDRSSE001-SurvivalMode.bsa"
        ]
    );
}
