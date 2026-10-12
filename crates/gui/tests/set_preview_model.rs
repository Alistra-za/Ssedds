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
