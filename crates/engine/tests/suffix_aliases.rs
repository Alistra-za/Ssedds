use engine::analyse::analyse;
use engine::config::*;
use engine::discover::*;
use engine::group::*;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::{parse_set, NO_FILE_SCOPES};
use engine::policy::*;
use engine::presets::*;
use engine::suggest::Confidence;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ULTRA: &str = r#"
schema_version = 1
id = "ultra"
version = "1.0.0"
base = "cs-quality"
target = "cs"

[suffix_aliases]
rmaosultrahigh = "rmaos"
backlit = "sk"
skintint = "s"

[subtypes.rmaos]
mips = { levels = 1 }
"#;

/// An alias preset without `base` (controller ruling 1: it sits on the target's quality preset).
const NOBASE: &str = r#"
schema_version = 1
id = "nobase"
version = "1.0.0"

[suffix_aliases]
rmaoslow = "rmaos"

[subtypes.rmaos]
mips = { levels = 2 }
"#;

/// An alias preset that chooses the alpha meaning of a diffuse (controller ruling 3: explicit, not built in).
const BLENDY: &str = r#"
schema_version = 1
id = "blendy"
version = "1.0.0"
base = "cs-quality"

[suffix_aliases]
diffblend = "d"

[subtypes.diffuse]
alpha = { policy = "blend" }
"#;

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

fn registry() -> PresetRegistry {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml(ULTRA).unwrap();
    r
}

fn preset_with(id: &str, aliases: &str) -> String {
    format!("schema_version = 1\nid = \"{id}\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[suffix_aliases]\n{aliases}\n")
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"x").unwrap();
}

fn resolve_img(
    reg: &PresetRegistry,
    name: &str,
    sets_cli: &[&str],
    accept: AcceptSuggestions,
    img: &Rgba32F,
    info: &SourceInfo,
) -> ResolvedJob {
    let overrides: Vec<_> = sets_cli.iter().map(|s| parse_set(s).unwrap()).collect();
    let allow = BTreeSet::new();
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept,
        allow: &allow,
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
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
    let set = TextureSet {
        key: "textures/armor/iron".into(),
        members: vec![0],
        suffixes: split.suffix.iter().cloned().collect(),
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

fn resolve(
    reg: &PresetRegistry,
    name: &str,
    sets_cli: &[&str],
    accept: AcceptSuggestions,
) -> ResolvedJob {
    let img = Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]);
    resolve_img(reg, name, sets_cli, accept, &img, &RGB8)
}

#[test]
fn registry_validates_and_looks_up_aliases() {
    let mut r = registry();
    assert_eq!(r.alias_for("rmaosultrahigh"), Some(("rmaos", "ultra")));
    assert_eq!(r.alias_for("RmaosUltraHigh"), Some(("rmaos", "ultra")));
    assert_eq!(r.alias_for("nultrahigh"), None);
    assert_eq!(
        r.alias_table().get("backlit").map(String::as_str),
        Some("sk")
    );
    // an alias may not be a canonical suffix, must name a canonical suffix, and is [a-z0-9]+
    for bad in [
        "rmaos = \"rmaos\"",
        "n = \"rmaos\"",
        "foo = \"bar\"",
        "foo = \"\"",
        "\"ultra_high\" = \"rmaos\"",
        "UltraHigh = \"rmaos\"",
    ] {
        let e = r.add_toml(&preset_with("bad", bad));
        assert!(matches!(e, Err(PresetError::Alias(_))), "{bad}");
        assert!(e.unwrap_err().to_string().starts_with("CFG-INVALID:"));
        assert!(r.get("bad").is_none(), "{bad}");
    }
    // an alias declared by a second preset is rejected and the registry is unchanged
    assert!(matches!(
        r.add_toml(&preset_with("other", "rmaosultrahigh = \"rmaos\"")),
        Err(PresetError::Alias(_))
    ));
    assert!(r.get("other").is_none());
    assert_eq!(r.alias_for("rmaosultrahigh"), Some(("rmaos", "ultra")));
    // the same preset id cannot be loaded twice, so its aliases stay (M6 Track D, Note T7)
    assert!(r.add_toml(ULTRA).is_err());
    assert!(r
        .add_toml(&preset_with("ultra", "rmaosmax = \"rmaos\""))
        .is_err());
    assert_eq!(r.alias_table().len(), 3);
    assert_eq!(r.alias_for("rmaosultrahigh"), Some(("rmaos", "ultra")));
    assert_eq!(r.alias_for("rmaosmax"), None);
    // built-in presets declare no aliases, and a user alias preset is not built in
    assert!(PresetRegistry::builtin().unwrap().alias_table().is_empty());
    assert!(!registry().is_builtin("ultra"));
}

