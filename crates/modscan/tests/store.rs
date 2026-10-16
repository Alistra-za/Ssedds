// Store API: raw slot strings + flags, geometry columns, schema version, error propagation.
use modscan::store::ChunkItem;
use modscan::store::{CopyResult, Store, PARSE_PARTIAL};
use nif::path::{canon_texture, PathFlags};
use nif::props::{Effect, EffectParams, FieldOffsets, Lighting, LightingParams, LightingTrailer};
use nif::scan::{NifScan, Shader, ShapeRow};
use nif::shape_head::{ShapeHead, ShapeKind};
use std::collections::BTreeMap;

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
        index3d: Default::default(),
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
    assert_eq!(v, 8);
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

// ---- P1-A Task 11: the set preview's mesh query and the read-only open ----

use modscan::store::{DeriveRow, OriginSpec, TexKey, KIND_MOD};
use std::collections::HashMap;

fn origin(s: &mut Store, name: &str, priority: i32) -> i64 {
    s.upsert_origin(&OriginSpec {
        kind: KIND_MOD,
        name: name.into(),
        root: name.into(),
        parent: None,
        priority,
        derived_by: None,
        derived_state: None,
    })
    .unwrap()
}

fn lit(slots: &[&str]) -> Shader {
    Shader::Lighting(
        lighting(),
        slots.iter().map(|p| canon_texture(p.as_bytes())).collect(),
    )
}

