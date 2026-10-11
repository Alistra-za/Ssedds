mod common;
use common::instance_fixture;
use modscan::mo2::{loose_origins, Instance, OriginKind};
use modscan::qtini::Ini;

#[test]
fn ini_placeholders_and_bytearray() {
    let ini = Ini::parse(
        "[General]\ngamePath=@ByteArray(C:\\\\Games\\\\Skyrim Special Edition)\nselected_profile=@ByteArray(Nexus)\n\
         [Settings]\nbase_directory=D:/MO2\nmod_directory=%BASE_DIR%/mods\noverwrite_directory=\"%BASE_DIR%/overwrite\"\n",
    );
    assert_eq!(
        ini.get("General", "gamePath").unwrap(),
        "C:\\Games\\Skyrim Special Edition"
    );
    assert_eq!(ini.get("General", "selected_profile").unwrap(), "Nexus");
    assert_eq!(
        ini.get("Settings", "mod_directory").unwrap(),
        "%BASE_DIR%/mods"
    );
    assert_eq!(
        ini.get("Settings", "overwrite_directory").unwrap(),
        "%BASE_DIR%/overwrite"
    );
    assert_eq!(ini.get("Settings", "nope"), None);
}

#[test]
fn modlist_first_line_is_highest_priority() {
    let d = instance_fixture("# comment\n+C\n-B\n+Sep_separator\n+A\n*DLC: Dawnguard\n");
    let inst = Instance::load(d.path(), None).unwrap();
    assert_eq!(inst.profile, "Default");
    let (o, missing) = loose_origins(&inst).unwrap();
    assert!(missing.is_empty());
    let order: Vec<_> = o.iter().map(|x| (x.kind, x.name.as_str())).collect();
    assert_eq!(
        order,
        [
            (OriginKind::GameData, "Data"),
            (OriginKind::Mod, "A"),
            (OriginKind::Mod, "C"),
            (OriginKind::Overwrite, "overwrite")
        ]
    );
    assert!(o[1].priority < o[2].priority);
}

#[test]
fn missing_and_unlisted_mods() {
    let d = instance_fixture("+Ghost\n+A\n");
    let inst = Instance::load(d.path(), None).unwrap();
    let (o, missing) = loose_origins(&inst).unwrap();
    assert_eq!(missing, ["Ghost"]);
    assert!(o.iter().all(|x| x.name != "B" && x.name != "C"));
}

#[test]
fn bad_profile_is_an_error() {
    let d = instance_fixture("+A\n");
    assert!(Instance::load(d.path(), Some("Nope")).is_err());
}

#[test]
fn relative_base_directory_is_joined_to_the_instance_root() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let game = r.join("Game");
    std::fs::create_dir_all(game.join("Data")).unwrap();
    std::fs::create_dir_all(r.join("Base/mods/A")).unwrap();
    std::fs::create_dir_all(r.join("Base/profiles/P")).unwrap();
    std::fs::write(r.join("Base/profiles/P/modlist.txt"), "+A\n").unwrap();
    std::fs::write(
        r.join("ModOrganizer.ini"),
        format!(
            "[General]\ngamePath=@ByteArray({})\nselected_profile=@ByteArray(P)\n[Settings]\nbase_directory=Base\n",
            game.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    // Run from another working directory: the relative base must not resolve against the cwd.
    let inst = Instance::load(r, None).unwrap();
    assert_eq!(inst.mods, r.join("Base").join("mods"));
    assert_eq!(inst.profiles, r.join("Base").join("profiles"));
    let (o, missing) = loose_origins(&inst).unwrap();
    assert!(missing.is_empty());
    assert!(o.iter().any(|x| x.name == "A"));
}
