use engine::codes::{Code, Warning};
use engine::config::*;
use engine::events::*;
use engine::manifest::*;
use engine::policy::{JobStatus, ResolvedJob};
use engine::presets::PresetRegistry;
use engine::shader::{Shader, ShaderOrigin, ShaderPick};
use std::sync::Arc;

fn record(
    output: &str,
    source_sha256: &str,
    profile: Profile,
    preset: PresetRef,
) -> ManifestRecord {
    ManifestRecord {
        schema: MANIFEST_SCHEMA,
        ts: now_rfc3339(),
        engine_version: ENGINE_VERSION.into(),
        encode_version: Some(encode_version(profile.subtype).into()),
        engine_commit: engine_commit().into(),
        encoders: EncoderVersions::current(),
        isa: dispatched_isa().into(),
        preset,
        profile_sha256: sha256_json(&profile).unwrap(),
        profile,
        provenance: Default::default(),
        source: "in/x_n.png".into(),
        source_sha256: source_sha256.into(),
        sources: vec![],
        output: output.into(),
        validation: Default::default(),
        bc1_model: Some("ideal".into()),
        shader: Some(ShaderPick {
            shader: Shader::Default,
            provenance: ShaderOrigin::Fallback,
            wanted: Some(Shader::Pbr),
        }),
        output_stamp: None,
        copied_from: None,
    }
}

fn cs_quality() -> (Profile, PresetRef) {
    let reg = PresetRegistry::builtin().unwrap();
    let profile = reg
        .profile("cs-quality", Target::Cs, Subtype::Normal)
        .unwrap();
    let preset = PresetRef::for_preset(&reg, "cs-quality", None).unwrap();
    (profile, preset)
}

#[test]
fn events_serialise_as_flat_json_lines_with_seq() {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "b1");
    rep.emit(
        None,
        None,
        EventKind::BatchStarted {
            target: Target::Cs,
            preset: "cs-quality".into(),
            inputs: 2,
        },
    );
    rep.emit(
        Some("set"),
        Some(std::path::Path::new("a/x.png")),
        EventKind::Warning(Warning::new(Code::DimNpot, "npot")),
    );
    rep.emit(None, None, EventKind::FileStarted);
    let ev = sink.take();
    assert_eq!(ev.iter().map(|e| e.seq).collect::<Vec<_>>(), [0, 1, 2]);
    let line = serde_json::to_string(&ev[1]).unwrap();
    assert!(
        line.contains("\"kind\":\"warning\"")
            && line.contains("\"code\":\"DIM-NPOT\"")
            && line.contains("\"schema\":2"),
        "{line}"
    );
    assert!(line.contains("\"path\":\"a/x.png\""));
    let back: EngineEvent = serde_json::from_str(&line).unwrap();
    assert_eq!(back, ev[1]);

    let mut buf = Vec::new();
    {
        let jl = JsonLinesSink::new(&mut buf);
        jl.emit(&ev[0]);
        jl.emit(&ev[2]);
    }
    let text = String::from_utf8(buf).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text
        .lines()
        .nth(1)
        .unwrap()
        .contains("\"kind\":\"file_started\""));
}

#[test]
fn timestamps_are_rfc3339_utc() {
    let sink = Arc::new(VecSink::default());
    Reporter::new(sink.clone(), "b").emit(None, None, EventKind::FileStarted);
    let ts = sink.take().remove(0).ts;
    // 2026-10-09T12:34:56.789Z
    let b = ts.as_bytes();
    assert_eq!(ts.len(), 24, "{ts}");
    assert!(
        ts.ends_with('Z') && b[4] == b'-' && b[10] == b'T' && b[19] == b'.',
        "{ts}"
    );
    assert!(humantime::parse_rfc3339(&ts).is_ok(), "{ts}");
    assert!(humantime::parse_rfc3339(&now_rfc3339()).is_ok());
}

