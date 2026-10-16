//! M4 Task 8: which sets bake (spec §6.2–6.3), the four SET-* codes, opting out and shared roughness slots
//! (DECISIONS R6–R10).
use engine::api::*;
use engine::codes::{Code, Severity};
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, ResolvedJob};
use engine::sets::SetEdit;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

/// An `_n` whose columns alternate between two tilts (unit length, short footprints).
fn normal(dir: &Path, rel: &str, w: u32, h: u32) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, h, |x, _| {
        Rgb([if x % 2 == 0 { 204u8 } else { 51 }, 128, 230])
    })
    .save(p)
    .unwrap();
}

fn rough(dir: &Path, rel: &str, w: u32, h: u32) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, h, |x, _| Rgba([(x * 4) as u8, 0, 200, 255]))
        .save(p)
        .unwrap();
}

fn gray(dir: &Path, rel: &str, v: u8) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(p)
        .unwrap();
}

fn fake_dds(dir: &Path, rel: &str) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"DDS not a real texture").unwrap();
}

fn req(dir: &Path, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    r
}

fn plan(r: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn job<'a>(p: &'a Plan, tail: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.to_string_lossy().replace('\\', "/").ends_with(tail))
        .unwrap_or_else(|| panic!("no job for {tail}"))
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn set_codes(j: &ResolvedJob) -> Vec<Code> {
    codes(j)
        .into_iter()
        .filter(|c| c.as_str().starts_with("SET-"))
        .collect()
}

/// `set_resolved.bake` of the set whose key ends with `tail`.
fn bake_flag(ev: &[EngineEvent], tail: &str) -> bool {
    ev.iter()
        .find_map(|e| match &e.kind {
            EventKind::SetResolved { bake, .. }
                if e.set.as_deref().is_some_and(|s| s.ends_with(tail)) =>
            {
                Some(*bake)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no set_resolved for {tail}"))
}

#[test]
fn an_n_and_its_rmaos_bake_and_the_n_records_the_set_decision() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, ev) = plan(req(d.path(), &[]));
    let b = job(&p, "iron_rmaos.png")
        .bake
        .as_ref()
        .expect("the rmaos bakes");
    assert!(b.normal.ends_with("iron_n.png"));
    assert_eq!(b.shift, 0);
    assert!(b.stamp.is_some());
    let n = job(&p, "iron_n.png");
    assert_eq!(n.profile.mips.normal, NormalMips::RenormalizeBake);
    assert_eq!(n.provenance["mips.normal"].to_string(), "set");
    assert!(bake_flag(&ev, "textures/pbr/iron"));
    assert!(set_codes(n).is_empty(), "{:?}", n.warnings);
    assert!(set_codes(job(&p, "iron_rmaos.png")).is_empty());
}

#[test]
fn rmaos_and_rfaos_in_one_set_both_bake() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/skin_n.png", 64, 64);
    rough(d.path(), "textures/pbr/skin_rmaos.png", 64, 64);
    rough(d.path(), "textures/pbr/skin_rfaos.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    assert!(job(&p, "skin_rmaos.png").bake.is_some());
    assert!(job(&p, "skin_rfaos.png").bake.is_some());
}

#[test]
fn a_packed_rmaos_bakes_like_a_prepacked_one() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    gray(d.path(), "textures/pbr/iron_roughness.png", 120);
    gray(d.path(), "textures/pbr/iron_ao.png", 200);
    let (p, ev) = plan(req(d.path(), &[]));
    let packed = p
        .jobs
        .iter()
        .find(|j| j.pack.is_some())
        .expect("a packed rmaos");
    assert!(packed.bake.is_some(), "{:?}", packed.warnings);
    assert!(bake_flag(&ev, "textures/pbr/iron"));
}

#[test]
fn size_ratios_give_the_level_shift() {
    for ((nw, nh), (mw, mh), shift) in [
        ((128u32, 128u32), (64u32, 64u32), 1i32),
        ((32, 32), (64, 64), -1),
        ((128, 64), (64, 32), 1),
    ] {
        let d = tempfile::tempdir().unwrap();
        normal(d.path(), "textures/pbr/iron_n.png", nw, nh);
        rough(d.path(), "textures/pbr/iron_rmaos.png", mw, mh);
        let (p, _) = plan(req(d.path(), &[]));
        let m = job(&p, "iron_rmaos.png");
        assert_eq!(
            m.bake.as_ref().map(|b| b.shift),
            Some(shift),
            "{nw}x{nh} / {mw}x{mh}: {:?}",
            m.warnings
        );
    }
}

