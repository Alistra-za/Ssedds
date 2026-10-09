//! Command-line arguments (spec §8) and their translation into an engine `BatchRequest`.
use clap::{Args, Parser, Subcommand, ValueEnum};
use encoders_sys::Bc1Model;
use engine::api::{BatchRequest, OnExisting};
use engine::codes::Code;
use engine::config::Target;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::presets::PresetRegistry;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "sdc",
    version,
    about = "Skyrim SE/AE DDS converter (working name)"
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
    /// Inspect presets and print schemas.
    #[command(subcommand)]
    Presets(PresetsCmd),
    /// Built-in encoder versions and NVTT detection.
    Encoders,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum TargetArg {
    Cs,
    Cpm,
    Vanilla,
}

impl From<TargetArg> for Target {
    fn from(t: TargetArg) -> Target {
        match t {
            TargetArg::Cs => Target::Cs,
            TargetArg::Cpm => Target::Cpm,
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
    /// Preset id, or a path to a preset .toml file.
    #[arg(long)]
    pub preset: Option<String>,
    /// Load a preset .toml without selecting it, e.g. one declaring suffix aliases (repeatable).
    #[arg(long = "preset-file")]
    pub preset_file: Vec<PathBuf>,
    /// Project config (default: sdc.toml in the first input folder).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Field override: field=value, subtype:<id>.field=value, file:<glob>.field=value.
    #[arg(long = "set")]
    pub set: Vec<String>,
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
    /// Output root (default: ./sdc-out).
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
    /// Path to nvcompress.exe (NVTT backend arrives in milestone M5).
    #[arg(long)]
    pub nvtt: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
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
    /// Write one JSON object per file to stdout (the human report goes to stderr).
    #[arg(long)]
    pub json: bool,
    #[arg(long, value_enum, default_value = "ideal")]
    pub bc1_decoder: Bc1Arg,
}

#[derive(Subcommand)]
pub enum PresetsCmd {
    /// List selectable presets.
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
    /// Print a JSON Schema (profile, preset, config, event, manifest) or write all to a folder.
    Schema {
        /// Task 43 changes this to `#[arg(value_enum)] which: engine::schemas::SchemaKind`.
        which: String,
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
    num.trim()
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mul))
        .ok_or_else(|| format!("--mem-budget '{s}': not a valid size"))
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
    r.overrides = b
        .set
        .iter()
        .map(|s| parse_set(s))
        .collect::<Result<_, _>>()?;
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
    fn parse_bytes_units() {
        assert_eq!(parse_bytes("512"), Ok(512));
        assert_eq!(parse_bytes("512M"), Ok(512 << 20));
        assert_eq!(parse_bytes("8G"), Ok(8 << 30));
        assert_eq!(parse_bytes(" 2g "), Ok(2 << 30));
        for bad in ["", "G", "8Q", "-1", "1.5G", "99999999999999999999G"] {
            assert!(parse_bytes(bad).is_err(), "{bad}");
        }
    }
}
