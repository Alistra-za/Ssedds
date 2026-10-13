//! esp scanner spec §2.2: the active load order, MO2 resolution of plugin files, and load-order FormIDs.
mod common;
use common::instance_fixture;
use modscan::archives::{active_plugins, ActivePlugin};
use modscan::mo2::{loose_origins, Instance};
use modscan::plugins::{clamp_local, is_light, plugin_files, prefixes};
use std::fs;
use std::path::Path;

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

fn ap(name: &str, optional: bool) -> ActivePlugin {
    ActivePlugin {
        name: name.into(),
        optional,
    }
}

/// Base masters, then Skyrim.ccc entries not in plugins.txt (optional), then plugins.txt `*` entries; disabled and
/// repeated names are left out.
#[test]
fn active_plugins_in_game_order() {
    let d = instance_fixture("+A\n");
    let r = d.path();
    put(
        r,
        "Game/Skyrim.ccc",
        b"ccQSSSE001-Fish.esm\nccBGSSSE025-AdvDSGS.esm\n",
    );
    put(
        r,
        "profiles/Default/plugins.txt",
        b"# comment\n*ccBGSSSE025-AdvDSGS.esm\n*A.esp\nB.esp\n*a.esp\n",
    );
    let inst = Instance::load(r, None).unwrap();
    assert_eq!(
        active_plugins(&inst),
        [
            ap("skyrim.esm", false),
            ap("update.esm", false),
            ap("dawnguard.esm", false),
            ap("hearthfires.esm", false),
            ap("dragonborn.esm", false),
            ap("ccqssse001-fish.esm", true),
            ap("ccbgssse025-advdsgs.esm", false),
            ap("a.esp", false),
        ]
    );
}

/// modlist.txt is highest priority first: B beats A; the game Data folder is lowest. Only top-level plugin files count.
#[test]
fn plugin_files_take_the_highest_priority_origin() {
    let d = instance_fixture("+B\n+A\n");
    let r = d.path();
    put(r, "mods/A/X.esp", b"a");
    put(r, "mods/B/X.esp", b"b");
    put(r, "mods/A/Only.ESM", b"a");
    put(r, "Game/Data/Skyrim.esm", b"s");
    put(r, "mods/A/readme.txt", b"t");
    put(r, "mods/A/sub/Deep.esp", b"d");
    let inst = Instance::load(r, None).unwrap();
    let (loose, _) = loose_origins(&inst).unwrap();
    let f = plugin_files(&loose);
    assert_eq!(fs::read(&f["x.esp"].0).unwrap(), b"b");
    assert_eq!(
        f["only.esm"].1, "Only.ESM",
        "the on-disk name is kept for display"
    );
    assert!(f["skyrim.esm"].0.starts_with(r.join("Game/Data")));
    assert!(!f.contains_key("readme.txt") && !f.contains_key("deep.esp"));
}

/// Full plugins count their own `LL`; light ones their own `LLL` in FE space (ruling R2).
#[test]
fn prefixes_count_full_and_light_separately() {
    assert_eq!(
        prefixes(&[false, true, false, true]),
        [
            Some((0x0000_0000, 0x00FF_FFFF)),
            Some((0xFE00_0000, 0xFFF)),
            Some((0x0100_0000, 0x00FF_FFFF)),
            Some((0xFE00_1000, 0xFFF)),
        ]
    );
    let full = prefixes(&[false; 255]);
    assert_eq!(full[253], Some((0xFD00_0000, 0x00FF_FFFF)));
    assert_eq!(full[254], None, "0xFE is the light space");
    let light = prefixes(&[true; 4097]);
    assert_eq!(light[4095], Some((0xFEFF_F000, 0xFFF)));
    assert_eq!(light[4096], None);
}

#[test]
fn a_master_index_past_the_list_is_the_plugin_itself() {
    assert_eq!(clamp_local(0x0500_0123, 2), 0x0200_0123);
    assert_eq!(clamp_local(0x0100_0123, 2), 0x0100_0123);
    assert_eq!(clamp_local(0x0200_0123, 2), 0x0200_0123);
}

#[test]
fn light_by_flag_or_extension() {
    assert!(is_light("X.esl", 0));
    assert!(is_light("X.esp", esp::FLAG_ESL));
    assert!(!is_light("X.esm", esp::FLAG_ESM));
}