#[test]
fn paths_use_forward_slashes_and_every_kind_round_trips() {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), new_batch_id());
    let kinds = vec![
        EventKind::FileDiscovered {
            output: "a.dds".into(),
        },
        EventKind::SetResolved {
            members: vec!["a_d.png".into()],
            pbr: true,
            bake: false,
            shader: None,
            warnings: vec![],
            recipes: vec![],
        },
        EventKind::FilePlanned {
            job: Box::new(ResolvedJob {
                input: "a_d.png".into(),
                output: "a_d.dds".into(),
                game_path: "textures/a_d.dds".into(),
                set_key: "a".into(),
                profile: cs_quality().0,
                provenance: Default::default(),
                suggestions: vec![],
                warnings: vec![],
                status: JobStatus::Ready,
                suffix_alias: None,
                source: None,
                pack: None,
                bake: None,
                rnm: None,
                derive: None,
                shader: None,
                cube: None,
                fill: None,
            }),
        },
        EventKind::Progress {
            stage: Stage::Compress,
            done: 1,
            total: 4,
        },
        EventKind::FileDone {
            output: "a.dds".into(),
            validation: Default::default(),
        },
        EventKind::FileSkipped {
            kind: SkipKind::Duplicate,
            code: Some(Code::DimNpot),
            reason: "r".into(),
        },
        EventKind::FileFailed {
            stage: Stage::Write,
            error: "e".into(),
        },
        EventKind::BatchDone {
            converted: 1,
            up_to_date: 2,
            skipped: 3,
            skipped_intentional: 1,
            failed: 4,
            validation_failed: 5,
            cancelled: vec!["z".into()],
        },
    ];
    for k in kinds {
        rep.emit(None, Some(std::path::Path::new("a\\b\\c.png")), k);
    }
    for e in sink.take() {
        assert_eq!(e.path.as_deref(), Some("a/b/c.png"));
        let line = serde_json::to_string(&e).unwrap();
        assert_eq!(
            serde_json::from_str::<EngineEvent>(&line).unwrap(),
            e,
            "{line}"
        );
    }
    let _ = schemars::schema_for!(EngineEvent);
}

#[test]
fn parallel_emits_keep_seq_in_delivery_order() {
    let sink = Arc::new(VecSink::default());
    let rep = Arc::new(Reporter::new(sink.clone(), "b"));
    std::thread::scope(|s| {
        for _ in 0..8 {
            let rep = rep.clone();
            s.spawn(move || {
                for _ in 0..100 {
                    rep.emit(None, None, EventKind::FileStarted);
                }
            });
        }
    });
    let seqs: Vec<u64> = sink.take().iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (0..800).collect::<Vec<_>>());
}

#[test]
fn manifest_appends_and_last_record_wins() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let (profile, preset) = cs_quality();
    let mut r = record("x_n.dds", "aa", profile, preset);
    m.append(&r).unwrap();
    r.source_sha256 = "bb".into();
    m.append(&r).unwrap();
    let mut bytes = std::fs::read(m.path()).unwrap();
    bytes.extend_from_slice(b"not json\n");
    std::fs::write(m.path(), bytes).unwrap();
    let all = m.load().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all["x_n.dds"].source_sha256, "bb");
    assert!(m.path().ends_with("_sdc/manifest.jsonl"));
    assert!(["sse2", "sse4", "avx", "avx2"].contains(&dispatched_isa()));
}

#[test]
fn load_of_a_missing_manifest_is_empty() {
    let d = tempfile::tempdir().unwrap();
    assert!(Manifest::at(d.path()).load().unwrap().is_empty());
}

#[test]
fn invalid_utf8_and_torn_lines_do_not_lose_later_records() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let (profile, preset) = cs_quality();
    m.append(&record("a.dds", "1", profile.clone(), preset.clone()))
        .unwrap();
    // A crashed writer left raw non-UTF-8 bytes and a torn line without a final newline.
    let mut bytes = std::fs::read(m.path()).unwrap();
    bytes.extend_from_slice(b"\xff\xfe\n{\"schema\":1,\"ts\":");
    std::fs::write(m.path(), bytes).unwrap();
    m.append(&record("b.dds", "2", profile, preset)).unwrap();
    let all = m.load().unwrap();
    assert_eq!(
        all.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a.dds", "b.dds"]
    );
}

