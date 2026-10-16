//! M4 Task 3: the profile fields `mips.normal_base` and `mips.bake_clamp`, their checks, and provenance `set`.
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, JobStatus, Provenance};
use engine::presets::PresetRegistry;
use image::{ImageBuffer, Rgba};
use std::path::Path;
use std::sync::Arc;

fn plan(dir: &Path, sets: &[&str]) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    Engine::new()
        .unwrap()
        .plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap()
}

fn rmaos(dir: &Path) {
    let p = dir.join("in/textures/pbr/iron_rmaos.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, _| Rgba([(x * 16) as u8, 0, 200, 255]))
        .save(p)
        .unwrap();
}

#[test]
fn every_builtin_profile_renormalises_mip_0_and_clamps_at_a_quarter() {
    let r = PresetRegistry::builtin().unwrap();
    for t in [Target::Cs, Target::Vanilla] {
        for st in Subtype::ALL {
            let p = r.base_profile(t, st);
            assert_eq!(
                (p.mips.normal_base, p.mips.bake_clamp),
                (NormalBase::Renormalize, 0.25),
                "{t:?} {st:?}"
            );
        }
    }
}

#[test]
fn the_new_fields_are_settable_at_every_scope() {
    assert!(parse_set("mips.normal_base=keep").is_ok());
    assert!(parse_set("file:**/iron_n.png.mips.normal_base=keep").is_ok());
    assert!(parse_set("subtype:rmaos.mips.bake_clamp=0.5").is_ok());
    assert!(parse_set("mips.normal_base_typo=keep").is_err());
}

#[test]
fn a_bake_clamp_outside_zero_to_one_is_cfg_invalid_on_a_roughness_member() {
    for (v, bad) in [
        ("0.0", true),
        ("1.5", true),
        ("-0.1", true),
        ("1.0", false),
        ("0.25", false),
    ] {
        let d = tempfile::tempdir().unwrap();
        rmaos(d.path());
        let p = plan(
            d.path(),
            &[&format!("file:**/iron_rmaos.png.mips.bake_clamp={v}")],
        );
        let j = &p.jobs[0];
        let invalid = j.warnings.iter().any(|w| w.code == Code::CfgInvalid);
        assert_eq!(invalid, bad, "{v}: {:?}", j.warnings);
        if bad {
            assert!(
                matches!(&j.status, JobStatus::Error { codes } if codes.contains(&Code::CfgInvalid)),
                "{v}: {:?}",
                j.status
            );
        }
    }
}

#[test]
fn srgb_content_on_a_roughness_member_is_tag_content_mismatch() {
    let d = tempfile::tempdir().unwrap();
    rmaos(d.path());
    let p = plan(d.path(), &["file:**/iron_rmaos.png.color.content=srgb"]);
    let w = p.jobs[0]
        .warnings
        .iter()
        .find(|w| w.code == Code::TagContentMismatch)
        .expect("TAG-CONTENT-MISMATCH");
    assert!(w.message.contains("roughness"), "{}", w.message);
}

#[test]
fn provenance_set_round_trips() {
    assert_eq!(Provenance::Set.to_string(), "set");
    assert_eq!("set".parse::<Provenance>(), Ok(Provenance::Set));
    assert_eq!(
        serde_json::to_value(Provenance::Set).unwrap(),
        serde_json::json!("set")
    );
}
