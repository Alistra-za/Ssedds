//! Carry 8: the BC1 model is process-global rgbcx state, so a batch asking for a different model than the one the
//! process was initialised with fails cleanly (fatal, exit 3) before converting anything. This test lives in its
//! own binary (its own process) so the mismatched initialisation cannot affect other tests.
use encoders_sys::Bc1Model;
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use image::{ImageBuffer, Rgb};
use std::sync::Arc;

#[test]
fn a_bc1_model_mismatch_is_fatal_before_any_work() {
    encoders_sys::init(Bc1Model::Ideal).unwrap();
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([x as u8 * 8, y as u8 * 8, 40]))
        .save(t.join("rock.png"))
        .unwrap();
    let mut req = BatchRequest::new(vec![t], d.path().join("out"), Target::Cs);
    req.overrides = vec![parse_set("subtype=diffuse").unwrap()];
    req.bc1_model = Bc1Model::Nvidia;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "init");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep).unwrap();
    let err = e.run(plan, &rep, &CancelToken::default()).err().unwrap();
    assert!(matches!(err, EngineError::Fatal(_)), "{err:?}");
    assert_eq!(err.exit_code(), 3);
    assert!(!d.path().join("out").exists());
    assert!(sink
        .take()
        .iter()
        .all(|e| !matches!(e.kind, EventKind::FileStarted)));
}