#[test]
fn alias_matching_is_exact_and_case_insensitive() {
    let r = registry();
    let s = |stem: &str| split_suffix_with(stem, r.alias_table());
    let split = |base: &str, suffix: Option<&str>, alias: Option<&str>| SuffixSplit {
        base: base.into(),
        suffix: suffix.map(Into::into),
        alias: alias.map(Into::into),
    };
    assert_eq!(
        s("iron_rmaosultrahigh"),
        split("iron", Some("rmaos"), Some("rmaosultrahigh"))
    );
    assert_eq!(
        s("Iron_RMAOSUltraHigh"),
        split("Iron", Some("rmaos"), Some("rmaosultrahigh"))
    );
    // declaring rmaosultrahigh does not make another "<suffix>ultrahigh" match
    assert_eq!(s("iron_nultrahigh"), split("iron_nultrahigh", None, None));
    // no separator inside an alias
    assert_eq!(s("iron_rmaos_ultrahigh").alias, None);
    // a stem that is only the alias is left whole, as with built-in suffixes
    assert_eq!(s("_rmaosultrahigh"), split("_rmaosultrahigh", None, None));
    // built-in suffixes still split, and the alias-free splitter is unchanged
    assert_eq!(s("iron_n"), split("iron", Some("n"), None));
    assert_eq!(
        split_suffix("iron_rmaosultrahigh"),
        ("iron_rmaosultrahigh".into(), None)
    );
}

#[test]
fn outputs_use_the_canonical_suffix_and_collisions_are_duplicates() {
    let r = registry();
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in");
    for f in [
        "iron.png",
        "iron_n.png",
        "iron_rmaosultrahigh.png",
        "iron_nultrahigh.png",
        "rock_rmaos.png",
        "Rock_RMAOSULTRAHIGH.tga",
    ] {
        touch(&src.join(f));
    }
    let out = d.path().join("out");
    let mut files = discover(&[src], &out, false).unwrap();
    canonicalise_outputs(&mut files, r.alias_table());
    let output_of = |name: &str| {
        files
            .iter()
            .find(|f| f.rel.as_path() == Path::new(name))
            .unwrap()
            .output
            .clone()
    };
    assert_eq!(
        output_of("iron_rmaosultrahigh.png"),
        out.join("iron_rmaos.dds")
    );
    assert_eq!(
        output_of("Rock_RMAOSULTRAHIGH.tga"),
        out.join("Rock_rmaos.dds")
    );
    assert_eq!(
        output_of("iron_nultrahigh.png"),
        out.join("iron_nultrahigh.dds")
    );
    assert_eq!(output_of("iron_n.png"), out.join("iron_n.dds"));
    // rel and game_path keep the source name
    let aliased = files
        .iter()
        .find(|f| f.rel.as_path() == Path::new("iron_rmaosultrahigh.png"))
        .unwrap();
    assert!(aliased.game_path.ends_with("iron_rmaosultrahigh.png"));
    let (sets, dups) = group_sets_with(&files, r.alias_table());
    let iron = sets.iter().find(|s| s.key.ends_with("/iron")).unwrap();
    assert_eq!(iron.members.len(), 3);
    assert!(iron.suffixes.contains("rmaos") && iron.suffixes.contains("n"));
    assert!(!iron.suffixes.contains("rmaosultrahigh"));
    // rock_rmaos.png and Rock_RMAOSULTRAHIGH.tga both write rock_rmaos.dds: one IN-DUPLICATE group
    assert_eq!(dups.len(), 1);
    let names: BTreeSet<String> = dups[0]
        .iter()
        .map(|&i| files[i].rel.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "rock_rmaos.png".to_string(),
            "Rock_RMAOSULTRAHIGH.tga".to_string()
        ])
    );
}

