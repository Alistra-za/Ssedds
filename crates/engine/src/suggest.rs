//! Suggestions (spec §4.3): ranked (subtype, confidence, reason). Suggestions never override explicit choices.
use crate::config::{AlphaPolicy, RawMap, Subtype, Target};
use crate::group::{Vocab, VocabRole};
use crate::presets::AlphaContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Suggestion {
    pub subtype: Subtype,
    pub confidence: Confidence,
    pub reason: String,
}

pub struct SuggestContext<'a> {
    pub target: Target,
    pub suffix: Option<&'a str>,
    pub game_path: &'a str,
    pub set_suffixes: &'a BTreeSet<String>,
    pub alpha: AlphaContent,
    pub is_float: bool,
}

fn has_component(path: &str, name: &str) -> bool {
    path.split('/').any(|c| c == name)
}

/// Spec §4.3 rule 1: under cs, a set is PBR if its path has a `pbr` component, it has _rmaos/_rfaos/_cnr/_f, or it
/// has a raw packing input that starts the _rmaos/_rfaos recipe (§4.4, §6.4).
pub fn is_pbr_set(c: &SuggestContext) -> bool {
    c.target == Target::Cs
        && (has_component(c.game_path, "pbr")
            || ["rmaos", "rfaos", "cnr", "f"]
                .iter()
                .any(|s| c.set_suffixes.contains(*s))
            || RawMap::ALL
                .iter()
                .any(|r| r.starts_rmaos() && c.set_suffixes.contains(&r.id())))
}

pub fn suggest(c: &SuggestContext) -> Vec<Suggestion> {
    use Confidence::*;
    use Subtype::*;
    let s = |st: Subtype, conf: Confidence, why: &str| Suggestion {
        subtype: st,
        confidence: conf,
        reason: why.to_string(),
    };
    let pbr = is_pbr_set(c);
    let landscape =
        c.game_path.starts_with("textures/landscape/") || has_component(c.game_path, "landscape");
    let ui_or_sky = has_component(c.game_path, "interface") || has_component(c.game_path, "sky");
    // Spec §4.4 (DECISIONS Note H): `_d` declares the colour role; a bare stem is only a guess.
    let colour = if c.suffix == Some("d") { High } else { Medium };
    match c.suffix {
        Some("n") if landscape => vec![s(LandscapeNormal, High, "_n under textures/landscape")],
        Some("n") if pbr => vec![s(PbrNormal, High, "_n in a PBR set"), s(Normal, Low, "_n")],
        Some("n") => vec![s(Normal, High, "_n, non-PBR set")],
        Some("msn") => vec![s(Msn, High, "_msn model-space normal")],
        Some("rmaos") => vec![s(Rmaos, High, "_rmaos")],
        Some("rfaos") => vec![s(Rfaos, High, "_rfaos (CS advanced skin)")],
        Some("cnr") | Some("crn") => {
            vec![s(CoatNormal, High, "_cnr/_crn coat normal + roughness")]
        }
        Some("flow") => vec![s(Flow, High, "_flow hair flow map")],
        Some("p") => vec![s(Height, High, "_p height/parallax")],
        Some("f") => vec![s(PbrFuzz, High, "_f fuzz")],
        Some("g") if pbr => vec![s(PbrEmissive, High, "_g in a PBR set"), s(Glow, Low, "_g")],
        Some("g") => vec![s(Glow, High, "_g, non-PBR set")],
        Some("s") if c.set_suffixes.contains("msn") => {
            vec![s(MsnSpecular, High, "_s with _msn in the set")]
        }
        Some("s") if pbr => vec![s(
            PbrSubsurface,
            High,
            "_s in a PBR set (subsurface / coat colour)",
        )],
        Some("s") => vec![s(
            SubsurfaceTint,
            Low,
            "_s, no PBR set, no _msn (PGPatcher subsurface tint?)",
        )],
        Some("m") | Some("em") | Some("envmask") => {
            if c.target == Target::Vanilla {
                vec![s(Envmask, High, "_m under vanilla")]
            } else if c.alpha == AlphaContent::Varying {
                vec![
                    s(ComplexMaterial, High, "_m with non-white alpha"),
                    s(Envmask, Low, "_m"),
                ]
            } else {
                vec![s(Envmask, High, "_m with no or all-255 alpha")]
            }
        }
        Some("b") | Some("bl") => vec![s(BacklightTint, High, "_b/_bl backlight")],
        Some("sk") => vec![s(BacklightTint, Low, "_sk (unconfirmed meaning)")],
        Some("d") | None if landscape => {
            vec![s(LandscapeDiffuse, High, "colour under textures/landscape")]
        }
        Some("d") | None if ui_or_sky && c.is_float => {
            vec![s(HdrSky, Medium, "float source under sky/")]
        }
        Some("d") | None if ui_or_sky => vec![s(UiSky, colour, "colour under interface/ or sky/")],
        Some("d") | None if pbr => vec![s(PbrAlbedo, colour, "colour member of a PBR set")],
        Some("d") | None => vec![s(Diffuse, colour, "colour member, non-PBR set")],
        Some(_) => vec![],
    }
}

