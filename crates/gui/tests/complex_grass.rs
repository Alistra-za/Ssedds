//! Complex grass in the GUI (design `2026-10-12-complex-grass-design.md`, rulings R1, R12).
use engine::config::{Subtype, Target};
use sdc_gui::ui::settings_panel::{offered_groups, subtype_groups};

/// DECISIONS X3 (2026-10-13): no picker offers complex grass on any target; it is grouped with the packed maps.
#[test]
fn pickers_never_offer_complex_grass() {
    for (t, want) in [(Target::Cs, false), (Target::Vanilla, false)] {
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
        index3d: Default::default(),
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

/// 2026-10-13: the set shader drives the sub-type. complex_grass locks the member's Sub-type row; going back to
/// auto returns the ordinary one.
#[test]
fn the_set_shader_makes_the_member_complex_grass_and_locks_it() {
    use engine::shader::Shader;
    use sdc_gui::model::{fields::field_views, sets::set_views};
    let d = tempfile::tempdir().unwrap();
    grass_png(&d.path().join("textures/plants/g.png"), 32);
    let mut m = support::opened(d.path());
    let set = set_views(&m)
        .into_iter()
        .find(|v| v.members.iter().any(|k| k == "plants/g.png"))
        .expect("the set")
        .id;
    let row = |m: &sdc_gui::model::AppModel| {
        let job = m
            .items
            .iter()
            .find(|i| i.key == "plants/g.png")
            .and_then(|i| i.job.clone())
            .expect("job");
        let v = field_views(m.target, &job)
            .into_iter()
            .find(|f| f.field == "subtype")
            .unwrap();
        (job.profile.subtype, v.lock)
    };
    assert_ne!(row(&m).0, Subtype::ComplexGrass);
    m.set_shader(&set, Some(Shader::ComplexGrass));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let (st, lock) = row(&m);
    assert_eq!(st, Subtype::ComplexGrass);
    assert!(
        lock.is_some_and(|l| l.contains("Complex grass")),
        "{lock:?}"
    );
    m.set_shader(&set, Some(Shader::Landscape));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(row(&m), (Subtype::LandscapeDiffuse, None));
    m.set_shader(&set, None);
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(row(&m).1, None);
    assert_ne!(row(&m).0, Subtype::ComplexGrass);
}

/// A hand-tagged complex_grass file (old `[files]` entry) keeps an editable row; no picker lists it unless it is
/// the current value.
#[test]
fn a_hand_tag_stays_editable_and_pickers_list_complex_grass_only_as_the_current_value() {
    use sdc_gui::model::fields::field_views;
    let d = tempfile::tempdir().unwrap();
    grass_png(&d.path().join("textures/plants/g.png"), 32);
    let mut m = support::opened(d.path());
    m.set_field(
        "plants/g.png",
        "subtype",
        serde_json::json!("complex_grass"),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m
        .items
        .iter()
        .find(|i| i.key == "plants/g.png")
        .and_then(|i| i.job.clone())
        .unwrap();
    assert_eq!(job.profile.subtype, Subtype::ComplexGrass);
    let v = field_views(m.target, &job)
        .into_iter()
        .find(|f| f.field == "subtype")
        .unwrap();
    assert_eq!(v.lock, None);
    for t in [Target::Cs, Target::Vanilla] {
        assert!(offered_groups(t)
            .iter()
            .all(|(_, m)| !m.contains(&Subtype::ComplexGrass)));
    }
}

/// Task 9 review I2 (Z11): a sub-type change no step caused (the scan DB names the file a GRAS diffuse) clears an
/// explicit encoding complex grass does not allow in its own "Encoding follows type" step; the last user step is
/// left as it was.
#[test]
fn a_scan_vote_retype_cleans_up_in_its_own_step() {
    use engine::config::{ColorTag, Format};
    let d = tempfile::tempdir().unwrap();
    grass_png(&d.path().join("textures/plants/g.png"), 32);
    let mut m = support::opened(d.path());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.set_format("plants/g.png", Format::Bc7, ColorTag::Srgb);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let last = m.history.undo.last().cloned().expect("a step");
    let dbdir = tempfile::tempdir().unwrap();
    m.scan_db = Some(gras_db(dbdir.path(), r"textures\plants\g.dds"));
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value("plants/g.png", "format"), None);
    let n = m.history.undo.len();
    assert_eq!(m.history.undo[n - 1].label, "Encoding follows type");
    assert_eq!(
        m.history.undo[n - 2],
        last,
        "the last user step is unchanged"
    );
}

/// PBR grass design §4.2, R5: a TruePBR GRAS shape is evidence for pbr_grass, and none for complex grass.
#[test]
fn a_pbr_set_whose_albedo_a_truepbr_gras_shape_uses_picks_pbr_grass() {
    use engine::shader::{AutoRule, Shader, ShaderOrigin};
    let d = tempfile::tempdir().unwrap();
    for (name, px) in [
        ("g.png", [64u8, 128, 38]),
        ("g_n.png", [128, 128, 255]),
        ("g_rmaos.png", [128, 0, 255]),
    ] {
        let p = d.path().join("textures/plants").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px))
            .save(p)
            .unwrap();
    }
    let mut m = support::opened(d.path());
    m.accept = engine::policy::AcceptSuggestions::High;
    let dbdir = tempfile::tempdir().unwrap();
    let db = gras_db(dbdir.path(), r"textures\plants\g.dds");
    modscan::store::Store::open(&db)
        .unwrap()
        .conn()
        .execute("UPDATE shape SET slsf2 = 8388608", [])
        .unwrap();
    m.scan_db = Some(db);
    m.replan();
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    assert!(
        m.gras_diffuses.is_empty(),
        "a TruePBR shape is no complex-grass evidence"
    );
    assert_eq!(
        m.gras_all_diffuses,
        ["textures/plants/g".to_string()].into_iter().collect()
    );
    assert_eq!(m.request().unwrap().gras_all_diffuses, m.gras_all_diffuses);
    let v = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "plants/g")
        .expect("the plants/g set");
    let p = v.shader.expect("a shader pick");
    assert_eq!(
        (p.shader, p.provenance),
        (Shader::PbrGrass, ShaderOrigin::Auto(AutoRule::PbrGrass))
    );
}
