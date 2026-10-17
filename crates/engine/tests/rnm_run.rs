//! M6 Track F2 Task 6: blended normals are written, re-encode on any RNM edit and keep `_cnr` alpha
//! (F2 spec §6, Review Focus 1, 3, 4).
use engine::api::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

mod rnm_support;
use rnm_support::{plan_dir, png};

fn run(plan: Plan) -> BatchSummary {
    let rep = Reporter::new(Arc::new(VecSink::default()), "rnm-run");
    Engine::new()
        .unwrap()
        .run(plan, &rep, &CancelToken::default())
        .unwrap()
}

/// A w×h ripple normal; `h` changes the phase so a rewrite changes bytes.
fn detail(dir: &Path, name: &str, w: u32, h: u32) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([
            (128.0 + 60.0 * (x as f32 + h as f32).sin()) as u8,
            (128.0 + 60.0 * (y as f32).cos()) as u8,
            230u8,
        ])
    })
    .save(p)
    .unwrap();
}

/// Mip 0 of `out/<name>` decoded to RGBA8.
fn texels(dir: &Path, name: &str) -> Vec<[u8; 4]> {
    let f = read_dds(&std::fs::read(dir.join("out").join(name)).unwrap()).unwrap();
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    px
}

#[test]
fn layers_change_the_normal_and_only_n_is_written() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_n.png"] {
        png(d.path(), n);
    }
    run(plan_dir(d.path(), vec![]));
    let plain = texels(d.path(), "iron_n.dds");
    detail(d.path(), "iron_detailnorm.png", 32, 32);
    let s = run(plan_dir(d.path(), vec![]));
    assert_eq!(s.converted, 1, "only the _n re-encodes");
    assert_ne!(texels(d.path(), "iron_n.dds"), plain);
    assert!(!d.path().join("out/iron_detailnorm.dds").exists());
}

/// Review Focus 1: each edit re-encodes; an unrelated edit does not.
#[test]
fn edits_re_encode() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_n.png", "iron_detailnormmask.png"] {
        png(d.path(), n);
    }
    detail(d.path(), "iron_detailnorm.png", 32, 32);
    run(plan_dir(d.path(), vec![]));
    for set in [
        "rnm.detail.strength=0.9",
        "rnm.detail.flip_green=true",
        "rnm.detail.mask=false",
        "rnm.enabled=false",
        "rnm.order=[\"detail\",\"bent\"]",
    ] {
        let s = run(plan_dir(d.path(), vec![set.into()]));
        assert_eq!(s.converted, 1, "{set}");
        let s = run(plan_dir(d.path(), vec![]));
        assert_eq!(s.converted, 1, "{set} back");
    }
    // rnm.order with the default order is the default: no re-encode (W6).
    let s = run(plan_dir(
        d.path(),
        vec!["rnm.order=[\"bent\",\"bump\"]".into()],
    ));
    assert_eq!(s.converted, 0);
    // Touching the layer file re-encodes.
    detail(d.path(), "iron_detailnorm.png", 32, 31);
    assert_eq!(run(plan_dir(d.path(), vec![])).converted, 1);
}

/// Review Focus 3. `rgba8` stores exact alpha bytes; BC7 could move alpha a code when only RGB changes.
#[test]
fn cnr_alpha_is_untouched() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_rmaos.png"] {
        png(d.path(), n);
    }
    let cnr = d.path().join("in/iron_cnr.png");
    image::RgbaImage::from_fn(32, 32, |x, y| {
        image::Rgba([128, 128, 255, (x * 7 + y * 3) as u8])
    })
    .save(&cnr)
    .unwrap();
    let sets = || vec!["mips.levels=1".to_string(), "format=rgba8".to_string()];
    assert_eq!(run(plan_dir(d.path(), sets())).failed, 0);
    let rgb_before = texels(d.path(), "iron_cnr.dds");
    detail(d.path(), "iron_cnr_detailnorm.png", 32, 32);
    assert_eq!(run(plan_dir(d.path(), sets())).failed, 0);
    let now = texels(d.path(), "iron_cnr.dds");
    assert_ne!(now, rgb_before, "the layer must have changed RGB");
    let a = |v: &[[u8; 4]]| v.iter().map(|p| p[3]).collect::<Vec<_>>();
    assert_eq!(a(&rgb_before), a(&now));
}