/// Review Focus 1.
#[test]
fn an_aspect_mismatch_is_set_bake_ratio_and_does_not_bake() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 32);
    let (p, ev) = plan(req(d.path(), &[]));
    let m = job(&p, "iron_rmaos.png");
    assert!(m.bake.is_none());
    let w = m
        .warnings
        .iter()
        .find(|w| w.code == Code::SetBakeRatio)
        .expect("SET-BAKE-RATIO");
    assert_eq!(w.severity, Severity::Warning);
    assert_eq!(w.data["member"], serde_json::json!([64, 32]));
    assert!(!bake_flag(&ev, "textures/pbr/iron"));
    assert_eq!(
        job(&p, "iron_n.png").profile.mips.normal,
        NormalMips::Renormalize
    );
}

#[test]
fn a_lone_n_or_a_lone_rmaos_in_a_pbr_set_is_set_missing_companion_info() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/steel_rmaos.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    for (tail, needle) in [
        ("iron_n.png", "renormalised only"),
        ("steel_rmaos.png", "without the variance bake"),
    ] {
        let j = job(&p, tail);
        assert_eq!(set_codes(j), vec![Code::SetMissingCompanion], "{tail}");
        let w = &j.warnings[j.warnings.len() - 1];
        assert_eq!(w.severity, Severity::Info);
        assert!(w.message.contains(needle), "{tail}: {}", w.message);
    }
}

#[test]
fn flow_without_n_and_msn_without_s_are_set_missing_companion() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/hair/h_flow.png", 64, 64);
    normal(d.path(), "textures/a/w_msn.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    assert_eq!(
        set_codes(job(&p, "h_flow.png")),
        vec![Code::SetMissingCompanion]
    );
    assert_eq!(
        set_codes(job(&p, "w_msn.png")),
        vec![Code::SetMissingCompanion]
    );
}

/// Review Focus 2.
#[test]
fn a_dds_companion_is_set_companion_dds_in_both_directions() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    fake_dds(d.path(), "textures/pbr/iron_rmaos.dds");
    fake_dds(d.path(), "textures/pbr/steel_n.dds");
    rough(d.path(), "textures/pbr/steel_rmaos.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    for tail in ["iron_n.png", "steel_rmaos.png"] {
        let j = job(&p, tail);
        assert_eq!(set_codes(j), vec![Code::SetCompanionDds], "{tail}");
        let w = j
            .warnings
            .iter()
            .find(|w| w.code == Code::SetCompanionDds)
            .unwrap();
        assert_eq!(w.severity, Severity::Warning);
        assert!(
            w.data["companion"].as_str().unwrap().ends_with(".dds"),
            "{tail}: {}",
            w.data
        );
    }
    assert!(job(&p, "steel_rmaos.png").bake.is_none());
}

/// Review Focus 2: converting one file of a set leaves the other on disk.
#[test]
fn a_companion_on_disk_but_not_in_the_batch_is_set_companion_outside() {
    for (given, other) in [
        ("iron_n.png", "iron_rmaos.png"),
        ("iron_rmaos.png", "iron_n.png"),
    ] {
        let d = tempfile::tempdir().unwrap();
        normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
        rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
        let mut r = req(d.path(), &[]);
        r.inputs = vec![d.path().join("in/textures/pbr").join(given)];
        let (p, _) = plan(r);
        let j = job(&p, given);
        assert_eq!(set_codes(j), vec![Code::SetCompanionOutside], "{given}");
        let w = j
            .warnings
            .iter()
            .find(|w| w.code == Code::SetCompanionOutside)
            .unwrap();
        assert!(
            w.data["companion"].as_str().unwrap().ends_with(other),
            "{given}: {}",
            w.data
        );
        assert!(j.bake.is_none());
    }
}

