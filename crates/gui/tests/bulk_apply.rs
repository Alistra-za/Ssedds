//! M6 Track F3a D2/D4/D5: every bulk command is one undo step and one plan.
mod support;
use image::{ImageBuffer, Rgb};
use sdc_gui::model::bulk::{plan_bulk_edit, BulkOp, Change};
use sdc_gui::model::*;
use sdc_gui::settings::GuiSettings;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const LEAF: &str = "landscape/leaf.png";
const ODD: &str = "landscape/odd.png";
const NORMAL: &str = "landscape/rock01_normal.png";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "planning did not finish"
    );
}

fn file(m: &AppModel, k: &str, f: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(k, f)
}

fn strings(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

/// `n` small colour textures under `textures/many`, each its own set.
fn many(dir: &Path, n: usize) -> PathBuf {
    let r = dir.join("textures/many");
    std::fs::create_dir_all(&r).unwrap();
    for i in 0..n {
        ImageBuffer::from_fn(8, 8, |x, y| Rgb([(x * 30) as u8, (y * 30) as u8, i as u8]))
            .save(r.join(format!("t{i:03}.png")))
            .unwrap();
    }
    dir.join("textures")
}

#[test]
fn a_bulk_edit_of_300_textures_is_one_step_and_one_plan() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_as_the_app(many(d.path(), 300), d.path().join("export"));
    m.select_all_visible();
    let keys = m.selected_items();
    assert_eq!(keys.len(), 300);
    let (steps, generation) = (m.history.undo.len(), m.generation());
    m.apply_bulk_edit(&keys, "format", BulkOp::Set(json!("bc1")));
    assert_eq!(m.history.undo.len(), steps + 1, "one undo step");
    assert_eq!(
        m.generation(),
        generation + 1,
        "one plan for the whole edit"
    );
    assert!(keys
        .iter()
        .all(|k| file(&m, k, "format") == Some(json!("bc1"))));
    assert_eq!(
        m.bulk.report.as_deref(),
        Some("Format: written to 300 of 300")
    );
    idle(&mut m);
    assert_eq!(m.selected_items().len(), 300, "the selection is kept");
    m.undo();
    assert!(keys.iter().all(|k| file(&m, k, "format").is_none()));
}

/// Two sets, each a diffuse and an `_n`; High suggestions not accepted (as `pbr_members.rs`).
fn two_sets(dir: &Path) -> AppModel {
    let r = dir.join("m");
    std::fs::create_dir_all(&r).unwrap();
    for s in ["rock01", "rock02"] {
        ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
            .save(r.join(format!("{s}.png")))
            .unwrap();
        ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
            .save(r.join(format!("{s}_n.png")))
            .unwrap();
    }
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

#[test]
fn a_bulk_sub_type_runs_the_pbr_follow_per_set_and_one_undo_restores_both_files() {
    let d = tempfile::tempdir().unwrap();
    let mut m = two_sets(d.path());
    let diffuses = strings(&["m/rock01.png", "m/rock02.png"]);
    let normals = ["m/rock01_n.png", "m/rock02_n.png"];
    let steps = m.history.undo.len();
    m.apply_bulk_edit(&diffuses, "subtype", BulkOp::Set(json!("pbr_albedo")));
    assert_eq!(m.history.undo.len(), steps + 1);
    for n in normals {
        assert_eq!(file(&m, n, "subtype"), Some(json!("pbr_normal")), "{n}");
    }
    let mut auto = m.sets_doc.as_ref().unwrap().auto_pbr();
    auto.sort();
    assert_eq!(auto, strings(&normals));
    idle(&mut m);
    m.undo();
    for k in diffuses.iter().map(String::as_str).chain(normals) {
        assert_eq!(file(&m, k, "subtype"), None, "{k}");
    }
    assert!(
        m.sets_doc.as_ref().unwrap().auto_pbr().is_empty(),
        "Review Focus 4"
    );
}

/// Perf task 4 pin: a bulk Sub-type apply prunes `auto_pbr` entries whose file is gone and keeps live ones, with the
/// members it switches appended in order; a bulk reset reverts them and prunes again.
#[test]
fn a_bulk_sub_type_prunes_gone_auto_pbr_entries_and_keeps_live_ones() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("m")).unwrap();
    ImageBuffer::from_fn(64, 64, |_, _| Rgb([10u8, 20, 30]))
        .save(d.path().join("m/keep.png"))
        .unwrap();
    let mut m = two_sets(d.path());
    let (keep, n1, n2) = ("m/keep.png", "m/rock01_n.png", "m/rock02_n.png");
    let set_auto =
        |m: &mut AppModel, a: &[&str]| m.sets_doc.as_mut().unwrap().set_auto_pbr(&strings(a));
    let auto = |m: &AppModel| m.sets_doc.as_ref().unwrap().auto_pbr();
    set_auto(&mut m, &["m/gone_a.png", keep, "m/gone_b.png"]);
    let diffuses = strings(&["m/rock01.png", "m/rock02.png"]);
    m.apply_bulk_edit(&diffuses, "subtype", BulkOp::Set(json!("pbr_albedo")));
    assert_eq!(auto(&m), strings(&[keep, n1, n2]));
    assert_eq!(file(&m, n1, "subtype"), Some(json!("pbr_normal")));
    assert_eq!(file(&m, n2, "subtype"), Some(json!("pbr_normal")));
    assert_eq!(file(&m, keep, "subtype"), None);
    for k in &diffuses {
        assert_eq!(file(&m, k, "subtype"), Some(json!("pbr_albedo")), "{k}");
    }
    idle(&mut m);
    set_auto(&mut m, &[keep, n1, n2, "m/gone_c.png"]);
    m.apply_bulk_edit(&diffuses, "subtype", BulkOp::Clear);
    assert_eq!(auto(&m), strings(&[keep]));
    for k in diffuses.iter().map(String::as_str).chain([n1, n2, keep]) {
        assert_eq!(file(&m, k, "subtype"), None, "{k}");
    }
}

