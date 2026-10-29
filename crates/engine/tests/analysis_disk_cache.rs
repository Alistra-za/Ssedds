//! The analysis disk cache (DECISIONS Note DX): a later session decodes nothing it already analysed.
use encoders_sys::Bc1Model;
use engine::analysis_store::AnalysisStore;
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::policy::{AcceptSuggestions, ResolvedJob};
use engine::session::{AnalysisCache, Session};
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

fn png_rgb(dir: &Path, rel: &str, w: u32) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, 32, |x, y| Rgb([(x * 7) as u8, (y * 5) as u8, 90]))
        .save(p)
        .unwrap();
}

fn png_gray(dir: &Path, rel: &str, v: u8) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(32, 32, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(p)
        .unwrap();
}

/// Three plain images plus a packed `_rmaos` recipe (two gray members).
fn fixtures(dir: &Path) {
    png_rgb(dir, "textures/a/one.png", 32);
    png_rgb(dir, "textures/a/two.png", 32);
    let three = dir.join("in/textures/a/three.png");
    ImageBuffer::from_fn(32, 32, |x, _| Rgba([x as u8, 3, 4, 200]))
        .save(three)
        .unwrap();
    png_gray(dir, "textures/pbr/iron_roughness.png", 120);
    png_gray(dir, "textures/pbr/iron_ao.png", 200);
}

fn request(dir: &Path) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r
}

fn session(cache: &Path) -> Session {
    Session::new(Bc1Model::Ideal)
        .unwrap()
        .with_disk_cache(cache.to_path_buf())
}

fn plan(s: &Session, dir: &Path) -> Plan {
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    s.plan(request(dir), &rep, &CancelToken::default()).unwrap()
}

fn entries(cache: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(cache)
        .map(|d| d.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default()
}

fn facts(s: &Session, p: &Plan) -> Vec<(String, Option<engine::analyse::Analysis>, bool)> {
    let one = |j: &ResolvedJob| {
        (
            j.output.to_string_lossy().into_owned(),
            s.cache().analysis(j),
            s.thumbnail(j).is_some(),
        )
    };
    p.jobs.iter().map(one).collect()
}

#[test]
fn a_second_session_decodes_nothing() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let cache = d.path().join("cache");
    let a = session(&cache);
    let pa = plan(&a, d.path());
    assert!(a.cache().loads() > 0);
    assert!(pa.jobs.iter().any(|j| j.pack.is_some()), "a packed job");
    let b = session(&cache);
    let pb = plan(&b, d.path());
    assert_eq!(b.cache().loads(), 0);
    let (fa, fb) = (facts(&a, &pa), facts(&b, &pb));
    assert_eq!(fa, fb);
    assert!(fa.iter().all(|f| f.1.is_some() && f.2));
    for (ja, jb) in pa.jobs.iter().zip(&pb.jobs) {
        assert_eq!(a.thumbnail(ja), b.thumbnail(jb));
    }
}

#[test]
fn an_edited_file_is_decoded_again() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let cache = d.path().join("cache");
    plan(&session(&cache), d.path());
    png_rgb(d.path(), "textures/a/one.png", 48);
    let b = session(&cache);
    let p = plan(&b, d.path());
    assert_eq!(b.cache().loads(), 1);
    let job = p
        .jobs
        .iter()
        .find(|j| j.input.ends_with("one.png"))
        .unwrap();
    assert_eq!(b.cache().analysis(job).unwrap().width, 48);
}

#[test]
fn other_hints_miss() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let store = d.path().join("cache");
    let run = |checks: bool| {
        let mut cache = AnalysisCache::default();
        cache.checks = checks;
        cache.disk = Some(AnalysisStore::new(store.clone()));
        let rep = Reporter::new(Arc::new(VecSink::default()), "t");
        let p = Engine::new()
            .unwrap()
            .plan_cached(request(d.path()), &rep, &CancelToken::default(), &cache)
            .unwrap();
        let has_checks = p
            .jobs
            .iter()
            .filter_map(|j| cache.analysis(j))
            .all(|a| a.checks.is_some() == checks);
        (cache.loads(), has_checks)
    };
    let (off, ok) = run(false);
    assert!(off > 0 && ok);
    let (on, ok) = run(true);
    assert_eq!((on, ok), (off, true), "checks on is a miss");
    assert_eq!(run(false), (0, true), "the earlier hints still hit");
}

#[test]
fn a_corrupt_entry_is_a_miss_and_is_replaced() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let cache = d.path().join("cache");
    let a = session(&cache);
    plan(&a, d.path());
    let first = a.cache().loads();
    let files = entries(&cache);
    assert!(!files.is_empty());
    for f in &files {
        let len = std::fs::metadata(f).unwrap().len();
        std::fs::OpenOptions::new()
            .write(true)
            .open(f)
            .unwrap()
            .set_len(len / 2)
            .unwrap();
    }
    let b = session(&cache);
    plan(&b, d.path());
    assert_eq!(b.cache().loads(), first);
    let c = session(&cache);
    plan(&c, d.path());
    assert_eq!(c.cache().loads(), 0, "the entries were rewritten");
}

#[test]
fn without_a_disk_dir_nothing_is_written() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    let s = Session::new(Bc1Model::Ideal).unwrap();
    plan(&s, d.path());
    assert!(!d.path().join("out").exists());
    let mut top: Vec<_> = entries(d.path())
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    top.sort();
    assert_eq!(top, ["in"]);
}

/// Load time of a real folder, cold then warm (Note DX11). Read-only on `SDC_LOADTIME_DIR`; the store is a tempdir.
#[test]
#[ignore]
fn load_time_cold_then_warm() {
    let Some(dir) = std::env::var_os("SDC_LOADTIME_DIR") else {
        println!("skipped: SDC_LOADTIME_DIR unset");
        return;
    };
    let cache = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let run = || {
        // Session::new has thumbnails and checks on, as the GUI's session does.
        let s = session(cache.path());
        let mut r = BatchRequest::new(
            vec![dir.clone().into()],
            out.path().to_path_buf(),
            Target::Cs,
        );
        r.recursive = true;
        let rep = Reporter::new(Arc::new(VecSink::default()), "t");
        let t = std::time::Instant::now();
        s.plan(r, &rep, &CancelToken::default()).unwrap();
        (t.elapsed(), s.cache().loads())
    };
    let (cold, cold_loads) = run();
    let (warm, warm_loads) = run();
    println!("cold {cold:?} ({cold_loads} loads), warm {warm:?} ({warm_loads} loads)");
    assert_eq!(warm_loads, 0);
}
