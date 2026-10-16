//! NVIDIA Texture Tools 3 (`nvcompress`) as an optional BC7/BC6H encoder (spec §7.5, DECISIONS Note NV). Never bundled
//! or linked: the user installs it and the engine runs it as a separate process.
use crate::config::{AlphaPolicy, Format, Profile};
use crate::dds::{dds_bytes, DdsDesc, ResolvedHeader, DDPF_FOURCC};
use crate::encode::{bc7_post_fix, pack_uncompressed};
use crate::quantize::EncoderInput;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::time::Duration;

/// spec §7.5, DECISIONS Note NV2/NV3.
pub const NVTT_TESTED: &str = "3.2.5";
pub const NVTT_DEFAULT: &str =
    r"C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe";

/// Every flag the backend passes (DECISIONS Note NV4); the probe checks the usage lists each one.
pub const NVTT_FLAGS: [&str; 8] = [
    "-silent",
    "-nomips",
    "-production",
    "-bc7",
    "-bc6",
    "-noalpha",
    "-alpha",
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

/// How long the probe waits for the program to exit (nvcompress answers in about 30 ms, Spike S7b).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Kill `child` and every process it started (a launcher's grandchild would keep running otherwise), then reap it.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let mut tk = Command::new(Path::new(&root).join("System32").join("taskkill.exe"));
        tk.args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        std::os::windows::process::CommandExt::creation_flags(&mut tk, CREATE_NO_WINDOW);
        let _ = tk.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `path` with no arguments (nvcompress has no `-h`; it prints the banner and usage and exits 1, Spike S7b),
/// parses the banner and checks the usage lists every flag in [`NVTT_FLAGS`].
/// Never runs NVIDIA's exporter app or the running program itself, and kills a program that has not exited after
/// [`PROBE_TIMEOUT`], with every process it started (DECISIONS AF3): a wrong pick must not hang or relaunch the
/// caller. Its output goes to files, not pipes, so nothing waits on a handle a grandchild still holds.
pub fn probe_path(path: &Path) -> NvttProbe {
    let missing = |why: &str| NvttProbe::Missing(format!("{}: {why}", path.display()));
    if path
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("nvtt_export.exe"))
    {
        return missing("NVIDIA's exporter app, not nvcompress (pick nvcompress.exe)");
    }
    let canon = |p: &Path| std::fs::canonicalize(p).ok();
    let me = std::env::current_exe().ok().and_then(|p| canon(&p));
    if me.is_some() && canon(path) == me {
        return missing("this is sdc itself, not nvcompress");
    }
    let tmp = match TempDir::new() {
        Ok(t) => t,
        Err(e) => return missing(&format!("temporary folder: {e}")),
    };
    let (out_path, err_path) = (tmp.path().join("probe.out"), tmp.path().join("probe.err"));
    let (out, err) = match (
        std::fs::File::create(&out_path),
        std::fs::File::create(&err_path),
    ) {
        (Ok(o), Ok(e)) => (o, e),
        (Err(e), _) | (_, Err(e)) => return missing(&format!("temporary file: {e}")),
    };
    let mut cmd = Command::new(path);
    cmd.stdin(Stdio::null()).stdout(out).stderr(err);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, CREATE_NO_WINDOW);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return missing(&e.to_string()),
    };
    let end = std::time::Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                kill_tree(&mut child);
                return missing(&format!(
                    "no answer within {} s, not nvcompress",
                    PROBE_TIMEOUT.as_secs()
                ));
            }
            Err(e) => {
                kill_tree(&mut child);
                return missing(&format!("waiting for it failed: {e}"));
            }
        }
    }
    let read =
        |p: &Path| String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).into_owned();
    let text = read(&out_path) + &read(&err_path);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NvttError {
    Cancelled,
    /// The process could not start, exited non-zero, or wrote no or a wrong-sized payload (DECISIONS Note NV8).
    Failed(String),
}

/// One folder per file under the system temp folder (spec §7), removed on drop: when the file finishes, fails or is
/// cancelled. A crash can leave it behind; it holds only intermediate level images.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Audit EP-7 (Note BC): a folder a crashed run left under a recycled pid is skipped, never reused.
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("sdc-nvtt-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(TempDir(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// NV7: one nvcompress process at a time in this process (BC7 Production is multi-threaded itself, Spike S7).
/// shortcut: fixed at 1, make it a setting if a many-core machine shows idle cores.
static NVTT_SLOT: Mutex<()> = Mutex::new(());

/// `D3DFMT_A16B16G16R16F`: the legacy FourCC for RGBA16F that nvcompress reads (Spike S7b).
const FOURCC_RGBA16F: u32 = 113;

/// NV5 (Spike S7b): nvcompress reads uncompressed DDS only with a legacy header. BC7 levels go as B8G8R8A8 with
/// BGRA masks, BC6H levels as RGBA16F with FourCC 113 (the engine's writer has no legacy RGBA16F form, so the DX10
/// header it writes is rewritten here; the texels are unchanged).
fn feed_bytes(input: &EncoderInput, w: u32, h: u32, format: Format) -> Result<Vec<u8>, String> {
    let (texel_format, header) = match format {
        Format::Bc7 => (Format::Rgba8, ResolvedHeader::Legacy),
        Format::Bc6hUf16 => (Format::Rgba16f, ResolvedHeader::Dx10),
        f => return Err(format!("NVTT does not encode {f:?}")),
    };
    let desc = DdsDesc {
        width: w,
        height: h,
        format: texel_format,
        srgb: false,
        header,
    };
    let b =
        dds_bytes(&desc, &[pack_uncompressed(input, texel_format)]).map_err(|e| e.to_string())?;
    if format == Format::Bc7 {
        return Ok(b);
    }
    let mut legacy = b[..128].to_vec();
    legacy[80..84].copy_from_slice(&DDPF_FOURCC.to_le_bytes());
    legacy[84..88].copy_from_slice(&FOURCC_RGBA16F.to_le_bytes());
    legacy.extend_from_slice(&b[148..]);
    Ok(legacy)
}

/// NV4: the flags for one level.
fn level_args(p: &Profile) -> Vec<&'static str> {
    let mut a = vec!["-silent", "-nomips", "-production"];
    if p.format == Format::Bc7 {
        a.push("-bc7");
        // `remove` is what Spike S7 measured (decoded alpha 255 on every level). Every other policy keeps alpha,
        // and nvcompress weights alpha 0 unless told the input has it.
        a.push(if p.alpha.policy == AlphaPolicy::Remove {
            "-noalpha"
        } else {
            "-alpha"
        });
    } else {
        a.push("-bc6");
    }
    // Byte-identical with and without CUDA for BC7 and BC6H Production; saves ~0.13 s per process (S7, S7b).
    a.push("-nocuda");
    a
}

