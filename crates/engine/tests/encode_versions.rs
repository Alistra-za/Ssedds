//! Per-sub-type encode versions (DECISIONS Note DL): the guard that fails when a sub-type's encode input changes
//! without its `encode_version` arm being bumped.
use engine::analyse::code8;
use engine::api::*;
use engine::config::{Subtype, Target};
use engine::dds::write_atomic;
use engine::events::*;
use engine::image::Rgba32F;
use engine::manifest::{encode_version, ENGINE_VERSION, RELEASED_ENGINE_VERSION};
use engine::policy::AcceptSuggestions;
use image::{DynamicImage, ImageBuffer, Rgb, Rgba};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

/// Sub-types no forced fixture can encode, with the reason. Each one is also listed in Note DL.
const UNGUARDED: &[(Subtype, &str)] = &[];

/// Checks `observed` (output key -> (sub-type, hash)) against the pins in `file` and rewrites the file when the
/// pinned map changes. A new key is pinned; a changed hash is accepted only when `bumped(subtype)`, else the old
/// hash stays and a failure is returned; keys no longer observed are dropped.
fn check_pins(
    file: &Path,
    observed: BTreeMap<String, (Subtype, String)>,
    what: &str,
    bumped: &dyn Fn(Subtype) -> bool,
) -> Vec<String> {
    let old: BTreeMap<String, Value> = match std::fs::read_to_string(file) {
        Ok(s) => serde_json::from_str(&s).unwrap(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => panic!("{}: {e}", file.display()),
    };
    let mut pins = BTreeMap::new();
    let mut failures = vec![];
    for (key, (st, sha)) in observed {
        let id = st.id();
        match old.get(&key) {
            Some(was) if was["sha256"] != sha.as_str() && !bumped(st) => {
                failures.push(format!(
                    "{id} output changed ({what} of {key}): set its encode_version arm in \
                     crates/engine/src/manifest.rs to ENGINE_VERSION (and bump the engine version if it is \
                     still the released version)"
                ));
                pins.insert(key, was.clone());
            }
            _ => {
                pins.insert(key, json!({ "subtype": id, "sha256": sha }));
            }
        }
    }
    // Compare the parsed map, not the bytes: the checkout is CRLF.
    if pins != old {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = serde_json::to_string_pretty(&pins).unwrap() + "\n";
        write_atomic(file, text.as_bytes()).unwrap();
    }
    failures
}

fn bumped(st: Subtype) -> bool {
    encode_version(st) == ENGINE_VERSION && ENGINE_VERSION != RELEASED_ENGINE_VERSION
}

fn put(dir: &Path, rel: &str, img: DynamicImage) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    img.save(p).unwrap();
}

/// 64×64 RGBA gradient with varying alpha.
fn rgba64() -> DynamicImage {
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(64, 64, |x, y| {
        Rgba([(x * 4) as u8, (y * 4) as u8, 90, ((x + y) * 2) as u8])
    }))
}

/// Float source for the sub-types that need one.
fn hdr64() -> DynamicImage {
    DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |x, y| {
        Rgb([4.0 * x as f32 / 63.0, 2.0 * y as f32 / 63.0, 0.25])
    }))
}

/// A 64×128 complex-grass source as complex_grass.rs builds one: top = colour with a cutout alpha, bottom = unit
/// normals with specular in alpha, so VAL-GRASS-DETECT passes.
fn grass64() -> DynamicImage {
    let w = 64;
    let mut img = Rgba32F::new(w, 2 * w, [0.0; 4]);
    for y in 0..2 * w {
        for x in 0..w {
            *img.at_mut(x, y) = if y < w {
                let a = if (x * 7 + y * 13) % 10 < 3 { 1.0 } else { 0.0 };
                [0.25, 0.5, 0.15, a]
            } else {
                let t = (x as f32 / w as f32 - 0.5) * 0.8;
                [t.sin() * 0.5 + 0.5, 0.5, t.cos() * 0.5 + 0.5, 12.0 / 255.0]
            };
        }
    }
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(img.width, img.height, |x, y| {
        Rgba(img.at(x, y).map(code8))
    }))
}

