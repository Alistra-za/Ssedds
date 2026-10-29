//! The set preview's model (P1-A Tasks 13–14): encodes, fingerprint and change classification, query keys,
//! PGPatcher simulation per shape, and the orchestration without a GPU.
mod support;
use render3d::perf_log;
use sdc_gui::model::items::{ItemKind, ItemStatus};
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::preview::DEBOUNCE;
use sdc_gui::model::set_preview::*;
use sdc_gui::model::sets::{SetView, SlotKind};
use sdc_gui::model::viewport3d::FileSource;
use sdc_gui::model::AppModel;
use sdc_gui::pgp::writer::EntryInput;
use std::time::{Duration, Instant};
use support::*;

fn rock(m: &AppModel) -> (SetView, EntryInput) {
    json_entries(m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .expect("the fixture's PBR set")
}

fn settle(e: &mut Encoder, m: &AppModel) {
    let t = Instant::now();
    while !e.settled() && t.elapsed() < Duration::from_secs(60) {
        e.pump(m.session.as_ref(), Instant::now(), false);
        e.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(e.settled(), "the encodes did not finish");
}

#[test]
fn member_slots_serve_the_set_outputs_under_their_canonical_paths() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let slots = member_slots(&m, &v, Some(&e));
    let path = |k: SlotKind| slots.iter().find(|s| s.kind == k).map(|s| s.paths.clone());
    assert_eq!(
        path(SlotKind::Diffuse),
        Some(vec!["textures/pbr/landscape/rock01.dds".to_string()])
    );
    assert_eq!(
        path(SlotKind::Normal),
        Some(vec!["textures/pbr/landscape/rock01_n.dds".to_string()])
    );
    // Without the PGPatcher entry (D3): the plain paths.
    let plain = member_slots(&m, &v, None);
    assert!(plain
        .iter()
        .any(|s| s.kind == SlotKind::Diffuse && s.paths == vec![plain_path(SlotKind::Diffuse)]));
}

#[test]
fn encodes_become_dds_bytes_served_as_memory_sources() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    enc.sync(&m, member_slots(&m, &v, Some(&e)), Instant::now(), true);
    assert!(!enc.settled());
    settle(&mut enc, &m);
    let st = &enc.members[&SlotKind::Diffuse].state;
    let EncState::Fresh(out) = st else {
        panic!("{st:?}")
    };
    assert_eq!(&out.dds[..4], b"DDS ");
    let f = engine::dds::read_dds(&out.dds).unwrap();
    assert_eq!((f.width, f.height), (64, 64));
    match &enc.memory_sources()["textures/pbr/landscape/rock01.dds"] {
        FileSource::Memory { id, .. } => assert_eq!(*id, out.id),
        s => panic!("{s:?}"),
    }
    assert!(enc.produces("textures/pbr/landscape/rock01_n.dds"));
}

#[test]
fn a_changed_member_goes_stale_keeps_its_bytes_and_re_encodes_after_the_debounce() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    let mut slots = member_slots(&m, &v, Some(&e));
    enc.sync(&m, slots.clone(), Instant::now(), true);
    settle(&mut enc, &m);
    let first = enc.members[&SlotKind::Diffuse].state.shown().unwrap().id;
    for s in &mut slots {
        if s.kind == SlotKind::Diffuse {
            s.sig.push_str(":edited");
        }
    }
    let now = Instant::now();
    enc.sync(&m, slots.clone(), now, false);
    let st = &enc.members[&SlotKind::Diffuse].state;
    assert!(
        matches!(st, EncState::Stale(Some(e)) if e.id == first),
        "{st:?}"
    );
    assert!(
        matches!(enc.members[&SlotKind::Normal].state, EncState::Fresh(_)),
        "an unchanged member keeps its bytes"
    );
    enc.pump(m.session.as_ref(), now, false);
    assert!(
        matches!(enc.members[&SlotKind::Diffuse].state, EncState::Stale(_)),
        "waits for the debounce"
    );
    enc.pump(m.session.as_ref(), now + DEBOUNCE, false);
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Encoding(Some(_))
    ));
    settle(&mut enc, &m);
    let second = enc.members[&SlotKind::Diffuse].state.shown().unwrap().id;
    assert_ne!(second, first);
    // A path-only change keeps the bytes and encodes nothing.
    for s in &mut slots {
        s.paths
            .push(format!("textures/extra/{:?}.dds", s.kind).to_lowercase());
    }
    enc.sync(&m, slots, Instant::now(), false);
    assert!(enc.settled());
    assert!(enc
        .memory_sources()
        .contains_key("textures/extra/diffuse.dds"));
    assert_eq!(
        enc.members[&SlotKind::Diffuse].state.shown().unwrap().id,
        second
    );
}

#[test]
fn nothing_encodes_while_an_export_runs() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    enc.sync(&m, member_slots(&m, &v, Some(&e)), Instant::now(), true);
    enc.pump(m.session.as_ref(), Instant::now(), true);
    assert!(enc.paused);
    assert!(enc
        .members
        .values()
        .all(|x| matches!(x.state, EncState::NotEncoded)));
    assert!(!enc.busy(), "no repaint loop while paused");
    settle(&mut enc, &m);
    assert!(!enc.paused);
}

#[test]
fn blocked_items_never_encode_and_dds_inputs_use_their_own_bytes() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let mut item = m
        .items
        .iter()
        .find(|i| i.job.is_some() && i.status == ItemStatus::Ready)
        .unwrap()
        .clone();
    assert!(matches!(source_of(&item), Source::Job(_)));
    item.status = ItemStatus::Error(vec![]);
    assert!(matches!(source_of(&item), Source::Blocked(_)));
    item.status = ItemStatus::PackedInto("x".into());
    assert!(matches!(source_of(&item), Source::Blocked(_)));
    item.status = ItemStatus::Ready;
    item.kind = ItemKind::Dds;
    let input = item.input.clone();
    assert!(matches!(source_of(&item), Source::Dds(p) if p == input));
}

// ---- fix round 1: invalidation. Completions are injected so the order is under the test's control.

fn started() -> (
    tempfile::TempDir,
    AppModel,
    Encoder,
    Vec<sdc_gui::model::set_preview::MemberSlot>,
) {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    enc.hold_workers(true);
    let slots = member_slots(&m, &v, Some(&e));
    (d, m, enc, slots)
}

fn fake() -> Result<(Vec<u8>, f64), String> {
    Ok((b"DDS fake".to_vec(), 1.0))
}

fn edit(slots: &mut [sdc_gui::model::set_preview::MemberSlot]) {
    for s in slots.iter_mut().filter(|s| s.kind == SlotKind::Diffuse) {
        s.sig.push_str(":edited");
    }
}

#[test]
fn a_completion_of_a_superseded_encode_never_lands_not_in_the_debounce_nor_late() {
    let (_d, m, mut enc, mut slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots.clone(), t, true);
    enc.pump(m.session.as_ref(), t, false);
    let t1 = enc.token_of(SlotKind::Diffuse);
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Encoding(None)
    ));
    // Edit while encoding: the old completion arrives during the debounce.
    edit(&mut slots);
    enc.sync(&m, slots.clone(), t, false);
    enc.inject(SlotKind::Diffuse, t1, DEFAULT_CAP, fake());
    assert!(!enc.poll());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Stale(None)
    ));
    // The replacement encodes; the old completion arriving late changes nothing.
    enc.pump(m.session.as_ref(), t + DEBOUNCE, false);
    let t2 = enc.token_of(SlotKind::Diffuse);
    assert_ne!(t1, t2);
    enc.inject(SlotKind::Diffuse, t1, DEFAULT_CAP, fake());
    assert!(!enc.poll());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Encoding(None)
    ));
    enc.inject(SlotKind::Diffuse, t2, DEFAULT_CAP, fake());
    assert!(enc.poll());
    let id = enc.members[&SlotKind::Diffuse].state.shown().unwrap().id;
    enc.inject(SlotKind::Diffuse, t1, DEFAULT_CAP, Err("x".into()));
    assert!(!enc.poll());
    let st = &enc.members[&SlotKind::Diffuse].state;
    assert!(matches!(st, EncState::Fresh(e) if e.id == id), "{st:?}");
    // A completion for a finished encode (token spent) is dropped too.
    enc.inject(SlotKind::Diffuse, t2, DEFAULT_CAP, fake());
    assert!(!enc.poll());
}