#[test]
fn a_read_only_sdc_toml_makes_every_bulk_write_a_no_op() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(LEAF, "format", json!("bc1"));
    idle(&mut m);
    std::fs::write(d.path().join("textures/sdc.toml"), "this is = [not toml").unwrap();
    m.reload_documents();
    idle(&mut m);
    assert!(m.project.as_ref().unwrap().read_only());
    let (steps, generation) = (m.history.undo.len(), m.generation());
    let ks = strings(&[ROCK, ODD]);
    m.apply_bulk_edit(&ks, "format", BulkOp::Set(json!("bc1")));
    m.apply_copy_settings(LEAF, &ks, &["format".to_string()]);
    m.reset_all_settings(&ks);
    assert_eq!((m.history.undo.len(), m.generation()), (steps, generation));
    assert_eq!(m.bulk.report, None);
}

#[test]
fn copy_from_active_lists_the_set_fields_and_writes_only_the_ticked_ones() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    // R13: a valid pair for the targets (BC1 linear is not one for a landscape diffuse).
    m.set_field(LEAF, "format", json!("bc7"));
    m.set_field(LEAF, "encoder.effort", json!("veryfast"));
    idle(&mut m);
    m.click_item(ROCK, false, false);
    m.click_item(ODD, true, false);
    m.click_item(LEAF, true, false);
    m.open_copy_settings();
    let Some(Dialog::CopySettings { from, to, fields }) = m.dialog.clone() else {
        panic!("expected the copy dialog, got {:?}", m.dialog);
    };
    assert_eq!(from, LEAF);
    assert_eq!(to, strings(&[ROCK, ODD]));
    assert_eq!(
        fields,
        vec![
            ("format".to_string(), true),
            ("encoder.effort".to_string(), true)
        ]
    );
    let steps = m.history.undo.len();
    m.apply_copy_settings(&from, &to, &["format".to_string()]);
    assert_eq!(m.dialog, None);
    assert_eq!(m.history.undo.len(), steps + 1);
    for k in [ROCK, ODD] {
        assert_eq!(file(&m, k, "format"), Some(json!("bc7")), "{k}");
        assert_eq!(file(&m, k, "encoder.effort"), None, "{k}: unticked");
    }
    assert_eq!(m.selected_items().len(), 3, "the selection is kept");
}

#[test]
fn reset_all_removes_the_entries_after_the_confirm_as_one_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(ROCK, "format", json!("bc1"));
    m.set_field(ODD, "encoder.effort", json!("veryfast"));
    idle(&mut m);
    m.click_item(ROCK, false, false);
    m.click_item(ODD, true, false);
    m.ask_reset_all();
    assert_eq!(
        m.dialog,
        Some(Dialog::ConfirmResetAll(strings(&[ROCK, ODD])))
    );
    let steps = m.history.undo.len();
    m.reset_all_settings(&strings(&[ROCK, ODD]));
    assert_eq!(m.dialog, None);
    assert_eq!(m.history.undo.len(), steps + 1);
    let keys = m.project.as_ref().unwrap().file_keys();
    assert!(!keys.iter().any(|k| k == ROCK || k == ODD), "{keys:?}");
}

