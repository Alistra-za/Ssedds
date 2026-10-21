//! Track A (DECISIONS Note CM): "Also export as" Vanilla and Complex Material. One export writes `<name> PBR` (the
//! plain cs export, byte for byte), `<name> Vanilla` and `<name> CM` side by side, as three engine runs in sequence.
use crate::api::{
    is_up_to_date, BatchRequest, BatchSummary, CancelToken, Engine, EngineError, LowDetailMode,
    OnExisting, Plan,
};
use crate::codes::{Code, Warning};
use crate::config::{GraySource, Profile, RawMap, Target};
use crate::dds::{may_replace, write_atomic, Kept};
use crate::derive::{vanilla_rel, DeriveMode, DeriveRequest, DeriveSet};
use crate::events::{
    new_batch_id, EngineEvent, EventKind, EventSink, PrefixSink, Reporter, SkipKind, Stage,
};
use crate::manifest::{output_key, EncoderVersions, Manifest, ManifestEntry, ManifestRecord};
use crate::pack::{rmaos_channel, ChannelSource, RecipeKind, Slot};
use crate::policy::{JobStatus, ResolvedJob, SourceStamp};
use crate::presets::{AlsoTarget, ExportConfig};
use crate::session::AnalysisCache;
use crate::shader::{assign, MemberMap, Shader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// One of the three runs of an Also export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Pbr,
    Vanilla,
    Cm,
}

/// What an Also export needs besides the batch request.
#[derive(Debug, Clone)]
pub struct AlsoSpec {
    pub cfg: ExportConfig,
    /// The folder the three output folders are created in (TA10).
    pub location: PathBuf,
    /// The mesh votes were read (TA13).
    pub scanned: bool,
    /// Set ids whose `_p` is 1.0-neutral (TA27), for the report. The GUI fills it; the CLI leaves it empty.
    // shortcut: the CLI has no resolved PGP flags, so it never marks an inner-layer `_p`; upgrade if the engine learns
    // them.
    pub inner_layer_sets: BTreeSet<String>,
}

/// A PBR set's fate in Vanilla and CM that the report lists (a skipped set, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetNote {
    pub id: String,
    pub why: String,
}

#[derive(Debug, Clone)]
pub struct AlsoSummary {
    /// The phases that ran, in order.
    pub phases: Vec<(Phase, BatchSummary)>,
    /// Vanilla outputs copied from the PBR folder this run (O4).
    pub copied: u32,
    pub sets: Vec<SetNote>,
    pub folders: [PathBuf; 3],
    /// Task 9.
    pub report: Option<PathBuf>,
    /// Task 8.
    pub zips: Vec<PathBuf>,
    pub notes: Vec<String>,
    /// A cancel stopped the export after this phase.
    pub cancelled_after: Option<Phase>,
    /// The phase (Vanilla or CM) an engine error stopped, its message and its exit code (`EngineError::exit_code`).
    pub error: Option<(Phase, String, i32)>,
    /// `zip` was on and a zip or the sums file was not written (`ALSO-ZIP-FAILED` or `ALSO-OWNED-UNREADABLE`).
    pub zip_failed: bool,
}

/// `b`'s counts added to `a`, `cancelled` concatenated.
fn add(a: &mut BatchSummary, b: &BatchSummary) {
    a.converted += b.converted;
    a.up_to_date += b.up_to_date;
    a.skipped += b.skipped;
    a.skipped_intentional += b.skipped_intentional;
    a.failed += b.failed;
    a.validation_failed += b.validation_failed;
    a.cancelled.extend(b.cancelled.iter().cloned());
}

impl AlsoSummary {
    /// Every phase's counts summed, `cancelled` concatenated.
    pub fn combined(&self) -> BatchSummary {
        let mut s = BatchSummary::default();
        for (_, p) in &self.phases {
            add(&mut s, p);
        }
        s
    }

    /// A later phase's engine error code (2 or 3) when there is one, as a plain export that errors exits with it;
    /// else `combined().exit_code()`, where an export a cancel stopped before its last phase is partial (1, which
    /// beats 4 as in spec §8) even when the phases that ran were clean. A zip not written is partial too.
    pub fn exit_code(&self) -> i32 {
        if let Some((_, _, code)) = &self.error {
            return *code;
        }
        match self.combined().exit_code() {
            0 | 4 if self.cancelled_after.is_some() || self.zip_failed => 1,
            c => c,
        }
    }
}

/// TA30: `also = ["cm"]` writes the Vanilla folder too.
pub const CM_NEEDS_VANILLA: &str =
    "CM installs on top of Vanilla: the Vanilla folder is written too";
/// TA30: no PBR set to derive.
pub const NO_PBR_SETS: &str = "no PBR sets: Vanilla holds copies only; no CM folder written";
/// C2: a set the user left out.
pub const SKIPPED_SET: &str = "skipped (Skip vanilla/CM for this set)";

