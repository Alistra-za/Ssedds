//! DECISIONS FC11: the files an earlier export wrote that the plan no longer writes, and which of them are still
//! Tessalith's own (their recorded `output_stamp` matches the file on disk).
use engine::api::*;
use engine::config::*;
use engine::events::*;
use engine::manifest::*;
use engine::policy::AcceptSuggestions;
use image::{ImageBuffer, Rgba};
use std::path::Path;
use std::sync::Arc;

/// A diffuse `x` and two normals `a_n`, `b_n` under `in/textures/t`.
fn fixture(dir: &Path) {
    let t = dir.join("in/textures/t");
    std::fs::create_dir_all(&t).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| {
        Rgba([(x * 16) as u8, (y * 16) as u8, 90, 255])
    })
    .save(t.join("x.png"))
    .unwrap();
    for n in ["a_n.png", "b_n.png"] {
        ImageBuffer::from_fn(16, 16, |_, _| Rgba([128u8, 128, 255, 255]))
            .save(t.join(n))
            .unwrap();
    }
}

fn plan(dir: &Path) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    Engine::new()
        .unwrap()
        .plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap()
}

/// Plans and runs the fixture, so the manifest holds one record (with its stamp) per output.
fn exported(dir: &Path) {
    fixture(dir);
    let s = Engine::new()
        .unwrap()
        .run(
            plan(dir),
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    assert_eq!(s.converted, 3, "{s:#?}");
}

/// Appends a copy of the record of `from` (an output key), changed by `f`.
fn append_copy(dir: &Path, from: &str, f: impl Fn(&mut serde_json::Value)) {
    let m = Manifest::at(&dir.join("out"));
    let text = std::fs::read_to_string(m.path()).unwrap();
    let mut v: serde_json::Value = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["output"].as_str().is_some_and(|o| o.ends_with(from)))
        .unwrap();
    f(&mut v);
    std::fs::write(m.path(), format!("{text}{v}\n")).unwrap();
}

fn stale(dir: &Path) -> Vec<StaleOutput> {
    stale_with(dir, false)
}

fn stale_with(dir: &Path, inputs_unreadable: bool) -> Vec<StaleOutput> {
    let root = dir.join("out");
    let mut s = stale_outputs(&Manifest::at(&root), &plan(dir), &root, inputs_unreadable);
    s.sort_by(|a, b| a.path.cmp(&b.path));
    s
}

/// Note FC14: while discovery skipped an entry (IN-UNREADABLE), nothing is listed, since that entry's outputs look
/// unclaimed; with every entry read, the same records are listed.
#[test]
fn an_unreadable_input_lists_nothing() {
    let d = tempfile::tempdir().unwrap();
    exported(d.path());
    std::fs::remove_file(d.path().join("in/textures/t/b_n.png")).unwrap();
    assert_eq!(stale_with(d.path(), true), []);
    let got: Vec<_> = stale_with(d.path(), false).iter().map(name).collect();
    assert_eq!(got, ["b_n.dds"]);
}

