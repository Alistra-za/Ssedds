mod support;
use engine::codes::Code;
use engine::config::{AlphaPolicy, GreenConvention, Subtype, Target};
use sdc_gui::model::items::*;
use sdc_gui::model::*;
use std::time::Duration;
use support::*;

fn status(m: &AppModel, key: &str) -> ItemStatus {
    m.item(key)
        .unwrap_or_else(|| panic!("no item {key}"))
        .status
        .clone()
}

#[test]
fn opening_a_folder_lists_items_sets_and_decisions() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let keys: Vec<&str> = m.items.iter().map(|i| i.key.as_str()).collect();
    for k in [
        "landscape/rock01.png",
        "landscape/leaf.png",
        "landscape/rock01_normal.png",
        "landscape/odd.png",
        "landscape/rock01_rmaos",
        "landscape/rock01_roughness.png",
        "landscape/rock01_ao.png",
    ] {
        assert!(keys.contains(&k), "missing {k} in {keys:?}");
    }
    assert!(
        matches!(status(&m, "landscape/rock01_roughness.png"), ItemStatus::PackedInto(j) if j == "landscape/rock01_rmaos.dds")
    );
    let leaf = m.item("landscape/leaf.png").unwrap();
    assert!(leaf
        .decisions
        .iter()
        .any(|d| matches!(d, Decision::Subtype { .. })));
    let n = m.item("landscape/rock01_normal.png").unwrap();
    // Note AW: only the sub-type is asked; the unstated green convention is an Info line.
    assert!(n
        .decisions
        .iter()
        .all(|d| matches!(d, Decision::Subtype { .. })));
    assert!(n
        .job
        .as_ref()
        .unwrap()
        .warnings
        .iter()
        .any(|w| w.code == Code::InGreenUnstated));
    assert!(n.thumb.is_some());
    assert!(m.undecided() >= 3);
    let set = m.sets.iter().find(|s| s.key.ends_with("rock01")).unwrap();
    assert!(set
        .recipes
        .iter()
        .any(|r| r.output.ends_with("rock01_rmaos.dds")));
    assert_eq!(m.target, Target::Cs);
}

#[test]
fn decisions_re_resolve_before_saving_and_save_writes_them() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/odd.png", Answer::Acknowledge(Code::DimNpot));
    m.decide("landscape/leaf.png", Answer::Subtype(Subtype::Diffuse));
    m.decide("landscape/leaf.png", Answer::Alpha(AlphaPolicy::Cutout));
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Subtype(Subtype::Normal),
    );
    m.decide(
        "landscape/rock01_normal.png",
        Answer::Green(GreenConvention::Directx),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    for k in [
        "landscape/rock01.png",
        "landscape/odd.png",
        "landscape/leaf.png",
        "landscape/rock01_normal.png",
    ] {
        assert_eq!(status(&m, k), ItemStatus::Ready, "{k}");
    }
    let toml_path = m.root.clone().unwrap().join("sdc.toml");
    assert!(!toml_path.exists(), "nothing is written before a save");
    assert!(m.project.as_ref().unwrap().is_dirty());
    m.save().unwrap();
    let text = std::fs::read_to_string(&toml_path).unwrap();
    assert!(text.contains("landscape/odd.png") && text.contains("DIM-NPOT"));
    assert!(text.contains("green_convention"));
}

#[test]
fn filters_search_and_selection_survive_a_replan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.filter.needs_decision = true;
    let shown: Vec<String> = m
        .visible_items()
        .iter()
        .map(|&i| m.items[i].key.clone())
        .collect();
    assert!(shown.contains(&"landscape/leaf.png".to_string()));
    assert!(!shown.contains(&"landscape/rock01_roughness.png".to_string()));
    m.filter.needs_decision = false;
    m.search = "LEAF".into();
    assert_eq!(m.visible_items().len(), 1);
    m.search.clear();
    m.select_item("landscape/leaf.png");
    m.set_field(
        "landscape/leaf.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.selection, Selection::Item("landscape/leaf.png".into()));
    let job = m.item("landscape/leaf.png").unwrap().job.as_ref().unwrap();
    assert_eq!(job.profile.alpha.threshold, 90);
    assert_eq!(
        job.provenance["alpha.threshold"].to_string(),
        "project_file"
    );
    m.clear_field("landscape/leaf.png", "alpha.threshold");
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_ne!(
        m.item("landscape/leaf.png")
            .unwrap()
            .job
            .as_ref()
            .unwrap()
            .profile
            .alpha
            .threshold,
        90
    );
}