#[test]
fn alias_preset_layers_on_its_chain_and_provenance_records_it() {
    let reg = registry();
    let j = resolve(
        &reg,
        "iron_rmaosultrahigh.png",
        &[],
        AcceptSuggestions::High,
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.subtype, Subtype::Rmaos);
    assert_eq!(j.profile.mips.levels, MipLevels::Count(1));
    assert_eq!(
        j.provenance["mips.levels"],
        Provenance::Preset("ultra".into())
    );
    assert_eq!(j.provenance["mips.levels"].to_string(), "preset:ultra");
    assert_eq!(
        j.suffix_alias,
        Some(AliasUse {
            alias: "rmaosultrahigh".into(),
            canonical: "rmaos".into(),
            preset: "ultra".into()
        })
    );
    assert!(
        j.suggestions[0]
            .reason
            .starts_with("suffix alias _rmaosultrahigh → _rmaos (preset ultra)"),
        "{}",
        j.suggestions[0].reason
    );
    let json = serde_json::to_value(&j).unwrap();
    assert_eq!(json["suffix_alias"]["preset"], "ultra");
    // the canonical name alone resolves with the batch preset only
    let plain = resolve(&reg, "iron_rmaos.png", &[], AcceptSuggestions::High);
    assert_eq!(
        plain.profile.mips.levels,
        MipLevels::Keyword(MipKeyword::Full)
    );
    assert_eq!(plain.provenance["mips.levels"], Provenance::Builtin);
    assert_eq!(plain.suffix_alias, None);
    assert!(serde_json::to_value(&plain)
        .unwrap()
        .get("suffix_alias")
        .is_none());
    assert!(!plain
        .provenance
        .values()
        .any(|p| *p == Provenance::Preset("ultra".into())));
    // --set still beats the alias preset
    let j = resolve(
        &reg,
        "iron_rmaosultrahigh.png",
        &["subtype:rmaos.mips.levels=none"],
        AcceptSuggestions::High,
    );
    assert_eq!(
        (j.profile.mips.levels, j.provenance["mips.levels"].clone()),
        (MipLevels::Keyword(MipKeyword::None), Provenance::User)
    );
}

#[test]
fn alias_preset_without_base_layers_on_the_target_quality_preset() {
    let mut reg = registry();
    reg.add_toml(NOBASE).unwrap();
    let ids = |id: &str, t: Target| -> Vec<String> {
        reg.alias_chain(id, t)
            .unwrap()
            .iter()
            .map(|p| p.id.clone())
            .collect()
    };
    assert_eq!(
        ids("nobase", Target::Cs),
        ["sdc-base", "cs-quality", "nobase"]
    );
    assert_eq!(
        ids("nobase", Target::Cpm),
        ["sdc-base", "cpm-quality", "nobase"]
    );
    assert_eq!(
        ids("nobase", Target::Vanilla),
        ["sdc-base", "vanilla-quality", "nobase"]
    );
    // an alias preset with a base keeps its declared chain
    assert_eq!(
        ids("ultra", Target::Cpm),
        ["sdc-base", "cs-quality", "ultra"]
    );
    let j = resolve(&reg, "iron_rmaoslow.png", &[], AcceptSuggestions::High);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.mips.levels, MipLevels::Count(2));
    assert_eq!(
        j.provenance["mips.levels"],
        Provenance::Preset("nobase".into())
    );
    // fields the alias preset does not set come from the base chain under it
    assert_eq!(j.provenance["format"], Provenance::Builtin);
    assert_eq!(j.suffix_alias.unwrap().preset, "nobase");
}