/// Review Focus 2: a member moved into the set from another folder bakes from the `_n`'s real path.
#[test]
fn a_member_from_another_folder_bakes_from_its_own_path() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "other/iron_rmaos.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.set_edits = vec![SetEdit::Move {
        file: "other/iron_rmaos.png".into(),
        to: "textures/pbr/iron".into(),
    }];
    let (p, _) = plan(r);
    let b = job(&p, "other/iron_rmaos.png")
        .bake
        .as_ref()
        .expect("the moved rmaos bakes");
    assert!(b.normal.ends_with("textures/pbr/iron_n.png"));
}

#[test]
fn an_explicit_renormalize_on_the_n_opts_out() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, ev) = plan(req(
        d.path(),
        &["file:**/iron_n.png.mips.normal=renormalize"],
    ));
    assert!(job(&p, "iron_rmaos.png").bake.is_none());
    assert!(set_codes(job(&p, "iron_rmaos.png")).is_empty());
    assert_eq!(
        job(&p, "iron_n.png").provenance["mips.normal"].to_string(),
        "user"
    );
    assert!(!bake_flag(&ev, "textures/pbr/iron"));
}

/// M3b spec §3.3 (v1 instruction): a shared roughness renormalises only, and its borrower reports nothing missing.
#[test]
fn a_shared_roughness_is_never_baked_and_reports_nothing() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    normal(d.path(), "textures/pbr/steel_n.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.shared_roughness = ["textures/pbr/iron", "textures/pbr/steel"]
        .into_iter()
        .map(String::from)
        .collect();
    let (p, ev) = plan(r);
    assert!(job(&p, "iron_rmaos.png").bake.is_none());
    assert!(set_codes(job(&p, "steel_n.png")).is_empty());
    assert!(set_codes(job(&p, "iron_rmaos.png")).is_empty());
    assert!(!bake_flag(&ev, "textures/pbr/iron"));
}

fn borrow(r: &mut BatchRequest, pairs: &[(&str, &str)]) {
    r.borrowed_normal = pairs
        .iter()
        .map(|(b, o)| ((*b).to_string(), (*o).to_string()))
        .collect();
}

/// DECISIONS Note R M4-final-1: a set whose Normal slot is borrowed (GUI share) bakes its own roughness from the
/// owner set's `_n`, and neither set reports a missing companion.
#[test]
fn a_borrowed_n_bakes_the_borrowers_own_roughness() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    rough(d.path(), "textures/pbr/steel_rmaos.png", 32, 32);
    // An owner with no roughness of its own lends its `_n` too.
    normal(d.path(), "textures/pbr/copper_n.png", 64, 64);
    rough(d.path(), "textures/pbr/brass_rmaos.png", 64, 64);
    let mut r = req(d.path(), &[]);
    borrow(
        &mut r,
        &[
            ("textures/pbr/steel", "textures/pbr/iron"),
            ("textures/pbr/brass", "textures/pbr/copper"),
        ],
    );
    let (p, ev) = plan(r);
    let s = job(&p, "steel_rmaos.png");
    let b = s.bake.as_ref().expect("the borrower's rmaos bakes");
    assert!(b.normal.ends_with("iron_n.png"));
    assert_eq!(b.shift, 1); // the 64 px `_n` over a 32 px member
    assert!(b.stamp.is_some());
    assert!(set_codes(s).is_empty(), "{:?}", s.warnings);
    assert!(job(&p, "iron_rmaos.png").bake.is_some());
    assert!(bake_flag(&ev, "textures/pbr/steel"));
    let brass = job(&p, "brass_rmaos.png");
    assert!(brass
        .bake
        .as_ref()
        .is_some_and(|b| b.normal.ends_with("copper_n.png")));
    assert!(set_codes(brass).is_empty(), "{:?}", brass.warnings);
    let copper = job(&p, "copper_n.png");
    assert!(set_codes(copper).is_empty(), "{:?}", copper.warnings);
    assert_eq!(copper.profile.mips.normal, NormalMips::RenormalizeBake);
    assert!(bake_flag(&ev, "textures/pbr/brass"));
    assert!(!bake_flag(&ev, "textures/pbr/copper"));
}

/// The usual rules hold for a borrowed `_n`: the size ratio.
#[test]
fn a_borrowed_n_of_another_aspect_is_set_bake_ratio() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/steel_rmaos.png", 64, 32);
    let mut r = req(d.path(), &[]);
    borrow(&mut r, &[("textures/pbr/steel", "textures/pbr/iron")]);
    let (p, _) = plan(r);
    let s = job(&p, "steel_rmaos.png");
    assert!(s.bake.is_none());
    assert_eq!(set_codes(s), vec![Code::SetBakeRatio]);
}

