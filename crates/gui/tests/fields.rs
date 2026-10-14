mod support;
use engine::config::*;
use engine::presets::PresetRegistry;
use sdc_gui::model::fields::*;
use support::*;

#[test]
fn format_labels_state_bc_format_and_colour_space() {
    assert_eq!(format_label(Format::Bc7, ColorTag::Srgb), "BC7 · sRGB");
    assert_eq!(format_label(Format::Bc7, ColorTag::Unorm), "BC7 · linear");
    assert_eq!(
        format_label(Format::Bc5, ColorTag::Unorm),
        "BC5 · linear (two channels)"
    );
    assert_eq!(
        format_label(Format::Bc6hUf16, ColorTag::NotApplicable),
        "BC6H · HDR linear"
    );
}

#[test]
fn locks_say_why() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg.base_profile(Target::Vanilla, Subtype::Diffuse);
    assert_eq!(
        field_lock(Target::Vanilla, &p, "color.tag"),
        Some("_SRGB is only written under cs")
    );
    p.alpha.policy = AlphaPolicy::Blend;
    assert_eq!(
        field_lock(Target::Vanilla, &p, "alpha.threshold"),
        Some("used only by cutout and cutout + blend")
    );
    p.alpha.policy = AlphaPolicy::Cutout;
    assert_eq!(field_lock(Target::Vanilla, &p, "alpha.threshold"), None);
    let n = reg.base_profile(Target::Cs, Subtype::Normal);
    assert_eq!(field_lock(Target::Cs, &n, "mips.normal"), None);
    assert_eq!(
        field_lock(Target::Cs, &p, "mips.normal"),
        Some("normal maps only")
    );
    // BC6H wins over the target for the tag; perceptual needs colour, no data alpha and BC7.
    let mut h = reg.base_profile(Target::Cs, Subtype::HdrSky);
    h.format = Format::Bc6hUf16;
    assert_eq!(
        field_lock(Target::Cs, &h, "color.tag"),
        Some("BC6H has no _SRGB form")
    );
    assert!(field_lock(Target::Cs, &h, "encoder.perceptual").is_some());
}

#[test]
fn field_views_mark_edited_fields() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(
        "landscape/leaf.png",
        "alpha.threshold",
        serde_json::json!(90),
    );
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let job = m.item("landscape/leaf.png").unwrap().job.clone().unwrap();
    let views = field_views(m.target, &job);
    let t = views.iter().find(|v| v.field == "alpha.threshold").unwrap();
    assert!(t.edited);
    assert_eq!(t.value, serde_json::json!(90));
    assert!(!views.iter().find(|v| v.field == "format").unwrap().edited);
    let mut groups: Vec<&str> = views.iter().map(|v| group_of(v.field)).collect();
    groups.dedup();
    assert_eq!(
        groups,
        vec!["Type", "Colour", "Alpha", "Mipmaps", "Compression"]
    );
}

#[test]
fn every_field_is_a_real_engine_path() {
    for (f, _) in FIELDS {
        assert!(engine::overrides::FIELD_PATHS.contains(&f), "{f}");
    }
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let job = m.item("landscape/leaf.png").unwrap().job.clone().unwrap();
    // A hidden row (`alpha.parallax` outside complex_material, DECISIONS AH2) may be unset.
    for v in field_views(m.target, &job)
        .into_iter()
        .filter(|v| !field_hidden(&job.profile, v.field))
    {
        assert!(
            !v.value.is_null(),
            "{} has no value in the profile JSON",
            v.field
        );
    }
}

#[test]
fn provenance_reads_as_prose() {
    assert_eq!(
        provenance_text("project_file"),
        "from this texture's settings"
    );
    assert_eq!(
        provenance_text("preset:cs-quality"),
        "from preset cs-quality"
    );
    assert_eq!(channel_text("default:0.5"), "default value 0.5");
    assert_eq!(channel_text("fill:255"), "fill value 255");
    assert_eq!(
        provenance_text("mesh:alpha"),
        "from the meshes that use this texture"
    );
}

#[test]
fn coverage_belongs_to_alpha_and_levels_to_the_drawer() {
    use sdc_gui::model::fields::{mips_count, mips_on, ADVANCED_FIELDS};
    assert_eq!(group_of("mips.coverage"), "Alpha");
    assert_eq!(group_of("mips.normal"), "Mipmaps");
    assert!(!FIELDS.iter().any(|(f, _)| *f == "mips.levels"));
    assert!(mips_on(&serde_json::json!("full")) && mips_on(&serde_json::json!(4)));
    assert!(!mips_on(&serde_json::json!("none")));
    assert_eq!(mips_count(&serde_json::json!(4)), Some(4));
    assert_eq!(mips_count(&serde_json::json!("full")), None);
    assert!(!ADVANCED_FIELDS.iter().any(|(f, _)| *f == "mips.levels"));
}

