//! 03 T-E3..T-E7 and the load-time behaviour of the background compile. The test is the pool: no threads, it
//! takes the jobs, runs them with `run_job`, and can withhold or reorder the results.
mod common;
use common::{cache_files, channel, covered, fixture, renderer, FLAT};
use nif::material::MaterialState;
use nif::props::Alpha;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{parse, run_job, Builtin, EditKind, Renderer, ShapeId, ShapeStatus};
use std::collections::HashSet;
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const CYL2: ShapeId = ShapeId(12);

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

fn material(r: &Renderer, id: ShapeId) -> MaterialState {
    r.shapes()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
        .material
}

fn status(r: &Renderer, id: ShapeId) -> ShapeStatus {
    r.shapes().into_iter().find(|s| s.id == id).unwrap().status
}

/// Everything queued so far, compiled on this thread and applied. Returns how many jobs there were.
fn drain(r: &mut Renderer) -> usize {
    let jobs = r.take_jobs();
    let n = jobs.len();
    for j in jobs {
        r.apply_compiled(run_job(j));
    }
    n
}

fn furniture(cache: &Path) -> Renderer {
    let mut r = renderer(cache, FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 2), "{rep:?}");
    assert!(
        rep.compile_errors.is_empty() && rep.errors.is_empty(),
        "{rep:?}"
    );
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: false
        }
    );
    assert!(drain(&mut r) >= 1);
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(status(&r, CYL2), ShapeStatus::Drawing);
    r
}

/// Vanilla → TruePBR with Rim (Subsurface): a permutation edit whose pixels differ (the stand-in writes 0.5
/// green when PBRFlags ≠ 0).
fn pbr_subsurface(mut m: MaterialState) -> MaterialState {
    m.slsf2 &= !((1 << 12) | (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27));
    m.slsf2 |= (1 << 23) | (1 << 26);
    m.params.emissive_color = [0.0; 3];
    m
}

fn alpha_test(mut m: MaterialState) -> MaterialState {
    m.alpha = Some(Alpha {
        flags: 0x200,
        threshold: 0,
    });
    m
}

#[test]
fn a_first_load_queues_one_job_per_program_and_draws_the_clear_colour_meanwhile() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!(
        (rep.shapes, rep.pending),
        (2, 2),
        "pending counts as drawable"
    );
    assert!(
        r.bounds().is_some(),
        "framing works before any program lands"
    );
    let jobs = r.take_jobs();
    assert!(!jobs.is_empty());
    let keys: HashSet<_> = jobs.iter().map(|j| j.key.clone()).collect();
    assert_eq!(keys.len(), jobs.len(), "one job per distinct program");
    assert!(r.take_jobs().is_empty());
    assert_eq!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()), 0);
    for j in jobs {
        assert!(r.apply_compiled(run_job(j)));
    }
    assert!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()) > 0);
}

/// `load` is the compatibility wrapper: it settles even in async mode, with the P0 report.
#[test]
fn load_settles_even_when_the_renderer_is_async() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
    r.set_async_compile(true);
    let rep = r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &none);
    assert_eq!(rep.pending, 0);
    assert_eq!(rep.compile_errors.len(), 1, "{:?}", rep.compile_errors);
    assert!(rep.compile_errors[0].starts_with("SkullPeg01:19: ps:"));
    assert!(r.take_jobs().is_empty());
}

#[test]
fn a_disk_cache_hit_is_applied_at_load_with_no_job() {
    let cache = tempfile::tempdir().unwrap();
    furniture(cache.path()); // writes the entries
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 0), "{rep:?}");
    assert!(r.take_jobs().is_empty());
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
}

/// 03 T-E3
#[test]
fn permutation_edit_keeps_old_draw_until_compiled() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let before = r.render(&scene, 64, 64).unwrap();
    let m = pbr_subsurface(material(&r, CYL1));
    assert_eq!(
        r.set_material(CYL1, m.clone(), &none),
        Ok(EditKind::Permutation)
    );
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    assert_eq!(status(&r, CYL2), ShapeStatus::Drawing);
    assert_eq!(material(&r, CYL1), m, "the wanted material is reported");
    let jobs = r.take_jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        r.render(&scene, 64, 64).unwrap(),
        before,
        "byte-identical while the program compiles"
    );
    // A second edit for the same program queues nothing and is what lands.
    let mut m2 = m.clone();
    m2.params.glossiness = 7.0;
    assert_eq!(
        r.set_material(CYL1, m2.clone(), &none),
        Ok(EditKind::Cbuffer)
    );
    assert!(r.take_jobs().is_empty(), "in flight already");
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    let files = cache_files(cache.path());
    let done = run_job(jobs.into_iter().next().unwrap());
    assert!(done.result.is_ok());
    assert!(r.apply_compiled(done));
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(material(&r, CYL1), m2);
    assert!(
        cache_files(cache.path()) > files,
        "the worker wrote the cache"
    );
    let after = r.render(&scene, 64, 64).unwrap();
    assert!(
        channel(&after, 1) > channel(&before, 1) + 1000,
        "green {} → {}",
        channel(&before, 1),
        channel(&after, 1)
    );
}