/// R7 still wins: a shared roughness slot never bakes, even from a borrowed `_n`.
#[test]
fn a_shared_roughness_never_bakes_even_with_a_borrowed_n() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/steel_rmaos.png", 64, 64);
    let mut r = req(d.path(), &[]);
    borrow(&mut r, &[("textures/pbr/steel", "textures/pbr/iron")]);
    r.shared_roughness = ["textures/pbr/steel".to_string()].into_iter().collect();
    let (p, ev) = plan(r);
    let s = job(&p, "steel_rmaos.png");
    assert!(s.bake.is_none());
    assert!(set_codes(s).is_empty(), "{:?}", s.warnings);
    assert!(!bake_flag(&ev, "textures/pbr/steel"));
    // The lent `_n` renormalises only, silently: its borrower's roughness is shared (R7).
    let n = job(&p, "iron_n.png");
    assert_eq!(n.profile.mips.normal, NormalMips::Renormalize);
    assert!(set_codes(n).is_empty(), "{:?}", n.warnings);
}

#[test]
fn an_undecided_n_does_not_bake_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    // Without --accept-suggestions the `_normal` sub-type is an unconfirmed suggestion, so the `_n` is unresolved.
    normal(d.path(), "textures/pbr/iron_normal.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.accept = AcceptSuggestions::None;
    let (p, _) = plan(r);
    let m = job(&p, "iron_rmaos.png");
    assert!(m.bake.is_none());
    let w = m
        .warnings
        .iter()
        .find(|w| w.code == Code::SetMissingCompanion)
        .expect("SET-MISSING-COMPANION");
    assert!(w.message.contains("not decided"), "{}", w.message);
}

#[test]
fn a_blocked_bc5_n_still_bakes_from_its_source() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, _) = plan(req(
        d.path(),
        &[
            "file:**/iron_n.png.format=bc5",
            "file:**/iron_n.png.encoder.backend=rgbcx",
        ],
    ));
    assert!(codes(job(&p, "iron_n.png")).contains(&Code::FmtBc5Normal));
    assert!(job(&p, "iron_rmaos.png").bake.is_some());
}

#[test]
fn an_srgb_roughness_member_does_not_bake() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, _) = plan(req(
        d.path(),
        &["file:**/iron_rmaos.png.color.content=srgb"],
    ));
    let m = job(&p, "iron_rmaos.png");
    assert!(m.bake.is_none());
    assert!(codes(m).contains(&Code::TagContentMismatch));
}

#[test]
fn vanilla_neither_bakes_nor_reports_bake_companions() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.target = Some(Target::Vanilla);
    let (p, _) = plan(r);
    let n = job(&p, "iron_n.png");
    assert_eq!(n.profile.subtype, Subtype::Normal);
    assert!(set_codes(n).is_empty(), "{:?}", n.warnings);
}

/// Controller ruling (pre-flight drift 1): an unavailable `_flow` (CFG-UNAVAILABLE) raises nothing else, so it gets
/// no `SET-MISSING-COMPANION` for its missing `_n`.
#[test]
fn an_unavailable_flow_reports_no_missing_companion() {
    let target = Target::Vanilla;
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/hair/h_flow.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.target = Some(target);
    let (p, _) = plan(r);
    let j = job(&p, "h_flow.png");
    assert_eq!(j.profile.subtype, Subtype::Flow, "{target:?}");
    assert_eq!(codes(j), vec![Code::CfgUnavailable], "{target:?}");
}

/// Controller ruling: only roughness members (`rmaos`, `skin_rfaos`) get a `BakeJob`; the set's other members never.
#[test]
fn only_roughness_members_get_a_bake_job() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_cnr.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_d.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    assert!(job(&p, "iron_rmaos.png").bake.is_some());
    for (tail, subtype) in [
        ("iron_cnr.png", Subtype::CoatNormal),
        ("iron_d.png", Subtype::PbrAlbedo),
        ("iron_n.png", Subtype::PbrNormal),
    ] {
        let j = job(&p, tail);
        assert_eq!(j.profile.subtype, subtype, "{tail}");
        assert!(j.bake.is_none(), "{tail}");
    }
}