#[test]
fn a_member_that_becomes_blocked_while_encoding_stays_blocked() {
    let (_d, mut m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots.clone(), t, true);
    enc.pump(m.session.as_ref(), t, false);
    let t1 = enc.token_of(SlotKind::Diffuse);
    let key = slots
        .iter()
        .find(|s| s.kind == SlotKind::Diffuse)
        .unwrap()
        .item
        .clone();
    m.items.iter_mut().find(|i| i.key == key).unwrap().status = ItemStatus::Error(vec![]);
    m.invalidate_views();
    let (v, e) = rock(&m);
    enc.sync(&m, member_slots(&m, &v, Some(&e)), t, false);
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Blocked(_)
    ));
    enc.inject(SlotKind::Diffuse, t1, DEFAULT_CAP, fake());
    enc.inject(SlotKind::Diffuse, t1, DEFAULT_CAP, Err("late".into()));
    assert!(!enc.poll());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Blocked(_)
    ));
}

#[test]
fn a_cap_change_while_encoding_rejects_that_completion_and_schedules_again() {
    let (_d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots.clone(), t, true);
    enc.pump(m.session.as_ref(), t, false);
    let t1 = enc.token_of(SlotKind::Diffuse);
    assert_ne!(t1, 0);
    enc.set_cap(32, t);
    assert!(enc
        .members
        .values()
        .all(|x| matches!(x.state, EncState::Stale(None))));
    assert_eq!(enc.token_of(SlotKind::Diffuse), 0);
    enc.inject(SlotKind::Diffuse, t1, DEFAULT_CAP, fake());
    assert!(!enc.poll());
    enc.pump(m.session.as_ref(), t, false);
    let t2 = enc.token_of(SlotKind::Diffuse);
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Encoding(None)
    ));
    enc.inject(SlotKind::Diffuse, t2, 32, fake());
    assert!(enc.poll());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Fresh(_)
    ));
    // A directly mutated cap is caught by sync too: the fresh member goes stale with its bytes on show.
    enc.cap = 64;
    enc.sync(&m, slots, Instant::now(), true);
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Stale(Some(_))
    ));
}

#[test]
fn a_cap_assigned_away_and_back_does_not_strand_an_encoding_member() {
    let (_d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots.clone(), t, true);
    enc.pump(m.session.as_ref(), t, false);
    let t1 = enc.token_of(SlotKind::Diffuse);
    enc.cap = 32;
    enc.inject(SlotKind::Diffuse, t1, DEFAULT_CAP, fake());
    enc.poll(); // rejected: the encode is useless now, the member is scheduled again
    enc.cap = DEFAULT_CAP;
    enc.sync(&m, slots, t, false);
    enc.pump(m.session.as_ref(), t + DEBOUNCE, false);
    let t2 = enc.token_of(SlotKind::Diffuse);
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Encoding(None)
    ));
    assert_ne!(t2, t1, "re-encoded, not the stranded generation");
    enc.inject(SlotKind::Diffuse, t2, DEFAULT_CAP, fake());
    assert!(enc.poll());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Fresh(_)
    ));
}

#[test]
fn another_source_with_the_same_stamp_is_another_signature_and_ids_are_process_wide() {
    let (_d, m, mut a, slots) = started();
    a.hold_workers(false);
    let item = m
        .items
        .iter()
        .find(|i| i.job.is_some() && i.kind != ItemKind::Dds)
        .unwrap();
    let mut other = item.clone();
    other.job.as_mut().unwrap().input = std::path::PathBuf::from("elsewhere/other.png");
    assert_ne!(signature(item), signature(&other));
    let mut b = Encoder::new(DEFAULT_CAP);
    a.sync(&m, slots.clone(), Instant::now(), true);
    b.sync(&m, slots, Instant::now(), true);
    settle(&mut a, &m);
    settle(&mut b, &m);
    let id = |e: &Encoder| e.members[&SlotKind::Diffuse].state.shown().unwrap().id;
    assert_ne!(id(&a), id(&b));
}

#[test]
fn the_member_signature_follows_the_fill_and_cube_face_inputs() {
    use engine::config::{CubeLayout, RawMap};
    use engine::cube::{CubeJob, CubeSource};
    use engine::helper::FillJob;
    use engine::pack::PackSource;
    let (_d, m, _enc, _slots) = started();
    let item = m
        .items
        .iter()
        .find(|i| i.job.is_some() && i.kind != ItemKind::Dds)
        .unwrap();
    let stamp = |n: u64| {
        Some(engine::policy::SourceStamp {
            size: n,
            mtime_ns: n,
        })
    };
    let mut base = item.clone();
    let job = base.job.as_mut().unwrap();
    job.fill = Some(FillJob {
        source: PackSource {
            path: "h.png".into(),
            role: RawMap::Roughness,
            stamp: stamp(1),
            read: None,
        },
        read: None,
        invert: false,
        assigned: true,
    });
    job.cube = Some(CubeJob {
        layout: CubeLayout::Faces,
        sources: (0..6)
            .map(|f| CubeSource {
                path: format!("f{f}.png").into(),
                stamp: stamp(10 + f as u64),
                face: Some(f),
            })
            .collect(),
    });
    let a = signature(&base);
    assert_eq!(
        a,
        signature(&base.clone()),
        "a re-plan with nothing changed"
    );
    let mut inverted = base.clone();
    inverted.job.as_mut().unwrap().fill.as_mut().unwrap().invert = true;
    assert_ne!(a, signature(&inverted), "fill invert");
    let mut face = base.clone();
    face.job.as_mut().unwrap().cube.as_mut().unwrap().sources[1].stamp = stamp(99);
    assert_ne!(a, signature(&face), "cube face 1 stamp");
}

// ---- Task 14: fingerprint, classification, query keys, simulation, orchestration ----

use modscan::store::{MatchRow, TexKey};
use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::ShapeId;
use sdc_gui::model::viewport3d::Content;
use sdc_gui::pgp::writer::entry_node;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn default_block() -> Value {
    sdc_gui::pgp::writer::document(&[]).to_value()["default"].clone()
}

fn entry(pairs: &[(&str, Value)]) -> EntryInput {
    EntryInput {
        settings: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        slots: Default::default(),
        derived_match: r"x\rock".into(),
        section: false,
    }
}

fn value(pairs: &[(&str, Value)]) -> Value {
    entry_node(&entry(pairs)).to_value()
}

fn fp_of(entry: Option<EntryInput>, sig: &str) -> SetFingerprint {
    SetFingerprint {
        set_key: "k".into(),
        label: "x/rock".into(),
        landscape: false,
        grass: false,
        entry,
        prefix: Some(r"textures\pbr\x\rock".into()),
        members: vec![MemberSlot {
            kind: SlotKind::Diffuse,
            item: "x/rock.png".into(),
            job_key: "textures/pbr/x/rock.dds".into(),
            status: "Ready".into(),
            sig: sig.into(),
            paths: vec!["textures/pbr/x/rock.dds".into()],
        }],
    }
}

#[test]
fn a_material_key_is_json_only_and_a_texture_change_re_encodes() {
    let d = default_block();
    let a = fp_of(Some(entry(&[])), "s1");
    let rough = fp_of(Some(entry(&[("roughness_scale", json!(0.5))])), "s1");
    assert_eq!(
        classify(Some(&a), &rough, &d),
        Delta {
            material: true,
            ..Default::default()
        }
    );
    let tex = fp_of(Some(entry(&[])), "s2");
    assert_eq!(
        classify(Some(&a), &tex, &d),
        Delta {
            encode: true,
            ..Default::default()
        }
    );
    assert_eq!(classify(Some(&a), &a.clone(), &d), Delta::default());
}

#[test]
fn match_keys_requery_paths_reload_and_another_set_or_d3_is_everything() {
    let d = default_block();
    let a = fp_of(Some(entry(&[])), "s1");
    let moved = fp_of(Some(entry(&[("match1", json!(r"y\rock"))])), "s1");
    let dm = classify(Some(&a), &moved, &d);
    assert!(dm.requery && dm.reload && !dm.material, "{dm:?}");
    let mut slot = fp_of(Some(entry(&[])), "s1");
    slot.entry
        .as_mut()
        .unwrap()
        .slots
        .insert(3, r"effects\white.dds".into());
    assert_eq!(
        classify(Some(&a), &slot, &d),
        Delta {
            reload: true,
            ..Default::default()
        }
    );
    let mut other = a.clone();
    other.set_key = "other".into();
    assert_eq!(
        classify(Some(&a), &other, &d),
        Delta {
            set: true,
            ..Default::default()
        }
    );
    assert!(
        classify(Some(&a), &fp_of(None, "s1"), &d).set,
        "simulated → plain (D3)"
    );
    assert!(classify(None, &a, &d).set);
}

#[test]
fn the_fixture_fingerprint_follows_edits_and_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    let d = default_block();
    let before = fingerprint(&m, &key).unwrap();
    assert!(before.entry.is_some());
    m.set_json_value("landscape/rock01", "roughness_scale", Some(json!(0.25)));
    let after = fingerprint(&m, &key).unwrap();
    assert_eq!(
        classify(Some(&before), &after, &d),
        Delta {
            material: true,
            ..Default::default()
        }
    );
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let d3 = fingerprint(&m, &key).unwrap();
    assert!(d3.entry.is_none(), "json_entries is CS-only");
    assert!(classify(Some(&after), &d3, &d).set);
}

