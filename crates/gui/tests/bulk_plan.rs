//! M6 Track F3a D2/D5: the bulk plan and the panel's rows share one rule.
mod support;
use sdc_gui::model::bulk::{bulk_fields, plan_bulk_edit, BulkField, BulkOp, Change, NO_SETTINGS};
use sdc_gui::model::AppModel;
use serde_json::json;
use std::time::Duration;
use support::*;

const ROCK: &str = "landscape/rock01.png";
const ODD: &str = "landscape/odd.png";
const NORMAL: &str = "landscape/rock01_normal.png";
const RAW: &str = "landscape/rock01_roughness.png";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn strings(k: &[&str]) -> Vec<String> {
    k.iter().map(|s| s.to_string()).collect()
}

fn row(m: &AppModel, keys: &[String], field: &str) -> BulkField {
    bulk_fields(m, keys)
        .into_iter()
        .find(|f| f.field == field)
        .expect("a panel row")
}

#[test]
fn equal_values_show_and_differing_ones_are_mixed() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let ks = strings(&[ROCK, ODD]);
    m.set_field(ROCK, "encoder.effort", json!("veryfast"));
    idle(&mut m);
    let r = row(&m, &ks, "encoder.effort");
    assert_eq!(
        (r.value.clone(), r.applies, r.edited, r.total),
        (None, 2, 1, 2)
    );
    assert_eq!(r.note(), "— mixed");
    m.set_field(ODD, "encoder.effort", json!("veryfast"));
    idle(&mut m);
    let r = row(&m, &ks, "encoder.effort");
    assert_eq!((r.value.clone(), r.edited), (Some(json!("veryfast")), 2));
    assert_eq!(r.note(), "");
}

#[test]
fn a_locked_field_applies_to_the_rest_and_says_why() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let ks = strings(&[ROCK, NORMAL]);
    assert_eq!(
        plan_bulk_edit(
            &m,
            &ks,
            "mips.normal",
            &BulkOp::Set(json!("renormalize_bake"))
        ),
        vec![
            (ROCK.to_string(), Change::Skip("normal maps only".into())),
            (NORMAL.to_string(), Change::Write),
        ]
    );
    let r = row(&m, &ks, "mips.normal");
    assert_eq!((r.applies, r.total), (1, 2));
    assert_eq!(r.note(), "applies to 1 of 2");
    assert_eq!(r.skipped_text(), "locked on 1 of 2: normal maps only");
}

#[test]
fn a_texture_without_a_job_or_gone_from_the_plan_is_skipped() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    assert!(
        m.item(RAW).is_some_and(|i| i.job.is_none()),
        "fixture: {RAW} is a raw source packed into rock01_rmaos"
    );
    // Review Focus 5: a key that left the plan between drawing and applying.
    let gone = "landscape/gone.png";
    let plan = plan_bulk_edit(
        &m,
        &strings(&[RAW, gone, ROCK]),
        "format",
        &BulkOp::Set(json!("bc1")),
    );
    assert_eq!(
        plan,
        vec![
            (RAW.to_string(), Change::Skip(NO_SETTINGS.into())),
            (gone.to_string(), Change::Skip(NO_SETTINGS.into())),
            (ROCK.to_string(), Change::Write),
        ]
    );
    let r = row(&m, &strings(&[RAW]), "format");
    assert_eq!((r.applies, r.value.clone()), (0, None));
    assert_eq!(r.note(), "applies to 0 of 1");
}

#[test]
fn a_barred_filter_skips_that_texture_on_a_write_only() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    // Kaiser is for sRGB colour only (engine `filter::filter_allowed`): the normal map takes no write of it; a reset
    // reaches both. If pbr_albedo were ever barred too, use `opened()` with ROCK decided `Diffuse` and NORMAL `Normal`.
    let ks = strings(&[ROCK, NORMAL]);
    let set = plan_bulk_edit(&m, &ks, "mips.filter", &BulkOp::Set(json!("kaiser")));
    assert_eq!(set[0], (ROCK.to_string(), Change::Write));
    assert!(
        matches!(&set[1].1, Change::Skip(r) if !r.is_empty()),
        "{set:?}"
    );
    assert!(plan_bulk_edit(&m, &ks, "mips.filter", &BulkOp::Clear)
        .iter()
        .all(|(_, c)| *c == Change::Write));
}