#[test]
fn concurrent_appends_write_one_whole_line_each() {
    let d = tempfile::tempdir().unwrap();
    let m = Arc::new(Manifest::at(d.path()));
    let (profile, preset) = cs_quality();
    const THREADS: usize = 8;
    const EACH: usize = 40;
    std::thread::scope(|s| {
        for t in 0..THREADS {
            let (m, profile, preset) = (m.clone(), profile.clone(), preset.clone());
            s.spawn(move || {
                for i in 0..EACH {
                    let r = record(
                        &format!("t{t}_{i}.dds"),
                        "s",
                        profile.clone(),
                        preset.clone(),
                    );
                    m.append(&r).unwrap();
                }
            });
        }
    });
    let text = std::fs::read_to_string(m.path()).unwrap();
    assert_eq!(text.lines().count(), THREADS * EACH);
    for l in text.lines() {
        serde_json::from_str::<ManifestRecord>(l).expect(l);
    }
    assert_eq!(m.load().unwrap().len(), THREADS * EACH);

    // Two Manifest values on one root share the process-wide lock.
    let m2 = Arc::new(Manifest::at(d.path()));
    std::thread::scope(|s| {
        for (k, mm) in [m.clone(), m2].into_iter().enumerate() {
            let (profile, preset) = (profile.clone(), preset.clone());
            s.spawn(move || {
                for i in 0..EACH {
                    let r = record(
                        &format!("u{k}_{i}.dds"),
                        "s",
                        profile.clone(),
                        preset.clone(),
                    );
                    mm.append(&r).unwrap();
                }
            });
        }
    });
    assert_eq!(m.load().unwrap().len(), THREADS * EACH + 2 * EACH);
}

#[test]
fn encoder_versions_come_from_the_encoders_crate() {
    let v = encoders_sys::versions();
    let e = EncoderVersions::current();
    assert_eq!(
        (e.bc7enc_rdo.as_str(), e.ispctc.as_str(), e.ispc.as_str()),
        (v.bc7enc_rdo, v.ispctc, v.ispc)
    );
    let patch = e.bc7e_patch.expect("the vendored bc7e is patched");
    let hash = sha256_file(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch"
    )))
    .unwrap();
    assert_eq!(patch, format!("{}@sha256:{hash}", v.bc7e_patch.unwrap()));
    // A manifest from before the rgbcx BC4 round-to-nearest patch must not match, so its BC3/BC4/BC5 re-encode.
    // The tag hashes the patched rgbcx.h, so an in-place change to the rounding code also re-encodes (T13).
    let rgbcx = sha256_file(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../encoders-sys/vendor/bc7enc_rdo/rgbcx.h"
    )))
    .unwrap();
    assert_eq!(v.rgbcx_patch, Some("bc4-round-to-nearest"));
    assert_eq!(
        e.rgbcx_patch.as_deref(),
        Some(format!("bc4-round-to-nearest@sha256:{rgbcx}").as_str())
    );
}

#[test]
fn engine_identity_is_filled() {
    assert!(!ENGINE_VERSION.is_empty());
    let c = engine_commit();
    let hash = c.strip_suffix("-dirty").unwrap_or(c);
    assert!(
        c == "unknown" || (hash.len() == 12 && hash.chars().all(|ch| ch.is_ascii_hexdigit())),
        "{c}"
    );
}

#[test]
fn preset_ref_names_the_resolving_preset_and_alias_chain() {
    let mut reg = PresetRegistry::builtin().unwrap();
    reg.add_toml(
        r#"
schema_version = 1
id = "ultra"
version = "2.1.0"
base = "cs-quality"
target = "cs"

[suffix_aliases]
rmaosultrahigh = "rmaos"
"#,
    )
    .unwrap();
    let plain = PresetRef::for_preset(&reg, "cs-quality", None).unwrap();
    assert_eq!(plain.id, "cs-quality");
    assert!(plain.alias_chain.is_empty());
    assert_eq!(plain.sha256, reg.sha256("cs-quality").unwrap());

    let chain = reg.alias_chain("ultra", Target::Cs).unwrap();
    let r = PresetRef::for_preset(&reg, "ultra", Some(&chain)).unwrap();
    assert_eq!((r.id.as_str(), r.version.as_str()), ("ultra", "2.1.0"));
    assert_eq!(r.sha256, reg.sha256("ultra").unwrap());
    assert_eq!(r.alias_chain.last().map(String::as_str), Some("ultra"));
    assert_eq!(
        r.alias_chain,
        chain.iter().map(|p| p.id.clone()).collect::<Vec<_>>()
    );

    // Records written without the field still load, and an empty chain is not written.
    let mut v = serde_json::to_value(&plain).unwrap();
    assert!(v.get("alias_chain").is_none());
    v.as_object_mut().unwrap().remove("alias_chain");
    assert_eq!(serde_json::from_value::<PresetRef>(v).unwrap(), plain);
    assert!(PresetRef::for_preset(&reg, "nope", None).is_none());
}