/// A path as hash records and folder rules compare it: absolute, `/`-separated, lower-cased (fix/pre-v2's
/// `record_key`). `shortcut:` a junction or 8.3 name is not resolved (no `canonicalize`: the folders may not exist
/// yet); upgrade if a user reports it.
fn norm(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

/// Review Focus 2: discovery skips everything under the output root, so the export folder must not be the opened
/// folder or contain it. A subfolder of the opened folder is fine.
pub fn export_folder_problem(root: &Path, export: &Path) -> Option<String> {
    let (r, e) = (norm(root), norm(export));
    (r == e || r.starts_with(&format!("{e}/"))).then(|| {
        "The export folder must not be the opened folder or contain it: Tessalith would not find the sources next time. \
         Choose another folder."
            .to_string()
    })
}

/// TA19: the location is not an input folder, and no target folder equals or contains one. A location that contains
/// the opened folder (MO2 `mods\`) is fine.
pub fn check_location(
    inputs: &[PathBuf],
    location: &Path,
    folders: &[PathBuf; 3],
) -> Result<(), String> {
    for r in inputs {
        if norm(r) == norm(location) {
            return Err(
                "The Also export location must not be the opened folder: choose the folder that holds it (for MO2, \
                 the mods folder) or another folder."
                    .into(),
            );
        }
        for f in folders {
            if let Some(p) = export_folder_problem(r, f) {
                return Err(format!("{}: {p}", f.display()));
            }
        }
    }
    Ok(())
}

/// The three folders: the location joined with `folder_names(project)`.
pub fn folders(spec: &AlsoSpec, project: &str) -> [PathBuf; 3] {
    spec.cfg
        .folder_names(project)
        .map(|n| spec.location.join(n))
}

/// The project name: the first input folder's name (a file input: its folder's).
fn project_name(inputs: &[PathBuf]) -> Result<String, EngineError> {
    let first = inputs
        .first()
        .ok_or_else(|| EngineError::Usage("Also export needs an input folder".into()))?;
    let abs = std::path::absolute(first).unwrap_or_else(|_| first.clone());
    let dir = if abs.is_file() {
        abs.parent().map(Path::to_path_buf).unwrap_or(abs)
    } else {
        abs
    };
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| EngineError::Usage(format!("{} has no folder name", dir.display())))
}

/// The final folder names, compared pairwise ASCII/Unicode case-folded once the project name is known (closes the
/// CM31 shortcut): a custom name equal to another folder's default is refused, naming the custom key.
fn check_names(cfg: &ExportConfig, project: &str) -> Result<(), String> {
    let names = cfg.folder_names(project);
    let custom = [
        cfg.names.pbr.is_some(),
        cfg.names.vanilla.is_some(),
        cfg.names.cm.is_some(),
    ];
    const KEYS: [&str; 3] = ["pbr", "vanilla", "cm"];
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        if names[a].to_lowercase() == names[b].to_lowercase() {
            let keys: Vec<String> = [a, b]
                .into_iter()
                .filter(|&i| custom[i])
                .map(|i| format!("export.names.{}", KEYS[i]))
                .collect();
            return Err(format!(
                "{}: two of the three folders would both be named {:?}",
                keys.join(" and "),
                names[a]
            ));
        }
    }
    Ok(())
}