/// The last `n` non-empty lines of `text`, joined with " | ".
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}

/// How long one `w`×`h` level may run before nvcompress counts as hung and is killed (audit #18, Note BC): 2 minutes
/// plus 1 s per 50 000 texels (an 8192² level: about 24 minutes), far above a working encode.
fn level_timeout(w: u32, h: u32) -> Duration {
    #[cfg(feature = "test-hooks")]
    if let Some(ms) = std::env::var("SDC_NVTT_LEVEL_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return Duration::from_millis(ms);
    }
    Duration::from_secs(120 + u64::from(w) * u64::from(h) / 50_000)
}

/// One level through nvcompress (DECISIONS Note NV4-NV8). `input` is the quantised level (`w` x `h` texels) and
/// `level` names its files in `tmp`; the result is the raw block payload, post-fixed for BC7. `cancel` is polled
/// while waiting for the slot and while the process runs; on cancel the process is killed.
#[allow(clippy::too_many_arguments)]
pub fn encode_level_nvtt(
    tool: &NvttTool,
    input: &EncoderInput,
    w: u32,
    h: u32,
    p: &Profile,
    tmp: &TempDir,
    level: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<u8>, NvttError> {
    let failed = |m: String| NvttError::Failed(m);
    let feed = tmp.path().join(format!("l{level}.in.dds"));
    let out = tmp.path().join(format!("l{level}.out.dds"));
    let log = tmp.path().join(format!("l{level}.log"));
    std::fs::write(&feed, feed_bytes(input, w, h, p.format).map_err(failed)?)
        .map_err(|e| failed(format!("{}: {e}", feed.display())))?;

    let mut wait = Duration::from_millis(5);
    let _slot = loop {
        if cancel() {
            return Err(NvttError::Cancelled);
        }
        match NVTT_SLOT.try_lock() {
            Ok(g) => break g,
            Err(TryLockError::Poisoned(e)) => break e.into_inner(),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    // A stale output must not pass for this run's when nvcompress exits 0 without writing (audit EP-7).
    let _ = std::fs::remove_file(&out);
    // stdout and stderr go to one file: an unread pipe could fill and stall the process.
    let logf =
        std::fs::File::create(&log).map_err(|e| failed(format!("{}: {e}", log.display())))?;
    let logf2 = logf.try_clone().map_err(|e| failed(e.to_string()))?;
    let mut cmd = Command::new(&tool.path);
    cmd.args(level_args(p))
        .arg(&feed)
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(logf)
        .stderr(logf2);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, CREATE_NO_WINDOW);
    let mut child = cmd
        .spawn()
        .map_err(|e| failed(format!("could not start {}: {e}", tool.path.display())))?;
    let limit = level_timeout(w, h);
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed(format!("waiting for nvcompress: {e}")));
            }
        }
        if cancel() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NvttError::Cancelled);
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failed(format!(
                "nvcompress did not finish a {w}x{h} level within {} s and was stopped",
                limit.as_secs_f32()
            )));
        }
        std::thread::sleep(wait);
        wait = (wait * 2).min(Duration::from_millis(50));
    };
    if !status.success() {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        return Err(failed(format!(
            "nvcompress exited with {status}: {}",
            tail(&text, 5)
        )));
    }
    let bytes = std::fs::read(&out).map_err(|e| failed(format!("no output: {e}")))?;
    let off = if bytes.len() >= 128 && &bytes[..4] == b"DDS " {
        if &bytes[84..88] == b"DX10" {
            148
        } else {
            128
        }
    } else {
        return Err(failed("output is not a DDS file".into()));
    };
    let payload = bytes.get(off..).unwrap_or_default();
    let want = w.div_ceil(4) as usize * h.div_ceil(4) as usize * 16;
    if payload.len() != want {
        return Err(failed(format!(
            "wrong payload size: {} bytes, expected {want} for {w}x{h}",
            payload.len()
        )));
    }
    let mut payload = payload.to_vec();
    if let (Format::Bc7, EncoderInput::Ldr(px)) = (p.format, input) {
        bc7_post_fix(
            &mut payload,
            &encoders_sys::gather_blocks(px, w, h),
            w,
            h,
            p,
        );
    }
    Ok(payload)
}
