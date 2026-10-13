//! Complex grass in the GUI (design `2026-10-12-complex-grass-design.md`, rulings R1, R12).
use engine::config::{Subtype, Target};
use sdc_gui::ui::settings_panel::{offered_groups, subtype_groups};

/// R1 (Review Focus 4): offered under cs and cpm (ENB), not under vanilla; grouped with the packed maps.
#[test]
fn pickers_hide_complex_grass_under_vanilla() {
    for (t, want) in [
        (Target::Cs, true),
        (Target::Cpm, true),
        (Target::Vanilla, false),
    ] {
        let listed = offered_groups(t)
            .into_iter()
            .any(|(_, m)| m.contains(&Subtype::ComplexGrass));
        assert_eq!(listed, want, "{t:?}");
    }
    let groups = subtype_groups(|_| true);
    let (heading, _) = groups
        .iter()
        .find(|(_, m)| m.contains(&Subtype::ComplexGrass))
        .unwrap();
    assert_eq!(*heading, "Packed and data");
}

mod support;
use std::time::Duration;

/// A `w` x `2w` grass PNG (as the engine tests' `grass`): top colour with cutout dots, bottom unit normals, alpha 12.
fn grass_png(path: &std::path::Path, w: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::ImageBuffer::from_fn(w, 2 * w, |x, y| {
        if y < w {
            image::Rgba([
                64u8,
                128,
                38,
                if (x * 7 + y * 13) % 10 < 3 { 255 } else { 0 },
            ])
        } else {
            let t = (x as f32 / w as f32 - 0.5) * 0.8;
            let c = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
            image::Rgba([c(t.sin()), 128, c(t.cos()), 12])
        }
    })
    .save(path)
    .unwrap();
}