/// The PBR sets of the PBR phase's plan (TA11: picked `pbr`, `pbr_landscape` or `pbr_grass`) by the slot rule (Task 4
/// Step 3): albedo and normal by `assign` on the set's own shader spec; the scalars from a packed `_rmaos`, else the
/// `_rmaos` member by channel, else the defaults; the height from a packed `_p`, else the `_p` member's R. A set whose
/// id is in `skip` gives a `SetNote` and its members go in the second list (`DeriveRequest::drop`).
pub fn derive_sets(
    pbr: &Plan,
    skip: &BTreeSet<String>,
) -> (Vec<DeriveSet>, Vec<PathBuf>, Vec<SetNote>) {
    let root = &pbr.request.output_root;
    let skip: BTreeSet<String> = skip
        .iter()
        .map(|id| crate::sets::set_key(root, id))
        .collect();
    let (mut out, mut drop, mut notes) = (vec![], vec![], vec![]);
    for (key, pick) in &pbr.shaders {
        if !matches!(
            pick.shader,
            Shader::Pbr | Shader::PbrLandscape | Shader::PbrGrass
        ) {
            continue;
        }
        let jobs: Vec<&ResolvedJob> = pbr.jobs.iter().filter(|j| &j.set_key == key).collect();
        let mut members: Vec<PathBuf> = vec![];
        for j in &jobs {
            match &j.pack {
                Some(pk) => members.extend(pk.sources.iter().map(|s| s.path.clone())),
                None => members.push(j.input.clone()),
            }
        }
        members.extend(
            pbr.skipped
                .iter()
                .filter(|s| s.set.as_deref() == Some(key))
                .map(|s| s.path.clone()),
        );
        members.sort();
        members.dedup();
        let id = crate::sets::set_id(root, key);
        if skip.contains(key) {
            drop.extend(members);
            notes.push(SetNote {
                id,
                why: SKIPPED_SET.into(),
            });
            continue;
        }
        let plain: Vec<&&ResolvedJob> = jobs.iter().filter(|j| j.pack.is_none()).collect();
        let maps: Vec<MemberMap> = plain
            .iter()
            .map(|j| MemberMap {
                name: j
                    .output
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                suffix: None,
                subtype: Some(j.profile.subtype),
            })
            .collect();
        let roles = assign(pick.shader.spec_for(Target::Cs), &maps);
        let role = |label: &str| {
            plain
                .iter()
                .zip(&roles)
                .find(|(_, r)| r.is_some_and(|r| r.label == label))
                .map(|(j, _)| **j)
        };
        let albedo = role("albedo");
        let rmaos_member = role("_rmaos");
        let packed = |k: RecipeKind| {
            jobs.iter()
                .find_map(|j| j.pack.as_ref().filter(|p| p.kind == k))
        };
        let channel = |slot: Slot, default: f32| match (packed(RecipeKind::Rmaos), rmaos_member) {
            (Some(pk), _) => pk.channels[rmaos_channel(slot).unwrap_or(0) as usize].clone(),
            (None, Some(j)) => ChannelSource::Source {
                path: j.input.clone(),
                role: RawMap::Rmaos,
                invert: false,
                channel: rmaos_channel(slot),
            },
            (None, None) => ChannelSource::Default { value: default },
        };
        let rough_default = albedo.map_or(0.5, |a| a.profile.packing.roughness_default);
        let height = match (packed(RecipeKind::Parallax), role("_p")) {
            (Some(pk), _) => Some(pk.channels[0].clone()),
            (None, Some(j)) => Some(ChannelSource::Source {
                path: j.input.clone(),
                role: RawMap::Height,
                invert: false,
                channel: None,
            }),
            _ => None,
        };
        out.push(DeriveSet {
            id,
            shader: pick.shader,
            dds: members
                .iter()
                .filter(|m| m.extension().is_some_and(|e| e.eq_ignore_ascii_case("dds")))
                .cloned()
                .collect(),
            members,
            albedo: albedo.map(|j| j.input.clone()),
            normal: role("_n").map(|j| j.input.clone()),
            rough: channel(Slot::Roughness, rough_default as f32),
            metal: channel(Slot::Metallic, 0.0),
            ao: channel(Slot::Ao, 1.0),
            height,
            curves: None,
            cavity: None,
            gray: GraySource::R,
        });
    }
    (out, drop, notes)
}

/// O4: the Vanilla job writes what its cs twin wrote, so the PBR folder's file can be copied: both ready, same input,
/// the same key relative to their roots (the cs key folded by `vanilla_rel`, TA12; O6's landscape `_p`), the same
/// pack, RNM and bake, not derived, and the same profile but for the target.
pub fn same_identity(cs: &ResolvedJob, cs_root: &Path, van: &ResolvedJob, van_root: &Path) -> bool {
    let folded = vanilla_rel(Path::new(&output_key(cs_root, &cs.output)), &cs.game_path)
        .to_string_lossy()
        .replace('\\', "/");
    matches!(cs.status, JobStatus::Ready)
        && matches!(van.status, JobStatus::Ready)
        && cs.input == van.input
        && folded == output_key(van_root, &van.output)
        && cs.pack == van.pack
        && cs.rnm == van.rnm
        && cs.bake == van.bake
        && van.derive.is_none()
        && Profile {
            target: Target::Cs,
            ..van.profile.clone()
        } == cs.profile
}

/// One owned-file record (TA21).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Owned {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    /// Written before the rename, so a crash between the rename and the final save still owns the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<String>,
}

/// TA21: the files an Also export writes outside the manifests (the report, the zips, the sums file), as hash
/// records in `<PBR folder>/_sdc/also.json`, keyed by absolute, lower-cased, `/`-separated paths.
pub struct OwnedFiles {
    path: PathBuf,
    records: BTreeMap<String, Owned>,
}

fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// Why [`OwnedFiles::write`] wrote nothing (or, after the rename, left the new file recorded as pending).
#[derive(Debug)]
pub enum OwnedWriteError {
    /// fix/pre-v2's `may_replace` kept the existing file.
    Kept(Kept),
    /// Writing the record or the file failed.
    Io(std::io::Error),
}

impl OwnedWriteError {
    /// The note a writer shows for the file.
    pub fn note(&self, path: &Path) -> String {
        match self {
            OwnedWriteError::Kept(k) => k.note(path),
            OwnedWriteError::Io(e) => format!("{} could not be written ({e})", path.display()),
        }
    }
}