fn name(s: &StaleOutput) -> String {
    s.path.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn stale_outputs_lists_only_recorded_files() {
    let d = tempfile::tempdir().unwrap();
    exported(d.path());
    // Everything planned is claimed: nothing is stale.
    assert_eq!(stale(d.path()), []);
    let t = d.path().join("out/textures/t");
    // A file no record names (the user's, or another tool's) is never listed.
    std::fs::write(t.join("foreign.dds"), b"not ours").unwrap();
    // A renamed set (AA2): the old output keeps its record and its file; the source is still in the project.
    std::fs::copy(t.join("a_n.dds"), t.join("old_n.dds")).unwrap();
    let stamp = engine::policy::SourceStamp::of(&t.join("old_n.dds")).unwrap();
    append_copy(d.path(), "a_n.dds", |v| {
        v["output"] = "textures/t/old_n.dds".into();
        v["output_stamp"] = serde_json::to_value(stamp).unwrap();
    });
    // A record whose file is gone is not listed.
    append_copy(d.path(), "a_n.dds", |v| {
        v["output"] = "textures/t/gone_n.dds".into();
    });
    // Another project's record in a shared export folder (its source is outside this plan's inputs) is not listed.
    std::fs::copy(t.join("a_n.dds"), t.join("other_n.dds")).unwrap();
    let stamp = engine::policy::SourceStamp::of(&t.join("other_n.dds")).unwrap();
    append_copy(d.path(), "a_n.dds", |v| {
        v["output"] = "textures/t/other_n.dds".into();
        v["source"] = "z:/another/project/other_n.png".into();
        v["output_stamp"] = serde_json::to_value(stamp).unwrap();
    });
    // A deleted source: its output stays on disk, recorded.
    std::fs::remove_file(d.path().join("in/textures/t/b_n.png")).unwrap();
    let s = stale(d.path());
    let got: Vec<_> = s.iter().map(|s| (name(s), s.reason, s.ours)).collect();
    assert_eq!(
        got,
        [
            ("b_n.dds".to_string(), StaleWhy::SourceGone, true),
            ("old_n.dds".to_string(), StaleWhy::NotWritten, true),
        ]
    );
    assert!(s.iter().all(|s| s.path.is_file()));
}

/// DV7 / DR9: a recipe switch leaves the old packed `_rmaos.dds` (or a skin set's `_f.dds`) recorded and no longer
/// planned: listed, spelled as on disk (the manifest key is case-folded).
#[test]
fn a_recipe_switch_lists_the_old_packed_output() {
    let d = tempfile::tempdir().unwrap();
    exported(d.path());
    let t = d.path().join("out/textures/t");
    for (key, on_disk) in [("x_rmaos.dds", "x_RMAOS.dds"), ("x_f.dds", "x_f.dds")] {
        std::fs::copy(t.join("x.dds"), t.join(on_disk)).unwrap();
        let stamp = engine::policy::SourceStamp::of(&t.join(on_disk)).unwrap();
        append_copy(d.path(), "/x.dds", |v| {
            v["output"] = format!("textures/t/{key}").into();
            v["output_stamp"] = serde_json::to_value(stamp).unwrap();
        });
    }
    let got: Vec<_> = stale(d.path())
        .iter()
        .map(|s| (name(s), s.reason, s.ours))
        .collect();
    assert_eq!(
        got,
        [
            ("x_RMAOS.dds".to_string(), StaleWhy::NotWritten, true),
            ("x_f.dds".to_string(), StaleWhy::NotWritten, true),
        ]
    );
}

#[test]
fn a_stamp_mismatch_or_no_stamp_is_not_ours() {
    let d = tempfile::tempdir().unwrap();
    exported(d.path());
    let t = d.path().join("out/textures/t");
    // a_n.dds: edited after Tessalith wrote it (its size changes, so the stamp differs).
    let mut bytes = std::fs::read(t.join("a_n.dds")).unwrap();
    bytes.extend_from_slice(b"edited");
    std::fs::write(t.join("a_n.dds"), bytes).unwrap();
    // b_n.dds: its record has no stamp (written before stamps existed).
    let m = Manifest::at(&d.path().join("out"));
    let text = std::fs::read_to_string(m.path()).unwrap();
    let mut out = String::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
        if v["output"].as_str().unwrap().ends_with("b_n.dds") {
            v.as_object_mut().unwrap().remove("output_stamp");
        }
        out.push_str(&format!("{v}\n"));
    }
    std::fs::write(m.path(), out).unwrap();
    for n in ["a_n.png", "b_n.png"] {
        std::fs::remove_file(d.path().join("in/textures/t").join(n)).unwrap();
    }
    let got: Vec<_> = stale(d.path()).iter().map(|s| (name(s), s.ours)).collect();
    assert_eq!(
        got,
        [
            ("a_n.dds".to_string(), false),
            ("b_n.dds".to_string(), false)
        ]
    );
}