#[test]
fn sha256_helpers() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("f");
    std::fs::write(&p, b"abc").unwrap();
    assert_eq!(
        sha256_file(&p).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_json(&serde_json::json!({"a": 1})).unwrap(),
        sha256_json(&serde_json::json!({"a": 1})).unwrap()
    );
    // A value that cannot be serialised is an error, not the hash of empty input.
    let mut bad = std::collections::HashMap::new();
    bad.insert((1, 2), 3);
    assert!(sha256_json(&bad).is_err());
    assert!(sha256_file(&d.path().join("missing")).is_err());
}

#[test]
fn output_key_is_root_relative_slashed_and_case_folded() {
    use std::path::Path;
    let k = |root: &str, out: &str| output_key(Path::new(root), Path::new(out));
    assert_eq!(k("out", "out/Armor/Iron_n.dds"), "armor/iron_n.dds");
    assert_eq!(k("out", "out\\Armor\\Iron_N.DDS"), "armor/iron_n.dds");
    assert_eq!(k(".\\out\\", "out/Armor/Iron_n.dds"), "armor/iron_n.dds");
    assert_eq!(k("out", ".\\out\\Armor\\Iron_n.dds"), "armor/iron_n.dds");
    assert_eq!(k("out/", "./out/./Armor//Iron_n.dds"), "armor/iron_n.dds");
    assert_eq!(k("C:\\Mods\\Out", "c:/mods/out/Tex/A.dds"), "tex/a.dds");
    // Unicode, not just ASCII, case folding.
    assert_eq!(
        k("out", "out/\u{c4}rmel/\u{d6}l.dds"),
        "\u{e4}rmel/\u{f6}l.dds"
    );
    // A sibling directory that merely shares the root's prefix is not under the root.
    assert_eq!(k("out", "outer/a.dds"), "outer/a.dds");
    // Already a key: unchanged.
    assert_eq!(k("out", "armor/iron_n.dds"), "armor/iron_n.dds");
}

#[test]
fn records_written_under_any_spelling_of_the_key_collide_on_load() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let (profile, preset) = cs_quality();
    let root = std::path::Path::new("out");
    let key = |o: &str| output_key(root, std::path::Path::new(o));
    m.append(&record(
        &key("out\\Armor\\Iron_n.dds"),
        "old",
        profile.clone(),
        preset.clone(),
    ))
    .unwrap();
    m.append(&record(
        &key("./out/armor/iron_n.dds"),
        "new",
        profile.clone(),
        preset.clone(),
    ))
    .unwrap();
    // A record from before the key rule: raw spelling, re-folded on load.
    m.append(&record("Armor\\Old_n.DDS", "x", profile, preset))
        .unwrap();
    let all = m.load().unwrap();
    assert_eq!(
        all.keys().map(String::as_str).collect::<Vec<_>>(),
        ["armor/iron_n.dds", "armor/old_n.dds"]
    );
    assert_eq!(all["armor/iron_n.dds"].source_sha256, "new");
}

#[test]
fn records_survive_profile_and_summary_evolution() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let (profile, preset) = cs_quality();
    let r = record("x_n.dds", "aa", profile, preset);
    let mut v = serde_json::to_value(&r).unwrap();
    // A later release added a Profile field (Profile is deny_unknown_fields) and a ValidationSummary field was
    // removed; old and new records must still load.
    v["profile"]["field_from_the_future"] = serde_json::json!(true);
    let summary = v["validation"].as_object_mut().unwrap();
    let dropped = summary.keys().next().cloned().expect("summary has fields");
    summary.remove(&dropped);
    assert!(serde_json::from_value::<ManifestRecord>(v.clone()).is_err());
    std::fs::create_dir_all(m.path().parent().unwrap()).unwrap();
    std::fs::write(m.path(), format!("{v}\n")).unwrap();
    let all = m.load().unwrap();
    let e = &all["x_n.dds"];
    assert_eq!(e.schema, MANIFEST_SCHEMA);
    assert!(!e.is_newer_schema());
    assert_eq!(e.source_sha256, "aa");
    assert_eq!(e.profile_sha256, r.profile_sha256);
    assert_eq!(e.engine_version, ENGINE_VERSION);
    assert_eq!(e.encoders.as_ref(), Some(&r.encoders));
    assert_eq!(e.preset.as_ref(), Some(&r.preset));
    assert_eq!(e.raw, v);
}