impl OwnedFiles {
    /// The record of `pbr_folder`; empty when there is none yet. A record that exists but cannot be read or parsed
    /// is an error, never read as empty: the caller leaves it as it is and writes no owned file (review M4).
    pub fn load(pbr_folder: &Path) -> std::io::Result<OwnedFiles> {
        let path = pbr_folder.join("_sdc").join("also.json");
        let records = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).map_err(std::io::Error::other)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e),
        };
        Ok(OwnedFiles { path, records })
    }

    /// Where the record lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write `bytes` to `path` unless fix/pre-v2's `may_replace` keeps the file there (its record or pending SHA
    /// must match). An I/O error is `OwnedWriteError::Io` (the file is unchanged or, after the rename, still
    /// recorded as pending).
    pub fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), OwnedWriteError> {
        let key = norm(path);
        let rec = self.records.get(&key).cloned().unwrap_or_default();
        may_replace(path, bytes, |old| {
            let h = Some(sha256_hex(old));
            rec.sha256 == h || rec.pending == h
        })
        .map_err(OwnedWriteError::Kept)?;
        self.records.entry(key.clone()).or_default().pending = Some(sha256_hex(bytes));
        self.save().map_err(OwnedWriteError::Io)?;
        write_atomic(path, bytes).map_err(OwnedWriteError::Io)?;
        let r = self.records.entry(key).or_default();
        r.sha256 = r.pending.take();
        // A failed save leaves the pending SHA on disk, which still matches the file: it stays ours.
        let _ = self.save();
        Ok(())
    }

    pub fn save(&self) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.records).map_err(std::io::Error::other)?;
        write_atomic(&self.path, &bytes)
    }
}

/// Drops batch-level warnings (no job key) of the Vanilla and CM phases: each re-plans the same inputs, and the PBR
/// phase already reported them (review M7).
struct NoBatchWarnings(Arc<dyn EventSink>);

impl EventSink for NoBatchWarnings {
    fn emit(&self, ev: &EngineEvent) {
        if !(ev.job.is_none() && matches!(ev.kind, EventKind::Warning(_))) {
            self.0.emit(ev);
        }
    }
}

/// P4: the job keys (relative to the phase's folder, lower-cased) a phase wrote or found up to date, copies included:
/// TA23's zip list. It is the first sink under the phase's `Reporter`, so the keys are not yet prefixed.
struct Recorder {
    inner: Arc<dyn EventSink>,
    keys: Arc<Mutex<BTreeSet<String>>>,
}

impl EventSink for Recorder {
    fn emit(&self, ev: &EngineEvent) {
        let done = matches!(
            ev.kind,
            EventKind::FileDone { .. }
                | EventKind::FileSkipped {
                    kind: SkipKind::UpToDate,
                    ..
                }
        );
        if let (true, Some(k)) = (done, &ev.job) {
            crate::api::lock(&self.keys).insert(k.clone());
        }
        self.inner.emit(ev);
    }
}

/// C8: why no zip was written.
pub const ZIP_SKIPPED: &str = "zip skipped: the export did not finish";
/// TA23: a file in a zipped folder that this run did not write or find up to date.
pub const NOT_ZIPPED: &str = "not zipped: not written by this export";