/// Controller ruling: an Advanced skin set's `_rfaos` bakes from its `_n` like an `_rmaos`, pre-packed or packed
/// from raw maps (`auto` with a fuzz source), outside a `pbr` folder too.
#[test]
fn an_advanced_skin_rfaos_bakes_from_its_n() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/actors/skin/a_n.png", 64, 64);
    rough(d.path(), "textures/actors/skin/a_rfaos.png", 64, 64);
    normal(d.path(), "textures/actors/skin/b_n.png", 64, 64);
    gray(d.path(), "textures/actors/skin/b_roughness.png", 120);
    gray(d.path(), "textures/actors/skin/b_fuzz.png", 30);
    let (p, ev) = plan(req(d.path(), &[]));
    let a = job(&p, "a_rfaos.png");
    assert_eq!(a.profile.subtype, Subtype::SkinRfaos);
    assert!(a
        .bake
        .as_ref()
        .expect("a_rfaos bakes")
        .normal
        .ends_with("a_n.png"));
    let b = p
        .jobs
        .iter()
        .find(|j| j.pack.is_some())
        .expect("a packed rfaos");
    assert_eq!(b.profile.subtype, Subtype::SkinRfaos);
    assert!(
        b.bake
            .as_ref()
            .expect("the packed rfaos bakes")
            .normal
            .ends_with("b_n.png"),
        "{:?}",
        b.warnings
    );
    for set in ["skin/a", "skin/b"] {
        assert!(bake_flag(&ev, set), "{set}");
    }
}

/// A borrowed Normal slot whose owner set has no usable `_n` says so, instead of "no _n in this PBR set".
#[test]
fn a_borrowed_n_from_a_set_without_one_is_set_missing_companion() {
    let d = tempfile::tempdir().unwrap();
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    rough(d.path(), "textures/pbr/steel_rmaos.png", 64, 64);
    let mut r = req(d.path(), &[]);
    borrow(&mut r, &[("textures/pbr/steel", "textures/pbr/iron")]);
    let (p, _) = plan(r);
    let s = job(&p, "steel_rmaos.png");
    assert!(s.bake.is_none());
    assert_eq!(set_codes(s), vec![Code::SetMissingCompanion]);
    let w = s.warnings.last().unwrap();
    assert!(w.message.contains("borrowed"), "{}", w.message);
}

/// S-mips-7: the bake's size check reads the post-resize sizes, so an NPOT pair with `resize = pot-down` bakes.
#[test]
fn resized_npot_pair_bakes_on_post_resize_sizes() {
    for resize in [true, false] {
        let d = tempfile::tempdir().unwrap();
        normal(d.path(), "textures/pbr/iron_n.png", 48, 48);
        rough(d.path(), "textures/pbr/iron_rmaos.png", 48, 48);
        let sets: &[&str] = if resize { &["resize=pot-down"] } else { &[] };
        let (p, _) = plan(req(d.path(), sets));
        let m = job(&p, "iron_rmaos.png");
        if resize {
            assert_eq!(
                m.bake.as_ref().map(|b| b.shift),
                Some(0),
                "{:?}",
                m.warnings
            );
            assert!(!codes(m).contains(&Code::SetBakeRatio), "{:?}", m.warnings);
        } else {
            assert!(m.bake.is_none());
            assert!(codes(m).contains(&Code::SetBakeRatio), "{:?}", m.warnings);
        }
    }
}

/// The bake sizes its inputs by content (`load::image_size`), as Load and admission do, not by extension: an
/// `_rmaos.tga` that holds a PNG still bakes. A DDS member in the same set changes nothing (DECISIONS S6).
#[test]
fn bake_sizes_members_by_content_beside_a_dds_member() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let png = d.path().join("in/textures/pbr/iron_rmaos.png");
    std::fs::rename(&png, png.with_extension("tga")).unwrap();
    fake_dds(d.path(), "textures/pbr/iron_s.dds");
    let (p, ev) = plan(req(d.path(), &[]));
    let m = job(&p, "iron_rmaos.tga");
    assert!(m.bake.is_some(), "{:?}", m.warnings);
    assert!(bake_flag(&ev, "textures/pbr/iron"));
}