#[test]
fn the_query_key_follows_the_match_mode() {
    let d = default_block();
    let key = |pairs: &[(&str, Value)]| tex_key(&value(pairs), &d).unwrap();
    let default_match1 = TexKey::Exact {
        slot: 0,
        path: r"x\rock.dds".into(),
    };
    assert_eq!(key(&[]), (default_match1.clone(), None));
    assert_eq!(
        key(&[
            ("match_diffuse", json!(r"y\z")),
            ("match_normal", json!(r"y\z"))
        ])
        .0,
        default_match1,
        "old keys are never written"
    );
    assert_eq!(
        key(&[("path_contains", json!("rock"))]).0,
        TexKey::Contains {
            slot: 0,
            frag: "rock".into()
        }
    );
    assert_eq!(
        key(&[("match1", json!("")), ("match3", json!(r"x\rock_g.dds"))]).0,
        TexKey::Exact {
            slot: 2,
            path: r"x\rock_g.dds".into()
        }
    );
    assert_eq!(
        key(&[("nif_filter", json!("armor"))]).1.as_deref(),
        Some("armor")
    );
}

#[test]
fn shapes_are_patched_skipped_or_left_alone() {
    let d = default_block();
    let e = value(&[]);
    let mut hit = MaterialState::builtin_default();
    hit.set_slot_raw(0, r"textures\x\rock.dds");
    let mut other = MaterialState::builtin_default();
    other.set_slot_raw(0, r"textures\x\stone.dds");
    let shapes = vec![
        (ShapeId(1), "Rock".to_string(), hit.clone()),
        (ShapeId(2), "Stone".to_string(), other.clone()),
    ];
    let rows = simulate_shapes(&e, &d, r"meshes\x\rock.nif", &shapes, &|_| true);
    assert!(
        matches!(rows[0].outcome, Outcome::Patched { .. }),
        "{:?}",
        rows[0]
    );
    assert_eq!(rows[0].material.slots[0].norm, "textures/pbr/x/rock.dds");
    assert!(rows[0].material.is_pbr());
    assert_eq!(rows[1].outcome, Outcome::NoMatch);
    assert_eq!(rows[1].material, other);
    // A patched slot that neither sdc nor the game provides: PGPatcher skips the shape; it keeps its source.
    let rows = simulate_shapes(&e, &d, r"meshes\x\rock.nif", &shapes, &|p| {
        !p.ends_with("_rmaos.dds")
    });
    let Outcome::Skip(why) = &rows[0].outcome else {
        panic!("{:?}", rows[0])
    };
    assert!(why.contains("rmaos"), "{why}");
    assert!(rows[0].text().contains("will be skipped"));
    assert_eq!(rows[0].material, hit);
}

#[test]
fn a_built_in_shape_stands_in_for_every_match_mode() {
    let d = default_block();
    let cases: [&[(&str, Value)]; 5] = [
        &[],
        &[("match1", json!("")), ("match2", json!(r"x\rock_n"))],
        &[("path_contains", json!("rock"))],
        &[("match1", json!(r"x\rock.dds"))],
        &[("nif_filter", json!(r"armor\iron"))],
    ];
    for pairs in cases {
        let e = value(pairs);
        let (src, nif) = builtin_source(&e, &d).unwrap_or_else(|| panic!("{pairs:?}"));
        let rows = simulate_shapes(&e, &d, &nif, &[(ShapeId(0), "b".into(), src)], &|_| true);
        assert!(
            matches!(rows[0].outcome, Outcome::Patched { .. }),
            "{pairs:?}: {:?}",
            rows[0]
        );
    }
}

#[test]
fn plain_textures_go_to_their_vanilla_slots() {
    let mut members = fp_of(None, "s").members;
    members.push(MemberSlot {
        kind: SlotKind::Normal,
        paths: vec![plain_path(SlotKind::Normal)],
        ..members[0].clone()
    });
    members[0].paths = vec![plain_path(SlotKind::Diffuse)];
    let m = plain_material(&members);
    assert_eq!(m.slots[0].norm, plain_path(SlotKind::Diffuse));
    assert_eq!(m.slots[1].norm, plain_path(SlotKind::Normal));
    assert!(!m.is_pbr());
}

fn row(copy: i64, load: i64, path: &str, block: u32, derived: bool) -> MatchRow {
    MatchRow {
        copy_id: copy,
        nif_path: path.into(),
        origin: "Mod".into(),
        shape_block: block,
        shape_name: Some(format!("S{block}")),
        le_mesh: false,
        skinned: false,
        derived,
        load_copy_id: load,
    }
}