#[test]
fn the_mode_changes_target_and_recipes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.target, Target::Vanilla);
    assert!(m.item("landscape/rock01_rmaos").is_none());
    // Roughness and AO make no env-mask `_m`; a complex-material set packs the complex `_m` (Note AZ).
    assert!(m.item("landscape/rock01_m").is_none());
    complex_material(&mut m, "landscape/rock01");
    assert!(m.item("landscape/rock01_m").is_some());
}

#[test]
fn an_invalid_sdc_toml_opens_read_only() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc.toml"), "target = [\n").unwrap();
    let mut m = sdc_gui::model::AppModel::new(Default::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root.clone());
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(m.banner.as_deref().unwrap().contains("sdc.toml"));
    assert!(
        !m.items.is_empty(),
        "the folder still plans without sdc.toml"
    );
    m.set_field(
        "landscape/leaf.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    assert!(!m.project.as_ref().unwrap().is_dirty());
    assert_eq!(
        std::fs::read_to_string(root.join("sdc.toml")).unwrap(),
        "target = [\n"
    );
}

#[test]
fn closing_with_unsaved_edits_asks_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.request_close());
    m.set_target(Target::Vanilla);
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(Dialog::CloseUnsaved));
}

#[test]
fn an_external_edit_is_not_overwritten_silently() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    let p = m.root.clone().unwrap().join("sdc.toml");
    std::fs::write(&p, "# written by hand meanwhile\n").unwrap();
    assert!(m.save().is_err());
    assert_eq!(m.dialog, Some(Dialog::ExternalChange(DocKind::SdcToml)));
    m.resolve_external_change(DocKind::SdcToml, false);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "# written by hand meanwhile\n"
    );
    assert_eq!(m.target, Target::Cs);
}

/// Audit 2026-10-14 finding 3: with no unsaved edits, a hand edit of `sdc.toml` is taken, never offered "Overwrite".
#[test]
fn a_hand_edit_of_a_clean_project_is_reloaded_and_never_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let p = m.root.clone().unwrap().join("sdc.toml");
    std::fs::write(&p, "target = \"vanilla\"\n").unwrap();
    assert!(m.save().is_ok());
    assert_eq!(m.dialog, None);
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "target = \"vanilla\"\n"
    );
    assert_eq!(m.check_sources(), 1);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.target, Target::Vanilla);
    assert!(!m.project.as_ref().unwrap().is_dirty());
    assert_eq!(m.check_sources(), 0);
}

#[test]
fn sets_list_their_members_and_unmatched_file_keys_are_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let set = m.sets.iter().find(|s| s.key.ends_with("rock01")).unwrap();
    assert!(set.pbr);
    for k in [
        "landscape/rock01.png",
        "landscape/rock01_rmaos",
        "landscape/rock01_ao.png",
    ] {
        assert!(
            set.members.iter().any(|x| x == k),
            "{k} missing from {:?}",
            set.members
        );
    }
    assert!(m.unmatched_keys.is_empty());
    m.set_field(
        "landscape/gone.png",
        "alpha.threshold",
        serde_json::json!(10),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.unmatched_keys, vec!["landscape/gone.png".to_string()]);
}

#[test]
fn opening_another_folder_with_unsaved_edits_asks_first() {
    let d = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let other_root = mod_folder(other.path());
    let mut m = opened(d.path());
    let first = m.root.clone();
    // Clean: opens straight away.
    assert!(m.request_open(other_root.clone()));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.root, Some(other_root.clone()));

    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    assert!(!m.request_open(other_root.clone()));
    assert_eq!(m.dialog, Some(Dialog::OpenUnsaved(other_root.clone())));
    assert_eq!(m.root, first);
    assert!(m.is_dirty());

    m.resolve_open_unsaved(other_root.clone(), false);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.root, Some(other_root));
    assert!(m.dialog.is_none());
    assert!(!m.is_dirty());
}

#[test]
fn saving_before_opening_writes_the_edit_first() {
    let d = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let other_root = mod_folder(other.path());
    let mut m = opened(d.path());
    m.set_target(Target::Vanilla);
    let toml = m.root.clone().unwrap().join("sdc.toml");
    assert!(!m.request_open(other_root.clone()));
    m.resolve_open_unsaved(other_root.clone(), true);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.root, Some(other_root));
    assert!(std::fs::read_to_string(toml).unwrap().contains("vanilla"));
}

