// Store API: raw slot strings + flags, geometry columns, schema version, error propagation.
use modscan::store::{CopyResult, Store, PARSE_PARTIAL};
use nif::path::{canon_texture, PathFlags};
use nif::props::{Effect, EffectParams, FieldOffsets, Lighting, LightingParams, LightingTrailer};
use nif::scan::{NifScan, Shader, ShapeRow};
use nif::shape_head::{ShapeHead, ShapeKind};

fn head(skinned: bool) -> ShapeHead {
    ShapeHead {
        kind: ShapeKind::BSTriShape,
        name: Some("S".into()),
        skinned,
        shader: 1,
        alpha: -1,
        vertex_desc: Some(0x0000_B000_0000_0007),
        num_triangles: Some(12),
        num_vertices: Some(30),
    }
}

fn lighting() -> Lighting {
    Lighting {
        name: None,
        shader_type: 0,
        slsf1: 1,
        slsf2: 2,
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
    }
}

fn effect(src: &[u8]) -> Effect {
    Effect {
        name: None,
        slsf1: 0,
        slsf2: 0,
        source: canon_texture(src),
        greyscale: canon_texture(b""),
        params: EffectParams {
            uv_offset: [0.0; 2],
            uv_scale: [1.0; 2],
            texture_clamp_mode: 3,
            lighting_influence: 0,
            env_map_min_lod: 0,
            falloff: [0.0; 4],
            base_color: [1.0; 4],
            base_color_scale: 1.0,
            soft_falloff_depth: 100.0,
        },
    }
}

fn scan_of(shapes: Vec<ShapeRow>) -> NifScan {
    NifScan {
        bsver: 100,
        le_mesh: false,
        shapes,
        problems: vec![],
    }
}

fn store() -> (tempfile::TempDir, Store, i64) {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    s.begin_scan().unwrap();
    let o = s
        .upsert_origin(&modscan::store::OriginSpec {
            kind: modscan::store::KIND_MOD,
            name: "A".into(),
            root: "A".into(),
            parent: None,
            priority: 0,
            derived_by: None,
            derived_state: None,
        })
        .unwrap();
    (d, s, o)
}

#[test]
fn raw_strings_and_flags_are_kept_for_odd_slots() {
    let (_d, mut s, o) = store();
    let slots = [
        b"Textures\\A_d.DDS".as_slice(), // trivial (case/slashes): not stored
        b"C:\\Skyrim\\Data\\textures\\b.dds".as_slice(), // rebased
        b"c_n".as_slice(),               // prefixed + non_dds
        b"".as_slice(),                  // empty: not stored
        b" textures\\d.dds".as_slice(),  // leading space: raw differs beyond case/slashes
    ]
    .map(canon_texture)
    .to_vec();
    let scan = scan_of(vec![
        ShapeRow {
            block: 0,
            head: head(true),
            shader: Shader::Lighting(lighting(), slots),
            alpha: None,
        },
        ShapeRow {
            block: 5,
            head: head(false),
            shader: Shader::Effect(effect(b"fx\\glow.dds")),
            alpha: None,
        },
    ]);
    let id = s
        .upsert_copy("meshes/x.nif", o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap();
    let v = s.shapes_of(id).unwrap();
    assert_eq!(v.len(), 2);
    let l = &v[0];
    assert_eq!(l.textures.len(), 5);
    assert_eq!(l.raw.len(), 5);
    assert_eq!(l.raw[0], None);
    let r1 = l.raw[1].as_ref().unwrap();
    assert_eq!(r1.raw, "C:\\Skyrim\\Data\\textures\\b.dds");
    assert!(r1.flags.rebased);
    let r2 = l.raw[2].as_ref().unwrap();
    assert_eq!(r2.raw, "c_n");
    assert_eq!(
        r2.flags,
        PathFlags {
            prefixed: true,
            non_dds: true,
            ..Default::default()
        }
    );
    assert_eq!(l.raw[3], None);
    assert_eq!(l.raw[4].as_ref().unwrap().raw, " textures\\d.dds");
    let e = &v[1];
    assert_eq!(e.raw.len(), 2);
    assert_eq!(e.raw[0].as_ref().unwrap().raw, "fx\\glow.dds");
    assert!(e.raw[0].as_ref().unwrap().flags.prefixed);
    assert_eq!(e.raw[1], None);
    // geometry facts (F5)
    assert!(l.skinned && !e.skinned);
    assert_eq!(l.vertex_desc, Some(0x0000_B000_0000_0007));
    assert_eq!((l.num_vertices, l.num_triangles), (Some(30), Some(12)));
    // re-writing the copy replaces its raw rows (no leftovers)
    let id2 = s
        .upsert_copy(
            "meshes/x.nif",
            o,
            2,
            2,
            CopyResult::Scanned(scan_of(vec![])),
            None,
        )
        .unwrap();
    assert_eq!(id, id2);
    let n: i64 = s
        .conn()
        .query_row("SELECT count(*) FROM tex_raw", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn more_than_nine_slots_is_recorded_as_a_problem() {
    let (_d, mut s, o) = store();
    let slots: Vec<_> = (0..11)
        .map(|i| canon_texture(format!("textures/t{i}.dds").as_bytes()))
        .collect();
    let scan = scan_of(vec![ShapeRow {
        block: 3,
        head: head(false),
        shader: Shader::Lighting(lighting(), slots),
        alpha: None,
    }]);
    let id = s
        .upsert_copy("meshes/x.nif", o, 1, 1, CopyResult::from(Ok(scan)), None)
        .unwrap();
    let (status, problem): (i32, Option<String>) = s
        .conn()
        .query_row(
            "SELECT parse_status, problem FROM nif_copy WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, PARSE_PARTIAL);
    assert!(problem.unwrap().contains("11 slots"));
    assert_eq!(s.shapes_of(id).unwrap()[0].textures.len(), 9);
}

#[test]
fn schema_v3_rebuilds_old_databases_and_sets_pragmas() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("old.db");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE shape (copy_id INTEGER, block INTEGER, PRIMARY KEY(copy_id, block));
             INSERT INTO shape VALUES (1, 1); PRAGMA user_version = 2;",
        )
        .unwrap();
    }
    let s = Store::open(&p).unwrap();
    let c = s.conn();
    let v: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, modscan::store::SCHEMA_VERSION);
    assert_eq!(v, 3);
    let n: i64 = c
        .query_row("SELECT count(*) FROM shape", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    for col in ["skinned", "vertex_desc", "num_vertices", "num_triangles"] {
        let has: i64 = c
            .query_row(
                "SELECT count(*) FROM pragma_table_info('shape') WHERE name = ?1",
                [col],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has, 1, "{col}");
    }
    let sql: String = c
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'texset'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!sql.contains("UNIQUE"), "{sql}");
    let sync: i64 = c.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
    assert_eq!(sync, 1, "synchronous = NORMAL");
}

#[test]
fn read_api_reports_database_errors() {
    let (_d, s, o) = store();
    s.conn()
        .execute_batch(
            "DROP TABLE tex_winner; DROP TABLE shape; DROP TABLE nif_winner; DROP TABLE tex_use;",
        )
        .unwrap();
    assert!(s.resolve_texture("textures/a.dds").is_err());
    assert!(s.nifs_of(o).is_err());
    assert!(s.shapes_of(1).is_err());
    assert!(s.users_of_texture("textures/a.dds").is_err());
    s.conn().execute_batch("DROP TABLE origin;").unwrap();
    assert!(s.mods().is_err());
}