#[test]
fn rows_group_by_nif_and_keep_only_shapes_pgpatcher_matches() {
    let rows = vec![
        row(1, 1, "meshes/a/rock.nif", 3, false),
        row(1, 1, "meshes/a/rock.nif", 4, false),
        row(2, 9, "meshes/armor/iron.nif", 0, true),
        row(5, 5, "meshes/b/gone.nif", 0, false),
    ];
    let check = |copy: i64, nif: &str, block: u32| {
        assert!(nif.starts_with(r"meshes\"), "PGPatcher form: {nif}");
        block != 4 && copy != 5
    };
    let c = group_rows(&rows, None, &check);
    assert_eq!(c.iter().map(|c| c.load_copy_id).collect::<Vec<_>>(), [1, 9]);
    assert_eq!(c[0].nif_path, r"meshes\a\rock.nif");
    assert_eq!(c[0].shapes, vec![(3, Some("S3".to_string()))]);
    assert!(c[1].derived && c[1].copy_id == 2);
    let f = group_rows(&rows, Some(r"ARMOR\"), &check);
    assert_eq!(f.iter().map(|c| c.load_copy_id).collect::<Vec<_>>(), [9]);
}

/// Polls like the app does (panel drawn, input every frame) until nothing is in flight.
fn run(s: &mut SetPreview, m: &AppModel) {
    let t = Instant::now();
    loop {
        s.shown = true;
        s.input = true;
        s.poll(m);
        if !s.busy() {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(90),
            "the preview did not settle"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn shown_ids(s: &SetPreview) -> Vec<u64> {
    s.enc
        .members
        .values()
        .filter_map(|x| x.state.shown().map(|e| e.id))
        .collect()
}

#[test]
fn without_a_scan_db_the_set_shows_patched_on_the_sphere_and_json_edits_encode_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    assert_eq!(s.candidates, Candidates::NoStore(NO_SCAN.into()));
    let Some(Content::Builtin {
        shape: Builtin::Sphere,
        material,
        textures,
        ..
    }) = s.v3.content().cloned()
    else {
        panic!("{:?}", s.v3.content())
    };
    assert!(material.is_pbr());
    assert!(textures.contains_key("textures/pbr/landscape/rock01.dds"));
    assert!(
        matches!(s.rows[0].outcome, Outcome::Patched { .. }),
        "{:?}",
        s.rows
    );
    let ids = shown_ids(&s);
    m.set_json_value("landscape/rock01", "roughness_scale", Some(json!(0.25)));
    run(&mut s, &m);
    let Some(Content::Builtin {
        material: after, ..
    }) = s.v3.content().cloned()
    else {
        panic!()
    };
    assert_ne!(material, after, "the material follows the edit");
    assert_eq!(shown_ids(&s), ids, "a JSON-only edit encodes nothing");
}

fn lighting_shape(block: u32, slots: &[&str]) -> nif::scan::ShapeRow {
    use nif::props::{Lighting, LightingParams, LightingTrailer};
    use nif::shape_head::{ShapeHead, ShapeKind};
    nif::scan::ShapeRow {
        block,
        head: ShapeHead {
            kind: ShapeKind::BSTriShape,
            name: Some("Rock".into()),
            skinned: false,
            shader: 1,
            alpha: -1,
            vertex_desc: None,
            num_triangles: Some(1),
            num_vertices: Some(3),
        },
        shader: nif::scan::Shader::Lighting(
            Lighting {
                name: None,
                shader_type: 0,
                slsf1: 0,
                slsf2: 0,
                texture_set: 2,
                params: LightingParams {
                    uv_offset: [0.0; 2],
                    uv_scale: [1.0; 2],
                    emissive_color: [0.0; 3],
                    emissive_multiple: 1.0,
                    texture_clamp_mode: 3,
                    alpha: 1.0,
                    refraction_strength: 0.0,
                    glossiness: 80.0,
                    specular_color: [1.0; 3],
                    specular_strength: 1.0,
                    lighting_effect_1: 0.3,
                    lighting_effect_2: 2.0,
                    trailer: LightingTrailer::None,
                },
            },
            slots
                .iter()
                .map(|p| nif::path::canon_texture(p.as_bytes()))
                .collect(),
        ),
        alpha: None,
    }
}

/// A scan DB with one winning NIF (not on disk) whose block 3 uses `slots`.
pub fn scan_db(dir: &Path, slots: &[&str]) -> PathBuf {
    scan_db_with(dir, slots, false)
}

pub fn scan_db_with(dir: &Path, slots: &[&str], le_mesh: bool) -> PathBuf {
    use modscan::store::{CopyResult, OriginSpec, Store, KIND_MOD};
    let p = dir.join("scan.db");
    let mut s = Store::open(&p).unwrap();
    s.begin_scan().unwrap();
    let o = s
        .upsert_origin(&OriginSpec {
            kind: KIND_MOD,
            name: "Rocks".into(),
            root: dir
                .join("mods")
                .join("Rocks")
                .to_string_lossy()
                .into_owned(),
            parent: None,
            priority: 0,
            derived_by: None,
            derived_state: None,
        })
        .unwrap();
    let scan = nif::scan::NifScan {
        bsver: 100,
        le_mesh,
        shapes: vec![lighting_shape(3, slots)],
        problems: vec![],
        index3d: Default::default(),
    };
    let path = "meshes/landscape/rocks/rock01.nif";
    let id = s
        .upsert_copy(path, o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap();
    let pid = s.path_ids(&[path]).unwrap()[0];
    s.set_winners(&[(pid, (id, 1))].into_iter().collect(), &Default::default())
        .unwrap();
    p
}

/// Part P's binding clarification: `exists` is asked for every non-empty slot, vanilla ones included. A texture the
/// game has only inside a BSA must count, or a shape that keeps a vanilla slot would be skipped.
#[test]
fn exists_accepts_sdc_outputs_and_any_vfs_winner_including_bsa_only_textures() {
    use modscan::store::{OriginSpec, Store, KIND_BSA};
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let db = scan_db(dir.path(), &[r"textures\landscape\rock01.dds"]);
    {
        let mut s = Store::open(&db).unwrap();
        let bsa = s
            .upsert_origin(&OriginSpec {
                kind: KIND_BSA,
                name: "Skyrim - Textures0.bsa".into(),
                root: dir
                    .path()
                    .join("Skyrim - Textures0.bsa")
                    .to_string_lossy()
                    .into_owned(),
                parent: None,
                priority: 0,
                derived_by: None,
                derived_state: None,
            })
            .unwrap();
        let pid = s.path_ids(&["textures/clutter/vanilla_m.dds"]).unwrap()[0];
        s.set_textures(&[(pid, (bsa, 1))].into_iter().collect())
            .unwrap();
    }
    let mut s = SetPreview::new(None, None, Some(db), DEFAULT_CAP);
    run(&mut s, &m);
    assert!(
        s.exists(r"textures\clutter\vanilla_m.dds"),
        "a BSA-only vanilla slot exists"
    );
    assert!(
        s.exists(r"Textures\Clutter\Vanilla_M.DDS"),
        "any case, PGPatcher form"
    );
    assert!(
        s.exists(r"textures\pbr\landscape\rock01_n.dds"),
        "sdc's own output"
    );
    assert!(!s.exists(r"textures\clutter\nothing.dds"));
}

#[test]
fn a_scanned_mesh_is_listed_and_an_unreadable_one_falls_back_to_the_sphere() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let db = scan_db(dir.path(), &[r"textures\landscape\rock01.dds"]);
    let mut s = SetPreview::new(None, None, Some(db), DEFAULT_CAP);
    run(&mut s, &m);
    let Candidates::Ready(list) = &s.candidates else {
        panic!("{:?}", s.candidates)
    };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].nif_path, r"meshes\landscape\rocks\rock01.nif");
    assert_eq!(list[0].shapes, vec![(3, Some("Rock".to_string()))]);
    assert!(s.timings.query_ms.is_some());
    // The NIF is not on disk: the sphere stands in and the reason is shown.
    assert!(matches!(
        s.v3.content(),
        Some(Content::Builtin {
            shape: Builtin::Sphere,
            ..
        })
    ));
    assert!(
        s.notices.iter().any(|n| n.contains("rock01.nif")),
        "{:?}",
        s.notices
    );
    // The choice is remembered per set, for the session.
    s.choose(MeshChoice::Builtin(Builtin::Cube));
    run(&mut s, &m);
    assert!(matches!(
        s.v3.content(),
        Some(Content::Builtin {
            shape: Builtin::Cube,
            ..
        })
    ));
    assert_eq!(
        s.choice.get(&key),
        Some(&MeshChoice::Builtin(Builtin::Cube))
    );
}

#[test]
fn every_built_in_is_named_and_the_default_preview_is_disabled_without_io() {
    assert_eq!(BUILTINS.len(), Builtin::ALL.len());
    for b in Builtin::ALL {
        assert!(BUILTINS.iter().any(|(x, _)| *x == b), "{b:?}");
        assert_ne!(builtin_name(b), "Built-in", "{b:?}");
    }
    assert_eq!(builtin_name(Builtin::MaterialBall), "Material ball");
    let s = SetPreview::default();
    assert!(!s.enabled && s.store_path.is_none() && !s.busy());
}

// ---- Task 14b: Encoded / Source mode

fn job_source(m: &AppModel) -> Source {
    let item = m
        .items
        .iter()
        .find(|i| i.job.is_some() && i.status == ItemStatus::Ready)
        .unwrap();
    source_of(item)
}

#[test]
fn default_mode_is_encoded() {
    assert_eq!(PreviewMode::default(), PreviewMode::Encoded);
    assert_eq!(Encoder::new(DEFAULT_CAP).mode(), PreviewMode::Encoded);
    assert_eq!(SetPreview::default().mode(), PreviewMode::Encoded);
}

#[test]
fn f16_bits_rounds_clamps_and_survives_non_finite() {
    assert_eq!(f16_bits(0.0), 0);
    assert_eq!(f16_bits(1.0), 0x3C00);
    assert_eq!(f16_bits(0.5), 0x3800);
    assert_eq!(f16_bits(-2.0), 0xC000);
    assert_eq!(f16_bits(65504.0), 0x7BFF);
    assert_eq!(f16_bits(1.0e9), 0x7BFF);
    assert_eq!(f16_bits(f32::INFINITY), 0x7BFF);
    assert_eq!(f16_bits(f32::NAN), 0);
}

#[test]
fn source_mode_builds_an_uncompressed_dds() {
    use engine::dds::{read_dds, DdsPixelFormat};
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let session = m.session.as_ref().unwrap();
    let src = job_source(&m);
    let cancel = engine::api::CancelToken::default();
    let enc = encode(session, &src, 32, PreviewMode::Encoded, &cancel).unwrap();
    let raw = encode(session, &src, 32, PreviewMode::Source, &cancel).unwrap();
    assert_ne!(enc, raw);
    let (a, b) = (read_dds(&enc).unwrap(), read_dds(&raw).unwrap());
    assert!(
        matches!(b.format, DdsPixelFormat::Rgba8 { .. }),
        "{:?}",
        b.format
    );
    assert_eq!((b.width, b.height), (a.width, a.height));
    assert!(b.width <= 32);
    assert!(!b.levels.is_empty());
    assert_eq!(b.levels.len(), a.levels.len());
    // The GPU path takes it: render3d's loader on WARP.
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    render3d::textures::load_dds(&gpu, &raw).unwrap();
    // HDR: a half-float DX10 file with the same parse.
    let px = |v: f32| engine::image::Rgba32F {
        width: 2,
        height: 1,
        px: vec![[v, 0.5, 0.0, 1.0]; 2],
    };
    let hdr = read_dds(&uncompressed_dds(&[px(2.0)], true, false).unwrap()).unwrap();
    assert_eq!(hdr.format, DdsPixelFormat::Rgba16f);
    assert_eq!(&hdr.data[..2], &0x4000u16.to_le_bytes());
    let srgb = read_dds(&uncompressed_dds(&[px(1.0)], false, true).unwrap()).unwrap();
    assert_eq!(srgb.format, DdsPixelFormat::Rgba8 { srgb: true });
    assert_eq!(srgb.data[..4], [255, 128, 0, 255]);
}

/// Source is the export's input uncompressed: its texels match the Encoded (BCn) texels up to compression error,
/// for colour (sRGB-encoded, not linear light) and normal maps (signed vectors stored as v * 0.5 + 0.5).
#[test]
fn source_mode_texels_match_the_encoded_texels() {
    use engine::dds::read_dds;
    use engine::decode::decode_level_rgba;
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let session = m.session.as_ref().unwrap();
    let cancel = engine::api::CancelToken::default();
    let mut checked = 0;
    for item in m
        .items
        .iter()
        .filter(|i| i.job.is_some() && i.status == ItemStatus::Ready)
    {
        let src = source_of(item);
        let texels = |mode| {
            let f = read_dds(&encode(session, &src, 32, mode, &cancel).unwrap()).unwrap();
            let (w, h, r) = f.levels[0].clone();
            decode_level_rgba(f.format, w, h, &f.data[r], None).unwrap()
        };
        let (enc, raw) = (texels(PreviewMode::Encoded), texels(PreviewMode::Source));
        assert_eq!(enc.len(), raw.len());
        let diff: f32 = enc
            .iter()
            .zip(&raw)
            .map(|(a, b)| (0..3).map(|k| (a[k] - b[k]).abs()).sum::<f32>() / 3.0)
            .sum::<f32>()
            / enc.len() as f32;
        assert!(diff < 0.03, "{}: mean RGB difference {diff}", item.key);
        checked += 1;
    }
    assert!(checked >= 2, "a colour and a normal map at least");
}

#[test]
fn switching_mode_marks_members_stale_and_reencodes() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    enc.sync(&m, member_slots(&m, &v, Some(&e)), Instant::now(), true);
    settle(&mut enc, &m);
    let old = match &enc.members[&SlotKind::Diffuse].state {
        EncState::Fresh(x) => x.clone(),
        s => panic!("{s:?}"),
    };
    enc.set_mode(PreviewMode::Source, Instant::now());
    assert_eq!(enc.mode(), PreviewMode::Source);
    match &enc.members[&SlotKind::Diffuse].state {
        EncState::Stale(Some(x)) => assert_eq!(x.id, old.id),
        s => panic!("{s:?}"),
    }
    settle(&mut enc, &m);
    let EncState::Fresh(new) = &enc.members[&SlotKind::Diffuse].state else {
        panic!()
    };
    assert_ne!(new.dds, old.dds);
    let f = engine::dds::read_dds(&new.dds).unwrap();
    assert!(matches!(
        f.format,
        engine::dds::DdsPixelFormat::Rgba8 { .. }
    ));
}

#[test]
fn a_completion_from_the_old_mode_is_rejected() {
    let (_d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots, t, true);
    enc.pump(m.session.as_ref(), t, false);
    let t1 = enc.token_of(SlotKind::Diffuse);
    assert_ne!(t1, 0);
    enc.set_mode(PreviewMode::Source, t);
    assert_eq!(enc.token_of(SlotKind::Diffuse), 0);
    enc.inject_as(
        SlotKind::Diffuse,
        t1,
        DEFAULT_CAP,
        PreviewMode::Encoded,
        fake(),
    );
    assert!(!enc.poll());
    enc.pump(m.session.as_ref(), t, false);
    let t2 = enc.token_of(SlotKind::Diffuse);
    assert_ne!(t2, t1);
    // Same token, but produced under the other mode: rejected and scheduled again under Source.
    enc.inject_as(
        SlotKind::Diffuse,
        t2,
        DEFAULT_CAP,
        PreviewMode::Encoded,
        fake(),
    );
    assert!(enc.poll());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Stale(None)
    ));
    enc.pump(m.session.as_ref(), Instant::now(), false);
    let t3 = enc.token_of(SlotKind::Diffuse);
    enc.inject(SlotKind::Diffuse, t3, DEFAULT_CAP, fake());
    assert!(enc.poll());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Fresh(_)
    ));
}