#[test]
fn the_has_warnings_filter_ignores_info_like_the_badge() {
    use engine::codes::{Code, Warning};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    for i in m.items.iter_mut() {
        i.warnings = vec![];
    }
    m.items[0].warnings = vec![Warning::new(Code::AlphaPbrNormalDropped, "dropped")];
    m.items[1].warnings = vec![Warning::new(Code::LossReencode, "re-encoded")];
    m.invalidate_views();
    m.filter.warnings = true;
    assert_eq!(m.visible_items(), vec![1]);
}

#[test]
fn npot_resize_choice_clears_the_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // F9: odd.png is undecided in the fixture.
    m.decide("landscape/odd.png", Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let odd = m.item("landscape/odd.png").unwrap();
    assert!(odd
        .decisions
        .contains(&Decision::Acknowledge(Code::DimNpot)));
    m.set_field("landscape/odd.png", "resize", serde_json::json!("pot-down"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let odd = m.item("landscape/odd.png").unwrap();
    assert!(
        !odd.decisions
            .contains(&Decision::Acknowledge(Code::DimNpot)),
        "{:?}",
        odd.decisions
    );
    assert!(
        odd.warnings.iter().any(|w| w.code == Code::DimResized),
        "{:?}",
        odd.warnings
    );
    assert_eq!(
        m.project
            .as_ref()
            .unwrap()
            .file_value("landscape/odd.png", "resize"),
        Some(serde_json::json!("pot-down"))
    );
}

/// M6 Track E spec §2, §6: the GUI accepts High suggestions. A PBR set's colour, `_n` and `_rmaos` members decide
/// themselves (no sub-type question; "(auto)" with the reason), and a Medium `_i` still asks.
#[test]
fn the_gui_accepts_high_suggestions_and_still_asks_for_medium_ones() {
    use engine::policy::{AcceptSuggestions, Provenance};
    use sdc_gui::model::fields::auto_subtype_reason;
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/a");
    std::fs::create_dir_all(&t).unwrap();
    for (name, px) in [
        ("iron.png", [120u8, 90, 60]),
        ("iron_n.png", [128, 128, 255]),
        ("iron_rmaos.png", [128, 0, 255]),
        ("wall_i.png", [90, 90, 90]),
    ] {
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px))
            .save(t.join(name))
            .unwrap();
    }
    let m = opened_as_the_app(d.path().join("textures"), d.path().join("export"));
    assert_eq!(m.request().unwrap().accept, AcceptSuggestions::High);
    let asks_subtype = |key: &str| {
        m.item(key)
            .unwrap()
            .decisions
            .iter()
            .any(|x| matches!(x, Decision::Subtype { .. }))
    };
    for (key, want) in [
        ("a/iron.png", Subtype::PbrAlbedo),
        ("a/iron_n.png", Subtype::PbrNormal),
        ("a/iron_rmaos.png", Subtype::Rmaos),
    ] {
        let job = m.item(key).unwrap().job.clone().unwrap();
        assert_eq!(
            (job.profile.subtype, job.provenance["subtype"].clone()),
            (want, Provenance::Suggestion),
            "{key}"
        );
        assert!(!asks_subtype(key), "{key}");
        assert_eq!(
            auto_subtype_reason(&job),
            Some(job.suggestions[0].reason.as_str()),
            "{key}"
        );
    }
    assert!(asks_subtype("a/wall_i.png"));
    let wall = m.item("a/wall_i.png").unwrap().job.clone().unwrap();
    assert_eq!(auto_subtype_reason(&wall), None);
}

/// Spec Review Focus 3: a sub-type the user chose (`sdc.toml` `[files]`) beats the accepted suggestion and is not
/// "(auto)".
#[test]
fn an_explicit_subtype_beats_an_accepted_suggestion() {
    use sdc_gui::model::fields::auto_subtype_reason;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_as_the_app(mod_folder(d.path()), d.path().join("export"));
    let job = m.item("landscape/rock01.png").unwrap().job.clone().unwrap();
    assert!(auto_subtype_reason(&job).is_some(), "{:?}", job.status);
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m.item("landscape/rock01.png").unwrap().job.clone().unwrap();
    assert_eq!(
        (job.profile.subtype, job.provenance["subtype"].to_string()),
        (Subtype::Diffuse, "project_file".to_string())
    );
    assert_eq!(auto_subtype_reason(&job), None);
}

#[test]
fn a_set_view_carries_its_recipes() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let set = m.sets.iter().find(|s| s.key.ends_with("rock01")).unwrap();
    let v = sets::set_views(&m)
        .into_iter()
        .find(|v| v.key == set.key)
        .unwrap();
    assert!(!v.recipes.is_empty());
    assert_eq!(v.recipes.len(), set.recipes.len());
}
