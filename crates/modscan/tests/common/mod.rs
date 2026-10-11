use std::fs;

pub fn instance_fixture(modlist: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    let game = r.join("Game");
    fs::create_dir_all(game.join("Data")).unwrap();
    fs::write(
        r.join("ModOrganizer.ini"),
        format!(
            "[General]\ngamePath=@ByteArray({})\nselected_profile=@ByteArray(Default)\n",
            game.display().to_string().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    for m in ["A", "B", "C", "Sep_separator"] {
        fs::create_dir_all(r.join("mods").join(m)).unwrap();
    }
    fs::create_dir_all(r.join("overwrite")).unwrap();
    fs::create_dir_all(r.join("profiles/Default")).unwrap();
    fs::write(r.join("profiles/Default/modlist.txt"), modlist).unwrap();
    d
}
