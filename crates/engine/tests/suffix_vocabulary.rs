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
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
        nvtt: None,
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
        real_rel: None,
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
        ("iron_orm", "orm"),
        ("iron_ORM", "orm"),
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
    assert_eq!(SUFFIXES.len(), 20);
    // longest first, so `_base_color` is one term
    assert_eq!(VOCABULARY.len(), 37);
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
    // a bare stem is the colour member at High too (M6 Track E, DECISIONS Note U1)
    let j = resolve_img("iron.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(
        (
            j.profile.subtype,
            j.suggestions[0].confidence,
            j.status.clone()
        ),
        (Subtype::Diffuse, Confidence::High, JobStatus::Ready)
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
fn plain_normal_is_directx_unless_opengl_is_stated() {
    for name in ["iron_normal.png", "iron_nrm.png"] {
        let j = resolve_img(name, &[], &[], &[], &flat_normal(), &RGB8);
        assert_eq!(
            (j.profile.subtype, j.suggestions[0].confidence),
            (Subtype::Normal, Confidence::High)
        );
        // Note AW1: the profile stays unset (bytes and settings unchanged); only the Info line records the assumption.
        assert_eq!(j.status, JobStatus::Ready, "{name}: {:?}", j.warnings);
        assert_eq!(
            (
                j.profile.channels.green_convention,
                j.profile.channels.flip_green
            ),
            (None, false)
        );
        let w = j
            .warnings
            .iter()
            .find(|w| w.code == Code::InGreenUnstated)
            .unwrap_or_else(|| panic!("{:?}", j.warnings));
        assert_eq!(w.severity, Severity::Info);
        assert!(w.message.contains("assumed DirectX"), "{}", w.message);
        assert!(!j.provenance.contains_key("channels.green_convention"));
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
    assert!(!j.warnings.iter().any(|w| w.code == Code::InGreenUnstated));
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
    assert!(!j.warnings.iter().any(|w| w.code == Code::InGreenUnstated));
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
        ("iron_orm.png", RawMap::Orm),
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
        "orm = \"m\"",
        "curvature = \"m\"",
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
    // DECISIONS Note AL6: an `orm` alias written before Note AL is told why it no longer loads
    let toml = "schema_version = 1\nid = \"old\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[suffix_aliases]\norm = \"m\"\n";
    let e = r.add_toml(toml).unwrap_err().to_string();
    assert!(
        e.contains("orm is a built-in packing input since Note AL"),
        "{e}"
    );
}

/// Note AW: the unstated green convention is an Info line (never blocking, never needs --allow); stating it clears it.
#[test]
fn unstated_green_convention_is_an_info_line_and_stating_it_clears_it() {
    for allow in [vec![], vec![Code::InGreenUnstated]] {
        let j = resolve_img("iron_normal.png", &[], &[], &allow, &flat_normal(), &RGB8);
        assert_eq!(j.status, JobStatus::Ready, "{allow:?}: {:?}", j.warnings);
        let w = j
            .warnings
            .iter()
            .find(|w| w.code == Code::InGreenUnstated)
            .unwrap_or_else(|| panic!("{:?}", j.warnings));
        assert_eq!(w.severity, Severity::Info, "{allow:?}");
        assert!(
            w.message.contains("channels.green_convention=opengl"),
            "{}",
            w.message
        );
    }
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

fn plan_cs(dir: &Path) -> engine::api::Plan {
    plan_for(dir, Target::Cs)
}

fn plan_for(dir: &Path, target: Target) -> engine::api::Plan {
    use engine::api::*;
    use engine::events::*;
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    let sink = std::sync::Arc::new(VecSink::default());
    Engine::new()
        .unwrap()
        .plan(req, &Reporter::new(sink, "t"), &CancelToken::default())
        .unwrap()
}

#[test]
fn coat_is_the_s_coat_colour_member() {
    let s = split_suffix_with("rock_coat", &Default::default());
    assert_eq!((s.base.as_str(), s.suffix.as_deref()), ("rock", Some("s")));
    assert_eq!(vocab_of("rock_coat").unwrap().role, VocabRole::CoatColour);
    // Plan: rock_coat.png joins set `rock`, resolves pbr_subsurface (High, decided) and is written rock_s.dds,
    // exactly as rock_s.png would be in the same PBR set.
    let d = tempfile::tempdir().unwrap();
    for (name, px) in [
        ("rock.png", [120u8, 90, 60]),
        ("rock_n.png", [128, 128, 255]),
        ("rock_rmaos.png", [128, 0, 255]),
        ("rock_coat.png", [200, 180, 160]),
    ] {
        let p = d.path().join("in/textures/a").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px))
            .save(p)
            .unwrap();
    }
    let p = plan_cs(d.path());
    let j = p
        .jobs
        .iter()
        .find(|j| j.input.ends_with("rock_coat.png"))
        .unwrap();
    assert_eq!(j.profile.subtype.id(), "pbr_subsurface");
    assert!(
        j.output.ends_with("textures/a/rock_s.dds"),
        "{}",
        j.output.display()
    );
    assert!(j.set_key.ends_with("textures/a/rock"));
    assert!(
        !matches!(j.status, engine::policy::JobStatus::Unresolved { .. }),
        "{:?}",
        j.status
    );
}

/// `_coat` is the coat colour whatever the set: High `pbr_subsurface` first (a bare `_s` outside a PBR set is only a
/// low tint), written `x_s.dds`, and blocked under vanilla like any PBR-only map.
#[test]
fn coat_outside_a_pbr_set_is_still_the_high_coat_colour_and_blocked_under_vanilla() {
    let d = tempfile::tempdir().unwrap();
    for name in ["x_d.png", "x_coat.png"] {
        let p = d.path().join("in/textures/a").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb([200u8, 180, 160]))
            .save(p)
            .unwrap();
    }
    let find = |p: &engine::api::Plan| {
        p.jobs
            .iter()
            .find(|j| j.input.ends_with("x_coat.png"))
            .unwrap()
            .clone()
    };
    let j = find(&plan_cs(d.path()));
    let top = &j.suggestions[0];
    assert_eq!(
        (top.subtype, top.confidence),
        (Subtype::PbrSubsurface, Confidence::High)
    );
    assert!(
        j.output.ends_with("textures/a/x_s.dds"),
        "{}",
        j.output.display()
    );
    let j = find(&plan_for(d.path(), Target::Vanilla));
    assert!(
        j.warnings.iter().any(|w| w.code == Code::CfgUnavailable),
        "{:?}",
        j.warnings
    );
}

