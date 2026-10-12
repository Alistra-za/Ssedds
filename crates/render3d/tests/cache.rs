//! 03 T-C2: pruning is pure filesystem work (tempdir, explicit `now`); a hit refreshes the entry's age.
use render3d::cache::{prune, touch, PruneLimits};
use render3d::compile::{cached, compile};
use render3d::permutation::Defines;
use render3d::source::SourceSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

const DAY: u64 = 86_400;

fn ago(now: SystemTime, days: u64) -> SystemTime {
    now - Duration::from_secs(days * DAY)
}

fn put(dir: &Path, name: &str, len: usize, mtime: SystemTime) {
    let p = dir.join(name);
    fs::write(&p, vec![7u8; len]).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
}

/// A cache entry: 1000-byte `.cso` + 100-byte `.json`.
fn entry(dir: &Path, key: &str, mtime: SystemTime) {
    put(dir, &format!("{key}.cso"), 1000, mtime);
    put(dir, &format!("{key}.json"), 100, mtime);
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn over_the_cap_the_oldest_entries_go_first() {
    let d = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    for (i, key) in ["a", "b", "c", "d"].iter().enumerate() {
        entry(d.path(), key, ago(now, 4 - i as u64)); // a oldest (4 days), d newest (1 day)
    }
    let limits = PruneLimits {
        max_bytes: 2500,
        ..PruneLimits::default()
    };
    let r = prune(d.path(), &limits, now);
    assert_eq!(names(d.path()), ["c.cso", "c.json", "d.cso", "d.json"]);
    assert_eq!((r.removed, r.bytes_after), (4, 2200));
    // Already under the cap: nothing more.
    assert_eq!(prune(d.path(), &limits, now).removed, 0);
}

#[test]
fn entries_untouched_for_90_days_expire() {
    let d = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    entry(d.path(), "old", ago(now, 91));
    entry(d.path(), "fresh", ago(now, 89));
    let r = prune(d.path(), &PruneLimits::default(), now);
    assert_eq!(names(d.path()), ["fresh.cso", "fresh.json"]);
    assert_eq!((r.removed, r.bytes_after), (2, 1100));
}

#[test]
fn stale_temp_files_and_orphan_cso_go_after_a_day_unrelated_files_stay() {
    let d = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    put(d.path(), ".tmp-1-0", 50, ago(now, 2));
    put(d.path(), ".tmp-1-1", 50, now);
    put(d.path(), "orphan_old.cso", 500, ago(now, 2));
    put(d.path(), "orphan_young.cso", 500, now); // a writer may be between its renames
    put(d.path(), "notes.txt", 10, ago(now, 400));
    let r = prune(d.path(), &PruneLimits::default(), now);
    assert_eq!(
        names(d.path()),
        [".tmp-1-1", "notes.txt", "orphan_young.cso"]
    );
    assert_eq!(r.removed, 2);
    assert_eq!(
        r.bytes_after,
        50 + 500,
        "young tmp + young orphan; notes.txt is not counted"
    );
    // A missing directory is not an error.
    assert_eq!(
        prune(&d.path().join("gone"), &PruneLimits::default(), now).removed,
        0
    );
}

fn tiny() -> (SourceSet, Defines) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    (
        SourceSet::new(dir.into()),
        Defines(vec![("PSHADER".into(), "1".into())]),
    )
}

#[test]
fn a_hit_refreshes_the_entry_age() {
    let cache = tempfile::tempdir().unwrap();
    let (set, d) = tiny();
    let src = set.get(Path::new("tiny.hlsl")).unwrap();
    compile(&src, &d, "ps_5_0", cache.path()).unwrap();
    let json = fs::read_dir(cache.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "json"))
        .unwrap();
    let old = SystemTime::now() - Duration::from_secs(30 * DAY);
    fs::OpenOptions::new()
        .write(true)
        .open(&json)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert!(cached(&src, &d, "ps_5_0", cache.path()).is_some());
    let age = SystemTime::now()
        .duration_since(fs::metadata(&json).unwrap().modified().unwrap())
        .unwrap();
    assert!(age < Duration::from_secs(60), "touched on the hit: {age:?}");
    // `touch` on its own, and on a path that is not there.
    fs::OpenOptions::new()
        .write(true)
        .open(&json)
        .unwrap()
        .set_modified(old)
        .unwrap();
    touch(&json);
    assert!(fs::metadata(&json).unwrap().modified().unwrap() > old);
    touch(&cache.path().join("missing.json"));
}

/// Pruning an entry away is a plain miss: the next compile writes it again.
#[test]
fn a_pruned_entry_is_recompiled() {
    let cache = tempfile::tempdir().unwrap();
    let (set, d) = tiny();
    let src = set.get(Path::new("tiny.hlsl")).unwrap();
    let first = compile(&src, &d, "ps_5_0", cache.path()).unwrap();
    let limits = PruneLimits {
        max_bytes: 0,
        ..PruneLimits::default()
    };
    let r = prune(cache.path(), &limits, SystemTime::now());
    assert_eq!((r.removed, r.bytes_after), (2, 0));
    assert!(cached(&src, &d, "ps_5_0", cache.path()).is_none());
    let again = compile(&src, &d, "ps_5_0", cache.path()).unwrap();
    assert_eq!(again.bytecode, first.bytecode);
    assert!(cached(&src, &d, "ps_5_0", cache.path()).is_some());
}

/// The only test in this binary that starts the process-wide startup prune.
#[test]
fn the_startup_prune_runs_off_the_calling_thread() {
    let d = tempfile::tempdir().unwrap();
    let stale = d.path().join(".tmp-9-9");
    put(
        d.path(),
        ".tmp-9-9",
        10,
        SystemTime::now() - Duration::from_secs(3 * DAY),
    );
    render3d::cache::prune_in_background(d.path().to_path_buf());
    let t = std::time::Instant::now();
    while stale.exists() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!stale.exists());
}