fn put(s: &mut Store, path: &str, o: i64, block: u32, shader: Shader, skinned: bool) -> i64 {
    let scan = scan_of(vec![ShapeRow {
        block,
        head: head(skinned),
        shader,
        alpha: None,
    }]);
    s.upsert_copy(path, o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap()
}

/// a: vanilla diffuse + normal (wins). b: already-PBR paths, skinned (wins). c: `myreserve` (not on a component
/// boundary). d: a PGPatcher-derived winner over its source copy in A. e: A's copy loses to B's. f: the texture on
/// an effect shader. g: `axb`, a trap for an unescaped LIKE `_`.
fn query_store() -> (tempfile::TempDir, Store, HashMap<&'static str, i64>) {
    let (d, mut s, a) = store();
    let b = origin(&mut s, "B", 1);
    let pgp = origin(&mut s, "PGPatcher output", 2);
    let mut ids = HashMap::new();
    let vanilla = [
        r"textures\xmead\reserve.dds",
        r"textures\xmead\reserve_n.dds",
    ];
    let pbr = [
        r"textures\pbr\xmead\reserve.dds",
        r"textures\pbr\xmead\reserve_msn.dds",
    ];
    ids.insert(
        "a",
        put(&mut s, "meshes/x/a.nif", a, 3, lit(&vanilla), false),
    );
    ids.insert("b", put(&mut s, "meshes/x/b.nif", a, 1, lit(&pbr), true));
    ids.insert(
        "c",
        put(
            &mut s,
            "meshes/x/c.nif",
            a,
            0,
            lit(&[r"textures\other\myreserve.dds"]),
            false,
        ),
    );
    ids.insert(
        "d_src",
        put(&mut s, "meshes/x/d.nif", a, 5, lit(&[vanilla[0]]), false),
    );
    ids.insert(
        "d",
        put(&mut s, "meshes/x/d.nif", pgp, 7, lit(&[pbr[0]]), false),
    );
    ids.insert(
        "e_lose",
        put(&mut s, "meshes/x/e.nif", a, 0, lit(&[vanilla[0]]), false),
    );
    ids.insert(
        "e_win",
        put(
            &mut s,
            "meshes/x/e.nif",
            b,
            0,
            lit(&[r"textures\other\rock.dds"]),
            false,
        ),
    );
    ids.insert(
        "f",
        put(
            &mut s,
            "meshes/x/f.nif",
            a,
            0,
            Shader::Effect(effect(vanilla[0].as_bytes())),
            false,
        ),
    );
    ids.insert(
        "g",
        put(
            &mut s,
            "meshes/x/g.nif",
            a,
            0,
            lit(&[r"textures\axb.dds"]),
            false,
        ),
    );
    s.set_derived(&[DeriveRow {
        copy_id: ids["d"],
        is_derived: true,
        source_copy_id: Some(ids["d_src"]),
        derive_check: Some(0),
    }])
    .unwrap();
    let names = ["a", "b", "c", "d", "e", "f", "g"];
    let paths: Vec<String> = names.iter().map(|n| format!("meshes/x/{n}.nif")).collect();
    let pid = s
        .path_ids(&paths.iter().map(String::as_str).collect::<Vec<_>>())
        .unwrap();
    let winner = |n: &str| match n {
        "e" => ids["e_win"],
        other => ids[other],
    };
    let game: HashMap<i64, (i64, u32)> = names
        .iter()
        .zip(&pid)
        .map(|(n, p)| (*p, (winner(n), 1)))
        .collect();
    let source: HashMap<i64, i64> = [(pid[3], ids["d_src"])].into_iter().collect();
    s.set_winners(&game, &source).unwrap();
    (d, s, ids)
}

fn nifs(rows: &[modscan::store::MatchRow]) -> Vec<&str> {
    rows.iter().map(|r| r.nif_path.as_str()).collect()
}

#[test]
fn suffix_on_the_diffuse_matches_whole_components_of_winners_and_pbr_paths() {
    let (_d, s, ids) = query_store();
    let key = TexKey::Suffix {
        slot: 0,
        stem: r"XMead\Reserve".into(),
    };
    let rows = s.nifs_using_texture(&key, 50).unwrap();
    // Non-skinned first, then fewest shapes, then path; b (skinned) last.
    assert_eq!(
        nifs(&rows),
        ["meshes/x/a.nif", "meshes/x/d.nif", "meshes/x/b.nif"]
    );
    assert_eq!(
        (rows[0].copy_id, rows[0].load_copy_id, rows[0].shape_block),
        (ids["a"], ids["a"], 3)
    );
    assert!(!rows[0].derived);
    // A derived winner is matched and loaded through its source copy.
    assert_eq!(
        (rows[1].copy_id, rows[1].load_copy_id, rows[1].shape_block),
        (ids["d"], ids["d_src"], 5)
    );
    assert!(rows[1].derived);
    assert!(rows[2].skinned);
    assert_eq!(rows[0].origin, "A");
    // `myreserve` is not on a component boundary, the loser and the effect shape are out, the limit holds.
    let all = s
        .nifs_using_texture(
            &TexKey::Suffix {
                slot: 0,
                stem: "reserve".into(),
            },
            50,
        )
        .unwrap();
    assert_eq!(
        nifs(&all),
        ["meshes/x/a.nif", "meshes/x/d.nif", "meshes/x/b.nif"]
    );
    assert_eq!(
        s.nifs_using_texture(
            &TexKey::Suffix {
                slot: 0,
                stem: "reserve".into()
            },
            2
        )
        .unwrap()
        .len(),
        2
    );
    // `_` is not a wildcard.
    assert!(s
        .nifs_using_texture(
            &TexKey::Suffix {
                slot: 0,
                stem: "a_b".into()
            },
            50
        )
        .unwrap()
        .is_empty());
}

#[test]
fn suffix_on_the_normal_strips_n_and_msn_on_the_shape_side_only() {
    let (_d, s, _) = query_store();
    let rows = s
        .nifs_using_texture(
            &TexKey::Suffix {
                slot: 1,
                stem: r"xmead\reserve".into(),
            },
            50,
        )
        .unwrap();
    assert_eq!(nifs(&rows), ["meshes/x/a.nif", "meshes/x/b.nif"]);
    // PGPatcher never strips `_n` from the JSON value (01 §1.3).
    assert!(s
        .nifs_using_texture(
            &TexKey::Suffix {
                slot: 1,
                stem: r"xmead\reserve_n".into()
            },
            50
        )
        .unwrap()
        .is_empty());
}

#[test]
fn contains_reads_the_extension_less_pbr_stripped_diffuse() {
    let (_d, s, _) = query_store();
    let rows = s
        .nifs_using_texture(
            &TexKey::Contains {
                slot: 0,
                frag: "XMEAD".into(),
            },
            50,
        )
        .unwrap();
    assert_eq!(
        nifs(&rows),
        ["meshes/x/a.nif", "meshes/x/d.nif", "meshes/x/b.nif"]
    );
    // `textures\pbr\` is read as `textures\`, and `.dds` is gone.
    for frag in ["pbr", ".dds"] {
        assert!(
            s.nifs_using_texture(
                &TexKey::Contains {
                    slot: 0,
                    frag: frag.into()
                },
                50
            )
            .unwrap()
            .is_empty(),
            "{frag}"
        );
    }
}

#[test]
fn exact_compares_the_full_path_with_textures_added() {
    let (_d, s, _) = query_store();
    for path in [r"textures\xmead\reserve.dds", r"xmead\RESERVE.dds"] {
        let rows = s
            .nifs_using_texture(
                &TexKey::Exact {
                    slot: 0,
                    path: path.into(),
                },
                50,
            )
            .unwrap();
        assert_eq!(nifs(&rows), ["meshes/x/a.nif", "meshes/x/d.nif"], "{path}");
    }
    let rows = s
        .nifs_using_texture(
            &TexKey::Exact {
                slot: 0,
                path: r"textures\pbr\xmead\reserve.dds".into(),
            },
            50,
        )
        .unwrap();
    assert_eq!(
        nifs(&rows),
        ["meshes/x/b.nif"],
        "the derived copy's own rows are never used"
    );
}

#[test]
fn open_read_only_never_creates_rebuilds_or_writes() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("nope").join("scan.db");
    assert!(Store::open_read_only(&missing).is_err());
    assert!(!missing.parent().unwrap().exists(), "no folder is created");
    let old = d.path().join("old.db");
    {
        let c = rusqlite::Connection::open(&old).unwrap();
        c.execute_batch("CREATE TABLE shape (a INTEGER); INSERT INTO shape VALUES (1); PRAGMA user_version = 2;")
            .unwrap();
    }
    assert!(matches!(
        Store::open_read_only(&old),
        Err(modscan::ScanError::SchemaVersion { found: 2, want: 8 })
    ));
    let n: i64 = rusqlite::Connection::open(&old)
        .unwrap()
        .query_row("SELECT count(*) FROM shape", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "another version is left alone");
    let (q, _rw, _) = query_store();
    let ro = Store::open_read_only(&q.path().join("scan.db")).unwrap();
    let rows = ro
        .nifs_using_texture(
            &TexKey::Exact {
                slot: 0,
                path: r"textures\xmead\reserve.dds".into(),
            },
            10,
        )
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(ro
        .conn()
        .execute_batch("CREATE TABLE x (a INTEGER)")
        .is_err());
}

