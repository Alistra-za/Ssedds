//! A cube job previewed through the worker: the GUI shows +X only until Task 5 draws the cross.
use engine::api::{BatchRequest, CancelToken, Engine};
use engine::config::Target;
use engine::events::{Reporter, VecSink};
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use sdc_gui::worker::{spawn_encode, WorkerMsg};
use std::sync::mpsc::channel;
use std::sync::Arc;

#[test]
fn a_cube_preview_shows_one_face_without_panicking() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/a");
    std::fs::create_dir_all(&dir).unwrap();
    // 6:1 strip of 16-px faces: +X red, the rest grey.
    let strip = image::RgbImage::from_fn(96, 16, |x, _| {
        if x < 16 {
            image::Rgb([255, 0, 0])
        } else {
            image::Rgb([128, 128, 128])
        }
    });
    strip.save(dir.join("chrome_e.png")).unwrap();
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    r.overrides = vec![parse_set("format=rgba8").unwrap()];
    r.accept = AcceptSuggestions::High;
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    let plan = Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap();
    let session = Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap());
    let (tx, rx) = channel();
    spawn_encode(
        tx,
        session,
        plan.jobs[0].clone(),
        4096,
        CancelToken::default(),
        1,
    );
    let WorkerMsg::Encoded { result, .. } = rx.recv().unwrap() else {
        panic!("not an Encoded message")
    };
    let out = result.expect("the preview must not fail");
    // One face's 16 -> 1 chain: 5 mips.
    assert_eq!(
        (out.result.faces, out.result.levels.len(), out.decoded.len()),
        (1, 5, 5)
    );
    assert!(out.decoded[0][0][0] > 0.9, "face 0 is +X red");
}
