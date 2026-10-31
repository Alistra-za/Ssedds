//! Note FC13 (CL1/CL2): a sub-type a `[files]` entry gives a file with no suffix picks its analysis hints, so the file
//! is measured again for its new type.
mod support;
use engine::config::Subtype;
use sdc_gui::model::Answer;
use std::time::Duration;
use support::*;

#[test]
fn a_files_subtype_on_a_suffixless_file_measures_it_again() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let odd = m
        .items
        .iter()
        .find(|i| i.name == "odd.png")
        .unwrap()
        .key
        .clone();
    let curl = |m: &sdc_gui::model::AppModel| {
        m.item(&odd)
            .and_then(|i| i.analysis.as_ref())
            .and_then(|a| a.checks.as_ref())
            .map(|c| c.curl.is_some())
    };
    assert_eq!(curl(&m), Some(false), "a plain file: no curl measured");
    m.decide(&odd, Answer::Subtype(Subtype::Normal));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(curl(&m), Some(true), "typed as a normal: measured again");
    m.decide(&odd, Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(curl(&m), Some(false), "and back");
}
