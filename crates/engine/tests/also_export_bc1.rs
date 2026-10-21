//! Track A Task 7 (DECISIONS Note CN): the PBR folder of an Also export equals a plain export byte for byte, with
//! `bc1_model = Nvidia` (not the default Ideal, so a dropped model would show). Its own binary: the BC1 model is
//! process-global rgbcx state (see `api_init.rs`).
mod also_support;
use also_support::*;
use engine::api::*;
use engine::events::*;
use engine::manifest::Manifest;
use engine::overrides::parse_set;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

#[test]
fn the_pbr_folder_is_byte_identical_to_a_plain_export() {
    let (d, m, loc) = project(|m| {
        steel(m);
        cup(m);
        plate(m);
    });
    let mut req = request(&m, &loc);
    req.bc1_model = encoders_sys::Bc1Model::Nvidia;
    req.overrides
        .push(parse_set("file:**/cup.png.format=bc1").unwrap());
    let s = also(req.clone(), &spec(&loc, BOTH)).0.unwrap();
    assert_eq!(s.exit_code(), 0, "{s:#?}");
    let pbr = loc.join("Mod PBR");
    let plain = d.path().join("plain");
    req.output_root = plain.clone();
    let e = Engine::new().unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "t");
    let p = e.plan(req, &rep, &CancelToken::default()).unwrap();
    e.run(p, &rep, &CancelToken::default()).unwrap();
    let files = dds_files(&plain);
    assert!(files.contains("textures/clutter/cup.dds"), "{files:?}");
    assert_eq!(dds_files(&pbr), files);
    for f in &files {
        assert_eq!(
            std::fs::read(pbr.join(f)).unwrap(),
            std::fs::read(plain.join(f)).unwrap(),
            "{f}"
        );
    }
    let strip = |folder: &Path| -> BTreeMap<String, serde_json::Value> {
        Manifest::at(folder)
            .read()
            .unwrap()
            .into_iter()
            .map(|(k, e)| {
                let mut v = e.raw;
                v.as_object_mut().unwrap().remove("ts");
                v.as_object_mut().unwrap().remove("output_stamp");
                (k, v)
            })
            .collect()
    };
    let (a, b) = (strip(&pbr), strip(&plain));
    assert_eq!(a, b);
    assert_eq!(a["textures/clutter/cup.dds"]["bc1_model"], "nvidia");
}
