//! `policy::allowed_encodings` (owner, 2026-10-13): the (format, tag) pairs a format picker may offer.
use engine::config::*;
use engine::policy::{allowed_encodings, availability, Availability};
use engine::presets::PresetRegistry;

fn pairs(v: &[(Format, ColorTag)]) -> String {
    v.iter()
        .map(|(f, t)| {
            format!(
                "{}/{}",
                f.id(),
                serde_json::to_value(t).unwrap().as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every built-in preset's sub-type default (and the encodings its rules or facts switch to) is allowed; the table
/// is printed (`--nocapture`) for the report.
#[test]
fn every_built_in_default_is_allowed() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut bad = vec![];
    for t in [Target::Cs, Target::Vanilla] {
        println!("== {t:?}");
        for st in Subtype::ALL {
            let allowed = allowed_encodings(st, t);
            if availability(t, st) != Availability::Available {
                // CFG-UNAVAILABLE: the file is never written, so no encoding is valid.
                assert!(allowed.is_empty(), "{st:?} {t:?}");
                println!("{:<20} (unavailable)", st.id());
                continue;
            }
            println!("{:<20} {}", st.id(), pairs(&allowed));
            assert!(!allowed.is_empty(), "{st:?} {t:?}");
            let mut wanted = vec![];
            for p in reg.selectable() {
                if p.target.is_none_or(|pt| pt == t) {
                    let prof = reg.profile(&p.id, t, st).unwrap();
                    wanted.push((p.id.clone(), prof.format, prof.color.tag));
                }
            }
            // DECISIONS S3: an LDR emissive becomes BC7 sRGB.
            if st == Subtype::PbrEmissive {
                wanted.push(("hdr-source fact".into(), Format::Bc7, ColorTag::Srgb));
            }
            // cs-compact's BC1 rule for opaque colour keeps the sub-type's tag.
            let bc1 = [
                Subtype::Diffuse,
                Subtype::PbrAlbedo,
                Subtype::Glow,
                Subtype::PbrSubsurface,
                Subtype::PbrFuzz,
                Subtype::SubsurfaceTint,
                Subtype::BacklightTint,
                Subtype::UiSky,
                Subtype::SkinAlbedo,
            ];
            if t == Target::Cs && bc1.contains(&st) {
                let tag = reg.profile("cs-compact", t, st).unwrap().color.tag;
                wanted.push(("cs-compact BC1 rule".into(), Format::Bc1, tag));
            }
            for (src, f, tag) in wanted {
                if !allowed.contains(&(f, tag)) {
                    bad.push(format!("{t:?} {} {src}: {f:?}/{tag:?}", st.id()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "defaults the rules forbid: {bad:#?}");
}

#[test]
fn owner_rules_hold() {
    use Subtype::*;
    assert_eq!(
        allowed_encodings(ComplexGrass, Target::Cs),
        vec![(Format::Bc7, ColorTag::Unorm)]
    );
    for st in [PbrAlbedo, PbrSubsurface, PbrFuzz] {
        let a = allowed_encodings(st, Target::Cs);
        assert!(a.iter().all(|(_, t)| *t == ColorTag::Srgb), "{st:?}: {a:?}");
    }
    for t in [Target::Cs, Target::Vanilla] {
        for st in Subtype::ALL {
            let a = allowed_encodings(st, t);
            if matches!(
                st.category(),
                Category::Normal | Category::Grayscale | Category::MixedPacked
            ) {
                assert!(
                    a.iter().all(|(_, t)| *t == ColorTag::Unorm),
                    "{st:?} {t:?}: {a:?}"
                );
            }
            // A grey `skin_wet` is a BC4 height map (Category::Grayscale), never BC6H.
            if st.category() == Category::Normal {
                assert!(
                    a.iter().all(
                        |(f, _)| *f != Format::Bc6hUf16 && (*f != Format::Bc4 || st == SkinWet)
                    ),
                    "{st:?} {t:?}: {a:?}"
                );
            }
            if t != Target::Cs {
                assert!(a.iter().all(|(f, t)| !f.is_hdr() && *t != ColorTag::Srgb));
            }
        }
    }
}
