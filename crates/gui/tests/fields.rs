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
    assert!(format_options(Target::Cs).contains(&Format::Bc6hUf16));
    assert!(!format_options(Target::Vanilla).contains(&Format::Bc6hUf16));
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
    for v in field_views(m.target, &job) {
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