/// Every file under `folder` but `_sdc` and leftover `.sdc-*.tmp` files (08d-3).
fn folder_files(folder: &Path) -> std::io::Result<Vec<PathBuf>> {
    let (mut out, mut todo) = (vec![], vec![folder.to_path_buf()]);
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            if p.is_dir() {
                if !(d == folder && name.eq_ignore_ascii_case("_sdc")) {
                    todo.push(p);
                }
            } else if !(name.starts_with(".sdc-") && name.ends_with(".tmp")) {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// C8 (Note CM): each folder that ran becomes `<location>/<folder name>.zip` of this run's outputs (TA23), and
/// `<location>/<project> SHA256SUMS.txt` gets a line per zip written, all through `OwnedFiles` (TA21). Returns what
/// was not written, one message each (review M1).
fn write_zips(
    s: &mut AlsoSummary,
    owned: &mut OwnedFiles,
    keys: &[BTreeSet<String>; 3],
    location: &Path,
    project: &str,
) -> Vec<String> {
    let (mut lines, mut failed) = (String::new(), vec![]);
    let ran: Vec<usize> = s.phases.iter().map(|(p, _)| *p as usize).collect();
    for i in ran {
        let folder = &s.folders[i];
        let name = folder
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let zip = location.join(format!("{name}.zip"));
        let files = match folder_files(folder) {
            Ok(f) => f,
            Err(e) => {
                failed.push(format!("{} not written ({e})", zip.display()));
                continue;
            }
        };
        let (mut take, mut left) = (vec![], vec![]);
        for f in files {
            match keys[i].contains(&output_key(folder, &f)) {
                true => take.push(f),
                false => left.push(f),
            }
        }
        left.sort();
        for f in left {
            let rel = slash(f.strip_prefix(folder).unwrap_or(&f));
            s.notes.push(format!("{name}/{rel}: {NOT_ZIPPED}"));
        }
        let bytes = match crate::zip::write_zip(folder, &take) {
            Ok(b) => b,
            Err(e) => {
                failed.push(format!("{} not written: {e}", zip.display()));
                continue;
            }
        };
        match owned.write(&zip, &bytes) {
            Ok(()) => {
                lines.push_str(&format!("{}  {name}.zip\n", sha256_hex(&bytes)));
                s.zips.push(zip);
            }
            Err(e) => failed.push(e.note(&zip)),
        }
    }
    let sums = location.join(format!("{project} SHA256SUMS.txt"));
    if !lines.is_empty() {
        if let Err(e) = owned.write(&sums, lines.as_bytes()) {
            failed.push(e.note(&sums));
        }
    } else if sums.exists() {
        failed.push(format!(
            "{} not written: no zip was written, so it still lists an earlier export's zips",
            sums.display()
        ));
    }
    failed
}

/// Buffers a phase's planning events until the plan is filtered, then forwards the kept ones and everything after.
struct Gate {
    inner: Arc<dyn EventSink>,
    held: Mutex<Option<Vec<EngineEvent>>>,
}

impl Gate {
    fn release(&self, keep: impl Fn(&EngineEvent) -> bool) {
        let held = crate::api::lock(&self.held).take().unwrap_or_default();
        for ev in held.iter().filter(|e| keep(e)) {
            self.inner.emit(ev);
        }
    }
}

impl EventSink for Gate {
    fn emit(&self, ev: &EngineEvent) {
        match crate::api::lock(&self.held).as_mut() {
            Some(v) => v.push(ev.clone()),
            None => self.inner.emit(ev),
        }
    }
}

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// A Vanilla job taken out of the plan to be copied from the PBR folder: the twin's current record and file.
struct Copy {
    job: ResolvedJob,
    twin: ManifestEntry,
    twin_output: PathBuf,
}

/// The copy rule (O4, TA25): write each copy into the Vanilla folder and append its record, under `on_existing`
/// and fix/pre-v2's `may_replace`. Each copy ends with one terminal event; counts go into `sum`. The record is the
/// twin's (cs profile, hash and preset included; Note CM33, review L1).
// shortcut: copies write no `--sidecar`, and their up-to-date check does not look for one; upgrade by copying
// `<twin>.dds.json` when sidecars matter for Vanilla.
fn do_copies(
    copies: &[Copy],
    req: &BatchRequest,
    rep: &Reporter,
    cancel: &CancelToken,
    sum: &mut BatchSummary,
) -> u32 {
    let root = &req.output_root;
    let manifest = Manifest::at(root);
    let existing = manifest.read().unwrap_or_default();
    let mut copied = 0;
    for c in copies {
        let job = &c.job;
        let key = output_key(root, &job.output);
        let emit =
            |kind: EventKind| rep.emit_job(Some(&key), Some(&job.set_key), Some(&job.input), kind);
        if req.dry_run {
            copied += 1;
            continue;
        }
        if cancel.is_cancelled() {
            sum.cancelled.push(job.input.clone());
            continue;
        }
        let mine = existing.get(&key);
        let dest_stamp = SourceStamp::of(&job.output);
        let skipped = |sum: &mut BatchSummary, kind: SkipKind, code, reason: String| {
            emit(EventKind::FileSkipped { kind, code, reason });
            sum.skipped += 1;
        };
        if dest_stamp.is_some() {
            match req.on_existing {
                OnExisting::Skip => {
                    skipped(
                        sum,
                        SkipKind::OnExistingSkip,
                        None,
                        "output exists (--on-existing skip)".into(),
                    );
                    sum.skipped_intentional += 1;
                    continue;
                }
                OnExisting::IfChanged
                    if mine.is_some_and(|e| {
                        e.sources == c.twin.sources
                            && e.source_sha256 == c.twin.source_sha256
                            && e.copied_from.is_some()
                            && e.copied_from == SourceStamp::of(&c.twin_output)
                            && e.output_stamp == dest_stamp
                    }) =>
                {
                    sum.up_to_date += 1;
                    emit(EventKind::FileSkipped {
                        kind: SkipKind::UpToDate,
                        code: None,
                        reason: "up to date".into(),
                    });
                    continue;
                }
                _ => {}
            }
        }
        let fail = |sum: &mut BatchSummary, stage: Stage, error: String| {
            emit(EventKind::FileFailed { stage, error });
            sum.failed += 1;
        };
        let bytes = match std::fs::read(&c.twin_output) {
            Ok(b) => b,
            Err(e) => {
                fail(sum, Stage::Load, format!("{}: {e}", slash(&c.twin_output)));
                continue;
            }
        };
        if req.on_existing == OnExisting::IfChanged {
            let ours = |_: &[u8]| {
                mine.is_some_and(|e| e.output_stamp.is_some() && e.output_stamp == dest_stamp)
            };
            if let Err(k) = may_replace(&job.output, &bytes, ours) {
                emit(EventKind::Warning(Warning::new(
                    Code::OutUnknownExisting,
                    k.note(&job.output),
                )));
                skipped(
                    sum,
                    SkipKind::UnknownExisting,
                    Some(Code::OutUnknownExisting),
                    "existing output without a manifest record".into(),
                );
                continue;
            }
        }
        let mut rec: ManifestRecord = match serde_json::from_value(c.twin.raw.clone()) {
            Ok(r) => r,
            Err(e) => {
                fail(
                    sum,
                    Stage::Write,
                    format!("the PBR record cannot be read: {e}"),
                );
                continue;
            }
        };
        if let Err(e) = write_atomic(&job.output, &bytes) {
            fail(sum, Stage::Write, e.to_string());
            continue;
        }
        rec.output = key.clone();
        rec.ts = crate::manifest::now_rfc3339();
        rec.output_stamp = SourceStamp::of(&job.output);
        rec.copied_from = c.twin.output_stamp;
        if let Err(e) = manifest.append(&rec) {
            // As `run_one`: no output is left without its record.
            let _ = std::fs::remove_file(&job.output);
            fail(sum, Stage::Write, format!("manifest: {e}"));
            continue;
        }
        emit(EventKind::FileDone {
            output: slash(&job.output),
            validation: rec.validation,
        });
        copied += 1;
    }
    copied
}

/// One Also export (Note CM): check the location and the `[export]` table, then run the PBR, Vanilla and CM phases
/// in order. Errors before the PBR phase writes anything are returned. A cancel stops the remaining phases and is
/// recorded in `cancelled_after`; an engine error of the Vanilla or CM phase stops them too and is recorded in
/// `error` with its exit code, plus an `ALSO-PHASE-FAILED` event. With `zip` on, an export that finished (no cancel,
/// no phase error) and whose owned record is readable is zipped (Task 8); file-level failures don't block it.
pub fn run_also(
    engine: &Engine,
    cache: &AnalysisCache,
    base: BatchRequest,
    spec: &AlsoSpec,
    sink: Arc<dyn EventSink>,
    cancel: &CancelToken,
) -> Result<AlsoSummary, EngineError> {
    let keys: [Arc<Mutex<BTreeSet<String>>>; 3] = Default::default();
    let project = project_name(&base.inputs)?;
    let dry_run = base.dry_run;
    let recorder = |i: usize, inner: Arc<dyn EventSink>| -> Arc<dyn EventSink> {
        Arc::new(Recorder {
            inner,
            keys: keys[i].clone(),
        })
    };
    // The PBR phase's reporter; the later phases' batch-level warnings and the zip step's go out under its batch.
    let rep = Reporter::new(recorder(0, sink.clone()), new_batch_id());
    let mut s = run_phases(engine, cache, base, spec, sink, cancel, &rep, &recorder)?;
    if !spec.cfg.zip || dry_run {
        return Ok(s);
    }
    let warn = |code: Code, message: &str| {
        rep.emit(
            None,
            None,
            EventKind::Warning(Warning::new(code, message.to_string())),
        )
    };
    if cancel.is_cancelled() || s.error.is_some() {
        s.notes.push(ZIP_SKIPPED.into());
        return Ok(s);
    }
    let failed = match OwnedFiles::load(&s.folders[0]) {
        Ok(mut owned) => {
            let keys = keys.map(|k| std::mem::take(&mut *crate::api::lock(&k)));
            write_zips(&mut s, &mut owned, &keys, &spec.location, &project)
        }
        // CM43: a record found unreadable at the start was reported then; one that broke since is reported here.
        Err(_) if s.notes.iter().any(|n| n.contains(OWNED_UNREADABLE)) => vec![],
        Err(e) => {
            let m = format!(
                "{} cannot be read ({e}): {OWNED_UNREADABLE}",
                s.folders[0].join("_sdc").join("also.json").display()
            );
            warn(Code::AlsoOwnedUnreadable, &m);
            s.notes.push(m);
            vec![]
        }
    };
    for m in &failed {
        warn(Code::AlsoZipFailed, m);
    }
    // Review M1: a zip or the sums file not written (or no zip at all) is a partial export.
    s.zip_failed = !failed.is_empty() || s.zips.len() < s.phases.len();
    s.notes.extend(failed);
    Ok(s)
}

/// CM43: the tail of the note for an unreadable `also.json`.
const OWNED_UNREADABLE: &str =
    "it is left as it is, and no report, zip or sums file is written until it is fixed or removed";

/// `run_also` without the zips; `rep` is the PBR phase's reporter, `recorder` wraps each later phase's sink (P4).
#[allow(clippy::too_many_arguments)]
fn run_phases(
    engine: &Engine,
    cache: &AnalysisCache,
    base: BatchRequest,
    spec: &AlsoSpec,
    sink: Arc<dyn EventSink>,
    cancel: &CancelToken,
    rep: &Reporter,
    recorder: &dyn Fn(usize, Arc<dyn EventSink>) -> Arc<dyn EventSink>,
) -> Result<AlsoSummary, EngineError> {
    let cfg = &spec.cfg;
    let project = project_name(&base.inputs)?;
    let folders = folders(spec, &project);
    let names = cfg.folder_names(&project);
    check_location(&base.inputs, &spec.location, &folders).map_err(EngineError::Usage)?;
    cfg.check().map_err(EngineError::Config)?;
    check_names(cfg, &project).map_err(EngineError::Config)?;
    let want_cm = cfg.also.contains(&AlsoTarget::Cm);
    let want_vanilla = want_cm || cfg.also.contains(&AlsoTarget::Vanilla);

    // TA14: the same request (preset id included) with target vanilla; each preflighted before anything is written.
    let mut pbr_req = base;
    pbr_req.output_root = folders[0].clone();
    pbr_req.skip_root = Some(spec.location.clone());
    let phase_req = |i: usize, mode: DeriveMode, sets: &[DeriveSet], drop: &[PathBuf]| {
        let mut r = pbr_req.clone();
        r.target = Some(Target::Vanilla);
        r.output_root = folders[i].clone();
        // P10: the low-detail check runs once, in the PBR phase.
        r.low_detail = LowDetailMode::Off;
        r.derive = Some(DeriveRequest {
            mode,
            params: cfg.legacy,
            sets: sets.to_vec(),
            drop: drop.to_vec(),
            scanned: spec.scanned,
        });
        r
    };
    engine.preflight(&pbr_req)?;
    if want_vanilla {
        engine.preflight(&phase_req(1, DeriveMode::Vanilla, &[], &[]))?;
    }
    if want_cm {
        engine.preflight(&phase_req(2, DeriveMode::Cm, &[], &[]))?;
    }

    let mut s = AlsoSummary {
        phases: vec![],
        copied: 0,
        sets: vec![],
        folders: folders.clone(),
        report: None,
        zips: vec![],
        notes: vec![],
        cancelled_after: None,
        error: None,
        zip_failed: false,
    };

    // PBR phase: today's export into folder 0, unprefixed, so the GUI's PGPatcher-JSON rows match.
    let pbr_plan = engine.plan_cached(pbr_req.clone(), rep, cancel, cache)?;
    if pbr_plan.target != Target::Cs {
        return Err(EngineError::Usage(
            "Also export needs the cs mode: the PBR folder is the cs export".into(),
        ));
    }
    let skip: BTreeSet<String> = cfg.skip.iter().cloned().collect();
    let (sets, drop, set_notes) = derive_sets(&pbr_plan, &skip);
    s.sets = set_notes;
    let pbr_jobs = pbr_plan.jobs.clone();
    let nvtt = pbr_plan
        .nvtt
        .as_ref()
        .and_then(|p| p.tool())
        .map(|t| t.version.clone());
    let pbr_sum = engine.run(pbr_plan, rep, cancel)?;
    s.phases.push((Phase::Pbr, pbr_sum));
    // Batch-level events of the later phases go out unprefixed, under the PBR phase's batch.
    let warn = |code: Code, message: String| {
        rep.emit(None, None, EventKind::Warning(Warning::new(code, message)))
    };
    if !pbr_req.dry_run {
        // TA21: the owned-file record lives in the PBR folder (Tasks 8 and 9 write through it). It is created when
        // missing and never rewritten here; one that cannot be read is left as it is (review M4).
        let rec = folders[0].join("_sdc").join("also.json");
        let made = OwnedFiles::load(&folders[0]).and_then(|o| match o.path().exists() {
            true => Ok(()),
            false => o.save(),
        });
        if let Err(e) = made {
            let m = format!(
                "{} cannot be read or written ({e}): {OWNED_UNREADABLE}; the files it lists are not touched",
                rec.display()
            );
            warn(Code::AlsoOwnedUnreadable, m.clone());
            s.notes.push(m);
        }
    }
    if want_cm && !cfg.also.contains(&AlsoTarget::Vanilla) {
        s.notes.push(CM_NEEDS_VANILLA.into());
    }
    if !want_vanilla {
        return Ok(s);
    }
    if cancel.is_cancelled() {
        s.cancelled_after = Some(Phase::Pbr);
        return Ok(s);
    }

    // Vanilla phase: derived jobs for the PBR sets; every other job copied from its cs twin when that is current.
    let vrep = Reporter::new(
        recorder(
            1,
            Arc::new(NoBatchWarnings(Arc::new(PrefixSink::new(
                format!("{}/", names[1]),
                sink.clone(),
            )))),
        ),
        new_batch_id(),
    );
    // Review M1: a later phase's error keeps its phase and exit code, and is reported as an event.
    let failed = |s: &mut AlsoSummary, phase: Phase, e: EngineError| {
        warn(
            Code::AlsoPhaseFailed,
            format!("{phase:?} phase failed: {e}"),
        );
        s.error = Some((phase, e.to_string(), e.exit_code()));
    };
    let vreq = phase_req(1, DeriveMode::Vanilla, &sets, &drop);
    let mut vplan = match engine.plan(vreq.clone(), &vrep, cancel) {
        Ok(p) => p,
        Err(e) => {
            failed(&mut s, Phase::Vanilla, e);
            return Ok(s);
        }
    };
    let pbr_root = &folders[0];
    let pbr_records = Manifest::at(pbr_root).read().unwrap_or_default();
    let encoders = EncoderVersions::current();
    let mut by_input: BTreeMap<&Path, Vec<&ResolvedJob>> = BTreeMap::new();
    for j in &pbr_jobs {
        by_input.entry(j.input.as_path()).or_default().push(j);
    }
    let mut copies = vec![];
    let mut keep = vec![];
    for j in std::mem::take(&mut vplan.jobs) {
        // shortcut: a packed job is never copied (its raw sources' `packed_source` events belong to its encode);
        // upgrade if a pre-packed copy matters for speed.
        let copy = (j.pack.is_none())
            .then(|| by_input.get(j.input.as_path()))
            .flatten()
            .and_then(|ts| {
                ts.iter()
                    .find(|t| same_identity(t, pbr_root, &j, &folders[1]))
            })
            .and_then(|t| {
                let e = pbr_records.get(&output_key(pbr_root, &t.output))?;
                // TA25: only a current twin is copied, so a failed or stale PBR output is encoded instead.
                is_up_to_date(e, t, &encoders, vreq.bc1_model, nvtt.as_deref()).then(|| Copy {
                    job: j.clone(),
                    twin: e.clone(),
                    twin_output: t.output.clone(),
                })
            });
        match copy {
            Some(c) => copies.push(c),
            None => keep.push(j),
        }
    }
    vplan.jobs = keep;
    // Review M2: the copies first, so their events come before the phase's `batch_done` (whose counts still leave
    // them out; they are in this phase's summary).
    let mut vsum = BatchSummary::default();
    s.copied = do_copies(&copies, &vreq, &vrep, cancel, &mut vsum);
    let run = engine.run(vplan, &vrep, cancel);
    if let Ok(r) = &run {
        add(&mut vsum, r);
    }
    vsum.cancelled.sort();
    s.phases.push((Phase::Vanilla, vsum));
    if let Err(e) = run {
        failed(&mut s, Phase::Vanilla, e);
        return Ok(s);
    }
    if !want_cm {
        return Ok(s);
    }
    if sets.is_empty() {
        s.notes.push(NO_PBR_SETS.into());
        return Ok(s);
    }
    if cancel.is_cancelled() {
        s.cancelled_after = Some(Phase::Vanilla);
        return Ok(s);
    }

    // CM phase: only the derived items, so the Vanilla phase's non-PBR outcomes are not counted (or shown) twice.
    let gate = Arc::new(Gate {
        inner: Arc::new(NoBatchWarnings(Arc::new(PrefixSink::new(
            format!("{}/", names[2]),
            sink,
        )))),
        held: Mutex::new(Some(vec![])),
    });
    let crep = Reporter::new(recorder(2, gate.clone()), new_batch_id());
    let mut cplan = match engine.plan(phase_req(2, DeriveMode::Cm, &sets, &drop), &crep, cancel) {
        Ok(p) => p,
        Err(e) => {
            gate.release(|_| true);
            failed(&mut s, Phase::Cm, e);
            return Ok(s);
        }
    };
    let members: BTreeSet<&Path> = sets
        .iter()
        .flat_map(|d| &d.members)
        .map(PathBuf::as_path)
        .collect();
    cplan.jobs.retain(|j| j.derive.is_some());
    cplan.skipped.retain(|f| members.contains(f.path.as_path()));
    cplan.failed.retain(|f| members.contains(f.path.as_path()));
    cplan.cancelled.retain(|p| members.contains(p.as_path()));
    let paths: BTreeSet<String> = members
        .iter()
        .map(|p| slash(p))
        .chain(cplan.jobs.iter().map(|j| slash(&j.input)))
        .collect();
    let set_keys: BTreeSet<&str> = cplan
        .jobs
        .iter()
        .map(|j| j.set_key.as_str())
        .chain(cplan.skipped.iter().filter_map(|f| f.set.as_deref()))
        .collect();
    gate.release(|e| match (&e.path, &e.set) {
        (Some(p), _) => paths.contains(p),
        (None, Some(k)) => set_keys.contains(k.as_str()),
        (None, None) => true,
    });
    match engine.run(cplan, &crep, cancel) {
        Ok(c) => s.phases.push((Phase::Cm, c)),
        Err(e) => failed(&mut s, Phase::Cm, e),
    }
    Ok(s)
}