#[test]
fn newer_schema_records_are_kept_and_flagged() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let line = serde_json::json!({
        "schema": MANIFEST_SCHEMA + 1,
        "output": "Tex\\New.dds",
        "profile": {"a": "shape nobody here understands"},
        "something_new": [1, 2, 3],
    });
    std::fs::create_dir_all(m.path().parent().unwrap()).unwrap();
    std::fs::write(m.path(), format!("{line}\n{{\"unrelated\":1}}\n")).unwrap();
    let all = m.load().unwrap();
    assert_eq!(all.len(), 1);
    let e = &all["tex/new.dds"];
    assert!(e.is_newer_schema());
    assert_eq!(e.raw["something_new"], serde_json::json!([1, 2, 3]));
}

#[test]
fn multi_source_records_round_trip_and_single_source_omits_the_list() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let (profile, preset) = cs_quality();
    let mut r = record("x_rmaos.dds", "aa", profile, preset);
    let line = serde_json::to_value(&r).unwrap();
    assert!(line.get("sources").is_none());
    r.sources = vec![
        SourceRef {
            path: "in/x_r.png".into(),
            sha256: "11".into(),
            role: Some(RawMap::Roughness),
            invert: true,
            bake: None,
            rnm: None,
            derive: None,
            derive_role: None,
            cube_face: None,
            fill: None,
        },
        SourceRef {
            path: "in/x_m.png".into(),
            sha256: "22".into(),
            role: None,
            invert: false,
            bake: None,
            rnm: None,
            derive: None,
            derive_role: None,
            cube_face: None,
            fill: None,
        },
    ];
    m.append(&r).unwrap();
    assert_eq!(m.load().unwrap()["x_rmaos.dds"].sources, r.sources);
    let text = std::fs::read_to_string(m.path()).unwrap();
    assert_eq!(
        serde_json::from_str::<ManifestRecord>(text.trim()).unwrap(),
        r
    );
}

#[test]
fn unit_and_newtype_events_have_an_exact_json_layout() {
    let ev = |kind| EngineEvent {
        schema: EVENT_SCHEMA,
        seq: 7,
        ts: "2026-10-09T12:34:56.789Z".into(),
        batch: "b1".into(),
        set: None,
        path: None,
        job: None,
        kind,
    };
    assert_eq!(
        serde_json::to_string(&ev(EventKind::FileStarted)).unwrap(),
        r#"{"schema":2,"seq":7,"ts":"2026-10-09T12:34:56.789Z","batch":"b1","kind":"file_started"}"#
    );
    let mut w = ev(EventKind::Warning(Warning::new(Code::DimNpot, "npot")));
    w.set = Some("s".into());
    w.path = Some("a/x.png".into());
    let sev = serde_json::to_string(&Warning::new(Code::DimNpot, "").severity).unwrap();
    assert_eq!(
        serde_json::to_string(&w).unwrap(),
        format!(
            r#"{{"schema":2,"seq":7,"ts":"2026-10-09T12:34:56.789Z","batch":"b1","set":"s","path":"a/x.png","kind":"warning","code":"DIM-NPOT","severity":{sev},"message":"npot","data":null}}"#
        )
    );
}

#[test]
fn batch_ids_are_unique_within_a_process() {
    let ids: std::collections::HashSet<String> = (0..1000).map(|_| new_batch_id()).collect();
    assert_eq!(ids.len(), 1000);
}

