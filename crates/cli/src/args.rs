//! Command-line arguments (spec §8) and their translation into an engine `BatchRequest`.
use clap::{Args, Parser, Subcommand, ValueEnum};
use encoders_sys::Bc1Model;
use engine::api::{BatchRequest, OnExisting};
use engine::codes::Code;
use engine::config::{RawMap, Target};
use engine::overrides::{parse_set, SetOverride};
use engine::policy::AcceptSuggestions;
use engine::presets::PresetRegistry;
use engine::schemas::SchemaKind;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "sdc",
    version,
    about = "sdc — the Tessalith command line: Skyrim SE/AE DDS converter"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Convert images or folders to DDS.
    Convert(ConvertArgs),
    /// Analyse inputs; print proposed sub-type, profile and warnings; DDS headers and mip statistics.
    Inspect(InspectArgs),
    /// Run the spec §5.8 checks on existing DDS files.
    Validate(ValidateArgs),
    /// Inspect, compare, export and validate presets, and print schemas. User presets in %APPDATA%\sdc\presets\*.toml
    /// are always loaded.
    Presets(PresetsArgs),
    /// Built-in encoder versions and NVTT detection.
    Encoders(EncodersArgs),
    /// Fix DDS header faults that need no pixel change (mip count, flags, caps, legacy/DX10, an explicit tag).
    PatchHeader(PatchHeaderArgs),
}

#[derive(Clone, Copy, ValueEnum)]
pub enum TargetArg {
    Cs,
    Vanilla,
}

