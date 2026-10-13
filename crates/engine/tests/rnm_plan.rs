//! M6 Track F2 Task 4: which base each layer feeds (F2 spec 2.3, 3.3; DECISIONS Note W4, W5, W9).
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::policy::*;
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

fn png(dir: &Path, name: &str) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(32, 32, |x, y| Rgb([(x * 8) as u8, (y * 8) as u8, 255u8]))
        .save(p)
        .unwrap();
}

fn plan_dir(dir: &Path, sets: Vec<String>) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.accept = AcceptSuggestions::High;
    req.recursive = true; // the eye fixture lives in a subfolder
    for s in sets {
        req.overrides
            .push(engine::overrides::parse_set(&s).unwrap());
    }
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink, "rnm-plan");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.ends_with(name))
        .unwrap_or_else(|| panic!("no job {name}"))
}

fn layer_ids(j: &ResolvedJob) -> Vec<(LayerId, String)> {
    j.rnm.as_ref().map_or(vec![], |r| {
        r.layers
            .iter()
            .map(|l| {
                (
                    l.id,
                    l.file
                        .path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                )
            })
            .collect()
    })
}
fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

#[test]
fn plain_layer_feeds_n_with_its_mask() {
    let d = tempfile::tempdir().unwrap();
    for n in [
        "iron.png",
        "iron_n.png",
        "iron_detailnorm.png",
        "iron_detailnormmask.png",
    ] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec![]);
    let n = job(&p, "iron_n.png");
    assert_eq!(
        layer_ids(n),
        vec![(LayerId::Detail, "iron_detailnorm.png".into())]
    );
    assert!(n.rnm.as_ref().unwrap().layers[0]
        .mask
        .as_ref()
        .unwrap()
        .path
        .ends_with("iron_detailnormmask.png"));
    assert!(n.active_rnm().is_some());
}

#[test]
fn n_and_cnr_targets_and_sharing() {
    let d = tempfile::tempdir().unwrap();
    for n in [
        "iron.png",
        "iron_rmaos.png",
        "iron_n.png",
        "iron_cnr.png",
        "iron_bump.png",
        "iron_cnr_detailnorm.png",
    ] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec![]);
    assert_eq!(
        layer_ids(job(&p, "iron_n.png")),
        vec![(LayerId::Bump, "iron_bump.png".into())]
    );
    assert_eq!(
        layer_ids(job(&p, "iron_cnr.png")),
        vec![(LayerId::Detail, "iron_cnr_detailnorm.png".into())]
    );
    let p = plan_dir(d.path(), vec!["file:iron_bump.png.rnm.target=both".into()]);
    assert_eq!(layer_ids(job(&p, "iron_cnr.png")).len(), 2);
}

#[test]
fn shadowed_layer_is_reported_on_the_base() {
    let d = tempfile::tempdir().unwrap();
    for n in [
        "iron.png",
        "iron_rmaos.png",
        "iron_n.png",
        "iron_cnr.png",
        "iron_bump.png",
        "iron_cnr_bump.png",
    ] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec!["file:iron_bump.png.rnm.target=both".into()]);
    let cnr = job(&p, "iron_cnr.png");
    assert_eq!(
        layer_ids(cnr),
        vec![(LayerId::Bump, "iron_cnr_bump.png".into())]
    );
    assert!(codes(cnr).contains(&Code::RnmLayerShadowed));
    assert_eq!(
        layer_ids(job(&p, "iron_n.png")),
        vec![(LayerId::Bump, "iron_bump.png".into())]
    );
}

#[test]
fn plain_layer_falls_back_to_cnr() {
    let d = tempfile::tempdir().unwrap();
    for n in [
        "iron.png",
        "iron_rmaos.png",
        "iron_cnr.png",
        "iron_bump.png",
    ] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec![]);
    assert_eq!(layer_ids(job(&p, "iron_cnr.png")).len(), 1);
}

#[test]
fn no_base_orphan_mask_and_eye() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_bump.png", "iron_bentmask.png"] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec![]);
    assert!(codes(job(&p, "iron_bump.png")).contains(&Code::RnmNoBase));
    assert!(codes(job(&p, "iron_bentmask.png")).contains(&Code::RnmMaskOrphan));
    let d = tempfile::tempdir().unwrap();
    for n in ["eyes/blue.png", "eyes/blue_n.png", "eyes/blue_bump.png"] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec![]);
    assert!(codes(job(&p, "blue_bump.png")).contains(&Code::RnmIgnoredEye));
    assert!(job(&p, "blue_n.png").rnm.is_none());
}

#[test]
fn ao_source_order() {
    let d = tempfile::tempdir().unwrap();
    for n in [
        "iron.png",
        "iron_n.png",
        "iron_bent.png",
        "iron_rmaos.png",
        "iron_ao.png",
    ] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec![]);
    let ao = job(&p, "iron_n.png")
        .rnm
        .as_ref()
        .unwrap()
        .ao
        .clone()
        .unwrap();
    assert!(ao.file.path.ends_with("iron_ao.png") && ao.channel == 0);
    std::fs::remove_file(d.path().join("in/iron_ao.png")).unwrap();
    let p = plan_dir(d.path(), vec![]);
    let ao = job(&p, "iron_n.png")
        .rnm
        .as_ref()
        .unwrap()
        .ao
        .clone()
        .unwrap();
    assert!(ao.file.path.ends_with("iron_rmaos.png") && ao.channel == 2);
    std::fs::remove_file(d.path().join("in/iron_rmaos.png")).unwrap();
    let p = plan_dir(d.path(), vec![]);
    assert!(codes(job(&p, "iron_n.png")).contains(&Code::RnmAoMissing));
    let p = plan_dir(d.path(), vec!["rnm.bent.ao=false".into()]);
    assert!(!codes(job(&p, "iron_n.png")).contains(&Code::RnmAoMissing));
}

#[test]
fn disabled_rnm_keeps_the_list_but_is_inactive() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_n.png", "iron_bump.png"] {
        png(d.path(), n);
    }
    let p = plan_dir(d.path(), vec!["rnm.enabled=false".into()]);
    let n = job(&p, "iron_n.png");
    assert!(n.rnm.is_some() && n.active_rnm().is_none());
}