/// M6 Track E (owner: "_rfaos _RFAOS _rmaos _RMaos should not need confirmation"; Note U1): with acceptance, every
/// member of a correctly named PBR or Advanced skin set decides itself, whatever the case of its suffix. The bare
/// stem is the colour member at High: `pbr_albedo` beside `_RMAOS`, `skin_albedo` beside `_RFAOS`.
#[test]
fn upper_case_suffixes_resolve_like_lower_case_and_nothing_asks_a_sub_type() {
    let d = tempfile::tempdir().unwrap();
    for (name, px) in [
        ("IRON.png", [120u8, 90, 60]),
        ("IRON_N.png", [128, 128, 255]),
        ("IRON_RMAOS.png", [128, 0, 255]),
        ("Body.png", [200, 160, 140]),
        ("Body_RFAOS.png", [128, 0, 255]),
    ] {
        let p = d.path().join("in/textures/a").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px))
            .save(p)
            .unwrap();
    }
    let p = plan_cs(d.path());
    for (input, want) in [
        ("IRON.png", Subtype::PbrAlbedo),
        ("IRON_N.png", Subtype::PbrNormal),
        ("IRON_RMAOS.png", Subtype::Rmaos),
        ("Body.png", Subtype::SkinAlbedo),
        ("Body_RFAOS.png", Subtype::SkinRfaos),
    ] {
        let j = p
            .jobs
            .iter()
            .find(|j| j.input.ends_with(input))
            .unwrap_or_else(|| panic!("no job for {input}"));
        assert_eq!(
            (j.profile.subtype, j.suggestions[0].confidence),
            (want, Confidence::High),
            "{input}"
        );
        assert!(
            !matches!(&j.status, JobStatus::Unresolved { needs, .. } if needs.contains(&Need::Subtype)),
            "{input}: {:?}",
            j.status
        );
    }
}

/// Track A Task 1 (DECISIONS Note CN): `_curves`, `_curvature` and `_cavity` are packing inputs; `Rmaos` is a
/// role but never a file-name one, and none of the three starts the cs `_rmaos` recipe.
#[test]
fn curves_curvature_and_cavity_are_packing_input_terms() {
    for (stem, role) in [
        ("rock_curves", RawMap::Curves),
        ("rock_curvature", RawMap::Curves),
        ("rock_cavity", RawMap::Cavity),
    ] {
        let (base, v) = split_vocab(stem).unwrap();
        assert_eq!(
            (base.as_str(), &v.role),
            ("rock", &VocabRole::PackingInput(role)),
            "{stem}"
        );
    }
    assert_eq!(RawMap::INPUTS.len(), 11);
    assert_eq!(RawMap::ALL.len(), 8);
    assert!(!RawMap::INPUTS.contains(&RawMap::Rmaos));
    assert_eq!(
        [RawMap::Curves, RawMap::Cavity, RawMap::Rmaos].map(RawMap::id),
        ["curves", "cavity", "rmaos"]
    );
    for r in [RawMap::Curves, RawMap::Cavity, RawMap::Rmaos] {
        assert!(!r.starts_rmaos(), "{r:?}");
    }
}