/// A temp scan DB holding one GRAS record whose model `meshes/plants/tundra.nif` uses `tex` in slot 0. NIF rows go
/// through the store API; the scanner's plugin rows are written as the scanner would (schema v4).
fn gras_db(dir: &std::path::Path, tex: &str) -> std::path::PathBuf {
    use modscan::store::{CopyResult, OriginSpec, Store, KIND_MOD};
    let p = dir.join("scan.db");
    let mut s = Store::open(&p).unwrap();
    s.begin_scan().unwrap();
    let o = s
        .upsert_origin(&OriginSpec {
            kind: KIND_MOD,
            name: "Grass".into(),
            root: dir.join("mods/Grass").to_string_lossy().into_owned(),
            parent: None,
            priority: 0,
            derived_by: None,
            derived_state: None,
        })
        .unwrap();
    let mesh = "meshes/plants/tundra.nif";
    let scan = nif::scan::NifScan {
        bsver: 100,
        le_mesh: false,
        shapes: vec![lighting_shape(3, &[tex])],
        problems: vec![],
    };
    let id = s
        .upsert_copy(mesh, o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap();
    let pid = s.path_ids(&[mesh]).unwrap()[0];
    s.set_winners(&[(pid, (id, 1))].into_iter().collect(), &Default::default())
        .unwrap();
    let c = s.conn();
    c.execute(
        "INSERT INTO plugin (id, name, path, size, mtime, lo_index, light, flags, masters_json)
         VALUES (1, 'Skyrim.esm', 'Skyrim.esm', 1, 1, 0, 0, 1, '[]')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO rec (formid, local, plugin_id, sig, edid, owner_ok) VALUES (2561, 2561, 1, 'GRAS', 'Tundra', 1)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO rec_winner (formid, plugin_id, n_overrides) VALUES (2561, 1, 0)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO model_ref (formid, local, plugin_id, slot, path_id) VALUES (2561, 2561, 1, 0, ?1)",
        [pid],
    )
    .unwrap();
    p
}

/// `set_preview_model.rs`'s lighting shape (block `block`, slots `slots`), copied: test binaries cannot share it.
fn lighting_shape(block: u32, slots: &[&str]) -> nif::scan::ShapeRow {
    use nif::props::{FieldOffsets, Lighting, LightingParams, LightingTrailer};
    use nif::shape_head::{ShapeHead, ShapeKind};
    nif::scan::ShapeRow {
        block,
        head: ShapeHead {
            kind: ShapeKind::BSTriShape,
            name: Some("Grass".into()),
            skinned: false,
            shader: 1,
            alpha: -1,
            vertex_desc: None,
            num_triangles: Some(1),
            num_vertices: Some(3),
        },
        shader: nif::scan::Shader::Lighting(
            Lighting {
                name: None,
                shader_type: 0,
                slsf1: 0,
                slsf2: 0,
                texture_set: 2,
                params: LightingParams {
                    uv_offset: [0.0; 2],
                    uv_scale: [1.0; 2],
                    emissive_color: [0.0; 3],
                    emissive_multiple: 1.0,
                    texture_clamp_mode: 3,
                    alpha: 1.0,
                    refraction_strength: 0.0,
                    glossiness: 80.0,
                    specular_color: [1.0; 3],
                    specular_strength: 1.0,
                    lighting_effect_1: 0.3,
                    lighting_effect_2: 2.0,
                    trailer: LightingTrailer::None,
                },
                field_offsets: FieldOffsets {
                    block_offset: 0,
                    slsf1: 0,
                    uv_offset: 0,
                    emissive_color: 0,
                    glossiness: 0,
                    trailer: 0,
                },
            },
            slots
                .iter()
                .map(|p| nif::path::canon_texture(p.as_bytes()))
                .collect(),
        ),
        alpha: None,
    }
}

/// Design §5 end to end in the model: the scan names the texture as a GRAS diffuse, the next plan tags it.
#[test]
fn a_gras_diffuse_from_the_scan_db_is_tagged_after_the_plan() {
    let d = tempfile::tempdir().unwrap();
    grass_png(&d.path().join("textures/plants/g.png"), 32);
    let mut m = support::opened(d.path());
    m.accept = engine::policy::AcceptSuggestions::High;
    let key = "plants/g.png";
    let subtype = |m: &sdc_gui::model::AppModel| {
        m.items
            .iter()
            .find(|i| i.key == key)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype)
    };
    assert_ne!(
        subtype(&m),
        Some(Subtype::ComplexGrass),
        "no scan DB, no tag"
    );
    let dbdir = tempfile::tempdir().unwrap();
    m.scan_db = Some(gras_db(dbdir.path(), r"textures\plants\g.dds"));
    m.replan();
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    assert_eq!(
        m.gras_diffuses,
        ["textures/plants/g".to_string()].into_iter().collect()
    );
    assert_eq!(m.request().unwrap().gras_diffuses, m.gras_diffuses);
    assert_eq!(subtype(&m), Some(Subtype::ComplexGrass));
}

/// 2026-10-13: a tagged grass file is one Diffuse-slot card, filled, and the set auto-picks the complex_grass shader.
#[test]
fn a_grass_set_shows_one_filled_card_under_the_complex_grass_shader() {
    use sdc_gui::model::sets::{set_views, slot_row, SlotFill};
    let d = tempfile::tempdir().unwrap();
    grass_png(&d.path().join("textures/plants/g.png"), 32);
    let mut m = support::opened(d.path());
    m.accept = engine::policy::AcceptSuggestions::High;
    let dbdir = tempfile::tempdir().unwrap();
    m.scan_db = Some(gras_db(dbdir.path(), r"textures\plants\g.dds"));
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let v = set_views(&m)
        .into_iter()
        .find(|v| v.members.iter().any(|k| k == "plants/g.png"))
        .expect("the grass set");
    assert_eq!(
        v.shader.as_ref().map(|p| p.shader),
        Some(engine::shader::Shader::ComplexGrass)
    );
    let row = slot_row(v.shader.as_ref(), &v.slots);
    assert_eq!(row.len(), 1, "{row:?}");
    assert!(row[0].usable && matches!(row[0].fill, SlotFill::Own { .. }));
}
