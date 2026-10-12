// real files written by nifly (GPL-3.0 fixtures)
use nif::geom::read_shape;
use nif::header::Header;
use nif::scan::scan;

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn unit_ok(normals: &[[f32; 3]]) -> usize {
    let len = |v: &[f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    normals
        .iter()
        .filter(|v| (len(v) - 1.0).abs() < 0.05)
        .count()
}

#[test]
fn se_fixture_shapes_decode_consistently() {
    for f in [
        "TestNifFile_Furniture_Col_SE.nif",
        "TestNifFile_Skinned_SE.nif",
        "TestNifFile_Skinned_Dynamic_SE.nif",
    ] {
        let b = read(f);
        let h = Header::parse(&b).unwrap();
        let s = scan(&b).unwrap();
        let rows: Vec<_> = s
            .shapes
            .iter()
            .filter(|r| r.head.vertex_desc.is_some())
            .collect();
        assert!(!rows.is_empty(), "{f}");
        for row in rows {
            let m = read_shape(&h, &b, row.block as usize)
                .unwrap_or_else(|e| panic!("{f} block {}: {e}", row.block));
            let n = m.positions.len();
            assert!(n > 0, "{f}");
            assert!(!m.triangles.is_empty(), "{f}");
            assert!(m.positions.iter().flatten().all(|v| v.is_finite()), "{f}");
            assert!(
                m.triangles.iter().flatten().all(|&i| (i as usize) < n),
                "{f}"
            );
            if !m.normals.is_empty() {
                assert_eq!(m.normals.len(), n, "{f}");
                assert!(
                    unit_ok(&m.normals) * 10 >= n * 9,
                    "{f}: normals mostly unit length"
                );
            }
            if !m.uvs.is_empty() {
                assert_eq!(m.uvs.len(), n, "{f}");
            }
            if row.head.skinned {
                assert_eq!(m.bone_weights.len(), n, "{f}");
                assert_eq!(m.bone_indices.len(), n, "{f}");
                // weights of a skinned vertex sum to ~1
                let ok = m
                    .bone_weights
                    .iter()
                    .filter(|w| (w.iter().sum::<f32>() - 1.0).abs() < 0.02)
                    .count();
                assert!(ok * 10 >= n * 9, "{f}: weights sum to 1");
            }
        }
    }
}

#[test]
fn skinned_fixture_reads_partition_data() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let h = Header::parse(&b).unwrap();
    let s = scan(&b).unwrap();
    let row = s
        .shapes
        .iter()
        .find(|r| r.head.skinned && r.head.vertex_desc.is_some())
        .expect("a skinned shape");
    let m = read_shape(&h, &b, row.block as usize).unwrap();
    assert!(!m.positions.is_empty());
    assert!(!m.tangents.is_empty() && m.tangents.len() == m.bitangents.len());
}

#[test]
fn bad_block_is_an_error_not_a_panic() {
    let b = read("TestNifFile_Skinned_SE.nif");
    let h = Header::parse(&b).unwrap();
    let s = scan(&b).unwrap();
    let block = s.shapes[0].block as usize;
    // a non-shape block
    let other = (0..h.sizes.len()).find(|&i| i != block && h.kind(i) == b"NiNode");
    if let Some(o) = other {
        assert!(read_shape(&h, &b, o).is_err());
    }
    assert!(read_shape(&h, &b, h.sizes.len() + 5).is_err());
    // truncated file: every prefix either decodes or errors, never panics
    for cut in (0..b.len()).step_by(97) {
        let _ = read_shape(&h, &b[..cut], block);
    }
}

#[test]
fn vertex_desc_stride_and_flags() {
    use nif::geom::{Attr, VertexDesc};
    // stride 7*4; attribute flags at bits 44+: 0xB = Vertex|UVs|Normals
    let d = VertexDesc(0x0000_B000_0000_0000 | 0x7);
    assert_eq!(d.stride(), 28);
    assert!(d.has(Attr::Vertex) && d.has(Attr::Uvs) && d.has(Attr::Normals));
    assert!(!d.has(Attr::Tangents));
}

