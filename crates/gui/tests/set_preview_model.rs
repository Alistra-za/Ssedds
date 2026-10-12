//! The set preview's model (P1-A Tasks 13–14): encodes, fingerprint and change classification, query keys,
//! PGPatcher simulation per shape, and the orchestration without a GPU.
mod support;
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
