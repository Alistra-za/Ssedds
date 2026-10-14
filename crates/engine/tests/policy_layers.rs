use engine::analyse::analyse;
use engine::config::*;
use engine::discover::{InputFile, InputKind};
use engine::group::TextureSet;
use engine::image::{Rgba32F, SourceInfo};
use engine::overrides::{parse_set, NO_FILE_SCOPES};
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
        real_rel: None,
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
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
        alias: None,
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
        alias: None,
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
            files: &NO_FILE_SCOPES,
            shaders: &NO_SHADERS,
            mesh_alpha: &NO_MESH_ALPHA,
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
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
        alias: None,
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
        alias: None,
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
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
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

const RGB32F: SourceInfo = SourceInfo {
    bit_depth: 32,
    channels: 3,
    has_alpha: false,
    is_float: true,
};

/// Resolves `game` as `st` under cs / cs-quality and returns the profile's `color.content` with its provenance.
fn content_of(
    game: &str,
    suffix: &str,
    info: &SourceInfo,
    st: Subtype,
    sets: &[&str],
) -> (ColorContent, String) {
    let reg = PresetRegistry::builtin().unwrap();
    let allow = BTreeSet::new();
    let overrides: Vec<_> = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "cs-quality",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::None,
        allow: &allow,
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
        mesh_alpha: &NO_MESH_ALPHA,
    };
    let img = Rgba32F::new(4, 4, [4.0, 2.0, 0.5, 1.0]);
    let a = analyse(&img, info);
    let f = input(game);
    let set = TextureSet {
        key: "x".into(),
        members: vec![0],
        suffixes: [suffix.to_string()].into(),
    };
    let facts = FileFacts {
        input: &f,
        suffix: Some(suffix),
        alias: None,
        set: &set,
        analysis: &a,
        info,
    };
    let (p, prov) = layered_profile(&ctx, &facts, st).unwrap();
    (p.color.content, prov["color.content"].to_string())
}

/// Spec §4.1 `pbr_emissive` / `hdr_sky`: content is linear for a float source that goes to BC6H, an analysis
/// fact (above presets, config and rules; below `--set`) with provenance `analysis:float-source`.
#[test]
fn float_source_to_bc6h_resolves_linear_content_as_an_analysis_fact() {
    let g = "textures/pbr/lamp_g.tif";
    assert_eq!(
        content_of(g, "g", &RGB32F, Subtype::PbrEmissive, &[]),
        (ColorContent::Linear, "analysis:float-source".into())
    );
    assert_eq!(
        content_of(
            "textures/sky/dome.tif",
            "d",
            &RGB32F,
            Subtype::HdrSky,
            &["file:**/dome.tif.format=bc6h_uf16"]
        )
        .0,
        ColorContent::Linear
    );
    // 8-bit stays sRGB.
    assert_eq!(
        content_of(
            "textures/pbr/lamp_g.png",
            "g",
            &RGB8,
            Subtype::PbrEmissive,
            &[]
        )
        .0,
        ColorContent::Srgb
    );
    // An explicit --set wins, at any scope.
    for s in [
        "color.content=srgb",
        "file:**/lamp_g.tif.color.content=srgb",
    ] {
        assert_eq!(
            content_of(g, "g", &RGB32F, Subtype::PbrEmissive, &[s]),
            (ColorContent::Srgb, "user".into()),
            "{s}"
        );
    }
    // The fact needs BC6H: a float source forced to BC7 keeps the LDR rule (separate carry).
    assert_eq!(
        content_of(g, "g", &RGB32F, Subtype::PbrEmissive, &["format=bc7"]).0,
        ColorContent::Srgb
    );
    // Controller ruling (Task 6): every HDR format carries linear radiance, RGBA16F included.
    assert_eq!(
        content_of(g, "g", &RGB32F, Subtype::PbrEmissive, &["format=rgba16f"]),
        (ColorContent::Linear, "analysis:float-source".into())
    );
    // A float LDR sub-type is out of scope.
    assert_eq!(
        content_of("textures/a/rock_d.tif", "d", &RGB32F, Subtype::Diffuse, &[]).0,
        ColorContent::Srgb
    );
}

/// M6 Track F2 Task 1: out-of-range `rnm.*` values warn on a base normal; another sub-type keeps the default.
#[test]
fn rnm_ranges_and_non_base_reset() {
    use engine::api::*;
    use engine::codes::Code;
    use engine::events::*;
    use image::{ImageBuffer, Rgb};
    use std::sync::Arc;

    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in");
    std::fs::create_dir_all(&dir).unwrap();
    ImageBuffer::from_pixel(64, 64, Rgb([128u8, 128, 255]))
        .save(dir.join("iron.png"))
        .unwrap();
    ImageBuffer::from_pixel(64, 64, Rgb([128u8, 128, 255]))
        .save(dir.join("iron_n.png"))
        .unwrap();
    let plan = |sets: &[&str]| {
        let mut req = BatchRequest::new(vec![dir.clone()], d.path().join("out"), Target::Cs);
        req.accept = AcceptSuggestions::High;
        req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
        let rep = Reporter::new(Arc::new(VecSink::default()), "rnm-ranges");
        Engine::new()
            .unwrap()
            .plan(req, &rep, &CancelToken::default())
            .unwrap()
    };
    let job = |p: &Plan, name: &str| {
        p.jobs
            .iter()
            .find(|j| j.input.ends_with(name))
            .cloned()
            .unwrap()
    };

    let p = plan(&["rnm.detail.strength=5"]);
    let n = job(&p, "iron_n.png");
    assert!(
        n.warnings
            .iter()
            .any(|w| w.code == Code::CfgInvalid && w.message.contains("rnm.detail.strength")),
        "{:?}",
        n.warnings
    );

    let p = plan(&["rnm.enabled=false"]);
    assert!(job(&p, "iron.png").profile.rnm.is_default());

    let p = plan(&["rnm.bent.ao_power=0.05"]);
    assert!(job(&p, "iron_n.png")
        .warnings
        .iter()
        .any(|w| w.code == Code::CfgInvalid && w.message.contains("rnm.bent.ao_power")));
}
