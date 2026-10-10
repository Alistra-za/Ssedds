use engine::analyse::analyse;
use engine::codes::{Code, Severity};
use engine::config::*;
use engine::discover::*;
use engine::group::*;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::{parse_set, NO_FILE_SCOPES};
use engine::policy::*;
use engine::presets::*;
use engine::suggest::Confidence;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const RGB8: SourceInfo = SourceInfo {
    bit_depth: 8,
    channels: 3,
    has_alpha: false,
    is_float: false,
};
const RGBA8: SourceInfo = SourceInfo {
    bit_depth: 8,
    channels: 4,
    has_alpha: true,
    is_float: false,
};

fn grey() -> Rgba32F {
    Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0])
}

fn flat_normal() -> Rgba32F {
    Rgba32F::new(64, 64, [0.5, 0.5, 1.0, 1.0])
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

/// Resolves `name` under cs as a member of `textures/armor/iron`, whose other members have the suffixes `companions`.
fn resolve_img(
    name: &str,
    companions: &[&str],
    sets_cli: &[&str],
    allow: &[Code],
    img: &Rgba32F,
    info: &SourceInfo,
) -> ResolvedJob {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow: BTreeSet<Code> = allow.iter().copied().collect();
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
        files: &NO_FILE_SCOPES,
    };
    let stem = Path::new(name)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let split = split_suffix_with(&stem, reg.alias_table());
    let f = InputFile {
        path: PathBuf::from(name),
        rel: PathBuf::from(name),
        game_path: format!("textures/armor/{}", name.to_ascii_lowercase()),
        output: PathBuf::from(name).with_extension("dds"),
        kind: InputKind::Image,
    };
    let mut suffixes: BTreeSet<String> = companions.iter().map(|s| s.to_string()).collect();
    suffixes.extend(split.suffix.clone());
    let set = TextureSet {
        key: "textures/armor/iron".into(),
        members: vec![0],
        suffixes,
    };
    let a = analyse(img, info);
    resolve_file(
        &ctx,
        &FileFacts {
            input: &f,
            suffix: split.suffix.as_deref(),
            alias: split.alias.as_deref(),
            set: &set,
            analysis: &a,
            info,
        },
    )
}

#[test]
fn vocabulary_splits_to_skyrim_roles() {
    let none = BTreeMap::new();
    let s = |stem: &str| {
        let x = split_suffix_with(stem, &none);
        (x.base, x.suffix, x.alias)
    };
    for stem in [
        "iron_colormap",
        "iron_basecolor",
        "iron_Base_Color",
        "iron_albedo",
        "iron_diffuse",
        "iron_color",
        "iron_col",
    ] {
        assert_eq!(s(stem), ("iron".into(), None, None), "{stem}");
        assert_eq!(vocab_of(stem).unwrap().role, VocabRole::Colour, "{stem}");
    }
    for stem in ["iron_normal", "iron_NRM", "iron_normalgl", "iron_normaldx"] {
        assert_eq!(s(stem), ("iron".into(), Some("n".into()), None), "{stem}");
    }
    assert_eq!(
        vocab_of("iron_normal").unwrap().role,
        VocabRole::Normal(None)
    );
    assert_eq!(
        vocab_of("iron_NormalGL").unwrap().role,
        VocabRole::Normal(Some(GreenConvention::Opengl))
    );
    assert_eq!(
        vocab_of("iron_normaldx").unwrap().role,
        VocabRole::Normal(Some(GreenConvention::Directx))
    );
    for stem in ["iron_emissive", "iron_emission"] {
        assert_eq!(s(stem).1.as_deref(), Some("g"), "{stem}");
    }
    for (stem, role) in [
        ("iron_roughness", "roughness"),
        ("iron_rough", "roughness"),
        ("iron_metallic", "metallic"),
        ("iron_metalness", "metallic"),
        ("iron_metal", "metallic"),
        ("iron_ao", "ao"),
        ("iron_ambientocclusion", "ao"),
        ("iron_occlusion", "ao"),
        ("iron_specular", "specular"),
        ("iron_spec", "specular"),
        ("iron_glossiness", "glossiness"),
        ("iron_gloss", "glossiness"),
        ("iron_height", "height"),
        ("iron_displacement", "height"),
        ("iron_disp", "height"),
        ("iron_opacity", "opacity"),
        ("iron_fuzz", "fuzz"),
        ("iron_sheen", "fuzz"),
    ] {
        assert_eq!(s(stem), ("iron".into(), Some(role.into()), None), "{stem}");
        assert!(
            matches!(vocab_of(stem).unwrap().role, VocabRole::PackingInput(r) if r.id() == role),
            "{stem}"
        );
    }
    // Skyrim suffixes keep their meaning next to the exporter terms
    for (skyrim, exporter, role) in [
        ("iron_s", "iron_spec", "specular"),
        ("iron_p", "iron_height", "height"),
        ("iron_m", "iron_metal", "metallic"),
        ("iron_g", "iron_gloss", "glossiness"),
        ("iron_f", "iron_fuzz", "fuzz"),
    ] {
        assert_eq!(s(skyrim).1, Some(skyrim[5..].to_string()), "{skyrim}");
        assert!(vocab_of(skyrim).is_none(), "{skyrim}");
        assert_eq!(s(exporter).1.as_deref(), Some(role), "{exporter}");
    }
    // `_d` stays a §6.1 suffix; a stem that is only a term is left whole; split_suffix and SUFFIXES are unchanged
    assert_eq!(s("iron_d"), ("iron".into(), Some("d".into()), None));
    assert_eq!(s("_normal"), ("_normal".into(), None, None));
    assert_eq!(
        split_suffix("iron_basecolor"),
        ("iron_basecolor".into(), None)
    );
    assert_eq!(SUFFIXES.len(), 19);
    // longest first, so `_base_color` is one term
    assert_eq!(VOCABULARY.len(), 31);
    assert!(VOCABULARY
        .windows(2)
        .all(|w| w[0].term.len() >= w[1].term.len()));
    assert_eq!(vocab_of("iron_base_color").unwrap().term, "base_color");
}