#[test]
fn dds_inputs_ignore_the_mode() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("x.dds");
    std::fs::write(&p, b"DDS raw").unwrap();
    let m = pbr_opened(d.path());
    let session = m.session.as_ref().unwrap();
    let src = Source::Dds(p);
    let cancel = engine::api::CancelToken::default();
    for mode in [PreviewMode::Encoded, PreviewMode::Source] {
        assert_eq!(
            encode(session, &src, 32, mode, &cancel).unwrap(),
            b"DDS raw"
        );
    }
    let blocked = Source::Blocked("no");
    assert!(encode(session, &blocked, 32, PreviewMode::Source, &cancel).is_err());
}

// ---- Task 14 fix round 1 ----

#[test]
fn a_no_op_cap_or_mode_change_keeps_members_fresh() {
    let (_d, m, mut enc, slots) = started();
    enc.hold_workers(false);
    enc.sync(&m, slots, Instant::now(), true);
    settle(&mut enc, &m);
    let t = enc.token_of(SlotKind::Diffuse);
    enc.set_cap(DEFAULT_CAP, Instant::now());
    enc.set_mode(PreviewMode::Encoded, Instant::now());
    assert!(matches!(
        enc.members[&SlotKind::Diffuse].state,
        EncState::Fresh(_)
    ));
    assert_eq!(enc.token_of(SlotKind::Diffuse), t);
}

fn bsa_db(dir: &Path) -> PathBuf {
    use modscan::store::{OriginSpec, Store, KIND_BSA};
    let db = scan_db(dir, &[r"textures\landscape\rock01.dds"]);
    let mut s = Store::open(&db).unwrap();
    let bsa = s
        .upsert_origin(&OriginSpec {
            kind: KIND_BSA,
            name: "Skyrim - Textures0.bsa".into(),
            root: dir
                .join("Skyrim - Textures0.bsa")
                .to_string_lossy()
                .into_owned(),
            parent: None,
            priority: 0,
            derived_by: None,
            derived_state: None,
        })
        .unwrap();
    let pid = s.path_ids(&["textures/clutter/vanilla_m.dds"]).unwrap()[0];
    s.set_textures(&[(pid, (bsa, 1))].into_iter().collect())
        .unwrap();
    db
}

#[test]
fn a_shape_keeping_a_bsa_only_slot_is_patched_not_skipped_with_the_panels_exists() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, Some(bsa_db(dir.path())), DEFAULT_CAP);
    run(&mut s, &m);
    let d = default_block();
    let e = entry_node(&rock(&m).1).to_value();
    let mut src = MaterialState::builtin_default();
    src.set_slot_raw(0, r"textures\landscape\rock01.dds");
    src.set_slot_raw(5, r"textures\clutter\vanilla_m.dds");
    let shapes = [(ShapeId(1), "Rock".to_string(), src)];
    let rows = simulate_shapes(
        &e,
        &d,
        r"meshes\landscape\rocks\rock01.nif",
        &shapes,
        &|p| s.exists(p),
    );
    assert!(
        matches!(rows[0].outcome, Outcome::Patched { .. }),
        "{:?}",
        rows[0]
    );
}

#[test]
fn a_store_error_never_reads_as_a_missing_texture() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let bad = dir.path().join("scan.db");
    std::fs::write(&bad, b"this is not a database, just text to break sqlite").unwrap();
    let mut s = SetPreview::new(None, None, Some(bad), DEFAULT_CAP);
    run(&mut s, &m);
    assert!(
        s.exists(r"textures\clutter\vanilla_m.dds"),
        "unknown must not mean missing"
    );
    run(&mut s, &m);
    assert!(
        s.notices.iter().any(|n| n.contains("scan database")),
        "{:?}",
        s.notices
    );
}

#[test]
fn an_unchanged_frame_within_the_throttle_does_no_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    let (checks, push) = (s.checks, s.timings.push_ms);
    let t = Instant::now();
    s.shown = true;
    s.poll_at(&m, t);
    s.shown = true;
    s.poll_at(&m, t + THROTTLE / 2);
    assert!(s.checks <= checks + 1, "at most the first poll was due");
    let after = s.checks;
    s.shown = true;
    s.poll_at(&m, t + THROTTLE / 2 + Duration::from_millis(1));
    assert_eq!(s.checks, after);
    s.shown = true;
    s.poll_at(&m, t + THROTTLE * 3);
    assert_eq!(s.checks, after + 1);
    assert!(!s.busy());
    assert_eq!(s.timings.push_ms, push, "no scene was rebuilt");
}

#[test]
fn stale_query_and_nif_results_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let db = scan_db(dir.path(), &[r"textures\landscape\rock01.dds"]);
    let mut s = SetPreview::new(None, None, Some(db), DEFAULT_CAP);
    s.shown = true;
    s.input = true;
    s.poll(&m);
    s.inject_stale(0, "stale boom");
    run(&mut s, &m);
    s.inject_stale(0, "stale boom");
    run(&mut s, &m);
    assert!(
        matches!(s.candidates, Candidates::Ready(_)),
        "{:?}",
        s.candidates
    );
    assert!(
        !s.notices.iter().any(|n| n.contains("stale boom")),
        "{:?}",
        s.notices
    );
}