#[test]
fn the_mesh_query_reports_database_errors() {
    let (_d, s, _) = query_store();
    s.conn().execute_batch("DROP TABLE nif_winner;").unwrap();
    assert!(s
        .nifs_using_texture(
            &TexKey::Suffix {
                slot: 0,
                stem: "reserve".into()
            },
            5
        )
        .is_err());
}

// ---- Task 11 fix round 1 ----

/// One origin, each `(nif, shapes)` its own winning copy; a shape is `(block, skinned, slots)`.
type Shapes<'a> = Vec<(u32, bool, Vec<&'a str>)>;

fn small_store(nifs: &[(&str, Shapes)]) -> (tempfile::TempDir, Store) {
    let (d, mut s, a) = store();
    let mut game = HashMap::new();
    let paths: Vec<String> = nifs
        .iter()
        .map(|(n, _)| format!("meshes/s/{n}.nif"))
        .collect();
    let mut ids = vec![];
    for ((_, shapes), path) in nifs.iter().zip(&paths) {
        let rows = shapes
            .iter()
            .map(|(block, skinned, slots)| ShapeRow {
                block: *block,
                head: head(*skinned),
                shader: lit(slots),
                alpha: None,
            })
            .collect();
        ids.push(
            s.upsert_copy(path, a, 1, 1, CopyResult::Scanned(scan_of(rows)), None)
                .unwrap(),
        );
    }
    let pid = s
        .path_ids(&paths.iter().map(String::as_str).collect::<Vec<_>>())
        .unwrap();
    for (p, id) in pid.iter().zip(&ids) {
        game.insert(*p, (*id, 1));
    }
    s.set_winners(&game, &HashMap::new()).unwrap();
    (d, s)
}