fn rgb(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, y| {
        Rgb([(x * 4) as u8, (y * 4) as u8, 90])
    }))
}

fn flat_normal(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |_, _| Rgb([128, 128, 255])))
}

/// golden.rs `pbr_fixture`: a cs PBR set with its `_rmaos` and the roughness variance bake.
fn pbr_fixture() -> Vec<(&'static str, DynamicImage)> {
    vec![
        ("textures/pbr/rock.png", rgb(16)),
        ("textures/pbr/rock_n.png", flat_normal(16)),
        ("textures/pbr/rock_rmaos.png", rgb(16)),
    ]
}

/// rnm_bake.rs: an RNM base `_n` with one detail layer, plus the `_rmaos` its bake reads.
fn rnm_fixture(dir: &Path) {
    let p = |n: &str| format!("textures/rnm/{n}");
    put(
        dir,
        &p("iron.png"),
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(32, 32, |x, y| {
            Rgb([(x * 8) as u8, (y * 8) as u8, 255])
        })),
    );
    put(
        dir,
        &p("iron_n.png"),
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(32, 32, |_, _| {
            Rgba([128, 128, 255, 255])
        })),
    );
    put(
        dir,
        &p("iron_rmaos.png"),
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(32, 32, |_, _| {
            Rgba([128, 0, 255, 255])
        })),
    );
    put(
        dir,
        &p("iron_detailnorm.png"),
        DynamicImage::ImageRgb8(ImageBuffer::from_fn(32, 32, |x, y| {
            Rgb([
                (128.0 + 100.0 * (x as f32 * 1.7).sin()) as u8,
                (128.0 + 100.0 * (y as f32 * 1.3).cos()) as u8,
                200,
            ])
        })),
    );
}

