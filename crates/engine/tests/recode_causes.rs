//! DECISIONS FC7: after an upgrade, what re-encodes and why, read from the manifest's records only.
use engine::api::*;
use engine::config::*;
use engine::events::*;
use engine::manifest::*;
use engine::policy::{AcceptSuggestions, JobStatus};
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
    let p = Engine::new()
        .unwrap()
        .plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    let ready = p.jobs.iter().filter(|j| j.status == JobStatus::Ready);
    assert_eq!(ready.count(), 3, "{:#?}", p.jobs);
    p
}

/// Plans and runs the fixture, so the manifest holds one record per output.
fn exported(dir: &Path) -> Plan {
    fixture(dir);
    let p = plan(dir);
    let s = Engine::new()
        .unwrap()
        .run(
            p,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap();
    assert_eq!(s.converted, 3, "{s:#?}");
    plan(dir)
}

/// Rewrites every manifest line; `f` gets the output key and the record, and drops the line by returning false.
fn rewrite(dir: &Path, f: impl Fn(&str, &mut serde_json::Value) -> bool) {
    let m = Manifest::at(&dir.join("out"));
    let text = std::fs::read_to_string(m.path()).unwrap();
    let mut out = String::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let mut v: serde_json::Value = serde_json::from_str(line).unwrap();
        let key = v["output"].as_str().unwrap().to_string();
        if f(&key, &mut v) {
            out.push_str(&v.to_string());
            out.push('\n');
        }
    }
    std::fs::write(m.path(), out).unwrap();
}

fn causes(dir: &Path, p: &Plan) -> Vec<RecodeCause> {
    let mut c = recode_causes(&Manifest::at(&dir.join("out")), p);
    c.sort_by_key(|c| c.line());
    c
}

fn is_normal(key: &str) -> bool {
    key.ends_with("_n.dds")
}

#[test]
fn encode_version_cause_counts_per_type() {
    let d = tempfile::tempdir().unwrap();
    let p = exported(d.path());
    let now = encode_version(Subtype::Normal);
    rewrite(d.path(), |_, v| {
        v["engine_version"] = "1.0.0".into();
        v["encode_version"] = "0.9.0".into();
        true
    });
    let c = causes(d.path(), &p);
    let diffuse = encode_version(Subtype::Diffuse);
    assert_eq!(
        c,
        [
            RecodeCause {
                why: RecodeWhy::EncodeVersion {
                    subtype: Subtype::Diffuse,
                    from: "0.9.0".into(),
                    to: diffuse.into(),
                },
                count: 1,
            },
            RecodeCause {
                why: RecodeWhy::EncodeVersion {
                    subtype: Subtype::Normal,
                    from: "0.9.0".into(),
                    to: now.into(),
                },
                count: 2,
            },
        ]
    );
    assert_eq!(
        c[1].line(),
        format!("2 files re-encode: the normal encoder changed in {now}")
    );
    assert_eq!(
        c[0].line(),
        format!("1 file re-encodes: the diffuse encoder changed in {diffuse}")
    );
}

#[test]
fn profile_changed_cause() {
    let d = tempfile::tempdir().unwrap();
    let p = exported(d.path());
    // Written by this engine: a changed hash is the user's own edit, not an upgrade's, so nothing is said.
    rewrite(d.path(), |k, v| {
        if is_normal(k) {
            v["profile_sha256"] = "0".repeat(64).into();
        }
        true
    });
    assert_eq!(causes(d.path(), &p), []);
    // Written by 1.0.0 (no encode_version, so 1.0.0's): the normals' settings changed.
    rewrite(d.path(), |_, v| {
        v["engine_version"] = "1.0.0".into();
        v.as_object_mut().unwrap().remove("encode_version");
        true
    });
    let c = causes(d.path(), &p);
    let want = |st| RecodeCause {
        why: RecodeWhy::ProfileChanged { subtype: st },
        count: 2,
    };
    if encode_version(Subtype::Normal) == LEGACY_ENCODE_VERSION {
        assert_eq!(c, [want(Subtype::Normal)]);
        assert_eq!(c[0].line(), "2 files re-encode: their settings changed");
    } else {
        assert!(
            c.iter().all(|c| c.why != want(Subtype::Normal).why),
            "{c:?}"
        );
    }
    // Per record: one normal rewritten by this engine (its edit is not listed), the other still 1.0.0's (listed).
    rewrite(d.path(), |k, v| {
        if k.ends_with("a_n.dds") {
            v["engine_version"] = ENGINE_VERSION.into();
            v["encode_version"] = encode_version(Subtype::Normal).into();
        }
        true
    });
    if encode_version(Subtype::Normal) == LEGACY_ENCODE_VERSION {
        assert_eq!(
            causes(d.path(), &p),
            [RecodeCause {
                count: 1,
                ..want(Subtype::Normal)
            }]
        );
    }
}

#[test]
fn new_output_cause() {
    let d = tempfile::tempdir().unwrap();
    let p = exported(d.path());
    // The diffuse has no record, and every other record is an older engine's: a new output after the upgrade.
    rewrite(d.path(), |k, v| {
        v["engine_version"] = "1.0.0".into();
        is_normal(k)
    });
    let c = causes(d.path(), &p);
    assert_eq!(
        c,
        [RecodeCause {
            why: RecodeWhy::NewOutput {
                subtype: Subtype::Diffuse
            },
            count: 1,
        }]
    );
    assert_eq!(c[0].line(), "1 file re-encodes: new output type");
    // Once this engine has written to the manifest, a file without a record is the user's new file: nothing is said.
    rewrite(d.path(), |_, v| {
        v["engine_version"] = ENGINE_VERSION.into();
        true
    });
    assert_eq!(causes(d.path(), &p), []);
}

#[test]
fn no_manifest_gives_first_export() {
    let d = tempfile::tempdir().unwrap();
    fixture(d.path());
    let p = plan(d.path());
    let c = causes(d.path(), &p);
    assert_eq!(
        c,
        [RecodeCause {
            why: RecodeWhy::FirstExport {
                version: ENGINE_VERSION.into()
            },
            count: 3,
        }]
    );
    assert_eq!(
        c[0].line(),
        format!("First export with {ENGINE_VERSION}: 3 files")
    );
}

#[test]
fn a_record_with_no_baseline_counts_as_first_export() {
    let d = tempfile::tempdir().unwrap();
    let p = exported(d.path());
    rewrite(d.path(), |k, v| {
        if is_normal(k) {
            v["engine_version"] = "0.9.0".into();
            v.as_object_mut().unwrap().remove("encode_version");
        }
        true
    });
    assert_eq!(
        causes(d.path(), &p),
        [RecodeCause {
            why: RecodeWhy::FirstExport {
                version: ENGINE_VERSION.into()
            },
            count: 2,
        }]
    );
}

#[test]
fn an_up_to_date_manifest_says_nothing() {
    let d = tempfile::tempdir().unwrap();
    let p = exported(d.path());
    assert_eq!(causes(d.path(), &p), []);
}