/// Spec §4.3 item 7, §4.4 (DECISIONS Note H): an exporter vocabulary term declares the file's role. Every reason
/// is prefixed `exporter suffix _<term> (<role>)`; a colour term raises a colour-member top suggestion to high
/// confidence (not `hdr_sky`: a float source under sky/ may be an HDRI). Alpha meaning is untouched.
pub fn apply_vocabulary(suggestions: &mut [Suggestion], v: &Vocab) {
    let role = match v.role {
        VocabRole::Colour => "colour",
        VocabRole::Normal(_) => "normal",
        VocabRole::Emissive => "emissive",
        VocabRole::PackingInput(_) => "packing input",
    };
    for s in suggestions.iter_mut() {
        s.reason = format!("exporter suffix _{} ({role}); {}", v.term, s.reason);
    }
    if v.role == VocabRole::Colour {
        if let Some(top) = suggestions.first_mut().filter(|t| {
            matches!(
                t.subtype,
                Subtype::Diffuse | Subtype::PbrAlbedo | Subtype::LandscapeDiffuse | Subtype::UiSky
            )
        }) {
            top.confidence = Confidence::High;
        }
    }
}

/// Alpha-policy proposals. No alpha (or constant 255 where alpha is optional) resolves to remove with high
/// confidence; varying alpha on a mesh-dependent sub-type is only ever a low-confidence proposal.
pub fn suggest_alpha(
    st: Subtype,
    alpha: AlphaContent,
    near_binary_share: f64,
) -> Option<(AlphaPolicy, Confidence, String)> {
    match alpha {
        AlphaContent::None => Some((
            AlphaPolicy::Remove,
            Confidence::High,
            "source has no alpha channel".into(),
        )),
        AlphaContent::Constant255 if st.alpha_from_mesh() || st.is_roughness_member() => Some((
            AlphaPolicy::Remove,
            Confidence::High,
            "alpha is constant 255".into(),
        )),
        AlphaContent::Varying if st.alpha_from_mesh() => {
            if near_binary_share >= 0.9 {
                Some((
                    AlphaPolicy::Cutout,
                    Confidence::Low,
                    "≥90% of alpha is ≤5 or ≥250; the mesh decides".into(),
                ))
            } else {
                Some((
                    AlphaPolicy::Blend,
                    Confidence::Low,
                    "soft alpha; the mesh decides".into(),
                ))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn ctx<'a>(
        t: Target,
        suffix: Option<&'a str>,
        path: &'a str,
        set: &'a BTreeSet<String>,
        alpha: AlphaContent,
    ) -> SuggestContext<'a> {
        SuggestContext {
            target: t,
            suffix,
            game_path: path,
            set_suffixes: set,
            alpha,
            is_float: false,
        }
    }

    fn set_of(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn suffix_rules() {
        let none = BTreeSet::new();
        let pbr: BTreeSet<String> = ["rmaos".to_string(), "n".to_string()].into();
        let msn: BTreeSet<String> = ["msn".to_string(), "s".to_string()].into();
        let top = |c: &SuggestContext| suggest(c)[0].clone();
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("n"),
                "textures/a/x_n.png",
                &none,
                AlphaContent::Varying
            ))
            .subtype,
            Subtype::Normal
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("n"),
                "textures/a/x_n.png",
                &pbr,
                AlphaContent::None
            ))
            .subtype,
            Subtype::PbrNormal
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                None,
                "textures/a/x.png",
                &pbr,
                AlphaContent::None
            ))
            .subtype,
            Subtype::PbrAlbedo
        );
        let d = top(&ctx(
            Target::Cs,
            None,
            "textures/a/x.png",
            &none,
            AlphaContent::None,
        ));
        assert_eq!(
            (d.subtype, d.confidence),
            (Subtype::Diffuse, Confidence::Medium)
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("s"),
                "textures/a/x_s.png",
                &msn,
                AlphaContent::None
            ))
            .subtype,
            Subtype::MsnSpecular
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("s"),
                "textures/a/x_s.png",
                &pbr,
                AlphaContent::None
            ))
            .subtype,
            Subtype::PbrSubsurface
        );
        let s = top(&ctx(
            Target::Cs,
            Some("s"),
            "textures/a/x_s.png",
            &none,
            AlphaContent::None,
        ));
        assert_eq!(
            (s.subtype, s.confidence),
            (Subtype::SubsurfaceTint, Confidence::Low)
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("m"),
                "textures/a/x_m.png",
                &none,
                AlphaContent::Varying
            ))
            .subtype,
            Subtype::ComplexMaterial
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("m"),
                "textures/a/x_m.png",
                &none,
                AlphaContent::Constant255
            ))
            .subtype,
            Subtype::Envmask
        );
        assert_eq!(
            top(&ctx(
                Target::Vanilla,
                Some("m"),
                "textures/a/x_m.png",
                &none,
                AlphaContent::Varying
            ))
            .subtype,
            Subtype::Envmask
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("rfaos"),
                "textures/a/x_rfaos.png",
                &none,
                AlphaContent::None
            ))
            .subtype,
            Subtype::Rfaos
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                None,
                "textures/landscape/dirt01.png",
                &none,
                AlphaContent::Varying
            ))
            .subtype,
            Subtype::LandscapeDiffuse
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("n"),
                "textures/landscape/dirt01_n.png",
                &none,
                AlphaContent::Varying
            ))
            .subtype,
            Subtype::LandscapeNormal
        );
        assert_eq!(
            top(&ctx(
                Target::Cs,
                Some("g"),
                "textures/pbr/x_g.png",
                &none,
                AlphaContent::None
            ))
            .subtype,
            Subtype::PbrEmissive
        );
        assert_eq!(
            top(&ctx(
                Target::Vanilla,
                Some("g"),
                "textures/pbr/x_g.png",
                &none,
                AlphaContent::None
            ))
            .subtype,
            Subtype::Glow
        );
        assert!(suggest(&ctx(
            Target::Cs,
            Some("e"),
            "textures/a/x_e.png",
            &none,
            AlphaContent::None
        ))
        .is_empty());
    }

    #[test]
    fn rfaos_is_high_confidence_and_marks_a_pbr_set() {
        let none = BTreeSet::new();
        let rfaos = set_of(&["rfaos"]);
        let r = &suggest(&ctx(
            Target::Cs,
            Some("rfaos"),
            "textures/a/x_rfaos.png",
            &none,
            AlphaContent::None,
        ))[0];
        assert_eq!(
            (r.subtype, r.confidence),
            (Subtype::Rfaos, Confidence::High)
        );
        // _rfaos in the set makes the other members PBR, like _rmaos
        assert!(is_pbr_set(&ctx(
            Target::Cs,
            None,
            "textures/a/x.png",
            &rfaos,
            AlphaContent::None
        )));
        assert_eq!(
            suggest(&ctx(
                Target::Cs,
                None,
                "textures/a/x.png",
                &rfaos,
                AlphaContent::None
            ))[0]
                .subtype,
            Subtype::PbrAlbedo
        );
        assert_eq!(
            suggest(&ctx(
                Target::Cs,
                Some("n"),
                "textures/a/x_n.png",
                &rfaos,
                AlphaContent::None
            ))[0]
                .subtype,
            Subtype::PbrNormal
        );
        // PBR sets exist only under cs
        assert!(!is_pbr_set(&ctx(
            Target::Vanilla,
            None,
            "textures/a/x.png",
            &rfaos,
            AlphaContent::None
        )));
        assert!(!is_pbr_set(&ctx(
            Target::Cpm,
            None,
            "textures/a/x.png",
            &rfaos,
            AlphaContent::None
        )));
    }

    #[test]
    fn s_has_three_readings_disambiguated_by_path_and_companion() {
        let none = BTreeSet::new();
        let msn = set_of(&["msn"]);
        let both = set_of(&["msn", "rmaos"]);
        let s_top = |t, path, set: &BTreeSet<String>| {
            suggest(&ctx(t, Some("s"), path, set, AlphaContent::None))[0].clone()
        };
        // textures/pbr/ path alone (no companions) => PBR subsurface
        let p = s_top(Target::Cs, "textures/pbr/x_s.png", &none);
        assert_eq!(
            (p.subtype, p.confidence),
            (Subtype::PbrSubsurface, Confidence::High)
        );
        // a companion _msn => MSN specular, even when the path is not PBR
        let m = s_top(Target::Cs, "textures/a/x_s.png", &msn);
        assert_eq!(
            (m.subtype, m.confidence),
            (Subtype::MsnSpecular, Confidence::High)
        );
        // _msn wins over the other PBR evidence
        assert_eq!(
            s_top(Target::Cs, "textures/pbr/x_s.png", &both).subtype,
            Subtype::MsnSpecular
        );
        // neither => low-confidence subsurface tint
        let t = s_top(Target::Cs, "textures/a/x_s.png", &none);
        assert_eq!(
            (t.subtype, t.confidence),
            (Subtype::SubsurfaceTint, Confidence::Low)
        );
        // under vanilla a pbr/ path is not a PBR set
        assert_eq!(
            s_top(Target::Vanilla, "textures/pbr/x_s.png", &none).subtype,
            Subtype::SubsurfaceTint
        );
        // `pbr` must be a whole path component
        assert_eq!(
            s_top(Target::Cs, "textures/pbrx/x_s.png", &none).subtype,
            Subtype::SubsurfaceTint
        );
    }

    #[test]
    fn diffuse_is_never_high_confidence() {
        let none = BTreeSet::new();
        let pbr = set_of(&["rmaos"]);
        for (set, path) in [
            (&none, "textures/a/x.png"),
            (&pbr, "textures/a/x.png"),
            (&none, "textures/pbr/x.png"),
        ] {
            for alpha in [
                AlphaContent::None,
                AlphaContent::Constant255,
                AlphaContent::Varying,
            ] {
                let sg = suggest(&ctx(Target::Cs, None, path, set, alpha));
                assert!(sg[0].confidence < Confidence::High, "{path} {alpha:?}");
            }
        }
    }

    #[test]
    fn d_is_high_and_raw_packing_inputs_mark_a_cs_set_pbr() {
        let none = BTreeSet::new();
        let d = suggest(&ctx(
            Target::Cs,
            Some("d"),
            "textures/a/x_d.png",
            &none,
            AlphaContent::None,
        ))[0]
            .clone();
        assert_eq!(
            (d.subtype, d.confidence),
            (Subtype::Diffuse, Confidence::High)
        );
        let sky = suggest(&ctx(
            Target::Cs,
            Some("d"),
            "textures/sky/x_d.png",
            &none,
            AlphaContent::None,
        ))[0]
            .clone();
        assert_eq!(
            (sky.subtype, sky.confidence),
            (Subtype::UiSky, Confidence::High)
        );
        for (raw, pbr) in [
            ("roughness", true),
            ("glossiness", true),
            ("metallic", true),
            ("ao", true),
            ("specular", false),
            ("fuzz", false),
            ("height", false),
            ("opacity", false),
        ] {
            let set = set_of(&[raw]);
            assert_eq!(
                is_pbr_set(&ctx(
                    Target::Cs,
                    None,
                    "textures/a/x.png",
                    &set,
                    AlphaContent::None
                )),
                pbr,
                "{raw}"
            );
            assert!(
                !is_pbr_set(&ctx(
                    Target::Cpm,
                    None,
                    "textures/a/x.png",
                    &set,
                    AlphaContent::None
                )),
                "{raw}"
            );
        }
        // a raw packing role gets no sub-type suggestion
        assert!(suggest(&ctx(
            Target::Cs,
            Some("roughness"),
            "textures/a/x_roughness.png",
            &none,
            AlphaContent::None
        ))
        .is_empty());
    }

    #[test]
    fn colour_vocabulary_raises_the_colour_member_only() {
        let none = BTreeSet::new();
        let mut sg = suggest(&ctx(
            Target::Cs,
            None,
            "textures/a/x_basecolor.png",
            &none,
            AlphaContent::None,
        ));
        apply_vocabulary(&mut sg, crate::group::vocab_of("x_basecolor").unwrap());
        assert_eq!(
            (sg[0].subtype, sg[0].confidence),
            (Subtype::Diffuse, Confidence::High)
        );
        assert!(
            sg[0]
                .reason
                .starts_with("exporter suffix _basecolor (colour); "),
            "{}",
            sg[0].reason
        );
        // a float source under sky/ may be an HDRI: stays medium
        let mut f = ctx(
            Target::Cs,
            None,
            "textures/sky/x_albedo.exr",
            &none,
            AlphaContent::None,
        );
        f.is_float = true;
        let mut sg = suggest(&f);
        apply_vocabulary(&mut sg, crate::group::vocab_of("x_albedo").unwrap());
        assert_eq!(
            (sg[0].subtype, sg[0].confidence),
            (Subtype::HdrSky, Confidence::Medium)
        );
        // normal terms only prefix the reason
        let mut sg = suggest(&ctx(
            Target::Cs,
            Some("n"),
            "textures/a/x_normalgl.png",
            &none,
            AlphaContent::None,
        ));
        apply_vocabulary(&mut sg, crate::group::vocab_of("x_normalgl").unwrap());
        assert_eq!(
            (sg[0].subtype, sg[0].confidence),
            (Subtype::Normal, Confidence::High)
        );
        assert!(sg[0]
            .reason
            .starts_with("exporter suffix _normalgl (normal); "));
    }

    #[test]
    fn sky_and_interface_colour() {
        let none = BTreeSet::new();
        let sky = suggest(&ctx(
            Target::Cs,
            None,
            "textures/sky/x.png",
            &none,
            AlphaContent::None,
        ));
        assert_eq!(sky[0].subtype, Subtype::UiSky);
        let mut f = ctx(
            Target::Cs,
            None,
            "textures/sky/x.exr",
            &none,
            AlphaContent::None,
        );
        f.is_float = true;
        assert_eq!(suggest(&f)[0].subtype, Subtype::HdrSky);
    }

    #[test]
    fn confidence_orders_and_serialises_lowercase() {
        assert!(Confidence::Low < Confidence::Medium && Confidence::Medium < Confidence::High);
        assert_eq!(
            serde_json::to_string(&Confidence::High).unwrap(),
            "\"high\""
        );
    }

    #[test]
    fn alpha_suggestions() {
        assert_eq!(
            suggest_alpha(Subtype::Diffuse, AlphaContent::None, 0.0)
                .unwrap()
                .0,
            AlphaPolicy::Remove
        );
        let (p, c, _) = suggest_alpha(Subtype::Diffuse, AlphaContent::Varying, 0.95).unwrap();
        assert_eq!((p, c), (AlphaPolicy::Cutout, Confidence::Low));
        assert_eq!(
            suggest_alpha(Subtype::Diffuse, AlphaContent::Varying, 0.5)
                .unwrap()
                .0,
            AlphaPolicy::Blend
        );
        assert_eq!(
            suggest_alpha(Subtype::Rmaos, AlphaContent::Constant255, 1.0).unwrap(),
            (
                AlphaPolicy::Remove,
                Confidence::High,
                "alpha is constant 255".into()
            )
        );
        assert_eq!(
            suggest_alpha(Subtype::Rfaos, AlphaContent::Constant255, 1.0)
                .unwrap()
                .0,
            AlphaPolicy::Remove
        );
        assert!(suggest_alpha(Subtype::Normal, AlphaContent::Constant255, 1.0).is_none());
    }
}