#[test]
fn outputs_use_skyrim_names_and_collisions_are_duplicates() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    for f in [
        "iron_basecolor.png",
        "iron_normalgl.png",
        "iron_emissive.png",
        "iron_roughness.png",
        "iron_AO.png",
        "wood_d.png",
        "rock.png",
        "rock_albedo.tga",
        "rock_n.png",
        "rock_normal.png",
    ] {
        touch(&src.join(f));
    }
    let out = d.path().join("out");
    let mut files = discover(&[src], &out, false).unwrap();
    // no preset aliases loaded: the vocabulary still applies
    canonicalise_outputs(&mut files, &BTreeMap::new());
    let output_of = |name: &str| {
        files
            .iter()
            .find(|f| f.rel.as_path() == Path::new(name))
            .unwrap()
            .output
            .clone()
    };
    assert_eq!(output_of("iron_basecolor.png"), out.join("iron.dds"));
    assert_eq!(output_of("iron_normalgl.png"), out.join("iron_n.dds"));
    assert_eq!(output_of("iron_emissive.png"), out.join("iron_g.dds"));
    // raw maps keep their name (never written on their own); `_d` keeps its stem
    assert_eq!(
        output_of("iron_roughness.png"),
        out.join("iron_roughness.dds")
    );
    assert_eq!(output_of("wood_d.png"), out.join("wood_d.dds"));
    let (sets, dups) = group_sets(&files);
    let iron = sets.iter().find(|s| s.key.ends_with("/iron")).unwrap();
    assert_eq!(iron.members.len(), 5);
    assert_eq!(
        iron.suffixes,
        BTreeSet::from(["n", "g", "roughness", "ao"].map(String::from))
    );
    // rock.png + rock_albedo.tga → rock.dds; rock_n.png + rock_normal.png → rock_n.dds
    let mut groups: Vec<BTreeSet<String>> = dups
        .iter()
        .map(|g| {
            g.iter()
                .map(|&i| files[i].rel.to_string_lossy().into_owned())
                .collect()
        })
        .collect();
    groups.sort();
    assert_eq!(
        groups,
        vec![
            BTreeSet::from(["rock.png".to_string(), "rock_albedo.tga".to_string()]),
            BTreeSet::from(["rock_n.png".to_string(), "rock_normal.png".to_string()]),
        ]
    );
}