#[test]
fn a_job_key_and_a_skip_kind_have_an_exact_json_layout() {
    let ev = EngineEvent {
        schema: EVENT_SCHEMA,
        seq: 1,
        ts: "2026-10-09T12:34:56.789Z".into(),
        batch: "b1".into(),
        set: None,
        path: Some("in/a_ao.png".into()),
        job: Some("a_rmaos.dds".into()),
        kind: EventKind::FileSkipped {
            kind: SkipKind::PackedSource,
            code: None,
            reason: "packed into a_rmaos.dds".into(),
        },
    };
    let text = serde_json::to_string(&ev).unwrap();
    assert_eq!(
        text,
        r#"{"schema":2,"seq":1,"ts":"2026-10-09T12:34:56.789Z","batch":"b1","path":"in/a_ao.png","job_key":"a_rmaos.dds","kind":"file_skipped","skip":"packed_source","code":null,"reason":"packed into a_rmaos.dds"}"#
    );
    assert_eq!(serde_json::from_str::<EngineEvent>(&text).unwrap(), ev);
}

#[test]
fn the_shader_pick_round_trips_through_the_manifest_json() {
    let (profile, preset) = cs_quality();
    let r = record("x.dds", "ab", profile, preset);
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["shader"]["wanted"], "pbr");
    assert_eq!(serde_json::from_value::<ManifestRecord>(v).unwrap(), r);
    let none = ManifestRecord { shader: None, ..r };
    assert!(serde_json::to_value(&none).unwrap().get("shader").is_none());
}

/// Spec §5: every manifest record passes the migration hook; a newer one is kept raw, a line without a schema is
/// not a record.
#[test]
fn manifest_records_pass_the_migration_hook() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    std::fs::create_dir_all(m.path().parent().unwrap()).unwrap();
    std::fs::write(
        m.path(),
        "{\"schema\":1,\"output\":\"a.dds\"}\n{\"schema\":9,\"output\":\"b.dds\",\"x\":1}\n{\"output\":\"c.dds\"}\n",
    )
    .unwrap();
    let e = m.load().unwrap();
    assert_eq!(e["a.dds"].schema, 2);
    assert!(!e["a.dds"].is_newer_schema());
    assert!(e["b.dds"].is_newer_schema());
    assert_eq!(e["b.dds"].raw["x"], 1);
    assert!(!e.contains_key("c.dds"));
}

/// A manifest of at least `min_bytes` alternating two keys (`a.dds` with source "1", `b.dds` with "2").
fn stale_manifest(m: &Manifest, min_bytes: u64) {
    let (profile, preset) = cs_quality();
    let mut pair = vec![];
    for (out, sha) in [("a.dds", "1"), ("b.dds", "2")] {
        pair.extend(
            serde_json::to_vec(&record(out, sha, profile.clone(), preset.clone())).unwrap(),
        );
        pair.push(b'\n');
    }
    let n = min_bytes as usize / pair.len() + 1;
    std::fs::create_dir_all(m.path().parent().unwrap()).unwrap();
    std::fs::write(m.path(), pair.repeat(n)).unwrap();
}

/// Audit EC-5 (Note BJ): a manifest over 32 MB with more than 3 lines per entry is rewritten on load to one line per
/// entry, and loads the same records afterwards.
#[test]
fn a_big_stale_manifest_is_compacted_on_load() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    stale_manifest(&m, COMPACT_MIN_BYTES);
    let first = m.load().unwrap();
    assert_eq!(first.len(), 2);
    let text = std::fs::read_to_string(m.path()).unwrap();
    assert_eq!(text.lines().count(), 2, "compacted to one line per entry");
    assert_eq!(m.load().unwrap(), first);
}

/// A small manifest, however stale, is left as it is.
#[test]
fn a_small_stale_manifest_is_not_rewritten() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    stale_manifest(&m, 1 << 20);
    let before = std::fs::read(m.path()).unwrap();
    assert_eq!(m.load().unwrap().len(), 2);
    assert_eq!(std::fs::read(m.path()).unwrap(), before);
}

/// Compaction holds the append lock: a record appended while a big manifest is being loaded is never lost.
#[test]
fn an_append_during_compaction_is_kept() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    stale_manifest(&m, COMPACT_MIN_BYTES);
    let done = AtomicBool::new(false);
    let mut appended = 0;
    let (profile, preset) = cs_quality();
    std::thread::scope(|s| {
        let writer = s.spawn(|| {
            let w = Manifest::at(d.path());
            // Appends until the load has returned, so some land between its read and its rewrite.
            let mut n = 0;
            while n < 20 || !done.load(Ordering::Relaxed) {
                let r = record(&format!("c{n}.dds"), "3", profile.clone(), preset.clone());
                w.append(&r).unwrap();
                n += 1;
            }
            n
        });
        m.load().unwrap();
        done.store(true, Ordering::Relaxed);
        appended = writer.join().unwrap();
    });
    let all = m.load().unwrap();
    assert_eq!(all.len(), 2 + appended);
    assert_eq!(all["b.dds"].source_sha256, "2");
}