#[test]
fn alias_preset_alpha_policy_is_explicit() {
    let mut reg = registry();
    reg.add_toml(BLENDY).unwrap();
    assert!(!reg.is_builtin("blendy"));
    let mut img = Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]);
    for y in 0..64 {
        for x in 0..64 {
            img.at_mut(x, y)[3] = x as f32 / 63.0;
        }
    }
    let unconfirmed = |j: &ResolvedJob| match &j.status {
        JobStatus::Unresolved { reasons, .. } => reasons
            .iter()
            .any(|r| r.contains("alpha meaning is unconfirmed")),
        _ => false,
    };
    let j = resolve_img(
        &reg,
        "iron_diffblend.png",
        &[],
        AcceptSuggestions::High,
        &img,
        &RGBA8,
    );
    assert_eq!(j.profile.subtype, Subtype::Diffuse);
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Blend);
    assert_eq!(
        j.provenance["alpha.policy"],
        Provenance::Preset("blendy".into())
    );
    assert!(!unconfirmed(&j), "{:?}", j.status);
    // the same source under its canonical name keeps the alpha meaning unconfirmed
    let plain = resolve_img(
        &reg,
        "iron_d.png",
        &[],
        AcceptSuggestions::High,
        &img,
        &RGBA8,
    );
    assert!(unconfirmed(&plain), "{:?}", plain.status);
}

#[test]
fn alias_raises_suffix_confidence_except_for_s() {
    let reg = registry();
    // _sk alone is a low-confidence backlight; the alias declares it
    let j = resolve(&reg, "iron_backlit.png", &[], AcceptSuggestions::High);
    assert_eq!(
        (j.profile.subtype, j.suggestions[0].confidence),
        (Subtype::BacklightTint, Confidence::High)
    );
    assert_eq!(j.provenance["subtype"], Provenance::Suggestion);
    // _s keeps its ranked readings
    let j = resolve(&reg, "iron_skintint.png", &[], AcceptSuggestions::High);
    assert_eq!(j.profile.subtype, Subtype::SubsurfaceTint);
    assert!(j.suggestions[0].confidence < Confidence::High);
    assert!(
        matches!(j.status, JobStatus::Unresolved { .. }),
        "{:?}",
        j.status
    );
    // an alias decides nothing without --accept-suggestions high
    let j = resolve(
        &reg,
        "iron_rmaosultrahigh.png",
        &[],
        AcceptSuggestions::None,
    );
    assert!(
        matches!(j.status, JobStatus::Unresolved { .. }),
        "{:?}",
        j.status
    );
}

#[test]
fn accepted_fallback_keeps_the_alias_preset() {
    let mut reg = registry();
    reg.add_toml(&format!(
        "{}\n[subtypes.normal]\nmips = {{ levels = 3 }}\n\n[[rules]]\nmatch = {{ suffix = \"n\" }}\nsubtype = \"pbr_normal\"\n",
        preset_with("nhq", "normalhq = \"n\"")
    ))
    .unwrap();
    let overrides = vec![];
    let allow = BTreeSet::from([engine::codes::Code::CfgUnavailable]);
    let ctx = ResolveCtx {
        target: Target::Cpm,
        registry: &reg,
        preset: "cpm-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
    };
    let split = split_suffix_with("iron_normalhq", reg.alias_table());
    let f = InputFile {
        path: PathBuf::from("iron_normalhq.png"),
        rel: PathBuf::from("iron_normalhq.png"),
        game_path: "textures/armor/iron_normalhq.png".into(),
        output: PathBuf::from("iron_n.dds"),
        kind: InputKind::Image,
    };
    // The alias preset's rule names pbr_normal, unavailable under CPM; its fallback is normal, and the
    // fallback is resolved with the alias preset's chain too.
    let set = TextureSet {
        key: "textures/armor/iron".into(),
        members: vec![0],
        suffixes: ["n".to_string()].into(),
    };
    let img = Rgba32F::new(64, 64, [0.5, 0.5, 1.0, 1.0]);
    let a = analyse(&img, &RGB8);
    let j = resolve_file(
        &ctx,
        &FileFacts {
            input: &f,
            suffix: split.suffix.as_deref(),
            alias: split.alias.as_deref(),
            set: &set,
            analysis: &a,
            info: &RGB8,
        },
    );
    assert_eq!(j.profile.subtype, Subtype::Normal);
    assert_eq!(j.provenance["subtype"], Provenance::Fallback);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.mips.levels, MipLevels::Count(3));
    assert_eq!(
        j.provenance["mips.levels"],
        Provenance::Preset("nhq".into())
    );
    assert_eq!(j.suffix_alias.unwrap().preset, "nhq");
}
