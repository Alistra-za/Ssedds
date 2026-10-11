use engine::discover::*;
use engine::group::*;
use std::path::Path;

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

fn rels(files: &[InputFile]) -> Vec<String> {
    files
        .iter()
        .map(|f| f.rel.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn suffixes_longest_first_case_insensitive() {
    assert_eq!(
        split_suffix("wood01_n"),
        ("wood01".into(), Some("n".into()))
    );
    assert_eq!(
        split_suffix("Armor_RMAOS"),
        ("Armor".into(), Some("rmaos".into()))
    );
    assert_eq!(
        split_suffix("body_msn"),
        ("body".into(), Some("msn".into()))
    );
    assert_eq!(split_suffix("rock_em"), ("rock".into(), Some("em".into())));
    assert_eq!(
        split_suffix("hair_flow"),
        ("hair".into(), Some("flow".into()))
    );
    assert_eq!(split_suffix("plain"), ("plain".into(), None));
    assert_eq!(split_suffix("_n"), ("_n".into(), None));
}

#[test]
fn rfaos_and_envmask_are_recognised() {
    assert_eq!(
        split_suffix("cloth_rfaos"),
        ("cloth".into(), Some("rfaos".into()))
    );
    assert_eq!(
        split_suffix("Cloth_RFAOS"),
        ("Cloth".into(), Some("rfaos".into()))
    );
    assert_eq!(
        split_suffix("rock_envmask"),
        ("rock".into(), Some("envmask".into()))
    );
    assert_eq!(SUFFIXES.len(), 20);
}

#[test]
fn split_suffix_handles_non_ascii_stems() {
    assert_eq!(split_suffix("Ärmör_N"), ("Ärmör".into(), Some("n".into())));
    assert_eq!(
        split_suffix("鎧_rmaos"),
        ("鎧".into(), Some("rmaos".into()))
    );
    assert_eq!(split_suffix("鎧"), ("鎧".into(), None));
}

#[test]
fn discovers_mirrors_paths_and_finds_game_path() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("mod/textures/armor/iron");
    for f in [
        "iron_n.png",
        "iron.png",
        "iron_m.tga",
        "notes.txt",
        "old.dds",
        "sub/deep.png",
    ] {
        touch(&src.join(f));
    }
    let out = d.path().join("out");
    let files = discover(std::slice::from_ref(&src), &out, false).unwrap();
    assert_eq!(
        rels(&files),
        ["iron.png", "iron_m.tga", "iron_n.png", "old.dds"]
    );
    let n = files
        .iter()
        .find(|f| f.rel.as_path() == Path::new("iron_n.png"))
        .unwrap();
    assert_eq!(n.game_path, "textures/armor/iron/iron_n.png");
    assert_eq!(n.output, out.join("iron_n.dds"));
    assert_eq!(files.iter().filter(|f| f.kind == InputKind::Dds).count(), 1);
    assert_eq!(discover(&[src], &out, true).unwrap().len(), 5);
}

#[test]
fn game_path_uses_last_textures_component_else_rel() {
    let abs = Path::new("C:/mods/Textures/x/textures/Armor/Iron_N.png");
    assert_eq!(
        game_path(abs, Path::new("Iron_N.png")),
        "textures/armor/iron_n.png"
    );
    let abs = Path::new("C:/art/wood/Wood_N.png");
    assert_eq!(
        game_path(abs, Path::new("Sub/Wood_N.png")),
        "sub/wood_n.png"
    );
}

#[test]
fn skips_sdc_and_output_root_and_unknown_extensions() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    touch(&src.join("a.png"));
    touch(&src.join("_sdc/manifest.png"));
    touch(&src.join("out/b.png"));
    touch(&src.join("sub/c.PNG"));
    touch(&src.join("sub/readme.md"));
    let files = discover(std::slice::from_ref(&src), &src.join("out"), true).unwrap();
    assert_eq!(rels(&files), ["a.png", "sub/c.PNG"]);
}

#[test]
fn output_root_match_is_case_insensitive_on_windows() {
    if !cfg!(windows) {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    touch(&src.join("a.png"));
    touch(&src.join("out/b.png"));
    let files = discover(std::slice::from_ref(&src), &src.join("OUT"), true).unwrap();
    assert_eq!(rels(&files), ["a.png"]);
}

#[test]
fn explicit_file_inputs_are_always_included() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("textures/odd.bmp");
    let g = d.path().join("textures/pre.dds");
    touch(&f);
    touch(&g);
    let out = d.path().join("out");
    let files = discover(&[f, g], &out, false).unwrap();
    assert_eq!(rels(&files), ["odd.bmp", "pre.dds"]);
    assert_eq!(files[0].kind, InputKind::Image);
    assert_eq!(files[1].kind, InputKind::Dds);
    assert_eq!(files[0].game_path, "textures/odd.bmp");
    assert_eq!(files[0].output, out.join("odd.dds"));
}

#[test]
fn non_ascii_paths_round_trip() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("ソース/textures/鎧");
    touch(&src.join("鎧_n.png"));
    let files = discover(&[src], &d.path().join("出力"), false).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].game_path, "textures/鎧/鎧_n.png");
    assert_eq!(files[0].output, d.path().join("出力").join("鎧_n.dds"));
}

#[test]
fn groups_sets_and_flags_duplicates() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    for f in [
        "wood01.png",
        "wood01_n.png",
        "wood01_n.tga",
        "Wood01_RMAOS.png",
        "rock_n.png",
    ] {
        touch(&src.join(f));
    }
    let files = discover(&[src], &d.path().join("out"), false).unwrap();
    let (sets, dups) = group_sets(&files);
    let wood = sets.iter().find(|s| s.key.ends_with("/wood01")).unwrap();
    assert_eq!(wood.members.len(), 4);
    assert!(wood.suffixes.contains("rmaos") && wood.suffixes.contains("n"));
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].len(), 2);
}

#[test]
fn sets_do_not_cross_directories_and_duplicates_ignore_case() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    for f in ["a/wood.png", "b/wood.png", "a/Rock.png", "a/rock.TGA"] {
        touch(&src.join(f));
    }
    let files = discover(&[src], &d.path().join("out"), true).unwrap();
    let (sets, dups) = group_sets(&files);
    assert_eq!(sets.iter().filter(|s| s.key.ends_with("/wood")).count(), 2);
    assert_eq!(dups.len(), 1);
    assert_eq!(dups[0].len(), 2);
}
