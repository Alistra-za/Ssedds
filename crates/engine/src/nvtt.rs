//! NVIDIA Texture Tools 3 (`nvcompress`) as an optional BC7/BC6H encoder (spec §7.5, DECISIONS Note NV). Never bundled
//! or linked: the user installs it and the engine runs it as a separate process.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// spec §7.5, DECISIONS Note NV2/NV3.
pub const NVTT_TESTED: &str = "3.2.5";
pub const NVTT_DEFAULT: &str =
    r"C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe";

/// Every flag the backend passes (DECISIONS Note NV4); the probe checks the usage lists each one.
pub const NVTT_FLAGS: [&str; 7] = [
    "-silent",
    "-nomips",
    "-production",
    "-bc7",
    "-bc6",
    "-noalpha",
    "-nocuda",
];

/// `CREATE_NO_WINDOW`: the GUI must not flash a console for each nvcompress process.
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvttTool {
    pub path: PathBuf,
    pub version: String,
}

impl NvttTool {
    pub fn tested(&self) -> bool {
        self.version == NVTT_TESTED
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NvttProbe {
    Found(NvttTool),
    /// Where it looked, or why the probe failed.
    Missing(String),
}

impl NvttProbe {
    pub fn tool(&self) -> Option<&NvttTool> {
        match self {
            NvttProbe::Found(t) => Some(t),
            NvttProbe::Missing(_) => None,
        }
    }
}

/// NV2: `explicit` (missing → not found, no fallback), else `nvcompress.exe` on `path_var`, else `default`. Returns
/// the path and where it came from (`"explicit"`, `"PATH"` or `"default"`), or where it looked.
pub fn locate(
    explicit: Option<&Path>,
    path_var: Option<&OsStr>,
    default: &Path,
) -> Result<(PathBuf, &'static str), String> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Ok((p.to_path_buf(), "explicit"))
        } else {
            Err(format!("{} does not exist", p.display()))
        };
    }
    if let Some(p) = path_var.and_then(|v| {
        std::env::split_paths(v)
            .map(|d| d.join("nvcompress.exe"))
            .find(|c| c.is_file())
    }) {
        return Ok((p, "PATH"));
    }
    if default.is_file() {
        return Ok((default.to_path_buf(), "default"));
    }
    Err(format!("not on PATH and not at {}", default.display()))
}

/// NV3: the version from the banner line `NVIDIA Texture Tools <x.y.z> ...` (the first line nvcompress prints).
pub fn parse_banner(text: &str) -> Result<String, String> {
    let line = text.lines().next().unwrap_or("");
    line.strip_prefix("NVIDIA Texture Tools ")
        .and_then(|s| s.split_whitespace().next())
        .filter(|v| v.split('.').count() == 3 && v.split('.').all(|n| n.parse::<u32>().is_ok()))
        .map(str::to_string)
        .ok_or_else(|| format!("unexpected banner {line:?}"))
}

/// [`locate`] with the process `PATH` and [`NVTT_DEFAULT`], then [`probe_path`].
pub fn probe(explicit: Option<&Path>) -> NvttProbe {
    match locate(
        explicit,
        std::env::var_os("PATH").as_deref(),
        Path::new(NVTT_DEFAULT),
    ) {
        Ok((p, _)) => probe_path(&p),
        Err(why) => NvttProbe::Missing(why),
    }
}

/// Runs `path` with no arguments (nvcompress has no `-h`; it prints the banner and usage and exits 1, Spike S7b),
/// parses the banner and checks the usage lists every flag in [`NVTT_FLAGS`].
/// shortcut: no timeout on the probe, add one if a hung nvcompress is ever seen.
pub fn probe_path(path: &Path) -> NvttProbe {
    let mut cmd = Command::new(path);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, CREATE_NO_WINDOW);
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => return NvttProbe::Missing(format!("{}: {e}", path.display())),
    };
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let version = match parse_banner(&text) {
        Ok(v) => v,
        Err(e) => return NvttProbe::Missing(format!("{}: {e}", path.display())),
    };
    let missing: Vec<&str> = NVTT_FLAGS
        .into_iter()
        .filter(|f| {
            !text
                .lines()
                .any(|l| l.split_whitespace().next() == Some(*f))
        })
        .collect();
    if !missing.is_empty() {
        return NvttProbe::Missing(format!(
            "{}: the usage does not list {}",
            path.display(),
            missing.join(" ")
        ));
    }
    NvttProbe::Found(NvttTool {
        path: path.to_path_buf(),
        version,
    })
}

/// The batch's probe, run on first use only, so a batch with no profile on NVTT never starts nvcompress.
#[derive(Debug, Default)]
pub struct NvttCell {
    explicit: Option<PathBuf>,
    cell: OnceLock<NvttProbe>,
}

impl NvttCell {
    pub fn new(explicit: Option<PathBuf>) -> Self {
        NvttCell {
            explicit,
            cell: OnceLock::new(),
        }
    }

    /// An already probed result (tests).
    pub fn fixed(p: NvttProbe) -> Self {
        NvttCell {
            explicit: None,
            cell: OnceLock::from(p),
        }
    }

    pub fn get(&self) -> &NvttProbe {
        self.cell.get_or_init(|| probe(self.explicit.as_deref()))
    }

    /// The probe result if anything asked for it.
    pub fn into_probed(self) -> Option<NvttProbe> {
        self.cell.into_inner()
    }
}
