//! M3b Task 2–3: GUI set edits and output paths (spec §2.1). The CLI never sets them.
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::sets::*;
use image::{ImageBuffer, Luma, Rgb};
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

fn rgb(dir: &Path, rel: &str, c: [u8; 3]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgb([c[0].wrapping_add(x as u8), c[1], c[2]]))
        .save(p)
        .unwrap();
}

fn gray(dir: &Path, rel: &str, v: u8) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(p)
        .unwrap();
}

/// `in/landscape/rock01{,_n}.png`, `in/landscape/rock02{,_n}.png`, and loose `in/stone/stone_roughness.png` +
/// `stone_ao.png` (they pack `stone/stone_rmaos` on their own).
fn fixture() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("in");
    rgb(&r, "landscape/rock01.png", [90, 60, 30]);
    rgb(&r, "landscape/rock01_n.png", [128, 128, 255]);
    rgb(&r, "landscape/rock02.png", [60, 90, 30]);
    rgb(&r, "landscape/rock02_n.png", [128, 128, 255]);
    gray(&r, "stone/stone_roughness.png", 120);
    gray(&r, "stone/stone_ao.png", 200);
    d
}

fn req(d: &TempDir) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    r
}

fn plan_events(req: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn set_of(p: &Plan, tail: &str) -> String {
    p.jobs
        .iter()
        .find(|j| j.input.to_string_lossy().replace('\\', "/").ends_with(tail))
        .unwrap_or_else(|| panic!("no job for {tail}"))
        .set_key
        .clone()
}

fn packed_outputs_of(p: &Plan) -> Vec<String> {
    let mut v: Vec<String> = p
        .jobs
        .iter()
        .filter(|j| j.pack.is_some())
        .map(|j| j.output.to_string_lossy().replace('\\', "/"))
        .collect();
    v.sort();
    v
}

#[test]
fn ids_and_keys_round_trip() {
    let root = Path::new("C:/Out");
    let key = set_key(root, "Landscape/Rock01");
    assert_eq!(key, "c:/out/landscape/rock01");
    assert_eq!(set_id(root, &key), "landscape/rock01");
    // A key not under the root is its own id.
    assert_eq!(set_id(root, "d:/x/y"), "d:/x/y");
}

/// Ids fold as keys do (Unicode lower case), so the GUI compares them with `same_id` (final review I3).
#[test]
fn ids_fold_as_keys_do() {
    let root = Path::new("C:/Out");
    assert_eq!(set_key(root, r"Landscape\Öl/"), "c:/out/landscape/öl");
    assert_eq!(fold_id("/Äpfel/Stein/"), "äpfel/stein");
    assert!(same_id("landscape/Öl", "landscape/öl"));
    assert!(same_id(r"Äpfel\stein", "äpfel/Stein"));
    assert!(!same_id("landscape/öl", "landscape/ol"));
}

#[test]
fn no_edits_change_nothing() {
    let d = fixture();
    let (a, _) = plan_events(req(&d));
    let mut r = req(&d);
    r.set_edits = vec![];
    let (b, _) = plan_events(r);
    let outs = |p: &Plan| {
        let mut v: Vec<_> = p
            .jobs
            .iter()
            .map(|j| (j.output.clone(), j.set_key.clone()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(outs(&a), outs(&b));
}

#[test]
fn moving_raw_maps_moves_the_packed_output_into_the_target_set() {
    let d = fixture();
    let (before, _) = plan_events(req(&d));
    assert!(packed_outputs_of(&before)
        .iter()
        .any(|o| o.ends_with("stone/stone_rmaos.dds")));
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Move {
            file: "stone/stone_roughness.png".into(),
            to: "landscape/rock02".into(),
        },
        SetEdit::Move {
            file: "stone/stone_ao.png".into(),
            to: "landscape/rock02".into(),
        },
    ];
    let (after, ev) = plan_events(r);
    let rock02 = set_of(&after, "landscape/rock02.png");
    let packed: Vec<_> = after.jobs.iter().filter(|j| j.pack.is_some()).collect();
    assert_eq!(packed.len(), 1);
    assert_eq!(
        packed[0].set_key, rock02,
        "the recipe now belongs to rock02's set"
    );
    // The set_resolved event of rock02 lists the moved raw maps.
    let members = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::SetResolved { members, .. } if e.set.as_deref() == Some(rock02.as_str()) => {
                Some(members.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(members
        .iter()
        .any(|m| m.ends_with("stone/stone_roughness.png")));
    assert!(ev
        .iter()
        .all(|e| !matches!(&e.kind, EventKind::Warning(w) if w.code == Code::SetEditStale)));
}

#[test]
fn create_then_move_builds_a_new_set_and_remove_splits_a_member_off() {
    let d = fixture();
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Create {
            set: "mine/boulder".into(),
        },
        SetEdit::Move {
            file: "landscape/rock01.png".into(),
            to: "mine/boulder".into(),
        },
        SetEdit::Remove {
            file: "landscape/rock02_n.png".into(),
        },
    ];
    let (p, _) = plan_events(r);
    let root = d.path().join("out");
    assert_eq!(
        set_id(&root, &set_of(&p, "landscape/rock01.png")),
        "mine/boulder"
    );
    let n2 = set_of(&p, "landscape/rock02_n.png");
    assert_ne!(n2, set_of(&p, "landscape/rock02.png"));
    assert_eq!(set_id(&root, &n2), "landscape/rock02_n");
}

#[test]
fn rename_keeps_members_and_changes_the_key() {
    let d = fixture();
    let mut r = req(&d);
    r.set_edits = vec![SetEdit::Rename {
        from: "landscape/rock01".into(),
        to: "landscape/boulder".into(),
    }];
    let (p, _) = plan_events(r);
    let root = d.path().join("out");
    assert_eq!(
        set_id(&root, &set_of(&p, "landscape/rock01.png")),
        "landscape/boulder"
    );
    assert_eq!(
        set_id(&root, &set_of(&p, "landscape/rock01_n.png")),
        "landscape/boulder"
    );
}

#[test]
fn a_stale_edit_is_reported_and_skipped() {
    let d = fixture();
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Move {
            file: "landscape/gone.png".into(),
            to: "landscape/rock01".into(),
        },
        SetEdit::Move {
            file: "landscape/rock02.png".into(),
            to: "nowhere/set".into(),
        },
        SetEdit::Rename {
            from: "landscape/rock01".into(),
            to: "landscape/rock02".into(),
        },
    ];
    let (p, ev) = plan_events(r);
    let stale: Vec<_> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == Code::SetEditStale => Some(w.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(stale.len(), 3, "{stale:?}");
    assert_eq!(stale[0].data["edit"]["op"], "move");
    assert!(stale[0].data["reason"]
        .as_str()
        .unwrap()
        .contains("gone.png"));
    // Nothing moved.
    assert_ne!(
        set_of(&p, "landscape/rock02.png"),
        set_of(&p, "landscape/rock01.png")
    );
}

use std::collections::BTreeMap;
use std::path::PathBuf;

fn outputs(p: &Plan) -> Vec<String> {
    let mut v: Vec<String> = p
        .jobs
        .iter()
        .map(|j| j.output.to_string_lossy().replace('\\', "/"))
        .collect();
    v.sort();
    v
}

#[test]
fn output_paths_rewrite_files_and_packed_outputs_but_not_set_keys() {
    let d = fixture();
    let (before, _) = plan_events(req(&d));
    let key_before = set_of(&before, "landscape/rock01.png");
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Move {
            file: "stone/stone_roughness.png".into(),
            to: "landscape/rock01".into(),
        },
        SetEdit::Move {
            file: "stone/stone_ao.png".into(),
            to: "landscape/rock01".into(),
        },
    ];
    r.output_paths = BTreeMap::from([
        (
            OutputKey::File("landscape/rock01.png".into()),
            PathBuf::from("textures/pbr/landscape/rock01.dds"),
        ),
        (
            OutputKey::File("landscape/rock01_n.png".into()),
            PathBuf::from("textures/pbr/landscape/rock01_n.dds"),
        ),
        (
            OutputKey::Packed {
                set: "landscape/rock01".into(),
                suffix: "rmaos".into(),
            },
            PathBuf::from("textures/pbr/landscape/rock01_rmaos.dds"),
        ),
    ]);
    let (p, _) = plan_events(r);
    let o = outputs(&p);
    for tail in [
        "out/textures/pbr/landscape/rock01.dds",
        "out/textures/pbr/landscape/rock01_n.dds",
        "out/textures/pbr/landscape/rock01_rmaos.dds",
    ] {
        assert!(
            o.iter().any(|x| x.ends_with(tail)),
            "{tail} missing from {o:?}"
        );
    }
    assert_eq!(
        set_of(&p, "landscape/rock01.png"),
        key_before,
        "keys never follow output paths"
    );
}

#[test]
fn two_files_given_one_output_path_are_duplicates() {
    let d = fixture();
    let mut r = req(&d);
    r.output_paths = BTreeMap::from([
        (
            OutputKey::File("landscape/rock01.png".into()),
            PathBuf::from("textures/pbr/x/rock.dds"),
        ),
        (
            OutputKey::File("landscape/rock02.png".into()),
            PathBuf::from("textures/pbr/x/rock.dds"),
        ),
    ]);
    let (p, _) = plan_events(r);
    let dup: Vec<_> = p
        .skipped
        .iter()
        .filter(|s| s.code == Some(Code::InDuplicate))
        .collect();
    assert_eq!(dup.len(), 2, "{:?}", p.skipped);
}

#[test]
fn an_output_path_for_a_missing_file_is_ignored() {
    let d = fixture();
    let (a, _) = plan_events(req(&d));
    let mut r = req(&d);
    r.output_paths = BTreeMap::from([(OutputKey::File("gone.png".into()), PathBuf::from("x.dds"))]);
    let (b, _) = plan_events(r);
    assert_eq!(outputs(&a), outputs(&b));
}

/// Final review Minor 3: an output path is joined under the output root, so one that is absolute or climbs out with
/// `..` is ignored (file and packed outputs alike): nothing is written outside the export folder.
#[test]
fn an_output_path_outside_the_output_root_is_ignored() {
    let d = fixture();
    let (a, _) = plan_events(req(&d));
    let mut bad = vec![
        PathBuf::from("../escape/rock01.dds"),
        PathBuf::from("textures/../../escape/rock01.dds"),
        d.path().join("elsewhere/rock01.dds"),
    ];
    if cfg!(windows) {
        bad.push(PathBuf::from(r"\escape\rock01.dds"));
        bad.push(PathBuf::from("C:escape/rock01.dds"));
    } else {
        bad.push(PathBuf::from("/escape/rock01.dds"));
    }
    for rel in bad {
        let mut r = req(&d);
        r.output_paths = BTreeMap::from([
            (OutputKey::File("landscape/rock01.png".into()), rel.clone()),
            (
                OutputKey::Packed {
                    set: "stone/stone".into(),
                    suffix: "rmaos".into(),
                },
                rel.clone(),
            ),
        ]);
        let (b, _) = plan_events(r);
        assert_eq!(outputs(&a), outputs(&b), "{}", rel.display());
    }
}
