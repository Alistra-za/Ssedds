//! A cube job previewed through the worker: all six faces come back, decoded as one cross per mip.
use engine::api::{BatchRequest, CancelToken, Engine};
use engine::config::Target;
use engine::events::{Reporter, VecSink};
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use sdc_gui::worker::{spawn_encode, WorkerMsg};
use std::sync::mpsc::channel;
use std::sync::Arc;

#[test]
fn a_cube_preview_decodes_a_cross_per_mip() {
    let out = cube_preview(4096);
    // Six faces, each a 16 -> 1 chain of 5 mips, face-major; one cross (4x3 faces) decoded per mip.
    assert_eq!(
        (out.result.faces, out.result.levels.len(), out.decoded.len()),
        (6, 30, 5)
    );
    assert_eq!(out.decoded[0].len(), 64 * 48);
    assert_eq!(out.decoded[4].len(), 4 * 3);
    // +X sits in cell (2, 1) of the cross, red; -X in cell (0, 1), grey.
    let at = |col: usize, row: usize| out.decoded[0][(row * 16 + 8) * 64 + col * 16 + 8];
    assert!(at(2, 1)[0] > 0.9 && at(2, 1)[1] < 0.1, "+X is red");
    assert!(at(0, 1)[1] > 0.1, "-X is grey");
    assert_eq!(at(0, 0), [0.0; 4], "a corner cell is empty");
}

/// The cross obeys the 2D preview cap: under a cap of 32 a 16-px-face cube previews from the 8-px mip (a 32x24 cross).
#[test]
fn a_cube_preview_keeps_its_cross_within_the_cap() {
    let out = cube_preview(32);
    assert_eq!(out.result.source.first_level, 1);
    assert_eq!(out.decoded.len(), 4, "mips 8, 4, 2, 1");
    assert_eq!(
        out.decoded[0].len(),
        32 * 24,
        "the whole cross fits the cap"
    );
}

fn cube_preview(cap: u32) -> Arc<sdc_gui::model::preview::PreviewOut> {
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
        cap,
        CancelToken::default(),
        1,
    );
    let WorkerMsg::Encoded { result, .. } = rx.recv().unwrap() else {
        panic!("not an Encoded message")
    };
    result.expect("the preview must not fail")
}