#[test]
fn a_vanished_set_clears_the_scene_and_settles() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    assert!(s.v3.content().is_some());
    m.select_set("no/such/set");
    run(&mut s, &m);
    assert!(s.fp.is_none() && s.v3.content().is_none() && s.rows.is_empty());
    let checks = s.checks;
    let t = Instant::now();
    s.shown = true;
    s.poll_at(&m, t);
    s.shown = true;
    s.poll_at(&m, t + THROTTLE / 2);
    assert!(s.checks <= checks + 1, "no per-frame re-evaluation");
}

#[test]
fn only_le_meshes_say_why_the_sphere_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let db = scan_db_with(dir.path(), &[r"textures\landscape\rock01.dds"], true);
    let mut s = SetPreview::new(None, None, Some(db), DEFAULT_CAP);
    run(&mut s, &m);
    assert!(matches!(
        s.v3.content(),
        Some(Content::Builtin {
            shape: Builtin::Sphere,
            ..
        })
    ));
    assert!(s.notices.iter().any(|n| n == LE_ONLY), "{:?}", s.notices);
}

// ---- final review fixes ----

#[test]
fn a_hidden_panel_with_a_debounced_encode_goes_idle_and_encodes_when_shown_again() {
    let dir = tempfile::tempdir().unwrap();
    let m = pbr_opened(dir.path());
    let (v, e) = rock(&m);
    let key = v.key.clone();
    let mut m = m;
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    let before = shown_ids(&s);
    // An edit marks a member Stale, due DEBOUNCE ahead; the user switches view before then.
    let mut slots = member_slots(&m, &v, Some(&e));
    edit(&mut slots);
    let now = Instant::now();
    s.enc.sync(&m, slots, now, false);
    assert!(s.enc.busy());
    s.poll_at(&m, now);
    assert!(!s.busy(), "hidden: a waiting encode is not work");
    s.poll_at(&m, now + DEBOUNCE * 2);
    assert!(!s.busy(), "and stays idle after the due time");
    assert!(matches!(
        s.enc.members[&SlotKind::Diffuse].state,
        EncState::Stale(_)
    ));
    run(&mut s, &m);
    assert!(
        matches!(s.enc.members[&SlotKind::Diffuse].state, EncState::Fresh(_)),
        "shown again: it encodes"
    );
    assert_ne!(shown_ids(&s), before);
}

fn builtin_key(s: &SetPreview) -> u64 {
    match s.v3.content() {
        Some(Content::Builtin { key, .. }) => *key,
        c => panic!("{c:?}"),
    }
}

fn poll_once(s: &mut SetPreview, m: &AppModel) {
    s.shown = true;
    s.input = true;
    s.poll(m);
}

#[test]
fn a_material_edit_naming_a_new_slot_path_pushes_the_scene_instead_of_set_material() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let fp = render3d::locate::Fingerprint {
        root: dir.path().to_path_buf(),
        owner_mod: Some("CS".into()),
        hash: "a".into(),
    };
    let mut s = SetPreview::new(Some(fp.clone()), None, None, DEFAULT_CAP);
    run(&mut s, &m);
    // `poll` started a real locate (nothing to find here): let it end, then confirm the shaders.
    while s.v3.locating() {
        poll_once(&mut s, &m);
        std::thread::sleep(Duration::from_millis(5));
    }
    s.v3.on_located(Ok(Some(fp)));
    let cmds = s.v3.attach_test_thread();
    s.v3.request_frame(64, 64);
    let id = cmds
        .try_iter()
        .find_map(|c| match c {
            sdc_gui::model::viewport3d::V3Cmd::LoadBuiltin { id, .. } => Some(id),
            _ => None,
        })
        .expect("a LoadBuiltin");
    s.v3.handle(sdc_gui::model::viewport3d::V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
        bounds: Some(([0.0; 3], 1.0)),
    });
    let sets = |cmds: &std::sync::mpsc::Receiver<_>| {
        cmds.try_iter()
            .filter(|c| matches!(c, sdc_gui::model::viewport3d::V3Cmd::SetMaterial { .. }))
            .count()
    };
    // Control: a plain parameter edit is a SetMaterial on the loaded content.
    let k0 = builtin_key(&s);
    m.set_json_value("landscape/rock01", "roughness_scale", Some(json!(0.25)));
    poll_once(&mut s, &m);
    assert_eq!(sets(&cmds), 1);
    assert_eq!(builtin_key(&s), k0, "no scene push");
    // A new slot path (the worker would not resolve it from the frozen texture map): a full push.
    m.set_json_value(
        "landscape/rock01",
        "cubemap",
        Some(json!("textures/cubemaps/new_e.dds")),
    );
    poll_once(&mut s, &m);
    poll_once(&mut s, &m);
    assert_eq!(sets(&cmds), 0, "no SetMaterial for a path the worker lacks");
    assert_ne!(builtin_key(&s), k0, "the scene was pushed");
}

// ---- P1-B Task 8b: A/B variants in the encoder ----

fn started_ab() -> (tempfile::TempDir, AppModel, Encoder) {
    let (d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots, t, true);
    enc.pump(m.session.as_ref(), t, false);
    for k in enc.members.keys().copied().collect::<Vec<_>>() {
        let tok = enc.token_of(k);
        assert_ne!(tok, 0);
        enc.inject(k, tok, DEFAULT_CAP, fake());
    }
    assert!(enc.poll());
    assert!(enc.settled());
    (d, m, enc)
}

fn kinds_of(e: &Encoder) -> Vec<SlotKind> {
    e.members.keys().copied().collect()
}

#[test]
fn the_first_flip_keeps_the_old_bytes_for_the_way_back_and_the_second_flip_is_free() {
    let (_d, m, mut enc) = started_ab();
    assert!(!enc.ab_active());
    let old: Vec<u64> = enc
        .members
        .values()
        .map(|x| x.state.shown().unwrap().id)
        .collect();
    let t = Instant::now();
    enc.set_mode(PreviewMode::Source, t);
    assert!(enc.ab_active());
    for k in kinds_of(&enc) {
        assert!(enc.has_alt(k), "the old Fresh bytes are the other copy");
        assert!(
            matches!(enc.members[&k].state, EncState::Stale(Some(_))),
            "as before the change"
        );
    }
    // The re-encode of the new mode as today.
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        let tok = enc.token_of(k);
        assert_ne!(tok, 0);
        enc.inject(k, tok, DEFAULT_CAP, fake());
    }
    assert!(enc.poll());
    for k in kinds_of(&enc) {
        assert!(matches!(enc.members[&k].state, EncState::Fresh(_)));
        assert_eq!(
            enc.alt_token_of(k),
            0,
            "nothing to encode: the other copy exists"
        );
    }
    // Back: no encode, the old bytes are on show again, and the Source bytes are the other copy.
    let shown_source: Vec<u64> = enc
        .members
        .values()
        .map(|x| x.state.shown().unwrap().id)
        .collect();
    enc.set_mode(PreviewMode::Encoded, t);
    enc.pump(m.session.as_ref(), t, false);
    let back: Vec<u64> = enc
        .members
        .values()
        .map(|x| x.state.shown().unwrap().id)
        .collect();
    assert_eq!(back, old);
    for k in kinds_of(&enc) {
        assert_eq!(
            enc.token_of(k),
            0,
            "a flip with both variants starts no encode"
        );
        assert_eq!(enc.alt_token_of(k), 0);
        assert!(enc.has_alt(k));
        assert!(matches!(enc.members[&k].state, EncState::Fresh(_)));
    }
    enc.set_mode(PreviewMode::Source, t);
    let again: Vec<u64> = enc
        .members
        .values()
        .map(|x| x.state.shown().unwrap().id)
        .collect();
    assert_eq!(again, shown_source);
}

#[test]
fn a_cap_change_drops_both_variants_and_the_other_one_encodes_after_the_shown_one_is_fresh() {
    let (_d, m, mut enc) = started_ab();
    let t = Instant::now();
    enc.set_mode(PreviewMode::Source, t); // A/B in use; Encoded kept as the other copy
    enc.set_cap(1024, t);
    for k in kinds_of(&enc) {
        assert!(!enc.has_alt(k), "the other copy was made at the old cap");
        assert_eq!(enc.alt_token_of(k), 0);
    }
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        let tok = enc.token_of(k);
        assert_ne!(tok, 0);
        assert_eq!(enc.alt_token_of(k), 0, "the shown mode first");
        enc.inject(k, tok, 1024, fake());
    }
    assert!(enc.poll());
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        assert_ne!(enc.alt_token_of(k), 0, "now the inactive mode encodes");
    }
    // Its result becomes the other copy; the shown bytes do not move and nothing is reported as changed.
    let shown: Vec<u64> = enc
        .members
        .values()
        .map(|x| x.state.shown().unwrap().id)
        .collect();
    for k in kinds_of(&enc) {
        let at = enc.alt_token_of(k);
        enc.inject_as(k, at, 1024, PreviewMode::Encoded, fake());
    }
    assert!(
        !enc.poll(),
        "an inactive copy is not a change of what is on show"
    );
    for k in kinds_of(&enc) {
        assert!(enc.has_alt(k));
        assert_eq!(enc.alt_token_of(k), 0);
    }
    assert_eq!(
        enc.members
            .values()
            .map(|x| x.state.shown().unwrap().id)
            .collect::<Vec<_>>(),
        shown
    );
    // A late result of the superseded other-mode encode never lands.
    let k = kinds_of(&enc)[0];
    enc.inject_as(k, 1, 1024, PreviewMode::Encoded, fake());
    assert!(!enc.poll());
}

