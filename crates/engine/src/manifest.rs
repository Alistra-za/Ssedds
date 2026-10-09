//! Batch manifest (spec §3.6): one JSON Lines file per output root, last record per output wins.
use crate::config::Profile;
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

/// The vendored bc7e patch as built into `encoders-sys`; hashed for the manifest (it is the only change to a
/// vendored encoder). `vendor/**` is `-text` in `.gitattributes`, so the bytes do not depend on the checkout.
const BC7E_PATCH: &[u8] =
    include_bytes!("../../encoders-sys/vendor/bc7e/patches/0001-exact-opaque-alpha.patch");

/// Short git commit of the source tree at build time, or `unknown` (no git, or not a checkout; see `build.rs`).
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
        if is_x86_feature_detected!("sse4.1") {
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
    pub output: String,
    pub validation: ValidationSummary,
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

    /// The latest record per `output`. Lines that are not a valid record (torn, foreign, not UTF-8) are skipped.
    pub fn load(&self) -> io::Result<BTreeMap<String, ManifestRecord>> {
        let mut map = BTreeMap::new();
        let f = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(map),
            Err(e) => return Err(e),
        };
        for line in io::BufReader::new(f).split(b'\n') {
            if let Ok(r) = serde_json::from_slice::<ManifestRecord>(&line?) {
                map.insert(r.output.clone(), r);
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
pub fn sha256_json<T: Serialize>(v: &T) -> String {
    hex::encode(Sha256::digest(serde_json::to_vec(v).unwrap_or_default()))
}