fn q(s: &Store, key: TexKey) -> Vec<(String, u32)> {
    s.nifs_using_texture(&key, 50)
        .unwrap()
        .into_iter()
        .map(|r| {
            (
                r.nif_path
                    .trim_start_matches("meshes/s/")
                    .trim_end_matches(".nif")
                    .to_string(),
                r.shape_block,
            )
        })
        .collect()
}

fn hit(n: &str, b: u32) -> (String, u32) {
    (n.to_string(), b)
}

#[test]
fn contains_sees_a_pbr_slot_through_its_stripped_form() {
    let (_d, s, _) = query_store();
    let rows = s
        .nifs_using_texture(
            &TexKey::Contains {
                slot: 0,
                frag: r"textures\xmead".into(),
            },
            50,
        )
        .unwrap();
    assert_eq!(
        nifs(&rows),
        ["meshes/x/a.nif", "meshes/x/d.nif", "meshes/x/b.nif"]
    );
    // The stripped form has no `textures/pbr/`.
    assert!(s
        .nifs_using_texture(
            &TexKey::Contains {
                slot: 0,
                frag: r"textures\pbr".into()
            },
            50
        )
        .unwrap()
        .is_empty());
}

#[test]
fn matching_reads_the_raw_slot_string() {
    let (_d, s) = small_store(&[
        ("reb", vec![(1, false, vec![r"Data\Textures\Rock.DDS"])]),
        ("lead", vec![(1, false, vec![r" textures\ws.dds"])]),
        ("trail", vec![(1, false, vec![r"textures\tw.dds "])]),
    ]);
    let suf = |stem: &str| {
        q(
            &s,
            TexKey::Suffix {
                slot: 0,
                stem: stem.into(),
            },
        )
    };
    // A rebased raw string keeps its `data/` prefix, which the whole-component suffix ignores.
    assert_eq!(suf(r"textures\rock"), [hit("reb", 1)]);
    assert_eq!(suf("rock"), [hit("reb", 1)]);
    // Surrounding whitespace is part of the raw string: `\ ws` is not a component boundary, the extension cut is.
    assert!(suf(r"textures\ws").is_empty());
    assert_eq!(suf("ws"), [hit("lead", 1)]);
    assert_eq!(suf("tw"), [hit("trail", 1)]);
    assert!(suf(r"textures	w").is_empty());
    // Exact compares the whole raw string: no `data/` prefix, no whitespace.
    for path in [
        r"textures\rock.dds",
        r"data\textures\rock.dds",
        r"textures\ws.dds",
        r"textures\tw.dds",
    ] {
        assert!(
            q(
                &s,
                TexKey::Exact {
                    slot: 0,
                    path: path.into()
                }
            )
            .is_empty(),
            "{path}"
        );
    }
    assert_eq!(
        q(
            &s,
            TexKey::Contains {
                slot: 0,
                frag: "data/textures/rock".into()
            }
        ),
        [hit("reb", 1)]
    );
}

#[test]
fn limit_zero_is_empty() {
    let (_d, s, _) = query_store();
    let key = TexKey::Suffix {
        slot: 0,
        stem: "reserve".into(),
    };
    assert!(s.nifs_using_texture(&key, 0).unwrap().is_empty());
}