/// Review Focus 4.
#[test]
fn odd_sizes_convert() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_n.png"] {
        png(d.path(), n);
    }
    detail(d.path(), "iron_detailnorm.png", 96, 40);
    detail(d.path(), "iron_bump.png", 7, 9);
    let s = run(plan_dir(d.path(), vec![]));
    assert_eq!(s.failed, 0);
    let dds = read_dds(&std::fs::read(d.path().join("out/iron_n.dds")).unwrap()).unwrap();
    assert_eq!((dds.width, dds.height), (32, 32));
}

/// Fix I1: the AO channel is part of the source identity. Same bytes as `_rmaos` (B) then `_ao` (R) must re-encode.
#[test]
fn ao_channel_swap_re_encodes() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron_n.png", "iron_bent.png"] {
        png(d.path(), n);
    }
    let rmaos = d.path().join("in/iron_rmaos.png");
    image::RgbaImage::from_fn(32, 32, |x, y| {
        image::Rgba([(x * 8) as u8, 10, (255 - y * 8) as u8, 255])
    })
    .save(&rmaos)
    .unwrap();
    assert_eq!(run(plan_dir(d.path(), vec![])).failed, 0);
    let bytes = std::fs::read(&rmaos).unwrap();
    std::fs::remove_file(&rmaos).unwrap();
    std::fs::write(d.path().join("in/iron_ao.png"), bytes).unwrap();
    let s = run(plan_dir(d.path(), vec![]));
    // The new `_ao` also packs into a fresh `_rmaos`, so the bent `_n` is one of the re-encodes, not the only one.
    assert!(s.converted >= 1);
    let m = engine::manifest::Manifest::at(&d.path().join("out"))
        .load()
        .unwrap();
    let roles: Vec<_> = m["iron_n.dds"]
        .sources
        .iter()
        .filter_map(|r| r.rnm.as_deref())
        .collect();
    assert_eq!(roles, ["bent", "bent.ao.r"]);
}

/// Review Focus 1: a target edit re-encodes the `_cnr` that gains the layer, and only it.
#[test]
fn target_edit_re_encodes_the_cnr() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_rmaos.png", "iron_n.png", "iron_bump.png"] {
        png(d.path(), n);
    }
    let cnr = d.path().join("in/iron_cnr.png");
    image::RgbaImage::from_fn(32, 32, |_, _| image::Rgba([128, 128, 255, 200]))
        .save(&cnr)
        .unwrap();
    run(plan_dir(d.path(), vec![]));
    let s = run(plan_dir(
        d.path(),
        vec!["file:iron_bump.png.rnm.target=both".into()],
    ));
    assert_eq!(s.converted, 1);
}

/// Trace-debug #25 (Note BQ): RNM layer and mask files are never converted on their own, so the shader map check
/// does not call them extra maps that are "still converted".
#[test]
fn rnm_layers_get_no_extra_map_warning() {
    let d = tempfile::tempdir().unwrap();
    for n in ["iron.png", "iron_n.png", "iron_detailnormmask.png"] {
        png(d.path(), n);
    }
    detail(d.path(), "iron_detailnorm.png", 32, 32);
    let mut req = BatchRequest::new(
        vec![d.path().join("in")],
        d.path().join("out"),
        engine::config::Target::Cs,
    );
    req.accept = engine::policy::AcceptSuggestions::High;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "rnm-extra");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    let events: Vec<String> = sink
        .take()
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect();
    assert!(
        events.iter().any(|e| e.contains("set_resolved")),
        "no set_resolved event"
    );
    let extra: Vec<&String> = events
        .iter()
        .filter(|e| e.contains("SHADER-EXTRA-MAP") && e.contains("detailnorm"))
        .collect();
    assert!(extra.is_empty(), "{extra:?}");
}
