use engine::analyse::analyse;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::*;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub fn input(game: &str) -> InputFile {
    let name = game.rsplit('/').next().unwrap();
    InputFile {
        path: PathBuf::from(name),
        rel: PathBuf::from(name),
        game_path: game.into(),
        output: PathBuf::from(name).with_extension("dds"),
        kind: InputKind::Image,
    }
}

const RGBA8: SourceInfo = SourceInfo {
    bit_depth: 8,
    channels: 4,
    has_alpha: true,
    is_float: false,
};
const RGB8: SourceInfo = SourceInfo {
    bit_depth: 8,
    channels: 3,
    has_alpha: false,
    is_float: false,
};

#[test]
fn explicit_set_decides_subtype_and_beats_presets() {
    let reg = PresetRegistry::builtin().unwrap();
    let overrides = [
        parse_set("file:**/x_n.png.subtype=normal").unwrap(),
        parse_set("subtype:normal.format=bc1").unwrap(),
    ];
    let allow = BTreeSet::new();
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::None,
        allow: &allow,
    };
    let img = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 0.5]);
    let a = analyse(&img, &RGBA8);
    let f = input("textures/a/x_n.png");
    let set = TextureSet {
        key: "x".into(),
        members: vec![0],
        suffixes: ["n".to_string()].into(),
    };
    let facts = FileFacts {
        input: &f,
        suffix: Some("n"),
        set: &set,
        analysis: &a,
        info: &RGBA8,
    };
    let d = decide_subtype(&ctx, &facts);
    assert_eq!(
        (d.subtype, d.decided, d.provenance.clone()),
        (Subtype::Normal, true, Provenance::User)
    );
    let (p, prov) = layered_profile(&ctx, &facts, Subtype::Normal).unwrap();
    assert_eq!(p.format, Format::Bc1);
    assert_eq!(prov["format"], Provenance::User);
    assert_eq!(prov["alpha.policy"], Provenance::Builtin);
}

#[test]
fn suggestions_decide_only_when_high_and_accepted() {
    let reg = PresetRegistry::builtin().unwrap();
    let allow = BTreeSet::new();
    let img = Rgba32F::new(4, 4, [0.5, 0.5, 0.5, 1.0]);
    let a = analyse(&img, &RGB8);
    let f = input("textures/a/x_rmaos.png");
    let set = TextureSet {
        key: "x".into(),
        members: vec![0],
        suffixes: ["rmaos".to_string()].into(),
    };
    let facts = FileFacts {
        input: &f,
        suffix: Some("rmaos"),
        set: &set,
        analysis: &a,
        info: &RGB8,
    };
    for (accept, decided) in [
        (AcceptSuggestions::None, false),
        (AcceptSuggestions::High, true),
    ] {
        let ctx = ResolveCtx {
            target: Target::Cs,
            registry: &reg,
            preset: "cs-quality",
            user_config: None,
            project_config: None,
            overrides: &[],
            accept,
            allow: &allow,
        };
        let d = decide_subtype(&ctx, &facts);
        assert_eq!((d.subtype, d.decided), (Subtype::Rmaos, decided));
    }
    // RGB source: alpha fact -> remove, recorded as Analysis
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &[],
        accept: AcceptSuggestions::High,
        allow: &allow,
    };
    let (p, prov) = layered_profile(&ctx, &facts, Subtype::Rmaos).unwrap();
    assert_eq!(
        (p.alpha.policy, prov["alpha.policy"].clone()),
        (AlphaPolicy::Remove, Provenance::Analysis)
    );
}

#[test]
fn rules_and_project_config_layer_in_order() {
    let reg = PresetRegistry::builtin().unwrap();
    let project: ConfigFile = toml::from_str(
        "[subtypes.diffuse]\nformat = \"bc1\"\n[[rules]]\nmatch = { glob = \"**/ui/**\" }\nsubtype = \"diffuse\"\noverrides = { mips = { levels = 1 } }\n",
    )
    .unwrap();
    let allow = BTreeSet::new();
    let ctx = ResolveCtx {
        target: Target::Vanilla,
        registry: &reg,
        preset: "vanilla-quality",
        user_config: None,
        project_config: Some(&project),
        overrides: &[],
        accept: AcceptSuggestions::None,
        allow: &allow,
    };
    let img = Rgba32F::new(4, 4, [0.5, 0.5, 0.5, 1.0]);
    let a = analyse(&img, &RGB8);
    let f = input("textures/ui/icon.png");
    let set = TextureSet {
        key: "i".into(),
        members: vec![0],
        suffixes: BTreeSet::new(),
    };
    let facts = FileFacts {
        input: &f,
        suffix: None,
        set: &set,
        analysis: &a,
        info: &RGB8,
    };
    let d = decide_subtype(&ctx, &facts);
    assert_eq!(
        (d.subtype, d.decided, d.provenance.clone()),
        (Subtype::Diffuse, true, Provenance::Rule("project#0".into()))
    );
    let (p, prov) = layered_profile(&ctx, &facts, d.subtype).unwrap();
    assert_eq!(
        (p.format, p.mips.levels),
        (Format::Bc1, MipLevels::Count(1))
    );
    assert_eq!(prov["format"], Provenance::ProjectConfig);
    assert_eq!(prov["mips.levels"], Provenance::Rule("project#0".into()));
}

#[test]
fn set_scopes_rank_above_command_line_order() {
    let reg = PresetRegistry::builtin().unwrap();
    let allow = BTreeSet::new();
    let img = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 0.5]);
    let a = analyse(&img, &RGBA8);
    let f = input("textures/a/x_n.png");
    let set = TextureSet {
        key: "x".into(),
        members: vec![0],
        suffixes: ["n".to_string()].into(),
    };
    let facts = FileFacts {
        input: &f,
        suffix: Some("n"),
        set: &set,
        analysis: &a,
        info: &RGBA8,
    };
    // file-scoped beats sub-type-scoped beats unscoped, whatever the command-line order
    let overrides = [
        parse_set("file:**/x_n.png.format=bc3").unwrap(),
        parse_set("subtype:normal.format=bc1").unwrap(),
        parse_set("subtype:normal.mips.levels=2").unwrap(),
        parse_set("format=bc7").unwrap(),
        parse_set("mips.levels=1").unwrap(),
        parse_set("subtype:msn.alpha.threshold=50").unwrap(),
        parse_set("file:**/x_n.png.subtype=normal").unwrap(),
        parse_set("subtype=msn").unwrap(),
    ];
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::None,
        allow: &allow,
    };
    let d = decide_subtype(&ctx, &facts);
    assert_eq!((d.subtype, d.decided), (Subtype::Normal, true));
    let (p, prov) = layered_profile(&ctx, &facts, d.subtype).unwrap();
    assert_eq!(
        (p.format, p.mips.levels, p.alpha.threshold),
        (Format::Bc3, MipLevels::Count(2), 127)
    );
    assert_eq!(prov["mips.levels"], Provenance::User);
    // a later --set in the same scope wins
    let overrides = [
        parse_set("format=bc3").unwrap(),
        parse_set("format=bc1").unwrap(),
    ];
    let ctx = ResolveCtx {
        overrides: &overrides,
        ..ctx
    };
    assert_eq!(
        layered_profile(&ctx, &facts, Subtype::Normal)
            .unwrap()
            .0
            .format,
        Format::Bc1
    );
}
