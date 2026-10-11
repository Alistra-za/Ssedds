//! Batch manifest (spec §3.6): one JSON Lines file per output root, last record per output wins.
use crate::config::{NormalMips, Profile, RawMap, Subtype};
use crate::presets::{PresetFile, PresetRegistry, Provenance};
use crate::validate::ValidationSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub use crate::events::now_rfc3339;

pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The `schema` written into every [`ManifestRecord`]. `load` keeps records of a newer schema as raw entries
/// (see [`ManifestEntry::is_newer_schema`]) instead of interpreting them.
pub const MANIFEST_SCHEMA: u32 = 2;

/// The vendored bc7e patch as built into `encoders-sys`; hashed for the manifest (it is the only change to a
/// vendored encoder). `vendor/**` is `-text` in `.gitattributes`, so the bytes do not depend on the checkout.
const BC7E_PATCH: &[u8] =
    include_bytes!("../../encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch");

/// Git commit of the source tree at build time (12 hex digits, plus `-dirty` when tracked files had uncommitted
/// changes when the build script last ran), or `unknown` (no git, or not a checkout; see `build.rs`).
pub fn engine_commit() -> &'static str {
    env!("SDC_GIT_COMMIT")
}

/// The instruction set the ISPC dispatcher runs bc7e and the texture compressor at on this CPU. The kernels are
/// built for sse2, sse4, avx and avx2 and `encoders-sys` does not expose the choice, so it is derived from the
/// CPU features each ISPC target requires; the AVX2 target also needs FMA and F16C. This mirrors the ISPC
/// runtime's own CPUID check and may differ from it in rare cases.
pub fn dispatched_isa() -> &'static str {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx2")
            && is_x86_feature_detected!("fma")
            && is_x86_feature_detected!("f16c")
        {
            return "avx2";
        }
        if is_x86_feature_detected!("avx") {
            return "avx";
        }
        // ISPC's sse4 target needs SSE4.2 (review m9).
        if is_x86_feature_detected!("sse4.2") {
            return "sse4";
        }
    }
    "sse2"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EncoderVersions {
    pub bc7enc_rdo: String,
    pub ispctc: String,
    pub ispc: String,
    /// `<patch name>@sha256:<hash of the patch file>`; `None` when the vendored bc7e is unpatched.
    pub bc7e_patch: Option<String>,
}

impl EncoderVersions {
    pub fn current() -> Self {
        let v = encoders_sys::versions();
        EncoderVersions {
            bc7enc_rdo: v.bc7enc_rdo.into(),
            ispctc: v.ispctc.into(),
            ispc: v.ispc.into(),
            bc7e_patch: v
                .bc7e_patch
                .map(|name| format!("{name}@sha256:{}", hex::encode(Sha256::digest(BC7E_PATCH)))),
        }
    }
}

/// The preset that resolved a file. For a file matched by a suffix alias this is the alias preset (the one
/// `policy::preset_for` returns), with `alias_chain` holding its full inheritance chain, root base first, so a
/// reader can tell which presets contributed. Empty for a file resolved by the batch preset's own chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PresetRef {
    pub id: String,
    pub version: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_chain: Vec<String>,
}

impl PresetRef {
    /// `id`'s version and hash from `registry`. Pass the `PresetRegistry::alias_chain` chain for an aliased file.
    /// `None` when `id` is not in the registry.
    pub fn for_preset(
        registry: &PresetRegistry,
        id: &str,
        alias_chain: Option<&[&PresetFile]>,
    ) -> Option<Self> {
        Some(PresetRef {
            id: id.to_string(),
            version: registry.get(id)?.version.clone(),
            sha256: registry.sha256(id)?,
            alias_chain: alias_chain
                .map(|c| c.iter().map(|p| p.id.clone()).collect())
                .unwrap_or_default(),
        })
    }
}

/// One source file of a record: its path and SHA-256. A recipe built from several sources (`_rmaos`, `_p`) lists
/// them all so `if-changed` can re-encode when any of them changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRef {
    pub path: String,
    pub sha256: String,
    /// The raw-map role this source fills in a packed output (DECISIONS N7). Absent in schema-1 records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<RawMap>,
    /// The source is stored inverted (a gloss map in a roughness channel).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    /// The `_n` a roughness member baked its variance from (DECISIONS R12): the SHA-256 of its decode settings and
    /// level shift (`BakeJob::identity_sha256`). Absent on every other source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bake: Option<String>,
}

