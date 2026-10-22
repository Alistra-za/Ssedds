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

#[test]
fn folders_discover_every_supported_image_extension() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("in");
    std::fs::create_dir_all(&t).unwrap();
    for n in [
        "a.png", "b.tga", "c.tif", "d.tiff", "e.exr", "f.hdr", "g.bmp", "h.jpg", "i.JPEG", "j.dds",
        "k.psd", "l.PSB", "n.webp", "o.kra", "p.ora", "m.txt",
    ] {
        std::fs::write(t.join(n), b"x").unwrap();
    }
    let found = engine::discover::discover(&[t], &d.path().join("out"), false).unwrap();
    let names: Vec<String> = found
        .iter()
        .map(|f| f.rel.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 15, "{names:?}");
    assert!(!names.iter().any(|n| n.ends_with(".txt")));
}

#[test]
fn an_i_member_joins_its_set() {
    let (base, suffix) = engine::group::split_suffix("glass_i");
    assert_eq!((base.as_str(), suffix.as_deref()), ("glass", Some("i")));
    assert!(!engine::group::is_vocabulary("i"));
}

/// Audit 6 (DECISIONS Note Y4): an unreadable folder is skipped and reported; the rest is still found.
#[test]
fn an_unreadable_folder_is_skipped_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    let locked = src.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(src.join("a.png"), b"x").unwrap();
    std::fs::write(locked.join("b.png"), b"x").unwrap();
    // *S-1-1-0 is Everyone, whatever the system language.
    let icacls = |args: &[&str]| {
        let ok = std::process::Command::new("icacls")
            .arg(&locked)
            .args(args)
            .status()
            .unwrap();
        assert!(ok.success());
    };
    icacls(&["/deny", "*S-1-1-0:(OI)(CI)(RX)"]);
    let effective = std::fs::read_dir(&locked).is_err();
    let r = discover_reporting(
        std::slice::from_ref(&src),
        &d.path().join("out"),
        true,
        None,
    );
    icacls(&["/remove:d", "*S-1-1-0"]);
    if !effective {
        eprintln!("ACL deny had no effect for this account; skipping");
        return;
    }
    let (files, skipped) = r.unwrap();
    let rels: Vec<String> = files
        .iter()
        .map(|f| f.rel.to_string_lossy().into_owned())
        .collect();
    assert_eq!(rels, ["a.png"]);
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].contains("locked"), "{skipped:?}");
}

/// Audit 6: an unreadable input folder itself still fails (the child case is the test above).
#[test]
fn an_unreadable_root_is_still_an_error() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    let locked = src.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(src.join("a.png"), b"x").unwrap();
    let deny = |p: &Path, on: bool| {
        let args: &[&str] = if on {
            &["/deny", "*S-1-1-0:(OI)(CI)(RX)"]
        } else {
            &["/remove:d", "*S-1-1-0"]
        };
        let ok = std::process::Command::new("icacls")
            .arg(p)
            .args(args)
            .status()
            .unwrap();
        assert!(ok.success());
    };
    let out = d.path().join("out");
    deny(&src, true);
    let effective = std::fs::read_dir(&src).is_err();
    let root = discover_reporting(std::slice::from_ref(&src), &out, true, None);
    deny(&src, false);
    if !effective {
        eprintln!("ACL deny had no effect for this account; skipping");
        return;
    }
    assert!(root.is_err(), "an unreadable input folder must fail");
}

/// Audit REPORT #3 (Note BC): an output root equal to the input folder (in place), or one that contains it, skips
/// nothing; only an output root strictly inside the input is skipped.
#[test]
fn an_output_root_at_or_above_the_input_skips_nothing() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    touch(&src.join("a.png"));
    touch(&src.join("sub/b.png"));
    let files = discover(std::slice::from_ref(&src), &src, true).unwrap();
    assert_eq!(rels(&files), ["a.png", "sub/b.png"]);
    let files = discover(&[src.join("sub")], &src, true).unwrap();
    assert_eq!(rels(&files), ["b.png"]);
}