/// R3: acknowledged warnings are decisions, not settings.
#[test]
fn reset_all_keeps_the_acknowledged_warnings() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(ROCK, "format", json!("bc1"));
    m.set_field(ROCK, "allow", json!(["CHAN-NOT-GRAY"]));
    idle(&mut m);
    assert_eq!(m.project.as_ref().unwrap().allow(ROCK).len(), 1, "fixture");
    m.reset_all_settings(&strings(&[ROCK]));
    assert_eq!(file(&m, ROCK, "format"), None);
    assert_eq!(
        m.project.as_ref().unwrap().allow(ROCK).len(),
        1,
        "allow kept"
    );
}

#[test]
fn the_report_says_how_many_were_written_and_why_the_rest_were_skipped() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.apply_bulk_edit(
        &strings(&[ROCK, NORMAL]),
        "mips.normal",
        BulkOp::Set(json!("renormalize_bake")),
    );
    assert_eq!(
        m.bulk.report.as_deref(),
        Some("Normal handling: written to 1 of 2; locked on 1 of 2: normal maps only")
    );
    assert_eq!(
        file(&m, NORMAL, "mips.normal"),
        Some(json!("renormalize_bake"))
    );
    assert_eq!(file(&m, ROCK, "mips.normal"), None);
}

/// R6: a Clear reaches a locked field (a stale `[files]` entry must be removable); a Set does not.
#[test]
fn a_bulk_clear_is_not_blocked_by_a_field_lock() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let ks = strings(&[ROCK, NORMAL]);
    let set = plan_bulk_edit(
        &m,
        &ks,
        "mips.normal",
        &BulkOp::Set(json!("renormalize_bake")),
    );
    assert!(matches!(set[0].1, Change::Skip(_)), "{set:?}");
    let clear = plan_bulk_edit(&m, &ks, "mips.normal", &BulkOp::Clear);
    assert!(clear.iter().all(|(_, c)| *c == Change::Write), "{clear:?}");
}

/// R7: an unparsable value is reported and not written.
#[test]
fn an_invalid_set_value_is_reported_and_not_written() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let ks = strings(&[ROCK, ODD]);
    let steps = m.history.undo.len();
    m.apply_bulk_edit(&ks, "mips.filter", BulkOp::Set(json!("nonsense")));
    let report = m.bulk.report.clone().unwrap();
    assert!(report.contains("written to 0 of 2"), "{report}");
    assert!(report.contains("not a valid value"), "{report}");
    assert_eq!(file(&m, ROCK, "mips.filter"), None);
    assert_eq!(m.history.undo.len(), steps);
}

/// A `w` x `2w` grass PNG: top colour with cutout dots, bottom unit normals (as `complex_grass.rs`).
fn grass_png(path: &Path, w: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::ImageBuffer::from_fn(w, 2 * w, |x, y| {
        if y < w {
            image::Rgba([
                64u8,
                128,
                38,
                if (x * 7 + y * 13) % 10 < 3 { 255 } else { 0 },
            ])
        } else {
            let t = (x as f32 / w as f32 - 0.5) * 0.8;
            let c = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
            image::Rgba([c(t.sin()), 128, c(t.cos()), 12])
        }
    })
    .save(path)
    .unwrap();
}

#[test]
fn the_shader_locked_sub_type_is_skipped_for_a_set_but_counted() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    grass_png(&d.path().join("textures/plants/g.png"), 32);
    ImageBuffer::from_fn(8, 8, |x, y| Rgb([(x * 30) as u8, (y * 30) as u8, 7u8]))
        .save(d.path().join("textures/plants/h.png"))
        .unwrap();
    let mut m = opened(d.path());
    let set = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.members.iter().any(|k| k == "plants/g.png"))
        .expect("the grass set")
        .id;
    m.set_shader(&set, Some(Shader::ComplexGrass));
    idle(&mut m);
    let ks = strings(&["plants/g.png", "plants/h.png"]);
    m.apply_bulk_edit(&ks, "subtype", BulkOp::Set(json!("landscape_diffuse")));
    let report = m.bulk.report.clone().unwrap();
    assert!(
        report.starts_with("Sub-type: written to 1 of 2; locked on 1 of 2: "),
        "{report}"
    );
    assert!(report.contains("Complex grass"), "{report}");
    assert_eq!(file(&m, "plants/g.png", "subtype"), None);
    assert_eq!(
        file(&m, "plants/h.png", "subtype"),
        Some(json!("landscape_diffuse"))
    );
}