#[test]
fn a_source_change_clears_the_other_variant_and_nothing_encodes_before_the_first_flip() {
    let (_d, m, mut enc, mut slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots.clone(), t, true);
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        let tok = enc.token_of(k);
        enc.inject(k, tok, DEFAULT_CAP, fake());
    }
    assert!(enc.poll());
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        assert_eq!(enc.alt_token_of(k), 0, "A/B is not in use: no doubled CPU");
    }
    enc.set_mode(PreviewMode::Source, t);
    assert!(kinds_of(&enc).iter().all(|k| enc.has_alt(*k)));
    edit(&mut slots);
    enc.sync(&m, slots, t, true);
    let diffuse = SlotKind::Diffuse;
    assert!(
        !enc.has_alt(diffuse),
        "the edited member's other copy is gone"
    );
    assert_eq!(enc.alt_token_of(diffuse), 0);
}

#[test]
fn a_flip_before_the_other_variant_exists_re_encodes_as_before() {
    let (_d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots, t, true);
    enc.pump(m.session.as_ref(), t, false);
    let t1 = enc.token_of(SlotKind::Diffuse); // still encoding the first mode
    enc.set_mode(PreviewMode::Source, t);
    assert!(!enc.has_alt(SlotKind::Diffuse), "nothing Fresh to keep");
    assert_eq!(
        enc.token_of(SlotKind::Diffuse),
        0,
        "the old encode is cancelled"
    );
    enc.pump(m.session.as_ref(), t, false);
    let t2 = enc.token_of(SlotKind::Diffuse);
    assert_ne!(t2, 0);
    assert_ne!(t2, t1);
}

// ---- P1-B Task 8c: swaps instead of scene pushes ----

use render3d::Variant;
use sdc_gui::model::viewport3d::{V3Cmd, V3Reply};

struct Live {
    _d: tempfile::TempDir,
    m: AppModel,
    s: SetPreview,
    cmds: std::sync::mpsc::Receiver<V3Cmd>,
}

fn kinds(s: &SetPreview) -> Vec<SlotKind> {
    s.enc.members.keys().copied().collect()
}

/// The workers are held: the test finishes every member's pending encode with a fake DDS.
fn finish(s: &mut SetPreview, cap: u32) {
    for k in kinds(s) {
        let t = s.enc.token_of(k);
        assert_ne!(t, 0, "{k:?} has an encode to finish");
        s.enc.inject(k, t, cap, fake());
    }
}

/// A set preview on the built-in sphere, scene pushed, load sent and answered; encodes held.
fn live() -> Live {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let fp = render3d::locate::Fingerprint {
        root: dir.path().to_path_buf(),
        owner_mod: Some("CS".into()),
        hash: "a".into(),
    };
    let mut s = SetPreview::new(Some(fp.clone()), None, None, DEFAULT_CAP);
    s.enc.hold_workers(true);
    poll_once(&mut s, &m); // starts the locate; the pump hands every member a held encode
    while s.v3.locating() {
        poll_once(&mut s, &m);
        std::thread::sleep(Duration::from_millis(5));
    }
    s.v3.on_located(Ok(Some(fp)));
    let cmds = s.v3.attach_test_thread();
    finish(&mut s, DEFAULT_CAP);
    poll_once(&mut s, &m); // Fresh → the scene is pushed
    s.v3.request_frame(64, 64);
    let id = cmds
        .try_iter()
        .find_map(|c| match c {
            V3Cmd::LoadBuiltin { id, .. } => Some(id),
            _ => None,
        })
        .expect("a LoadBuiltin");
    s.v3.handle(V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
        bounds: Some(([0.0; 3], 1.0)),
    });
    Live {
        _d: dir,
        m,
        s,
        cmds,
    }
}

fn drain(t: &Live) -> Vec<V3Cmd> {
    t.cmds.try_iter().collect()
}

fn n_paths(s: &SetPreview) -> usize {
    s.enc
        .members
        .values()
        .flat_map(|m| m.slot.paths.iter())
        .collect::<std::collections::HashSet<_>>()
        .len()
}

fn count(c: &[V3Cmd], f: impl Fn(&V3Cmd) -> bool) -> usize {
    c.iter().filter(|x| f(x)).count()
}

fn swaps(c: &[V3Cmd], v: Variant) -> usize {
    count(
        c,
        |x| matches!(x, V3Cmd::SwapTexture { variant, bytes, .. } if *variant == v && !bytes.is_empty()),
    )
}

fn drops(c: &[V3Cmd], v: Variant) -> usize {
    count(
        c,
        |x| matches!(x, V3Cmd::SwapTexture { variant, bytes, .. } if *variant == v && bytes.is_empty()),
    )
}

fn flips(c: &[V3Cmd], v: Variant) -> usize {
    count(
        c,
        |x| matches!(x, V3Cmd::SetTextureVariant { variant, .. } if *variant == v),
    )
}

fn loads(c: &[V3Cmd]) -> usize {
    count(c, |x| {
        matches!(
            x,
            V3Cmd::Load { .. } | V3Cmd::LoadParsed { .. } | V3Cmd::LoadBuiltin { .. }
        )
    })
}

#[test]
fn an_encode_completion_on_loaded_content_swaps_the_texture_and_pushes_no_scene() {
    let mut t = live();
    drain(&t);
    let key0 = builtin_key(&t.s);
    // A new size cap re-encodes every member; the old bytes stay on show meanwhile.
    t.s.enc.set_cap(1024, Instant::now());
    poll_once(&mut t.s, &t.m);
    assert!(
        drain(&t).is_empty(),
        "nothing to send while the encodes run"
    );
    finish(&mut t.s, 1024);
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(swaps(&c, Variant::Encoded), n_paths(&t.s));
    assert_eq!(loads(&c), 0, "no scene push");
    assert_eq!(flips(&c, Variant::Encoded), 0, "already the shown variant");
    assert_eq!(builtin_key(&t.s), key0);
    // Nothing changed since: nothing is sent again.
    poll_once(&mut t.s, &t.m);
    assert!(drain(&t).is_empty());
}

#[test]
fn a_failed_encode_still_pushes_the_scene_so_the_games_texture_comes_back() {
    let mut t = live();
    drain(&t);
    let key0 = builtin_key(&t.s);
    t.s.enc.set_cap(1024, Instant::now());
    poll_once(&mut t.s, &t.m);
    for k in kinds(&t.s) {
        let tok = t.s.enc.token_of(k);
        t.s.enc.inject(k, tok, 1024, Err("boom".into()));
    }
    poll_once(&mut t.s, &t.m);
    poll_once(&mut t.s, &t.m);
    assert_ne!(builtin_key(&t.s), key0, "the scene was pushed");
}

#[test]
fn flipping_the_mode_with_both_variants_sends_only_a_variant_flip_and_starts_no_encode() {
    let mut t = live();
    drain(&t);
    let n = n_paths(&t.s);
    // First flip: today's re-encode of the new mode; the old bytes are kept as the other copy.
    t.s.set_mode(PreviewMode::Source);
    assert!(t.s.enc.ab_active());
    assert!(drain(&t).is_empty(), "nothing Fresh to send yet");
    poll_once(&mut t.s, &t.m);
    finish(&mut t.s, DEFAULT_CAP);
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(swaps(&c, Variant::Source), n);
    assert_eq!(flips(&c, Variant::Source), n, "then show it");
    assert_eq!(
        swaps(&c, Variant::Encoded),
        0,
        "the renderer already holds the Encoded copy"
    );
    assert_eq!(loads(&c), 0);
    // Second flip: both variants exist.
    t.s.set_mode(PreviewMode::Encoded);
    let c = drain(&t);
    assert_eq!(c.len(), n, "only the flips");
    assert_eq!(flips(&c, Variant::Encoded), n);
    poll_once(&mut t.s, &t.m);
    assert!(drain(&t).is_empty());
    for k in kinds(&t.s) {
        assert_eq!(t.s.enc.token_of(k), 0, "no encode started");
        assert_eq!(t.s.enc.alt_token_of(k), 0);
    }
}