#[test]
fn encode_inputs_are_pinned_or_their_type_was_bumped() {
    let d = tempfile::tempdir().unwrap();
    let hooks = tempfile::tempdir().unwrap();
    let mut sets = vec![];
    for st in Subtype::ALL {
        use Subtype::*;
        let id = st.id();
        let (file, img) = match st {
            HdrSky => (format!("{id}.hdr"), hdr64()),
            ComplexGrass => (format!("{id}.png"), grass64()),
            // These store no alpha: an alpha source is blocked by LOSS-ALPHA-DATA.
            PbrEmissive | Glow | SubsurfaceTint | BacklightTint | Msn | Envmask | Flow | Height
            | MsnSpecular | GenericTwoChannel | SkinSk => (format!("{id}.png"), rgb(64)),
            _ => (format!("{id}.png"), rgba64()),
        };
        put(d.path(), &format!("textures/h/{file}"), img);
        sets.push(format!("file:**/h/{file}.subtype={id}"));
        // The mesh decides these alphas, so a forced sub-type still needs the meaning given.
        if st.alpha_from_mesh() && st != ComplexGrass {
            sets.push(format!("file:**/h/{file}.alpha.policy=blend"));
        }
    }
    for (rel, img) in pbr_fixture() {
        put(d.path(), rel, img);
    }
    rnm_fixture(d.path());

    let mut req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.on_existing = OnExisting::Overwrite;
    req.overrides = sets
        .iter()
        .map(|s| engine::overrides::parse_set(s).unwrap())
        .collect();
    // The only test in this binary that encodes, so setting the process-wide variable is safe.
    std::env::set_var("SDC_TEST_ENCODE_INPUT_DIR", hooks.path());
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "encode-versions");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let summary = e.run(plan, &rep, &CancelToken::default()).unwrap();
    std::env::remove_var("SDC_TEST_ENCODE_INPUT_DIR");
    let failed: Vec<_> = sink
        .take()
        .into_iter()
        .filter(|e| {
            matches!(
                e.kind,
                EventKind::FileFailed { .. } | EventKind::FileSkipped { .. }
            )
        })
        .collect();
    // Every fixture encodes and passes validation, so each pin covers the passing path (complex_grass included).
    assert_eq!(
        (summary.failed, summary.skipped, summary.validation_failed),
        (0, 0, 0),
        "{failed:#?}"
    );

    let mut observed = BTreeMap::new();
    for f in std::fs::read_dir(hooks.path()).unwrap() {
        let f = f.unwrap().path();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let st: Subtype = serde_json::from_value(v["subtype"].clone()).unwrap();
        let name = f.file_name().unwrap().to_str().unwrap();
        let key = name.strip_suffix(".json").unwrap().replace('~', "/");
        observed.insert(key, (st, v["sha256"].as_str().unwrap().to_string()));
    }
    let seen: BTreeSet<Subtype> = observed.values().map(|(st, _)| *st).collect();
    let want: BTreeSet<Subtype> = Subtype::ALL
        .into_iter()
        .filter(|st| UNGUARDED.iter().all(|(u, _)| u != st))
        .collect();
    assert_eq!(seen, want, "sub-types with an encode-input pin");

    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/encode_versions/encode_inputs.json");
    let failures = check_pins(&file, observed, "encode input", &bumped);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn check_pins_fails_an_unbumped_change_and_accepts_a_bumped_one() {
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("pins.json");
    let read =
        || -> Value { serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap() };
    let normal =
        |h: &str| BTreeMap::from([("h/normal.dds".to_string(), (Subtype::Normal, h.to_string()))]);
    std::fs::write(
        &file,
        r#"{"h/normal.dds": {"subtype":"normal","sha256":"aa"}}"#,
    )
    .unwrap();

    let f = check_pins(&file, normal("bb"), "encode input", &|_| false);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].contains("normal output changed"), "{f:?}");
    assert_eq!(read()["h/normal.dds"]["sha256"], "aa");

    let f = check_pins(&file, normal("bb"), "encode input", &|_| true);
    assert!(f.is_empty(), "{f:?}");
    assert_eq!(read()["h/normal.dds"]["sha256"], "bb");

    let mut both = normal("bb");
    both.insert("h/msn.dds".into(), (Subtype::Msn, "cc".into()));
    let f = check_pins(&file, both, "encode input", &|_| false);
    assert!(f.is_empty(), "{f:?}");
    assert_eq!(
        read()["h/msn.dds"],
        json!({"subtype": "msn", "sha256": "cc"})
    );

    let f = check_pins(&file, normal("bb"), "encode input", &|_| false);
    assert!(f.is_empty(), "{f:?}");
    assert!(read().get("h/msn.dds").is_none(), "unobserved key dropped");
}

/// Adding a `Profile` field must not change any built-in preset's `profile_sha256`, or `--on-existing if-changed`
/// re-encodes everything. Optional fields need `skip_serializing_if` so they vanish from the hash while unset.
#[test]
fn builtin_profile_hashes_are_pinned() {
    let reg = engine::presets::PresetRegistry::builtin().unwrap();
    let mut ids: Vec<String> = reg.selectable().iter().map(|p| p.id.clone()).collect();
    ids.push("sdc-base".into());
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 6, "a new built-in preset: check it is pinned");
    let mut observed = BTreeMap::new();
    for id in &ids {
        for (target, tid) in [(Target::Cs, "cs"), (Target::Vanilla, "vanilla")] {
            for st in Subtype::ALL {
                if let Ok(p) = reg.profile(id, target, st) {
                    let sha = engine::manifest::profile_sha256(&p).unwrap();
                    observed.insert(format!("{id}/{tid}/{}", st.id()), (st, sha));
                }
            }
        }
    }
    assert!(observed.keys().any(|k| k.starts_with("cs-quality/cs/")));
    // Every id x target x sub-type resolves today; Err stays a skip for a combination that later becomes invalid.
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/encode_versions/profile_hashes.json");
    let failures = check_pins(&file, observed, "profile_sha256", &bumped);
    assert!(
        failures.is_empty(),
        "{}\nif a new Profile field caused it, make the field `#[serde(default, skip_serializing_if = ...)]` instead",
        failures.join("\n")
    );
}