#[test]
fn a_bulk_write_that_changes_nothing_sets_no_report() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let keys = strings(&[ROCK, ODD]);
    m.apply_bulk_edit(&keys, "format", BulkOp::Set(json!("bc1")));
    m.bulk.report = None;
    m.apply_bulk_edit(&keys, "format", BulkOp::Set(json!("bc1")));
    assert_eq!(m.bulk.report, None, "same values: nothing was written");
    m.reset_all_settings(&strings(&[LEAF]));
    assert_eq!(m.bulk.report, None, "no entry to remove");
}

#[test]
fn one_texture_is_singular_in_the_step_label_and_report() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.apply_bulk_edit(&strings(&[ROCK]), "format", BulkOp::Set(json!("bc1")));
    assert_eq!(
        m.history.undo.last().unwrap().label,
        "Change Format on 1 texture"
    );
    m.reset_all_settings(&strings(&[ROCK]));
    assert_eq!(
        m.history.undo.last().unwrap().label,
        "Reset all settings on 1 texture"
    );
    assert_eq!(
        m.bulk.report.as_deref(),
        Some("Reset all settings on 1 texture")
    );
}

/// Task 9 merge: the bulk Format row offers the pairs every texture allows (else, sharing none, every pair any
/// allows) and writes the pair (format and tag); a pair only some allow skips the others ("applies to K of N").
#[test]
fn the_bulk_format_row_offers_the_shared_pairs_and_writes_both() {
    use sdc_gui::model::bulk::bulk_encodings;
    use sdc_gui::model::fields::encoding_options;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let opts = |m: &AppModel, k: &str| {
        let j = m.item(k).unwrap().job.clone().unwrap();
        encoding_options(m.target, &j.profile)
    };
    let (rock, normal) = (opts(&m, ROCK), opts(&m, NORMAL));
    assert_eq!(bulk_encodings(&m, &strings(&[ROCK, ROCK])), rock);
    // An sRGB albedo and a linear normal map share no pair: the row lists both lists.
    assert!(rock.iter().all(|(e, _)| normal.iter().all(|(o, _)| o != e)));
    let ks = strings(&[ROCK, NORMAL]);
    assert_eq!(bulk_encodings(&m, &ks), [rock.clone(), normal].concat());
    let pick = json!(rock[0].0);
    let plan = plan_bulk_edit(&m, &ks, "format", &BulkOp::Set(pick.clone()));
    assert_eq!(plan[0].1, Change::Write);
    assert_eq!(plan[1].1, Change::Skip("not valid for this map".into()));
    m.apply_bulk_edit(&ks, "format", BulkOp::Set(pick));
    assert_eq!(file(&m, ROCK, "format"), Some(json!(rock[0].0 .0)));
    assert_eq!(file(&m, ROCK, "color.tag"), Some(json!(rock[0].0 .1)));
    assert_eq!(file(&m, NORMAL, "format"), None);
    assert_eq!(
        m.bulk.report.as_deref(),
        Some("Format: written to 1 of 2; locked on 1 of 2: not valid for this map")
    );
}

/// R13: Copy settings copies the format with its tag (a pair, through `set_format`), and skips a texture whose
/// sub-type does not allow the pair.
#[test]
fn copy_settings_copies_the_format_pair_and_skips_where_not_allowed() {
    use engine::policy::allowed_encodings;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let allowed = |m: &AppModel, k: &str| {
        let st = m.item(k).unwrap().job.as_ref().unwrap().profile.subtype;
        allowed_encodings(st, m.target)
    };
    // A pair the two landscape diffuses allow and the TruePBR albedo does not.
    let (leaf, odd, rock) = (allowed(&m, LEAF), allowed(&m, ODD), allowed(&m, ROCK));
    let (f, t) = *leaf
        .iter()
        .find(|e| odd.contains(e) && !rock.contains(e))
        .expect("fixture: such a pair");
    m.set_format(LEAF, f, t);
    idle(&mut m);
    let to = strings(&[ODD, ROCK]);
    m.apply_copy_settings(LEAF, &to, &["format".to_string(), "color.tag".to_string()]);
    idle(&mut m);
    assert_eq!(file(&m, ODD, "format"), Some(json!(f)));
    assert_eq!(file(&m, ODD, "color.tag"), Some(json!(t)));
    assert_eq!(
        file(&m, ROCK, "format"),
        None,
        "not valid for a TruePBR albedo"
    );
    assert_eq!(
        file(&m, ROCK, "color.tag"),
        None,
        "the tag is not copied alone"
    );
    assert_eq!(
        m.bulk.report.as_deref(),
        Some("Format: written to 1 of 2; locked on 1 of 2: not valid for this map")
    );
}