/// Trace-debug #13 (Note BQ): a file reached through two inputs (a folder and a file in it, or a folder and its
/// subfolder) is discovered once, through the first input that reaches it.
#[test]
fn overlapping_inputs_yield_one_entry_per_file() {
    let d = tempfile::tempdir().unwrap();
    let tex = d.path().join("tex");
    touch(&tex.join("rock.png"));
    touch(&tex.join("sub/moss.png"));
    let out = d.path().join("out");
    let files = discover(&[tex.clone(), tex.join("rock.png")], &out, false).unwrap();
    assert_eq!(rels(&files), ["rock.png"]);
    let files = discover(&[tex.clone(), tex.join("sub")], &out, true).unwrap();
    assert_eq!(rels(&files), ["rock.png", "sub/moss.png"]);
}

/// Trace-debug #24 (Note BQ8): a `textures` folder nested under the mod's own `textures` stays in the game path, as
/// `nif::path::canon_texture` keeps it; the cut is at the first `textures` below the input root, and the root's own
/// `textures` counts only when none lies below it (owner, 2026-10-16).
#[test]
fn game_path_keeps_a_nested_textures_folder() {
    let abs = Path::new("C:/mods/Data/textures/armor/textures/foo_d.dds");
    assert_eq!(
        game_path(abs, Path::new("textures/armor/textures/foo_d.dds")),
        "textures/armor/textures/foo_d.dds"
    );
    // The input root is itself a `textures` folder: the one below it wins (BQ8, the container layout).
    assert_eq!(
        game_path(abs, Path::new("armor/textures/foo_d.dds")),
        "textures/foo_d.dds"
    );
}

/// Owner, 2026-10-16 (Note BQ8): an input root named `textures` that holds mod folders keys each file from the mod's
/// own `textures`, as before #24.
#[test]
fn game_path_of_a_textures_root_holding_mods_starts_at_the_mods_textures() {
    let abs = Path::new("D:/Textures/MyMod/textures/landscape/rock.png");
    assert_eq!(
        game_path(abs, Path::new("MyMod/textures/landscape/rock.png")),
        "textures/landscape/rock.png"
    );
    // No `textures` below the root: the root's own is the textures root.
    let abs = Path::new("D:/Textures/landscape/rock.png");
    assert_eq!(
        game_path(abs, Path::new("landscape/rock.png")),
        "textures/landscape/rock.png"
    );
}

/// A directory link `link` -> `target`: a symlink, else (Windows, no privilege) a junction. `false` when neither can
/// be made here.
fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        return std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

/// Trace-debug #27 (Note CU): recursive discovery follows directory links, and a link loop is reported, not dropped.
#[test]
fn recursive_discovery_follows_links_and_reports_a_loop() {
    let d = tempfile::tempdir().unwrap();
    let real = d.path().join("real");
    let input = d.path().join("in");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::create_dir_all(&input).unwrap();
    std::fs::write(real.join("a.png"), b"x").unwrap();
    if !make_dir_link(&input.join("link"), &real) {
        eprintln!("SKIPPED: cannot create a directory link here (no privilege)");
        return;
    }
    let (files, skipped) =
        discover_reporting(&[input.clone()], &d.path().join("out"), true, None).unwrap();
    let rels: Vec<_> = files.iter().map(|f| f.rel.clone()).collect();
    assert_eq!(rels, vec![Path::new("link").join("a.png")], "{skipped:?}");
    assert!(skipped.is_empty(), "{skipped:?}");

    // A link back up to its own ancestor is a loop: one warning line, and the walk ends.
    assert!(make_dir_link(&real.join("up"), &input));
    let (_, skipped) = discover_reporting(&[input], &d.path().join("out"), true, None).unwrap();
    assert!(!skipped.is_empty(), "a link loop was dropped silently");
}