/// Opt-in real-data check: `SDC_NIF_GEOM_CORPUS=C:\a\meshes;C:\b\meshes cargo test -p nif --test geom -- --ignored --nocapture`
#[test]
#[ignore]
fn geom_corpus() {
    let roots = std::env::var("SDC_NIF_GEOM_CORPUS").expect("set SDC_NIF_GEOM_CORPUS");
    let (mut files, mut shapes, mut ok, mut normals, mut unit) = (0, 0, 0, 0usize, 0usize);
    let (mut skinned, mut dynamic, mut short) = (0, 0, 0);
    let mut fails = vec![];
    let mut rel = [0usize; 4]; // copy==tris & copy==map(tris), copy==tris only, copy==map(tris) only, neither
    use nif::geom::skin_partition::read_skin_partition;
    for root in roots.split(';').filter(|r| !r.is_empty()) {
        for e in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !e
                .path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("nif"))
            {
                continue;
            }
            let b = std::fs::read(e.path()).unwrap();
            let (Ok(h), Ok(s)) = (Header::parse(&b), scan(&b)) else {
                continue;
            };
            if s.le_mesh {
                continue;
            }
            files += 1;
            // How partition `Triangles` relate to SSE `Triangles Copy` (only where the vertex map is not identity)
            for i in (0..h.sizes.len()).filter(|&i| h.kind(i) == b"NiSkinPartition") {
                let Ok(p) = read_skin_partition(&h, &b, i) else {
                    continue;
                };
                for q in &p.partitions {
                    let identity = q
                        .vertex_map
                        .iter()
                        .enumerate()
                        .all(|(k, &v)| k == v as usize);
                    if identity || q.triangles.is_empty() {
                        continue;
                    }
                    let mapped: Vec<[u16; 3]> = q
                        .triangles
                        .iter()
                        .map(|t| {
                            t.map(|x| q.vertex_map.get(x as usize).copied().unwrap_or(u16::MAX))
                        })
                        .collect();
                    match (q.triangles == q.triangles_copy, mapped == q.triangles_copy) {
                        (true, true) => rel[0] += 1,
                        (true, false) => rel[1] += 1,
                        (false, true) => rel[2] += 1,
                        (false, false) => rel[3] += 1,
                    }
                }
            }
            for row in s.shapes.iter().filter(|r| r.head.vertex_desc.is_some()) {
                shapes += 1;
                match read_shape(&h, &b, row.block as usize) {
                    Ok(m) => {
                        let n = m.positions.len();
                        let bad_idx = m.triangles.iter().flatten().any(|&i| i as usize >= n);
                        if bad_idx || n == 0 {
                            fails.push(format!(
                                "{} block {}: {n} verts, bad index {bad_idx}",
                                e.path().display(),
                                row.block
                            ));
                            continue;
                        }
                        ok += 1;
                        if !row.head.skinned
                            && Some(m.triangles.len() as u32) != row.head.num_triangles
                        {
                            short += 1;
                        }
                        skinned += row.head.skinned as usize;
                        dynamic += (row.head.kind == nif::shape_head::ShapeKind::BSDynamicTriShape)
                            as usize;
                        normals += m.normals.len();
                        unit += unit_ok(&m.normals);
                    }
                    Err(err) => {
                        fails.push(format!("{} block {}: {err}", e.path().display(), row.block))
                    }
                }
            }
        }
    }
    println!(
        "{files} SE files, {shapes} shapes, {ok} decoded ({skinned} skinned, {dynamic} dynamic), {} failed; unit normals {unit}/{normals} = {:.4}",
        fails.len(),
        unit as f64 / normals.max(1) as f64
    );
    println!(
        "unskinned shapes whose triangle count differs from the head (dropped tris): {short}; non-identity partitions: copy==tris&mapped {}, copy==tris only {}, copy==vertexmap(tris) only {}, neither {}",
        rel[0], rel[1], rel[2], rel[3]
    );
    for f in fails.iter().take(30) {
        println!("FAIL {f}");
    }
}

#[test]
fn skin_partition_block_out_of_range_is_an_error_not_a_panic() {
    use nif::geom::skin_partition::read_skin_partition;
    let b = read("TestNifFile_Skinned_SE.nif");
    let h = Header::parse(&b).unwrap();
    for block in [h.sizes.len(), h.sizes.len() + 5, usize::MAX] {
        match read_skin_partition(&h, &b, block) {
            Err(nif::NifError::Geometry { msg, .. }) => assert!(msg.contains("out of range")),
            other => panic!("block {block}: {other:?}"),
        }
    }
}