mod also_support;

#[test]
fn records_carry_the_encode_version_of_their_subtype() {
    use also_support::*;
    let (_d, m, loc) = project(cup);
    let sp = spec(&loc, &[engine::presets::AlsoTarget::Vanilla]);
    also(request(&m, &loc), &sp).0.unwrap();
    let e = record(&loc.join("Mod PBR"), "textures/clutter/cup.dds");
    let rec: ManifestRecord = serde_json::from_value(e.raw.clone()).unwrap();
    let want = encode_version(rec.profile.subtype);
    assert_eq!(e.encode_version.as_deref(), Some(want));
    assert!(e.raw["encode_version"].is_string());
}

#[test]
fn a_record_without_encode_version_loads_with_none() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    let (profile, preset) = cs_quality();
    let r = record("x_n.dds", "aa", profile, preset);
    let line = |v: Option<serde_json::Value>| {
        let mut j = serde_json::to_value(&r).unwrap();
        let o = j.as_object_mut().unwrap();
        match v {
            Some(v) => o.insert("encode_version".into(), v),
            None => o.remove("encode_version"),
        };
        j
    };
    std::fs::create_dir_all(m.path().parent().unwrap()).unwrap();
    for (v, want) in [
        (None, None),
        (Some(serde_json::json!(7)), None),
        (Some(serde_json::json!("")), Some("")),
    ] {
        std::fs::write(m.path(), format!("{}\n", line(v))).unwrap();
        let all = m.load().unwrap();
        assert_eq!(all["x_n.dds"].encode_version.as_deref(), want);
    }
}

/// Note DX: a filled output records its helper with the fill identity; an output without a fill has no `fill` key.
#[test]
fn fill_source_is_recorded() {
    use image::{ImageBuffer, Luma, Rgb};
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/a");
    std::fs::create_dir_all(&dir).unwrap();
    ImageBuffer::from_fn(32, 32, |x, y| Rgb([(x * 8) as u8, (y * 8) as u8, 90]))
        .save(dir.join("x.png"))
        .unwrap();
    let grey = |name: &str| {
        ImageBuffer::from_fn(32, 32, |x, _| Luma([(x * 8) as u8]))
            .save(dir.join(name))
            .unwrap()
    };
    grey("x_opacity.png");
    grey("x_roughness.png");
    let mut r =
        engine::api::BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    r.accept = engine::policy::AcceptSuggestions::High;
    r.overrides = ["file:**/x.png.subtype=diffuse", "alpha.policy=blend"]
        .iter()
        .map(|s| engine::overrides::parse_set(s).unwrap())
        .collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "fill-record");
    let e = engine::api::Engine::new().unwrap();
    let cancel = engine::api::CancelToken::default();
    let s = e
        .run(e.plan(r, &rep, &cancel).unwrap(), &rep, &cancel)
        .unwrap();
    assert_eq!(s.converted, 2, "{s:?} {:#?}", sink.take());
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    let rec = |name: &str| {
        m.values()
            .find(|e| e.raw["output"].as_str().is_some_and(|o| o.ends_with(name)))
            .unwrap_or_else(|| panic!("no record of {name}"))
    };
    let x = rec("x.dds");
    let fill: Vec<_> = x.sources.iter().filter(|s| s.fill.is_some()).collect();
    assert_eq!(fill.len(), 1, "{:?}", x.sources);
    assert!(fill[0].path.ends_with("x_opacity.png"), "{}", fill[0].path);
    assert_eq!(fill[0].fill.as_deref(), Some("a:r"));
    let rmaos = rec("x_rmaos.dds");
    assert!(!rmaos.sources.is_empty());
    assert!(
        rmaos.raw["sources"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s.get("fill").is_none()),
        "{}",
        rmaos.raw["sources"]
    );
}