#[test]
fn like_metacharacters_and_quotes_are_literal() {
    let (_d, s) = small_store(&[
        ("pct", vec![(1, false, vec![r"textures\p%q.dds"])]),
        ("pxq", vec![(1, false, vec![r"textures\pxq.dds"])]),
        ("und", vec![(1, false, vec![r"textures\a_b.dds"])]),
        ("axb", vec![(1, false, vec![r"textures\axb.dds"])]),
        ("quo", vec![(1, false, vec![r"textures\it's.dds"])]),
    ]);
    for (stem, want) in [("p%q", "pct"), ("a_b", "und"), ("it's", "quo")] {
        assert_eq!(
            q(
                &s,
                TexKey::Suffix {
                    slot: 0,
                    stem: stem.into()
                }
            ),
            [hit(want, 1)],
            "{stem}"
        );
        assert_eq!(
            q(
                &s,
                TexKey::Contains {
                    slot: 0,
                    frag: stem.into()
                }
            ),
            [hit(want, 1)],
            "{stem}"
        );
        let exact = format!("textures/{stem}.dds");
        assert_eq!(
            q(
                &s,
                TexKey::Exact {
                    slot: 0,
                    path: exact
                }
            ),
            [hit(want, 1)],
            "{stem}"
        );
    }
    // A LIKE that skipped the escaping would only add candidates, which the exact post-filter removes again, so
    // no fixture can tell a missing escape from a working one at this level; these cover the positives.
    assert!(q(
        &s,
        TexKey::Suffix {
            slot: 0,
            stem: "p_q".into()
        }
    )
    .is_empty());
}

#[test]
fn a_limited_query_returns_the_best_rows_in_order() {
    let (_d, s) = small_store(&[
        (
            "multi",
            vec![
                (1, false, vec![r"textures\m\t.dds"]),
                (2, false, vec![r"textures\o.dds"]),
                (3, false, vec![r"textures\m\t.dds"]),
            ],
        ),
        ("one", vec![(4, false, vec![r"textures\m\t.dds"])]),
        ("skin", vec![(1, true, vec![r"textures\m\t.dds"])]),
    ]);
    let key = |_| TexKey::Suffix {
        slot: 0,
        stem: r"m\t".into(),
    };
    let full = q(&s, key(0));
    assert_eq!(
        full,
        [
            hit("one", 4),
            hit("multi", 1),
            hit("multi", 3),
            hit("skin", 1)
        ]
    );
    let rows = s.nifs_using_texture(&key(0), 3).unwrap();
    let got: Vec<_> = rows
        .iter()
        .map(|r| (r.nif_path.as_str(), r.shape_block))
        .collect();
    assert_eq!(
        got,
        [
            ("meshes/s/one.nif", 4),
            ("meshes/s/multi.nif", 1),
            ("meshes/s/multi.nif", 3)
        ]
    );
}

#[test]
fn winning_nifs_finds_one_winner_per_path_by_substring() {
    let (_d, s, ids) = query_store();
    let rows = s.winning_nifs("X\\", 100).unwrap();
    let paths: Vec<_> = rows.iter().map(|(n, _)| n.path.as_str()).collect();
    assert_eq!(paths.len(), 7, "one row per path: {paths:?}");
    assert!(rows.iter().all(|(n, _)| n.wins));
    let by_copy = |id: i64| rows.iter().find(|(n, _)| n.copy_id == id);
    assert_eq!(by_copy(ids["e_win"]).map(|r| r.1.as_str()), Some("B"));
    assert!(
        by_copy(ids["e_lose"]).is_none(),
        "the losing copy is excluded"
    );
    assert_eq!(
        by_copy(ids["d"]).map(|r| r.1.as_str()),
        Some("PGPatcher output")
    );
    assert!(by_copy(ids["d_src"]).is_none());
    // Case-insensitive, either slash, narrower text narrows.
    assert_eq!(s.winning_nifs("X/D.NIF", 100).unwrap().len(), 1);
    assert!(s.winning_nifs("nothing", 100).unwrap().is_empty());
}

#[test]
fn winning_nifs_wildcards_are_literal_and_limit_plus_one_marks_truncation() {
    let (_d, s, _) = query_store();
    assert!(s.winning_nifs("%", 100).unwrap().is_empty());
    assert!(s.winning_nifs("x/_.nif", 100).unwrap().is_empty());
    assert_eq!(s.winning_nifs("x/", 3).unwrap().len(), 4, "limit + 1 rows");
    assert_eq!(s.winning_nifs("x/", 7).unwrap().len(), 7, "not truncated");
}

#[test]
fn winning_nifs_matches_the_path_after_the_meshes_folder() {
    let (_d, s, _) = query_store();
    assert!(s.winning_nifs("mes", 100).unwrap().is_empty());
    assert!(s.winning_nifs("meshes", 100).unwrap().is_empty());
    assert_eq!(s.winning_nifs("x/d", 100).unwrap().len(), 1);
}