impl From<TargetArg> for Target {
    fn from(t: TargetArg) -> Target {
        match t {
            TargetArg::Cs => Target::Cs,
            TargetArg::Vanilla => Target::Vanilla,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum AcceptArg {
    None,
    High,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OnExistingArg {
    IfChanged,
    Skip,
    Overwrite,
}

impl From<OnExistingArg> for OnExisting {
    fn from(o: OnExistingArg) -> OnExisting {
        match o {
            OnExistingArg::IfChanged => OnExisting::IfChanged,
            OnExistingArg::Skip => OnExisting::Skip,
            OnExistingArg::Overwrite => OnExisting::Overwrite,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Bc1Arg {
    Ideal,
    IdealRound4,
    Nvidia,
    Amd,
}

impl From<Bc1Arg> for Bc1Model {
    fn from(b: Bc1Arg) -> Bc1Model {
        match b {
            Bc1Arg::Ideal => Bc1Model::Ideal,
            Bc1Arg::IdealRound4 => Bc1Model::IdealRound4,
            Bc1Arg::Nvidia => Bc1Model::Nvidia,
            Bc1Arg::Amd => Bc1Model::Amd,
        }
    }
}

#[derive(Args, Clone)]
pub struct BatchArgs {
    /// Image files or folders.
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,
    /// Target (default: from the configs or the selected preset, finally `cs`).
    #[arg(long, value_enum)]
    pub target: Option<TargetArg>,
    /// Preset id (built-in, or from %APPDATA%\sdc\presets\*.toml), or a path to a preset .toml file.
    #[arg(long)]
    pub preset: Option<String>,
    /// Load a preset .toml without selecting it, after the user presets folder, e.g. one declaring suffix aliases
    /// (repeatable). An id that is already loaded is an error.
    #[arg(long = "preset-file")]
    pub preset_file: Vec<PathBuf>,
    /// Project config (default: sdc.toml in the first input folder).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Field override: field=value, subtype:<id>.field=value, file:<glob>.field=value. `set:<id>.shader=<shader>`
    /// picks a set's shader (ids as `inspect` prints them after `set`).
    #[arg(long = "set")]
    pub set: Vec<String>,
    /// Fill value for a packed channel without a source map: <role>=<0..255>, e.g. metallic=255 (repeatable). Same
    /// as `--set packing.fill.<role>=<value>`, applied after every `--set`.
    #[arg(long = "fill", value_name = "ROLE=VALUE")]
    pub fill: Vec<String>,
    #[arg(long, value_enum, default_value = "none")]
    pub accept_suggestions: AcceptArg,
    /// Acknowledge a blocking warning code (repeatable).
    #[arg(long)]
    pub allow: Vec<String>,
    #[arg(long)]
    pub recursive: bool,
    /// Write JSON Lines events to stdout (human output stays on stderr).
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ConvertArgs {
    #[command(flatten)]
    pub batch: BatchArgs,
    /// Output root (default: ./sdc-out). With `[export] also` in `sdc.toml`, the folder the PBR, Vanilla and CM folders go in.
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    /// Worker threads (0 = all cores).
    #[arg(long, default_value_t = 0)]
    pub jobs: usize,
    /// Memory budget, e.g. 8G or 512M (default 50% of RAM).
    #[arg(long)]
    pub mem_budget: Option<String>,
    #[arg(long, value_enum, default_value = "if-changed")]
    pub on_existing: OnExistingArg,
    #[arg(long)]
    pub dry_run: bool,
    /// Also write <name>.dds.json next to each output.
    #[arg(long)]
    pub sidecar: bool,
    /// Path to nvcompress.exe for `encoder.backend = "nvtt"` (default: PATH, then the NVIDIA install folder).
    /// NVTT is never bundled; it is looked for only when a texture uses it.
    #[arg(long)]
    pub nvtt: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
}

#[derive(Args)]
pub struct EncodersArgs {
    /// Path to nvcompress.exe (default: PATH, then the NVIDIA install folder; NVTT is never bundled).
    #[arg(long)]
    pub nvtt: Option<PathBuf>,
    /// BC1 decoder model to report as active (the batch-wide default is `ideal`).
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
    /// Print one JSON object instead of the human report.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct InspectArgs {
    #[command(flatten)]
    pub batch: BatchArgs,
    /// BC1 decoder model used to decode DDS inputs.
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
}

#[derive(Args)]
pub struct ValidateArgs {
    #[arg(required = true)]
    pub dds: Vec<PathBuf>,
    /// Target whose rules apply (default cs).
    #[arg(long, value_enum)]
    pub target: Option<TargetArg>,
    /// Sub-type the files are meant to be (default: inferred from the file name).
    #[arg(long)]
    pub subtype: Option<String>,
    /// Alpha policy the files are meant to follow (default: inferred from the stored alpha; `remove` if
    /// every mip-0 alpha is at least 254).
    #[arg(long, value_parser = ["remove", "cutout", "blend", "cutout_blend", "data"])]
    pub alpha_policy: Option<String>,
    /// Hold BC7 files under `remove` to sdc's exact-opaque guarantee (decoded alpha 255, not 254). Off by
    /// default: only sdc's own encoder promises it, so third-party files are not judged by it.
    #[arg(long)]
    pub exact_opaque: bool,
    /// Write one JSON object per file to stdout (the human report goes to stderr).
    #[arg(long)]
    pub json: bool,
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
}

/// The schema to print. Mirrors `engine::schemas::SchemaKind`, which stays free of clap.
#[derive(Clone, Copy, ValueEnum)]
pub enum SchemaArg {
    All,
    Profile,
    Preset,
    Config,
    Event,
    Manifest,
    DdsInfo,
    DdsValidation,
}

impl From<SchemaArg> for SchemaKind {
    fn from(s: SchemaArg) -> SchemaKind {
        match s {
            SchemaArg::All => SchemaKind::All,
            SchemaArg::Profile => SchemaKind::Profile,
            SchemaArg::Preset => SchemaKind::Preset,
            SchemaArg::Config => SchemaKind::Config,
            SchemaArg::Event => SchemaKind::Event,
            SchemaArg::Manifest => SchemaKind::Manifest,
            SchemaArg::DdsInfo => SchemaKind::DdsInfo,
            SchemaArg::DdsValidation => SchemaKind::DdsValidation,
        }
    }
}

#[derive(Args)]
pub struct PresetsArgs {
    /// Load a preset .toml after the user presets folder (repeatable). An id that is already loaded is an error.
    #[arg(long = "preset-file", global = true)]
    pub preset_file: Vec<PathBuf>,
    #[command(subcommand)]
    pub cmd: PresetsCmd,
}

#[derive(Subcommand)]
pub enum PresetsCmd {
    /// List selectable presets, built-in and user (with their source file).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show a preset's resolved profile(s) with provenance.
    Show {
        id: String,
        #[arg(long)]
        subtype: Option<String>,
        #[arg(long, value_enum)]
        target: Option<TargetArg>,
    },
    /// Print the fields where two presets resolve differently: `<subtype>.<field>: <a> → <b>`.
    ///
    /// Profiles only: rules are not compared.
    Diff {
        a: String,
        b: String,
        #[arg(long)]
        subtype: Option<String>,
        #[arg(long, value_enum)]
        target: Option<TargetArg>,
        #[arg(long)]
        json: bool,
    },
    /// Write a preset's own TOML (keeping its `base` link) under a new id (default `<id>-export`).
    ///
    /// The table is re-serialised: comments are dropped and keys may reorder. Suffix aliases are copied as they are,
    /// so a copy of a preset with suffix aliases does not load beside the original: rename or remove the aliases
    /// before you put the copy in the user presets folder. `--out` refuses the source file of a loaded preset.
    Export {
        id: String,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Id of the exported preset.
        #[arg(long = "id")]
        new_id: Option<String>,
    },
    /// Check preset files (migration, parse, ids, base, aliases, rules, every profile). Exit 1 if any has a problem.
    Validate {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Print a JSON Schema (profile, preset, config, event, manifest, dds-info, dds-validation) or write all to a folder.
    Schema {
        #[arg(value_enum)]
        which: SchemaArg,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// `--mem-budget`: plain bytes or a `G`/`M` suffix (binary multiples).
pub fn parse_bytes(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let (num, mul) = match s.chars().last() {
        Some('G' | 'g') => (&s[..s.len() - 1], 1u64 << 30),
        Some('M' | 'm') => (&s[..s.len() - 1], 1u64 << 20),
        Some(c) if c.is_ascii_digit() => (s, 1),
        _ => return Err(format!("--mem-budget '{s}': use bytes or a G/M suffix")),
    };
    match num
        .trim()
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mul))
    {
        // The engine reads 0 as "default", so an explicit 0 would silently mean 50% of RAM.
        Some(0) => Err(format!(
            "--mem-budget '{s}': must be greater than 0 (omit the flag for the default, 50% of RAM)"
        )),
        Some(n) => Ok(n),
        None => Err(format!("--mem-budget '{s}': not a valid size")),
    }
}

/// `--fill <role>=<0..255>` (spec §6.4, DECISIONS Note M5): an unscoped `packing.fill.<role>` override.
pub fn parse_fill(s: &str) -> Result<SetOverride, String> {
    let (role, value) = s
        .split_once('=')
        .ok_or_else(|| format!("--fill '{s}': expected <role>=<0..255>"))?;
    let role = role.trim().to_ascii_lowercase();
    if !RawMap::ALL.iter().any(|r| r.id() == role) {
        let roles: Vec<String> = RawMap::ALL.iter().map(|r| r.id()).collect();
        return Err(format!(
            "--fill '{s}': unknown role '{role}' (one of {})",
            roles.join(", ")
        ));
    }
    let code: u8 = value
        .trim()
        .parse()
        .map_err(|_| format!("--fill '{s}': the value must be a whole number 0..255"))?;
    parse_set(&format!("packing.fill.{role}={code}"))
}

/// `%APPDATA%\sdc\presets`, the user presets folder (M6 Track D spec §3); `None` when APPDATA is unset.
pub fn user_presets_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("presets"))
}

/// Translate the shared batch flags into a `BatchRequest`. Errors are usage or configuration errors (exit 2).
pub fn build_request(b: &BatchArgs, output: PathBuf) -> Result<BatchRequest, String> {
    // `BatchRequest::new` takes a target; `--target` is optional, so reset it to "not given".
    let mut r = BatchRequest::new(b.inputs.clone(), output, Target::Cs);
    r.target = b.target.map(Target::from);
    match &b.preset {
        Some(p) if p.to_ascii_lowercase().ends_with(".toml") => {
            // `--preset <file.toml>` loads the file and selects the id it declares.
            let path = PathBuf::from(p);
            let id = PresetRegistry::builtin()
                .and_then(|mut reg| reg.add_file(&path))
                .map_err(|e| format!("--preset {p}: {e}"))?;
            r.preset_files.push(path);
            r.preset = Some(id);
        }
        Some(p) => r.preset = Some(p.clone()),
        None => {}
    }
    // `--preset-file` loads without selecting (spec §3.7, §8).
    r.preset_files.extend(b.preset_file.iter().cloned());
    r.project_config = b.config.clone();
    r.user_config =
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("config.toml"));
    r.user_presets = user_presets_dir();
    r.overrides = b
        .set
        .iter()
        .map(|s| parse_set(s))
        .collect::<Result<_, _>>()?;
    // DECISIONS Note M5: fills rank after every --set (the same precedence group as an unscoped --set, later).
    for f in &b.fill {
        r.overrides.push(parse_fill(f)?);
    }
    r.allow = b
        .allow
        .iter()
        .map(|a| Code::parse(a).ok_or_else(|| format!("--allow {a}: unknown code")))
        .collect::<Result<_, _>>()?;
    r.accept = match b.accept_suggestions {
        AcceptArg::None => AcceptSuggestions::None,
        AcceptArg::High => AcceptSuggestions::High,
    };
    r.recursive = b.recursive;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_is_an_unscoped_packing_override() {
        let o = parse_fill("Metallic=255").unwrap();
        assert_eq!(o.field, "packing.fill.metallic");
        assert_eq!(o.value, serde_json::json!(255));
        assert!(parse_fill("metal=1").unwrap_err().contains("unknown role"));
        assert!(parse_fill("ao=300").unwrap_err().contains("0..255"));
    }

    #[test]
    fn parse_bytes_units() {
        assert_eq!(parse_bytes("512"), Ok(512));
        assert_eq!(parse_bytes("512M"), Ok(512 << 20));
        assert_eq!(parse_bytes("8G"), Ok(8 << 30));
        assert_eq!(parse_bytes(" 2g "), Ok(2 << 30));
        for bad in [
            "",
            "G",
            "8Q",
            "-1",
            "1.5G",
            "99999999999999999999G",
            "0",
            "0G",
            "00M",
        ] {
            assert!(parse_bytes(bad).is_err(), "{bad}");
        }
        assert!(parse_bytes("0").unwrap_err().contains("greater than 0"));
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum TagArg {
    Srgb,
    Unorm,
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("dest").required(true).args(["out_dir", "in_place"])))]
pub struct PatchHeaderArgs {
    #[arg(required = true)]
    pub dds: Vec<PathBuf>,
    /// Write each patched file into this folder (by file name). The input is never changed.
    #[arg(long)]
    pub out_dir: Option<PathBuf>,
    /// Rewrite each input in place (atomically).
    #[arg(long)]
    pub in_place: bool,
    /// Target whose rules give the tag and mip expectation of `--subtype` (default cs).
    #[arg(long, value_enum)]
    pub target: Option<TargetArg>,
    /// Sub-type the files are meant to be: sets the tag and requires a full mip chain when its profile has one.
    #[arg(long)]
    pub subtype: Option<String>,
    /// Tag to write; overrides `--subtype`'s. Without either, the file's tag is kept.
    #[arg(long, value_enum)]
    pub tag: Option<TagArg>,
    /// Block format the files must be; another one is refused.
    #[arg(long)]
    pub format: Option<String>,
    /// Report what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}