#[test]
fn normal_base_level_is_a_normal_map_field() {
    let reg = PresetRegistry::builtin().unwrap();
    let n = reg.base_profile(Target::Cs, Subtype::PbrNormal);
    let d = reg.base_profile(Target::Cs, Subtype::Diffuse);
    assert!(FIELDS.iter().any(|(f, _)| *f == "mips.normal_base"));
    assert_eq!(field_lock(Target::Cs, &n, "mips.normal_base"), None);
    assert_eq!(
        field_lock(Target::Cs, &d, "mips.normal_base"),
        Some("normal maps only")
    );
    assert_eq!(
        provenance_text("set"),
        "decided by the texture set (variance bake)"
    );
}

#[test]
fn uncompressed_format_labels_say_so() {
    assert_eq!(
        format_label(Format::Rgba8, ColorTag::Srgb),
        "RGBA8 · sRGB (uncompressed)"
    );
    assert_eq!(
        format_label(Format::R8, ColorTag::Unorm),
        "R8 · linear (one channel, uncompressed)"
    );
    assert_eq!(
        format_label(Format::Rg8, ColorTag::Unorm),
        "RG8 · linear (two channels, uncompressed)"
    );
    assert_eq!(
        format_label(Format::Rgba16f, ColorTag::NotApplicable),
        "RGBA16F · HDR linear (uncompressed)"
    );
}

#[test]
fn uncompressed_formats_are_offered_and_rgba16f_only_under_cs() {
    let reg = PresetRegistry::builtin().unwrap();
    let formats = |t: Target, st: Subtype| -> Vec<Format> {
        encoding_options(t, &reg.base_profile(t, st))
            .into_iter()
            .map(|((f, _), _)| f)
            .collect()
    };
    for f in [Format::Rgba8, Format::R8, Format::Rg8] {
        assert!(
            formats(Target::Vanilla, Subtype::Diffuse).contains(&f),
            "{f:?}"
        );
    }
    assert!(formats(Target::Cs, Subtype::HdrSky).contains(&Format::Rgba16f));
    assert!(!formats(Target::Cpm, Subtype::Diffuse).contains(&Format::Rgba16f));
}

#[test]
fn uncompressed_locks_explain_themselves() {
    let reg = engine::presets::PresetRegistry::builtin().unwrap();
    let mut p = reg.base_profile(Target::Cs, Subtype::Height);
    p.format = Format::R8;
    assert_eq!(
        field_lock(Target::Cs, &p, "color.tag"),
        Some("R8 and RG8 have no _SRGB form")
    );
    assert_eq!(
        field_lock(Target::Cs, &p, "encoder.effort"),
        Some("no encoder runs for uncompressed formats")
    );
    p.format = Format::Rgba16f;
    assert_eq!(
        field_lock(Target::Cs, &p, "color.tag"),
        Some("float formats have no _SRGB form")
    );
}

#[test]
fn filter_and_edge_stay_in_the_basic_mipmaps_group() {
    assert_eq!(group_of(FILTER_FIELD.0), "Mipmaps");
    assert_eq!(group_of(EDGE_FIELD.0), "Mipmaps");
    assert_eq!(group_of("mips.filter"), "Mipmaps");
    assert_eq!(group_of("mips.edge"), "Mipmaps");
    assert!(!ADVANCED_FIELDS.iter().any(|f| f.0 == "mips.filter"));
    assert!(ADVANCED_FIELDS.iter().any(|f| f.0 == "resize"));
    for f in ["mips.filter", "mips.edge", "resize"] {
        assert!(engine::overrides::FIELD_PATHS.contains(&f), "{f}");
    }
}