/// Spec §2 step 5 + review focus 2: one id per path with and without the scan's in-memory map, also for a path
/// another connection added after the map was loaded.
#[test]
fn path_ids_are_stable_inside_and_outside_a_scan() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("s.db");
    let mut s = Store::open(&p).unwrap();
    let a = s.path_ids(&["meshes/a.nif", "meshes/b.nif"]).unwrap();
    s.begin_scan().unwrap();
    let mut other = Store::open(&p).unwrap();
    let x = other.path_ids(&["meshes/x.nif"]).unwrap()[0];
    let b = s
        .path_ids(&[
            "meshes/b.nif",
            "meshes/a.nif",
            "meshes/c.nif",
            "meshes/c.nif",
            "meshes/x.nif",
        ])
        .unwrap();
    s.end_scan().unwrap();
    assert_eq!((b[0], b[1]), (a[1], a[0]));
    assert_eq!(b[2], b[3]);
    assert_eq!(b[4], x, "a path another connection interned");
    assert_eq!(s.path_ids(&["meshes/c.nif"]).unwrap()[0], b[2]);
}

/// Spec §2 step 6: cache pragmas on every connection; synchronous OFF only while a cold scan runs.
#[test]
fn a_cold_scan_runs_with_synchronous_off_and_ends_normal() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("s.db")).unwrap();
    let pragma = |s: &Store, p: &str| -> i64 {
        s.conn()
            .query_row(&format!("PRAGMA {p}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(pragma(&s, "cache_size"), -65536);
    assert_eq!(pragma(&s, "temp_store"), 2, "MEMORY");
    assert_eq!(pragma(&s, "synchronous"), 1, "NORMAL");
    s.begin_scan().unwrap();
    assert_eq!(pragma(&s, "synchronous"), 0, "OFF during a cold scan");
    s.end_scan().unwrap();
    assert_eq!(pragma(&s, "synchronous"), 1);
    s.upsert_copy(
        "meshes/a.nif",
        1,
        1,
        1,
        CopyResult::Failed("x".into()),
        None,
    )
    .unwrap();
    s.begin_scan().unwrap();
    assert_eq!(pragma(&s, "synchronous"), 1, "a warm scan keeps NORMAL");
    s.end_scan().unwrap();
}

/// Review focus 3 / ruling R4: a file SQLite cannot read is rebuilt (the store is a cache).
#[test]
fn a_corrupt_store_file_is_rebuilt() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("scan.db");
    std::fs::write(&p, vec![0xAB; 8192]).unwrap();
    let s = Store::open(&p).unwrap();
    let v: i64 = s
        .conn()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, modscan::store::SCHEMA_VERSION);
}

/// A running scan holds scan.lock: a writable open that would rebuild an outdated db must wait, give up with
/// Busy and leave the tables alone; once the lock is free it rebuilds.
#[cfg(windows)]
#[test]
fn outdated_schema_rebuild_waits_for_the_scan_lock() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    let old = d.path().join("old.db");
    {
        let c = rusqlite::Connection::open(&old).unwrap();
        c.execute_batch("CREATE TABLE shape (a INTEGER); INSERT INTO shape VALUES (1); PRAGMA user_version = 2;")
            .unwrap();
    }
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(d.path().join("scan.lock"))
        .unwrap();
    assert!(matches!(Store::open(&old), Err(modscan::ScanError::Busy)));
    let n: i64 = rusqlite::Connection::open(&old)
        .unwrap()
        .query_row("SELECT count(*) FROM shape", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "no table was dropped under a running scan");
    drop(lock);
    Store::open(&old).unwrap();
    let v: i64 = rusqlite::Connection::open(&old)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, modscan::store::SCHEMA_VERSION);
}