/// 03 T-E4 / Review Focus #3
#[test]
fn failing_permutation_keeps_old_draw() {
    let cache = tempfile::tempdir().unwrap();
    // No STANDIN_OPAQUE_OK: any permutation without DO_ALPHA_TEST fails to compile.
    let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_DeepGraph_SE.nif")), &none);
    assert!(
        rep.compile_errors.is_empty(),
        "failures arrive later: {rep:?}"
    );
    drain(&mut r);
    let peg = ShapeId(103);
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    assert!(matches!(
        status(&r, ShapeId(143)),
        ShapeStatus::Failed {
            showing_previous: false,
            ..
        }
    ));
    let scene = Scene::neutral();
    let before = r.render(&scene, 96, 96).unwrap();
    let good = material(&r, peg);
    let mut bad = good.clone();
    bad.alpha = None;
    assert_eq!(
        r.set_material(peg, bad.clone(), &none),
        Ok(EditKind::Permutation)
    );
    // Shape 143 already failed on this very permutation (no alpha test), so the failure is memoised under its
    // key: the edit fails at once with no job, and the old draw stays.
    assert!(r.take_jobs().is_empty());
    assert!(
        matches!(status(&r, peg), ShapeStatus::Failed { error, showing_previous: true } if error.starts_with("ps:"))
    );
    assert_eq!(material(&r, peg), bad);
    assert_eq!(r.render(&scene, 96, 96).unwrap(), before, "old pixels stay");
    assert_eq!(
        status(&r, ShapeId(107)),
        ShapeStatus::Drawing,
        "others unaffected"
    );
    // Back to the good material: its program is still in memory, so the swap is immediate.
    assert_eq!(r.set_material(peg, good, &none), Ok(EditKind::Permutation));
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    assert!(r.take_jobs().is_empty());
    assert_eq!(r.render(&scene, 96, 96).unwrap(), before);
}

/// 03 T-E5: vertex colours on and off change the VS inputs, so the swap rebuilds the input layout.
#[test]
fn swap_rebuilds_input_layout() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let before = r.render(&scene, 64, 64).unwrap();
    let off = material(&r, CYL1);
    let mut on = off.clone();
    on.has_vertex_colors = true;
    on.slsf2 |= 1 << 5; // Vertex_Colors → VC (VS)
    assert_eq!(r.set_material(CYL1, on, &none), Ok(EditKind::Permutation));
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    assert_eq!(drain(&mut r), 1);
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(!r.gpu().is_removed());
    assert_eq!(
        r.render(&scene, 64, 64).unwrap(),
        before,
        "white vertex colour"
    );
    // And back: the first program is in memory, no job, drawn at once.
    r.set_material(CYL1, off, &none).unwrap();
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(r.take_jobs().is_empty());
    assert_eq!(r.render(&scene, 64, 64).unwrap(), before);
}

/// 03 T-E6: edits A then B; B's program lands first, A's later. B stays on screen, A is only memoised.
#[test]
fn out_of_order_results() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let base = material(&r, CYL1);
    let a = alpha_test(base.clone());
    let b = pbr_subsurface(base);
    r.set_material(CYL1, a.clone(), &none).unwrap();
    let job_a = r.take_jobs().into_iter().next().unwrap();
    r.set_material(CYL1, b.clone(), &none).unwrap();
    let job_b = r.take_jobs().into_iter().next().unwrap();
    assert_ne!(job_a.key, job_b.key);
    assert!(r.apply_compiled(run_job(job_b)), "B is wanted");
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(material(&r, CYL1), b);
    let on_screen = r.render(&scene, 64, 64).unwrap();
    assert!(
        !r.apply_compiled(run_job(job_a)),
        "A is stale: memoised only"
    );
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(r.render(&scene, 64, 64).unwrap(), on_screen);
    // The memo works: asking for A now needs no compile.
    r.set_material(CYL1, a, &none).unwrap();
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(r.take_jobs().is_empty());
}

/// A result for content the user already left is harmless: it applies to no shape of the new content.
#[test]
fn a_result_for_a_replaced_load_only_warms_the_memo() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    let old_jobs = r.take_jobs();
    let mut pbr = pbr_subsurface(MaterialState::builtin_default());
    pbr.skinned = false;
    let rep = r.load_builtin(Builtin::Sphere, pbr, &none);
    assert_eq!((rep.shapes, rep.pending), (1, 1));
    let new_jobs = r.take_jobs();
    assert_eq!(new_jobs.len(), 1);
    for j in old_jobs {
        assert!(!r.apply_compiled(run_job(j)), "nobody waits for it");
    }
    assert_eq!(
        status(&r, ShapeId(0)),
        ShapeStatus::Pending {
            showing_previous: false
        }
    );
    assert!(r.apply_compiled(run_job(new_jobs.into_iter().next().unwrap())));
    assert_eq!(status(&r, ShapeId(0)), ShapeStatus::Drawing);
}

