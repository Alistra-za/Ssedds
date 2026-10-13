//! Shared fixtures for the F2 RNM tests: PNG writer and a planner over `<dir>/in` (recursive, so subfolders work).
#![allow(dead_code)]
use engine::api::*;
use engine::config::*;
use engine::events::*;
use engine::policy::*;
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

pub fn png(dir: &Path, name: &str) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(32, 32, |x, y| Rgb([(x * 8) as u8, (y * 8) as u8, 255u8]))
        .save(p)
        .unwrap();
}

pub fn plan_dir(dir: &Path, sets: Vec<String>) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.accept = AcceptSuggestions::High;
    req.recursive = true;
    for s in sets {
        req.overrides
            .push(engine::overrides::parse_set(&s).unwrap());
    }
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink, "rnm-support");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}