#[test]
fn a_source_change_voids_both_variants_and_the_inactive_one_follows_the_active() {
    let mut t = live();
    t.s.set_mode(PreviewMode::Source);
    poll_once(&mut t.s, &t.m);
    finish(&mut t.s, DEFAULT_CAP);
    poll_once(&mut t.s, &t.m);
    drain(&t);
    let n = n_paths(&t.s);
    t.s.enc.set_cap(1024, Instant::now()); // a settings change as the encoder sees it
    for k in kinds(&t.s) {
        assert!(!t.s.enc.has_alt(k));
    }
    poll_once(&mut t.s, &t.m); // the renderer's stale Encoded copy is dropped
    let c = drain(&t);
    assert_eq!(drops(&c, Variant::Encoded), n);
    finish(&mut t.s, 1024); // the shown (Source) mode
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(swaps(&c, Variant::Source), n);
    for k in kinds(&t.s) {
        assert_ne!(t.s.enc.alt_token_of(k), 0, "the inactive mode encodes now");
    }
    for k in kinds(&t.s) {
        let at = t.s.enc.alt_token_of(k);
        t.s.enc.inject_as(k, at, 1024, PreviewMode::Encoded, fake());
    }
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(
        swaps(&c, Variant::Encoded),
        n,
        "stored beside the shown one"
    );
    assert_eq!(flips(&c, Variant::Encoded), 0, "not shown");
    assert_eq!(loads(&c), 0);
}

#[test]
fn a_flip_before_the_other_variant_exists_falls_back_to_the_re_encode() {
    let mut t = live();
    drain(&t);
    t.s.enc.set_cap(1024, Instant::now()); // everything stale: no Fresh bytes to keep
    t.s.set_mode(PreviewMode::Source);
    poll_once(&mut t.s, &t.m);
    for k in kinds(&t.s) {
        assert_ne!(t.s.enc.token_of(k), 0, "encoded as before the A/B");
        assert!(!t.s.enc.has_alt(k));
    }
    assert!(drain(&t)
        .iter()
        .all(|c| !matches!(c, V3Cmd::SetTextureVariant { .. })));
}

#[test]
fn members_sharing_a_path_with_one_other_copy_do_not_swap_and_drop_every_frame() {
    let mut t = live();
    let ks = kinds(&t.s);
    let shared = t.s.enc.members[&ks[0]].slot.paths.clone();
    t.s.enc.members.get_mut(&ks[1]).unwrap().slot.paths = shared;
    t.s.enc.set_cap(1024, Instant::now()); // no other copies anywhere
    t.s.set_mode(PreviewMode::Source);
    poll_once(&mut t.s, &t.m);
    finish(&mut t.s, 1024);
    poll_once(&mut t.s, &t.m);
    poll_once(&mut t.s, &t.m); // the other-mode encodes are started
    let at = t.s.enc.alt_token_of(ks[0]);
    assert_ne!(at, 0);
    t.s.enc
        .inject_as(ks[0], at, 1024, PreviewMode::Encoded, fake());
    for _ in 0..3 {
        poll_once(&mut t.s, &t.m);
    }
    drain(&t);
    for _ in 0..5 {
        poll_once(&mut t.s, &t.m);
    }
    let c = drain(&t);
    assert!(
        c.is_empty(),
        "nothing changed: no swap or drop is sent again ({} cmds)",
        c.len()
    );
}
// ---- P1-B Task 9: encode lines ----

#[test]
fn an_encode_completion_is_logged_with_its_slot_mode_time_and_size() {
    let (_d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots, t, true);
    enc.pump(m.session.as_ref(), t, false);
    let k = *enc.members.keys().next().unwrap();
    let tok = enc.token_of(k);
    let ((), ev) = perf_log::capture(|| {
        enc.inject(k, tok, DEFAULT_CAP, fake());
        assert!(enc.poll());
    });
    let e: Vec<_> = ev.iter().filter(|e| e["event"] == "encode").collect();
    assert_eq!(e.len(), 1, "{ev:?}");
    assert_eq!(
        (
            e[0]["ms"].as_f64(),
            e[0]["bytes"].as_u64(),
            e[0]["ok"].as_bool()
        ),
        (Some(1.0), Some(8), Some(true))
    );
    assert_eq!(e[0]["mode"], "Encoded");
    assert!(e[0]["slot"].is_string());
}

#[test]
fn the_set_preview_tags_its_viewport_for_the_gap_watch() {
    assert_eq!(
        SetPreview::new(None, None, None, DEFAULT_CAP).v3.panel,
        "set_preview"
    );
}

/// Note AO9 (owner, 2026-10-15): the Mesh list puts loose meshes before BSA ones, and ground models (`gnd` in the
/// name, a stem ending `_go`) and `1stperson` meshes last; within a tier the query's order stays. Auto takes the first.
#[test]
fn mesh_candidates_prefer_loose_then_leave_ground_and_first_person_last() {
    let c = |id: i64, nif: &str| Candidate {
        load_copy_id: id,
        copy_id: id,
        nif_path: nif.into(),
        origin: "m".into(),
        le_mesh: false,
        derived: false,
        shapes: vec![(0, None)],
    };
    let mut list = vec![
        c(1, r"meshes\armor\bootsgnd.nif"),
        c(2, r"meshes\armor\1stpersonboots.nif"),
        c(3, r"meshes\armor\boots_1.nif"),
        c(4, r"meshes\armor\ct77vampirethighboots_go.nif"),
        c(5, r"meshes\armor\dragonboots.nif"),
        c(6, r"meshes\armor\boots_0.nif"),
        c(7, r"meshes\armor\gone_1.nif"),
    ];
    // 3 and 7 come from a BSA.
    order_candidates(&mut list, |c| {
        c.load_copy_id == 3 || c.load_copy_id == 7 || c.load_copy_id == 2
    });
    let ids: Vec<i64> = list.iter().map(|c| c.load_copy_id).collect();
    // loose normal (5 "dragon" is no ground model, 6), bsa normal (3, 7 "gone" is not `_go`), loose ground/1st (1, 4),
    // bsa ground/1st (2).
    assert_eq!(ids, vec![5, 6, 3, 7, 1, 4, 2]);
    // Auto draws the first usable one: a loose, non-ground mesh.
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    s.candidates = Candidates::Ready(list);
    assert_eq!(s.choice(), MeshChoice::Auto);
    assert!(matches!(s.pick(), Pick::Nif(c) if c.load_copy_id == 5));
    assert!(is_ground_or_first_person(r"meshes\a\Boots_GO.NIF"));
    assert!(!is_ground_or_first_person(r"meshes\a\dragon.nif"));
    assert!(!is_ground_or_first_person(r"meshes\a\gone.nif"));
}

/// Review fix: `gnd` and `_go` are whole tokens (or the stem's end), not any substring.
#[test]
fn ground_models_are_whole_tokens() {
    for (p, ground) in [
        (r"meshes\a\bootsgnd.nif", true),
        (r"meshes\a\boots_gnd.nif", true),
        (r"meshes\a\gnd_boots.nif", true),
        (r"meshes\a\boots-gnd-1.nif", true),
        (r"meshes\a\boots gnd.nif", true),
        (r"meshes\a\boots2gnd3.nif", true),
        (r"meshes\a\boots_go.nif", true),
        (r"meshes\a\1stpersonboots.nif", true),
        (r"meshes\a\signdoor.nif", false),
        (r"meshes\a\designdetail.nif", false),
        (r"meshes\a\dragon.nif", false),
        (r"meshes\a\gone_1.nif", false),
        (r"meshes\a\boots_gone.nif", false),
    ] {
        assert_eq!(is_ground_or_first_person(p), ground, "{p}");
    }
}

/// Design §7 (ruling R1): a texture-set block locks the plane; tiling multiplies the pgpsim `uv_scale` (2 at 2x = 4).
#[test]
fn a_texture_set_block_locks_the_plane_and_tiling_multiplies_after_pgpsim() {
    use sdc_gui::model::cs_pbr::Kind;
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    m.set_json_value("landscape/rock01", "uv_scale", Some(json!(2.0)));
    m.add_cs_block("landscape/rock01", Kind::Txst);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    let uv = |s: &SetPreview| match s.v3.content() {
        Some(Content::Builtin {
            shape: Builtin::Plane,
            material,
            ..
        }) => material.params.uv_scale,
        c => panic!("{c:?}"),
    };
    assert_eq!(uv(&s), [2.0, 2.0]);
    s.choose(MeshChoice::Builtin(Builtin::Cube));
    run(&mut s, &m);
    assert_eq!(uv(&s), [2.0, 2.0], "the plane stays");
    s.set_tiling(2.2);
    assert_eq!(s.tiling, 2.0, "steps of 0.5");
    assert_eq!(s.rows[0].material.params.uv_scale, [4.0, 4.0]);
    run(&mut s, &m);
    assert_eq!(uv(&s), [4.0, 4.0]);
}