/// Audit 2026-10-13b F-I1 (DECISIONS AF5): a bulk DDS tag write checks the tag against each texture's own format;
/// a pair its sub-type does not allow is skipped and counted.
#[test]
fn a_bulk_tag_skips_a_texture_whose_format_has_no_such_tag() {
    use engine::policy::allowed_encodings;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let pair_ok = |m: &AppModel, k: &str, t: engine::config::ColorTag| {
        let p = &m.item(k).unwrap().job.as_ref().unwrap().profile;
        allowed_encodings(p.subtype, m.target).contains(&(p.format, t))
    };
    let t = [
        engine::config::ColorTag::Srgb,
        engine::config::ColorTag::Unorm,
    ]
    .into_iter()
    .find(|&t| pair_ok(&m, ROCK, t) && !pair_ok(&m, NORMAL, t))
    .expect("fixture: a tag the albedo takes and the normal map does not");
    let ks = strings(&[ROCK, NORMAL]);
    m.apply_bulk_edit(&ks, "color.tag", BulkOp::Set(json!(t)));
    idle(&mut m);
    assert_eq!(file(&m, NORMAL, "color.tag"), None);
    assert_eq!(
        m.bulk.report.as_deref(),
        Some("DDS tag: written to 1 of 2; locked on 1 of 2: not valid for this map")
    );
    // Copy settings with only the tag ticked takes the same rule.
    m.undo();
    idle(&mut m);
    m.set_field(ROCK, "color.tag", json!(t));
    idle(&mut m);
    m.apply_copy_settings(ROCK, &strings(&[NORMAL]), &["color.tag".to_string()]);
    idle(&mut m);
    assert_eq!(file(&m, NORMAL, "color.tag"), None, "copy skips it too");
    let report = m.bulk.report.clone().unwrap_or_default();
    assert!(report.contains("not valid for this map"), "{report}");
}

/// F3a ledger R15 (DECISIONS AF5): Copy with Format ticked from a source that has a tag but no format of its own
/// copies the tag (checked like a lone tag) instead of dropping it.
#[test]
fn copy_with_format_ticked_keeps_a_tag_only_source_tag() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    use engine::config::ColorTag;
    use engine::policy::allowed_encodings;
    let pair_ok = |m: &AppModel, k: &str, t: ColorTag| {
        let p = &m.item(k).unwrap().job.as_ref().unwrap().profile;
        allowed_encodings(p.subtype, m.target).contains(&(p.format, t))
    };
    let t = [ColorTag::Srgb, ColorTag::Unorm]
        .into_iter()
        .find(|&t| pair_ok(&m, ODD, t) && pair_ok(&m, LEAF, t))
        .expect("fixture: a tag both diffuses take");
    m.set_field(LEAF, "color.tag", json!(t));
    idle(&mut m);
    assert_eq!(file(&m, LEAF, "format"), None);
    m.apply_copy_settings(
        LEAF,
        &strings(&[ODD]),
        &["format".to_string(), "color.tag".to_string()],
    );
    idle(&mut m);
    assert_eq!(file(&m, ODD, "color.tag"), Some(json!(t)));
}

/// Trace #28: a write of the value already stored is no edit: no undo step, and the document stays clean.
#[test]
fn writing_an_equal_value_leaves_sdc_toml_clean() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(LEAF, "format", json!("bc1"));
    idle(&mut m);
    m.save().unwrap();
    assert!(!m.is_dirty());
    let steps = m.history.undo.len();
    m.set_field(LEAF, "format", json!("bc1"));
    assert_eq!(m.history.undo.len(), steps, "no undo step");
    assert!(!m.is_dirty(), "an equal write does not dirty the document");
    m.set_field(LEAF, "format", json!("bc3"));
    assert!(m.is_dirty(), "a change still dirties");
}