/// 03 T-E7: a rebuilt device reuses `compiled`: nothing is compiled or read from disk again.
#[test]
fn device_rebuild_reuses_compiled() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let before = r.render(&scene, 64, 64).unwrap();
    for e in std::fs::read_dir(cache.path()).unwrap() {
        std::fs::remove_file(e.unwrap().path()).unwrap();
    }
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    assert!(
        r.shapes().is_empty(),
        "the loaded model is gone: load again"
    );
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 0), "{rep:?}");
    assert!(r.take_jobs().is_empty());
    assert_eq!(cache_files(cache.path()), 0, "no compile, no disk entry");
    assert_eq!(r.render(&scene, 64, 64).unwrap(), before);
    assert_eq!(r.features(), common::FLAT, "features survive the rebuild");
}

/// Review Focus 1, async path: a not-yet-memoised failing permutation keeps the old pixels, then fails with
/// `showing_previous`.
#[test]
fn an_unmemoised_failing_permutation_keeps_the_old_draw_until_it_fails() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
    r.set_async_compile(true);
    r.load_parsed(&parse(&fixture("TestNifFile_DeepGraph_SE.nif")), &none);
    drain(&mut r);
    let peg = ShapeId(103);
    let scene = Scene::neutral();
    let before = r.render(&scene, 96, 96).unwrap();
    // No alpha test fails; Soft_Lighting makes it a permutation shape 143 never asked for.
    let mut bad = material(&r, peg);
    bad.alpha = None;
    bad.slsf2 |= 1 << 25; // Soft_Lighting
    assert_eq!(
        r.set_material(peg, bad.clone(), &none),
        Ok(EditKind::Permutation)
    );
    let jobs = r.take_jobs();
    assert_eq!(jobs.len(), 1, "not memoised: a job is queued");
    assert_eq!(
        status(&r, peg),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    assert_eq!(r.render(&scene, 96, 96).unwrap(), before);
    assert!(r.apply_compiled(run_job(jobs.into_iter().next().unwrap())));
    assert!(
        matches!(status(&r, peg), ShapeStatus::Failed { error, showing_previous: true } if error.starts_with("ps:"))
    );
    assert_eq!(material(&r, peg), bad);
    assert_eq!(r.render(&scene, 96, 96).unwrap(), before, "old pixels stay");
}

/// A worker panic is not memoised: the next edit queues the job again.
#[test]
fn a_panicked_job_is_not_memoised() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    let job = r.take_jobs().into_iter().next().unwrap();
    let done = render3d::CompileDone {
        key: job.key,
        result: Err("internal error: the shader compiler panicked".into()),
    };
    assert!(r.apply_compiled(done));
    assert!(matches!(status(&r, CYL1), ShapeStatus::Failed { .. }));
    let m = material(&r, CYL1);
    r.set_material(CYL1, m, &none).unwrap();
    assert_eq!(r.take_jobs().len(), 1, "retried, not served from the memo");
}

/// Sync mode compiles inline: a `Textures` edit after `set_shaders` (programs dropped) still works.
#[test]
fn a_sync_texture_edit_after_set_shaders_compiles_inline() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    r.set_shaders(render3d::locate::fingerprint(&common::standin_root(), None).unwrap());
    let red = common::dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == "textures/standin/red.dds").then(|| red.clone());
    let mut m = material(&r, CYL1);
    m.set_slot_raw(0, r"Textures\Standin\Red.dds");
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Textures));
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
}

/// Trace-debug #21: a sync load whose program the pool already holds (in flight, not applied) still settles.
#[test]
fn sync_load_compiles_key_already_taken_by_pool() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let nif = parse(&fixture("TestNifFile_Furniture_Col_SE.nif"));
    r.load_parsed(&nif, &none);
    let taken = r.take_jobs();
    assert!(!taken.is_empty());
    let rep = r.load_parsed_sync(&nif, &none);
    assert_eq!((rep.shapes, rep.pending), (2, 0), "{rep:?}");
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    // The pool's late results only warm the memo.
    for j in taken {
        assert!(!r.apply_compiled(run_job(j)));
    }
}

/// Trace-debug #16: a replaced load's queued jobs must not hold up the new load in the pool's FIFO.
#[test]
fn a_replaced_loads_jobs_are_dropped_not_queued_behind_the_new_load() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let nif = parse(&fixture("TestNifFile_Furniture_Col_SE.nif"));
    r.load_parsed(&nif, &none);
    let sent = r.take_jobs(); // handed to the pool
    assert!(!sent.is_empty());
    r.load_parsed(&nif, &none);
    let unsent = r.take_jobs();
    // the unsent queue holds only the new load's jobs, and the same program is queued again
    assert_eq!(unsent.len(), sent.len());
    assert!(sent.iter().all(|j| !j.is_current()), "old jobs are stale");
    assert!(unsent.iter().all(|j| j.is_current()));
    // an edit after the load is current too
    r.load_parsed(&nif, &none);
    r.load_parsed(&nif, &none);
    assert_eq!(r.take_jobs().len(), sent.len(), "no pile-up of unsent jobs");
}