#[test]
fn colour_terms_and_d_are_high_confidence_but_alpha_meaning_is_never_assumed() {
    let j = resolve_img("iron_colormap.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(
        (j.profile.subtype, j.suggestions[0].confidence),
        (Subtype::Diffuse, Confidence::High)
    );
    assert!(
        j.suggestions[0]
            .reason
            .starts_with("exporter suffix _colormap (colour); "),
        "{}",
        j.suggestions[0].reason
    );
    let j = resolve_img("iron_d.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(
        (
            j.profile.subtype,
            j.suggestions[0].confidence,
            j.status.clone()
        ),
        (Subtype::Diffuse, Confidence::High, JobStatus::Ready)
    );
    // a bare stem stays medium
    let j = resolve_img("iron.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(j.suggestions[0].confidence, Confidence::Medium);
    assert!(
        matches!(j.status, JobStatus::Unresolved { .. }),
        "{:?}",
        j.status
    );
    // the set still picks the sub-type: a raw roughness companion makes the cs set PBR
    let j = resolve_img(
        "iron_basecolor.png",
        &["roughness"],
        &[],
        &[],
        &grey(),
        &RGB8,
    );
    assert_eq!(
        (j.profile.subtype, j.suggestions[0].confidence),
        (Subtype::PbrAlbedo, Confidence::High)
    );
    // varying alpha: the colour role is certain, the alpha meaning is not
    let mut img = grey();
    for y in 0..64 {
        for x in 0..64 {
            img.at_mut(x, y)[3] = x as f32 / 63.0;
        }
    }
    let j = resolve_img("iron_albedo.png", &[], &[], &[], &img, &RGBA8);
    assert_eq!(j.suggestions[0].confidence, Confidence::High);
    match &j.status {
        JobStatus::Unresolved { reasons, .. } => assert!(
            reasons
                .iter()
                .any(|r| r.contains("alpha meaning is unconfirmed")),
            "{reasons:?}"
        ),
        s => panic!("{s:?}"),
    }
}

#[test]
fn plain_normal_stays_unresolved_until_the_green_convention_is_stated() {
    let green_reason = |j: &ResolvedJob| matches!(&j.status, JobStatus::Unresolved { reasons, .. } if reasons.iter().any(|r| r.contains("green convention")));
    for name in ["iron_normal.png", "iron_nrm.png"] {
        let j = resolve_img(name, &[], &[], &[], &flat_normal(), &RGB8);
        assert_eq!(
            (j.profile.subtype, j.suggestions[0].confidence),
            (Subtype::Normal, Confidence::High)
        );
        assert_eq!(j.profile.channels.green_convention, None);
        assert!(green_reason(&j), "{name}: {:?}", j.status);
        // not a suggestion: --allow SUG-LOW-CONFIDENCE does not settle it
        let j = resolve_img(
            name,
            &[],
            &[],
            &[Code::SugLowConfidence],
            &flat_normal(),
            &RGB8,
        );
        assert!(green_reason(&j), "{name}: {:?}", j.status);
    }
    let j = resolve_img(
        "iron_normal.png",
        &[],
        &["channels.green_convention=directx"],
        &[],
        &flat_normal(),
        &RGB8,
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(
        (
            j.profile.channels.green_convention,
            j.profile.channels.flip_green
        ),
        (Some(GreenConvention::Directx), false)
    );
    assert_eq!(j.provenance["channels.green_convention"], Provenance::User);
    let j = resolve_img(
        "iron_normal.png",
        &[],
        &["file:iron_normal.png.channels.green_convention=opengl"],
        &[],
        &flat_normal(),
        &RGB8,
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(j.profile.channels.flip_green);
    assert_eq!(j.provenance["channels.flip_green"], Provenance::User);
    // Skyrim's own _n is DirectX by definition
    let j = resolve_img("iron_n.png", &[], &[], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(!j.profile.channels.flip_green);
}

#[test]
fn normalgl_flips_to_directx_and_normaldx_does_not() {
    let j = resolve_img("iron_normalgl.png", &[], &[], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(
        (
            j.profile.channels.green_convention,
            j.profile.channels.flip_green
        ),
        (Some(GreenConvention::Opengl), true)
    );
    assert_eq!(
        j.provenance["channels.green_convention"],
        Provenance::Suffix("normalgl".into())
    );
    assert_eq!(
        j.provenance["channels.flip_green"].to_string(),
        "suffix:normalgl"
    );
    assert_eq!(
        "suffix:normalgl".parse::<Provenance>().unwrap(),
        Provenance::Suffix("normalgl".into())
    );
    let j = resolve_img("iron_normaldx.png", &[], &[], &[], &flat_normal(), &RGB8);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(
        (
            j.profile.channels.green_convention,
            j.profile.channels.flip_green
        ),
        (Some(GreenConvention::Directx), false)
    );
    // --set beats the suffix
    let j = resolve_img(
        "iron_normalgl.png",
        &[],
        &["channels.green_convention=directx"],
        &[],
        &flat_normal(),
        &RGB8,
    );
    assert_eq!(
        (
            j.profile.channels.flip_green,
            j.provenance["channels.green_convention"].clone()
        ),
        (false, Provenance::User)
    );
    // msn ignores the convention
    let j = resolve_img(
        "iron_normalgl.png",
        &[],
        &["subtype=msn"],
        &[],
        &flat_normal(),
        &RGB8,
    );
    assert_eq!(
        (
            j.profile.subtype,
            j.profile.channels.green_convention,
            j.profile.channels.flip_green
        ),
        (Subtype::Msn, None, false)
    );
}

#[test]
fn raw_maps_are_packing_inputs_not_errors() {
    for (name, role) in [
        ("iron_roughness.png", RawMap::Roughness),
        ("iron_spec.png", RawMap::Specular),
        ("iron_height.png", RawMap::Height),
        ("iron_metal.png", RawMap::Metallic),
        ("iron_opacity.png", RawMap::Opacity),
    ] {
        let j = resolve_img(name, &[], &[], &[], &grey(), &RGB8);
        assert_eq!(j.status, JobStatus::PackingInput { role }, "{name}");
        assert_eq!(j.warnings.len(), 1, "{name}");
        assert_eq!(
            (j.warnings[0].code, j.warnings[0].severity),
            (Code::InPackingInput, Severity::Warning)
        );
        assert!(j.suggestions.is_empty());
    }
    let j = resolve_img("iron_roughness.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(
        serde_json::to_value(&j.status).unwrap(),
        serde_json::json!({ "state": "packing_input", "role": "roughness" })
    );
    // the Skyrim suffixes next to them resolve as before
    assert_eq!(
        resolve_img("iron_p.png", &[], &[], &[], &grey(), &RGB8)
            .profile
            .subtype,
        Subtype::Height
    );
    assert_eq!(
        resolve_img("iron_m.png", &[], &[], &[], &grey(), &RGB8)
            .profile
            .subtype,
        Subtype::Envmask
    );
    assert_eq!(
        resolve_img("iron_s.png", &[], &[], &[], &grey(), &RGB8)
            .profile
            .subtype,
        Subtype::SubsurfaceTint
    );
    // a sub-type named by --set makes a raw map an ordinary file
    let j = resolve_img(
        "iron_height.png",
        &[],
        &["file:iron_height.png.subtype=height"],
        &[],
        &grey(),
        &RGB8,
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.subtype, Subtype::Height);
}

#[test]
fn aliases_may_not_shadow_the_vocabulary() {
    let mut r = PresetRegistry::builtin().unwrap();
    for bad in [
        "normal = \"n\"",
        "nrm = \"n\"",
        "roughness = \"rmaos\"",
        "basecolor = \"d\"",
        "spec = \"s\"",
    ] {
        let toml = format!("schema_version = 1\nid = \"bad\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[suffix_aliases]\n{bad}\n");
        let e = r.add_toml(&toml);
        assert!(matches!(e, Err(PresetError::Alias(_))), "{bad}");
        assert!(
            e.unwrap_err().to_string().contains("exporter vocabulary"),
            "{bad}"
        );
        assert!(r.get("bad").is_none(), "{bad}");
    }
    assert!(is_vocabulary("normalgl") && !is_vocabulary("n") && !is_vocabulary("rmaosultrahigh"));
}

/// 27a review: the unstated green convention is its own blocking code, never allowable, with a file-scoped hint.
#[test]
fn unstated_green_convention_is_in_green_unstated_and_not_allowable() {
    for allow in [
        vec![],
        vec![Code::InGreenUnstated],
        vec![Code::SugLowConfidence],
    ] {
        let j = resolve_img("iron_normal.png", &[], &[], &allow, &flat_normal(), &RGB8);
        let reasons = match &j.status {
            JobStatus::Unresolved { reasons, .. } => reasons,
            s => panic!("{allow:?}: {s:?}"),
        };
        let w = j
            .warnings
            .iter()
            .find(|w| w.code == Code::InGreenUnstated)
            .unwrap_or_else(|| panic!("{:?}", j.warnings));
        assert_eq!(w.severity, Severity::Blocking, "{allow:?}");
        assert!(
            !j.warnings.iter().any(|w| w.code == Code::SugLowConfidence),
            "{:?}",
            j.warnings
        );
        let hint = "--set 'file:iron_normal.png.channels.green_convention=opengl|directx'";
        assert!(w.message.contains(hint), "{}", w.message);
        assert!(reasons.iter().any(|r| r.contains(hint)), "{reasons:?}");
    }
    // stating it clears the code
    let j = resolve_img(
        "iron_normal.png",
        &[],
        &["channels.green_convention=directx"],
        &[],
        &flat_normal(),
        &RGB8,
    );
    assert!(!j.warnings.iter().any(|w| w.code == Code::InGreenUnstated));
}