#[test]
fn filter_options_lock_per_s_mips_2() {
    use MipFilter::*;
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg.base_profile(Target::Cs, Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Remove;
    for f in [Box, Tent, Mitchell, Kaiser] {
        assert_eq!(filter_option_lock(&p, f), None, "diffuse {f:?}");
    }
    let n = reg.base_profile(Target::Cs, Subtype::Normal);
    assert_eq!(filter_option_lock(&n, Box), None);
    for f in [Tent, Mitchell, Kaiser] {
        assert!(filter_option_lock(&n, f).is_some(), "normal {f:?}");
    }
    let mut c = p.clone();
    c.alpha.policy = AlphaPolicy::Cutout;
    assert_eq!(filter_option_lock(&c, Box), None);
    for f in [Tent, Mitchell, Kaiser] {
        assert!(
            filter_option_lock(&c, f).is_some_and(|r| r.contains("alpha-weighted")),
            "cutout {f:?}"
        );
    }
    let mut e = reg.base_profile(Target::Cs, Subtype::PbrEmissive);
    e.format = Format::Bc6hUf16;
    e.alpha.policy = AlphaPolicy::Remove;
    assert_eq!(filter_option_lock(&e, Tent), None);
    assert!(filter_option_lock(&e, Mitchell).is_some());
    assert!(filter_option_lock(&e, Kaiser).is_some());
    // F2: rgba16f is HDR too; tent only without alpha weighting.
    let mut r = p.clone();
    r.format = Format::Rgba16f;
    assert_eq!(filter_option_lock(&r, Tent), None);
    assert!(filter_option_lock(&r, Kaiser).is_some());
    r.alpha.policy = AlphaPolicy::Cutout;
    assert!(filter_option_lock(&r, Tent).is_some_and(|m| m.contains("alpha-weighted")));
}

#[test]
fn edge_is_locked_while_the_filter_is_box() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg.base_profile(Target::Cs, Subtype::Diffuse);
    p.mips.filter = MipFilter::Box;
    assert_eq!(
        field_lock(Target::Cs, &p, "mips.edge"),
        Some("only tent, Mitchell and Kaiser read the edge")
    );
    p.mips.filter = MipFilter::Kaiser;
    assert_eq!(field_lock(Target::Cs, &p, "mips.edge"), None);
    assert_eq!(field_lock(Target::Cs, &p, "mips.filter"), None);
}

/// F5: default `mips.edge` and `resize` are not serialised; the views fall back to their defaults.
#[test]
fn filter_edge_and_resize_views_show_their_defaults() {
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let job = m.item("landscape/leaf.png").unwrap().job.clone().unwrap();
    assert_eq!(filter_view(m.target, &job).value, serde_json::json!("box"));
    assert_eq!(edge_view(m.target, &job).value, serde_json::json!("wrap"));
    let resize = advanced_field_views(m.target, &job)
        .into_iter()
        .find(|v| v.field == "resize")
        .unwrap();
    assert_eq!(resize.value, serde_json::json!("none"));
}

/// Owner, 2026-10-13: the format pickers list only what the engine allows (`policy::allowed_encodings`).
#[test]
fn format_pickers_offer_only_compatible_encodings() {
    let reg = PresetRegistry::builtin().unwrap();
    let pairs = |st: Subtype| -> Vec<(Format, ColorTag)> {
        let p = reg.base_profile(Target::Cs, st);
        encoding_options(Target::Cs, &p)
            .into_iter()
            .map(|(e, _)| e)
            .collect()
    };
    let grass = reg.base_profile(Target::Cs, Subtype::ComplexGrass);
    assert_eq!(
        encoding_options(Target::Cs, &grass),
        vec![((Format::Bc7, ColorTag::Unorm), "BC7 · linear".to_string())]
    );
    assert!(pairs(Subtype::PbrAlbedo)
        .iter()
        .all(|(_, t)| *t == ColorTag::Srgb));
    let n = pairs(Subtype::Normal);
    assert!(n.contains(&(Format::Bc7, ColorTag::Unorm)));
    assert!(n
        .iter()
        .all(|(f, _)| !matches!(f, Format::Bc4 | Format::Bc6hUf16)));
    let mut emissive = reg.base_profile(Target::Cs, Subtype::PbrEmissive);
    assert!(encoding_options(Target::Cs, &emissive)
        .iter()
        .any(|(e, _)| *e == (Format::Bc6hUf16, ColorTag::NotApplicable)));
    // Vanilla has no HDR.
    let d = reg.base_profile(Target::Vanilla, Subtype::Diffuse);
    assert!(encoding_options(Target::Vanilla, &d)
        .iter()
        .all(|((f, t), _)| !f.is_hdr() && *t == ColorTag::Unorm));
    emissive.color.tag = ColorTag::Srgb;
    emissive.format = Format::Bc7;
    assert!(!encoding_options(Target::Cs, &emissive)
        .iter()
        .any(|(_, l)| l.ends_with(NOT_VALID)));
}

/// An incompatible current value (an old project, a hand edit) stays listed, marked, never rewritten.
#[test]
fn an_incompatible_current_encoding_is_kept_and_marked() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut n = reg.base_profile(Target::Cs, Subtype::Normal);
    n.format = Format::Bc4;
    let o = encoding_options(Target::Cs, &n);
    assert!(o.contains(&(
        (Format::Bc4, ColorTag::Unorm),
        format!("BC4 · linear (one channel){NOT_VALID}")
    )));
    assert_eq!(o.iter().filter(|(_, l)| l.ends_with(NOT_VALID)).count(), 1);
    let mut a = reg.base_profile(Target::Cs, Subtype::PbrAlbedo);
    a.color.tag = ColorTag::Unorm;
    assert!(encoding_options(Target::Cs, &a).contains(&(
        (Format::Bc7, ColorTag::Unorm),
        format!("BC7 · linear{NOT_VALID}")
    )));
}