/// Final review M2: when the corrupt-file rebuild cannot delete the database because another handle holds it, the
/// error says to restart the app.
#[cfg(windows)]
#[test]
fn a_rebuild_blocked_by_an_open_handle_says_to_restart() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("scan.db");
    std::fs::write(&p, vec![b'x'; 4096]).unwrap();
    // Share read+write but not delete: SQLite can open the file, `remove_file` cannot.
    let _held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&p)
        .unwrap();
    let e = Store::open(&p)
        .err()
        .expect("the rebuild cannot delete the file");
    assert!(
        e.to_string()
            .contains("restart the app to rebuild the scan database"),
        "{e}"
    );
}

#[test]
fn shapes_of_round_trips_the_3d_index() {
    let (_d, mut s, o) = store();
    let mut scan = scan_of(vec![
        ShapeRow {
            block: 3,
            head: head(false),
            shader: Shader::None,
            alpha: None,
        },
        ShapeRow {
            block: 7,
            head: head(false),
            shader: Shader::None,
            alpha: None,
        },
    ]);
    scan.index3d = BTreeMap::from([(7, 0)]);
    let id = s
        .upsert_copy("meshes/x.nif", o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap();
    let v: Vec<(u32, Option<u32>)> = s
        .shapes_of(id)
        .unwrap()
        .iter()
        .map(|x| (x.block, x.index3d))
        .collect();
    assert_eq!(
        v,
        [(3, None), (7, Some(0))],
        "NULL when no node reaches the shape"
    );
}

#[test]
fn a_derived_copy_keeps_its_3d_index_through_commit_pending() {
    let (_d, mut s, o) = store();
    let mut scan = scan_of(vec![ShapeRow {
        block: 2,
        head: head(false),
        shader: Shader::None,
        alpha: None,
    }]);
    scan.index3d = BTreeMap::from([(2, 5)]);
    let pid = s.path_ids(&["meshes/d.nif"]).unwrap()[0];
    s.write_chunk(&[ChunkItem {
        path_id: pid,
        origin_id: o,
        size: 1,
        mtime: 1,
        crc32: None,
        derived: true,
        result: CopyResult::Scanned(scan),
    }])
    .unwrap();
    s.commit_pending().unwrap();
    let id = s.existing_copies().unwrap()[&(pid, o)].id;
    assert_eq!(s.shapes_of(id).unwrap()[0].index3d, Some(5));
}

#[test]
fn a_schema_7_database_is_rebuilt_with_the_index3d_column() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("v7.db");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE shape (copy_id INTEGER, block INTEGER, PRIMARY KEY(copy_id, block));
             INSERT INTO shape VALUES (1, 1); PRAGMA user_version = 7;",
        )
        .unwrap();
    }
    let s = Store::open(&p).unwrap();
    let n: i64 = s
        .conn()
        .query_row("SELECT count(index3d) FROM shape", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "rebuilt: the column exists and the old row is gone");
}

/// Audit 2026-10-15 item 17: texsets (and their tex_use rows) that no live or pending shape uses are dropped at the
/// end of a run; the interner cache must not hand a dropped id to a later shape.
#[test]
fn delete_unseen_drops_texsets_no_shape_uses() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    s.begin_scan().unwrap();
    s.conn()
        .execute_batch(
            "INSERT INTO texset(id, n, t0) VALUES (1, 1, 10), (2, 1, 20), (3, 1, 30);
             INSERT INTO tex_use VALUES (10, 1, 0), (20, 2, 0), (30, 3, 0);
             INSERT INTO shape(copy_id, block, kind, shader_kind, texset_id) VALUES (1, 0, 'x', 1, 1);
             INSERT INTO pending_shape(pid, block, kind, shader_kind, texset_id) VALUES (1, 0, 'x', 1, 3);",
        )
        .unwrap();
    s.delete_unseen(&Default::default()).unwrap();
    let ids = |q: &str| -> Vec<i64> {
        let mut st = s.conn().prepare(q).unwrap();
        let v = st.query_map([], |r| r.get(0)).unwrap();
        v.map(|x| x.unwrap()).collect()
    };
    assert_eq!(ids("SELECT id FROM texset ORDER BY id"), [1, 3]);
    assert_eq!(
        ids("SELECT texset_id FROM tex_use ORDER BY texset_id"),
        [1, 3]
    );
    s.end_scan().unwrap();
}