/// Fix D2 M3: a bulk-panel write of the value already stored (the `batch_edit` path) leaves the document clean.
#[test]
fn a_bulk_write_of_an_equal_value_leaves_the_documents_clean() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(LEAF, "format", json!("bc1"));
    idle(&mut m);
    m.save().unwrap();
    assert!(!m.is_dirty());
    let steps = m.history.undo.len();
    m.apply_bulk_edit(&strings(&[LEAF]), "format", BulkOp::Set(json!("bc1")));
    assert_eq!(m.history.undo.len(), steps, "no undo step");
    assert!(!m.is_dirty(), "an equal bulk write does not dirty anything");
    m.apply_bulk_edit(&strings(&[LEAF]), "format", BulkOp::Set(json!("bc3")));
    assert!(m.is_dirty(), "a change still dirties");
}

/// Review C8 (Note FB3, Review Focus 4): the document is clean when its text is the saved text, whichever way it got
/// there. Undo back to the saved state clears the dot; Redo sets it again; an edit after an Undo stays dirty.
#[test]
fn undo_back_to_the_saved_state_is_not_dirty() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(LEAF, "format", json!("bc1"));
    idle(&mut m);
    m.save().unwrap();
    assert!(!m.is_dirty(), "fixture: saved");
    let ks = strings(&[ROCK, ODD]);
    m.apply_bulk_edit(&ks, "format", BulkOp::Set(json!("bc1")));
    assert!(m.is_dirty());
    idle(&mut m);
    m.undo();
    assert!(!m.is_dirty(), "Undo to the saved text is clean");
    idle(&mut m);
    m.redo();
    assert!(m.is_dirty(), "Redo sets it again");
    idle(&mut m);
    m.undo();
    assert!(!m.is_dirty());
    idle(&mut m);
    // An edit, an Undo, then a different edit.
    m.set_field(LEAF, "format", json!("bc3"));
    assert!(m.is_dirty());
    idle(&mut m);
    m.undo();
    assert!(!m.is_dirty());
    idle(&mut m);
    m.apply_bulk_edit(&ks, "format", BulkOp::Set(json!("bc3")));
    assert!(m.is_dirty(), "a different edit after an Undo is dirty");
    idle(&mut m);
    // Saved, then Undo: the text now differs from the file.
    m.save().unwrap();
    assert!(!m.is_dirty());
    m.undo();
    assert!(
        m.is_dirty(),
        "Undo past the saved text is an unsaved change"
    );
}

/// Review C8: the result line belongs to the edit that made it, so Undo and Redo clear it, and so does a new
/// selection.
#[test]
fn the_bulk_notice_clears_on_undo_and_on_selection_change() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let ks = strings(&[ROCK, ODD]);
    m.apply_bulk_edit(&ks, "format", BulkOp::Set(json!("bc1")));
    assert!(m.bulk.report.is_some(), "fixture: a notice");
    idle(&mut m);
    m.undo();
    assert_eq!(m.bulk.report, None, "Undo clears the notice");
    idle(&mut m);
    m.bulk.report = Some("stale".into());
    m.redo();
    assert_eq!(m.bulk.report, None, "Redo clears it too");
    idle(&mut m);
    m.apply_bulk_edit(&ks, "format", BulkOp::Set(json!("bc3")));
    assert!(m.bulk.report.is_some());
    m.click_item(LEAF, false, false);
    m.click_item(ROCK, true, false);
    assert_eq!(m.bulk.report, None, "a new selection starts clean");
}

/// Review C8: a write that reaches some textures and not others leaves the row mixed, at once (the plan is still
/// running and the jobs are the old ones) and after the plan lands.
#[test]
fn a_partial_bulk_write_shows_mixed() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.click_item(ROCK, false, false);
    m.click_item(NORMAL, true, false);
    let ks = m.selected_items();
    assert_eq!(ks.len(), 2);
    let filter = |m: &mut AppModel| {
        let v = m.bulk_view();
        v.rows
            .iter()
            .find(|r| r.field == "mips.filter")
            .cloned()
            .unwrap()
    };
    let before = filter(&mut m);
    assert!(before.value.is_some(), "fixture: both start alike");
    // Kaiser is for sRGB colour: it reaches the albedo and is barred on the normal map.
    m.apply_bulk_edit(&ks, "mips.filter", BulkOp::Set(json!("kaiser")));
    assert_eq!(file(&m, ROCK, "mips.filter"), Some(json!("kaiser")));
    assert_eq!(file(&m, NORMAL, "mips.filter"), None);
    assert_eq!(filter(&mut m).value, None, "mixed while the plan runs");
    idle(&mut m);
    let after = filter(&mut m);
    assert_eq!(after.value, None, "mixed once it lands");
    assert_eq!(after.edited, 1);
}