impl SourceRef {
    /// `path` ('/'-separated, as the record's `source`) and its SHA-256; no role.
    pub fn of(path: &Path) -> io::Result<SourceRef> {
        Ok(SourceRef {
            path: path.to_string_lossy().replace('\\', "/"),
            sha256: sha256_file(path)?,
            role: None,
            invert: false,
            bake: None,
        })
    }

    /// A packed output's source with the role it fills (DECISIONS N7).
    pub fn of_pack(path: &Path, role: RawMap, invert: bool) -> io::Result<SourceRef> {
        Ok(SourceRef {
            role: Some(role),
            invert,
            ..SourceRef::of(path)?
        })
    }

    /// The `_n` a roughness member bakes from, with its bake identity (DECISIONS R12).
    pub fn of_bake(path: &Path, identity: String) -> io::Result<SourceRef> {
        Ok(SourceRef {
            bake: Some(identity),
            ..SourceRef::of(path)?
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ManifestRecord {
    pub schema: u32,
    /// RFC 3339, UTC.
    pub ts: String,
    pub engine_version: String,
    pub engine_commit: String,
    pub encoders: EncoderVersions,
    pub isa: String,
    pub preset: PresetRef,
    pub profile: Profile,
    pub profile_sha256: String,
    pub provenance: BTreeMap<String, Provenance>,
    pub source: String,
    pub source_sha256: String,
    /// Every source of a multi-source recipe, then the `_n` a roughness member baked from (DECISIONS R12); empty for a
    /// single-source output that does not bake (`source` is then the only one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRef>,
    /// The canonical key from [`output_key`].
    pub output: String,
    pub validation: ValidationSummary,
    /// The batch BC1 decoder model ([`bc1_model_id`]) that rgbcx encoded the BC1 colour blocks against (BC1 and
    /// BC3) and that validation decoded BC1 with. `None` in records written before the field existed.
    #[serde(default)]
    pub bc1_model: Option<String>,
}

/// The stable id of a BC1 decoder model, as `--bc1-decoder` spells it (spec §8).
pub fn bc1_model_id(m: encoders_sys::Bc1Model) -> &'static str {
    use encoders_sys::Bc1Model::*;
    match m {
        Ideal => "ideal",
        IdealRound4 => "ideal-round4",
        Nvidia => "nvidia",
        Amd => "amd",
    }
}

/// What the up-to-date check needs from a stored record, read tolerantly: a field added to or removed from
/// `Profile`, `ValidationSummary` or the other embedded types never makes an older record unreadable.
/// `raw` is the whole line as JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestEntry {
    pub schema: u32,
    /// The canonical key (re-normalised on load).
    pub output: String,
    pub source_sha256: String,
    pub sources: Vec<SourceRef>,
    pub profile_sha256: String,
    pub engine_version: String,
    pub encoders: Option<EncoderVersions>,
    pub preset: Option<PresetRef>,
    pub bc1_model: Option<String>,
    pub raw: serde_json::Value,
}

impl ManifestEntry {
    /// Written by a newer engine: only `raw` and `output` are dependable, so a caller should treat the record as
    /// not comparable (and warn) rather than as up to date.
    pub fn is_newer_schema(&self) -> bool {
        self.schema > MANIFEST_SCHEMA
    }

    /// `None` when the line is not a manifest record (no string `output`, no integer `schema`).
    fn from_value(raw: serde_json::Value) -> Option<Self> {
        let get = |k: &str| raw.get(k);
        let text = |k: &str| {
            get(k)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        fn lenient<T: serde::de::DeserializeOwned>(raw: &serde_json::Value, k: &str) -> Option<T> {
            serde_json::from_value(raw.get(k)?.clone()).ok()
        }
        let schema = get("schema")?.as_u64()?.try_into().unwrap_or(u32::MAX);
        let output = normalize_key(get("output")?.as_str()?);
        Some(ManifestEntry {
            schema,
            output,
            source_sha256: text("source_sha256"),
            sources: lenient(&raw, "sources").unwrap_or_default(),
            profile_sha256: text("profile_sha256"),
            engine_version: text("engine_version"),
            encoders: lenient(&raw, "encoders"),
            preset: lenient(&raw, "preset"),
            bc1_model: lenient(&raw, "bc1_model"),
            raw,
        })
    }
}

/// Folds a root-relative path into the manifest key form: `/` separators, no empty or `.` segments (so no leading
/// `./`), and Unicode lower case, because Windows paths are case-insensitive.
fn normalize_key(rel: &str) -> String {
    rel.replace('\\', "/")
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect::<Vec<_>>()
        .join("/")
        .to_lowercase()
}

/// The canonical manifest key for `output` under `output_root`: root-relative, `/` separators, no leading `./`,
/// case-folded. `ManifestRecord::output` stores it and `Manifest::load` is keyed by it, so the up-to-date check
/// (Task 37) must look records up with this function and nothing else. An `output` that is not under
/// `output_root` keeps its whole (normalised) path.
pub fn output_key(output_root: &Path, output: &Path) -> String {
    let root = normalize_key(&output_root.to_string_lossy());
    let out = normalize_key(&output.to_string_lossy());
    if root.is_empty() {
        return out;
    }
    match out.strip_prefix(&root) {
        Some(rest) if rest.starts_with('/') => rest[1..].to_string(),
        _ => out,
    }
}

/// Serialises appends from every `Manifest` in the process, so two values on one root cannot interleave either.
static APPEND_LOCK: Mutex<()> = Mutex::new(());

pub struct Manifest {
    path: PathBuf,
}

impl Manifest {
    pub fn at(output_root: &Path) -> Self {
        Manifest {
            path: output_root.join("_sdc").join("manifest.jsonl"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line. Safe to call from several worker threads: the whole line goes out in one write under a
    /// process-wide lock. If an earlier crashed writer left the file without a final newline, a newline is written
    /// first so the torn line cannot swallow this record.
    pub fn append(&self, r: &ManifestRecord) -> io::Result<()> {
        let mut line = serde_json::to_vec(r).map_err(io::Error::other)?;
        line.push(b'\n');
        let _g = APPEND_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&self.path)?;
        if f.metadata()?.len() > 0 {
            let mut last = [0u8; 1];
            f.seek(SeekFrom::End(-1))?;
            f.read_exact(&mut last)?;
            if last[0] != b'\n' {
                line.insert(0, b'\n');
            }
        }
        f.write_all(&line)
    }

    /// The latest entry per canonical output key. Lines that are not a manifest record (torn, foreign, not UTF-8)
    /// are skipped; records of a newer schema are kept (see [`ManifestEntry::is_newer_schema`]).
    pub fn load(&self) -> io::Result<BTreeMap<String, ManifestEntry>> {
        let mut map = BTreeMap::new();
        let f = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(map),
            Err(e) => return Err(e),
        };
        for line in io::BufReader::new(f).split(b'\n') {
            let Ok(raw) = serde_json::from_slice::<serde_json::Value>(&line?) else {
                continue;
            };
            if let Some(e) = ManifestEntry::from_value(raw) {
                map.insert(e.output.clone(), e);
            }
        }
        Ok(map)
    }
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

/// SHA-256 of the value's compact JSON. Struct fields serialise in declaration order and maps are ordered, so the
/// hash is stable for equal values.
pub fn sha256_json<T: Serialize>(v: &T) -> Result<String, serde_json::Error> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(v)?)))
}

/// The manifest's `profile_sha256` for an output written with `p` (spec §8 `if-changed`). A `pbr_normal`'s
/// `mips.normal = renormalize_bake` hashes as `renormalize`: whether its set bakes changes none of the `_n`'s own
/// bytes (only its roughness member's), so the bake turning on or off does not re-encode the `_n` (DECISIONS R12,
/// M4 final review Minor 2). A `coat_normal`'s self-bake does change its bytes and keeps its own hash.
pub fn profile_sha256(p: &Profile) -> Result<String, serde_json::Error> {
    if p.subtype == Subtype::PbrNormal && p.mips.normal == NormalMips::RenormalizeBake {
        let mut q = p.clone();
        q.mips.normal = NormalMips::Renormalize;
        return sha256_json(&q);
    }
    sha256_json(p)
}
