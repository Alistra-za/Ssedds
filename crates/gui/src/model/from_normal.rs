//! v2 Track B (Note CO): "Generate from normal map…". The offers and their disabled reasons, the set's normal, the
//! offered sizes, the `[generated]` provenance records, and the dialog's state, preview and Generate jobs (Task 6).
use super::bulk::{MenuAction, MenuEntry};
use super::checks::{self, HeightNeutral};
use super::items::{ItemKind, TextureItem};
use super::sets::{SetView, SlotFill, SlotKind, SLOT_LOCKED};
use super::shell::STAT_TTL;
use super::AppModel;
use crate::pgp::settings::Settings;
use crate::worker::{or_send, spawn_guarded, WorkerMsg};
use engine::codes::Code;
use engine::config::{GreenConvention, RawMap};
use engine::dds::{may_replace, write_atomic, Kept};
use engine::discover::InputKind;
use engine::from_normal::{
    generate, png16, sizes, GenerateError, Generated, HeightOpts, Neutral, Opts, Tiling, AO_MAX,
    COST_WARN_ABOVE, HEIGHT_MAX, HEIGHT_MAX_RECIPE,
};
use engine::group::{
    is_generated, real_rival, split_generated, split_suffix_with, vocab_of, VocabRole,
};
use engine::policy::SourceStamp;
use engine::shader::Shader;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenMap {
    Height,
    Ao,
    Curvature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenOp {
    /// Open the dialog, with this map ticked.
    Open(Option<GenMap>),
    /// Delete the generated file with this item key (Task 7).
    Delete(String),
}

/// What a generated file was made from and with, recorded in `sdc-project.toml`'s `[generated]` table. The recorded
/// neutral is `opts.height.neutral` (D7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub from: String,
    pub from_sha256: String,
    pub sha256: String,
    pub kind: GenMap,
    pub m_alpha: bool,
    pub opts: Opts,
}

/// The Size control of one map: a free choice, or the one size its recipe needs (P6, OWNER-Q1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenSize {
    Free { sizes: Vec<u32>, default: u32 },
    Locked(u32),
}

/// Header sizes read for the offers, by path: (stat time, stamp, size). `AppModel::gen_dims`.
pub(crate) type GenDims = std::collections::HashMap<
    std::path::PathBuf,
    (Instant, Option<SourceStamp>, Option<(u32, u32)>),
>;

pub const NO_NORMAL: &str = "this set has no tangent-space normal map";
pub const MULTILAYER: &str =
    "vanilla multilayer has no height map: its layer alpha is a depth shift";
pub const VANILLA_LANDSCAPE: &str =
    "vanilla landscape keeps its height in the diffuse alpha; there is no _p slot";
pub const COMPLEX_CS: &str =
    "complex material reads height from the _m alpha, which is packed only on the vanilla target";
/// AO and curvature are offered from Tasks 10 and 12.
pub const NOT_OFFERED: &str = "not offered yet";
/// The maps the entries offer; Tasks 10 and 12 add AO and curvature.
const OFFERED: [GenMap; 1] = [GenMap::Height];

/// The height's neutral for a set, from Track C's one rule (P8). `resolved` is the set's settings through
/// `auto_flags::resolved`, so a `_cnr`'s auto multilayer counts (D7).
pub fn height_neutral(shader: Option<Shader>, resolved: &Settings) -> Option<Neutral> {
    match checks::height_neutral(shader, resolved) {
        HeightNeutral::Half => Some(Neutral::Half),
        HeightNeutral::White => Some(Neutral::White),
        HeightNeutral::NotHeight => None,
    }
}

/// The generated file's name next to the normal.
pub fn output_name(base: &str, map: GenMap, m_alpha: bool) -> String {
    let role = match map {
        GenMap::Height if m_alpha => "height",
        GenMap::Height => "p",
        GenMap::Ao => "ao",
        GenMap::Curvature => "curves",
    };
    format!("{base}_{role}_generated.png")
}

fn stem(key: &str) -> String {
    Path::new(key)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The generated role of a generated stem (`rock_p_generated` → `p`), lower-case.
fn gen_role(stem: &str) -> Option<String> {
    let (rest, generated) = split_generated(stem);
    generated.then(|| {
        rest.rsplit_once('_')
            .map_or("", |(_, r)| r)
            .to_ascii_lowercase()
    })
}

fn map_of(role: &str) -> Option<GenMap> {
    match role {
        "p" | "height" => Some(GenMap::Height),
        "ao" => Some(GenMap::Ao),
        "curves" => Some(GenMap::Curvature),
        _ => None,
    }
}

/// A member with a file of its own (not a packed output, which `SetView.members` also lists).
fn file_backed(i: &TextureItem) -> bool {
    matches!(i.kind, ItemKind::Image | ItemKind::Dds)
}

/// The rival named by the engine's GEN-REPLACED skip (P4): "generated … replaced by <name>[ (a DDS…)]".
fn replaced_by(i: &TextureItem) -> Option<String> {
    let w = i.warnings.iter().find(|w| w.code == Code::GenReplaced)?;
    let tail = w.message.split_once("replaced by ")?.1;
    Some(tail.split(" (a DDS").next().unwrap_or(tail).to_string())
}

fn size_locked((w, h): (u32, u32), what: &str, why: String) -> String {
    format!("this set's maps are {w}×{h}; a generated {what} must match them ({why})")
}

impl AppModel {
    /// The suffix aliases the last plan matched (each job's `suffix_alias`), so member suffixes read canonical as
    /// the engine's (Task 2: `real_rival` takes a canonical stem).
    /// An alias names only a §6.1 suffix (never a raw role), so every aliased image is a slot file with a job.
    /// shortcut: an alias used only by DDS inputs (no job) is not seen; read the plan's preset registry here if a
    /// DDS `_m` or normal under a user alias ever needs it.
    fn plan_aliases(&self) -> BTreeMap<String, String> {
        self.items
            .iter()
            .filter_map(|i| i.job.as_ref()?.suffix_alias.as_ref())
            .map(|a| (a.alias.clone(), a.canonical.clone()))
            .collect()
    }

    /// The set's file-backed members with their canonical suffix.
    fn real_members<'a>(
        &'a self,
        v: &'a SetView,
        aliases: &'a BTreeMap<String, String>,
    ) -> impl Iterator<Item = (&'a TextureItem, Option<String>)> + 'a {
        v.members
            .iter()
            .filter_map(|k| self.item(k))
            .filter(|i| file_backed(i))
            .map(|i| (i, split_suffix_with(&stem(&i.key), aliases).suffix))
    }

    /// The item key of the set's normal: the Normal slot's own member, else the first file-backed member whose
    /// canonical suffix is `n` (P16; `_msn` is not).
    pub fn set_normal(&self, set: &str) -> Option<String> {
        self.view_of(set)
            .and_then(|v| self.normal_of(&v, &self.plan_aliases()))
    }

    fn normal_of(&self, v: &SetView, aliases: &BTreeMap<String, String>) -> Option<String> {
        if let Some(SlotFill::Own { item }) = v.slots.get(&SlotKind::Normal) {
            return Some(item.clone());
        }
        let normal = self
            .real_members(v, aliases)
            .find(|(_, s)| s.as_deref() == Some("n"))
            .map(|(i, _)| i.key.clone());
        normal
    }

    /// An image's size from its analysis, else from its header, read once per (path, stamp) (spec-M5: offers are
    /// built on every menu draw).
    fn normal_size(&self, key: &str) -> Option<(u32, u32)> {
        let i = self.item(key)?;
        match &i.analysis {
            Some(a) => Some((a.width, a.height)),
            None => self.header_dims(&i.input),
        }
    }

    /// A header size by path, re-stated at most once per `STAT_TTL` and read again only when the stamp moved.
    fn header_dims(&self, path: &Path) -> Option<(u32, u32)> {
        let now = Instant::now();
        let mut cache = self.gen_dims.borrow_mut();
        let old = cache.get(path).copied();
        if let Some((at, _, dims)) = old {
            if now.saturating_duration_since(at) < STAT_TTL {
                return dims;
            }
        }
        let stamp = SourceStamp::of(path);
        let dims = match old {
            Some((_, s, dims)) if stamp.is_some() && s == stamp => dims,
            _ => stamp.and_then(|_| engine::load::image_size(path).ok()),
        };
        cache.insert(path.to_path_buf(), (now, stamp, dims));
        dims
    }

    /// Header sizes kept for the offers (tests).
    pub fn gen_dims_len(&self) -> usize {
        self.gen_dims.borrow().len()
    }

    /// Forget every header size (tests: the disk changed and the clock did not move).
    pub fn clear_gen_dims(&self) {
        self.gen_dims.borrow_mut().clear();
    }

    /// The recorded provenance of item `key` (keys match by Unicode lowercase).
    pub fn provenance(&self, key: &str) -> Option<Provenance> {
        let fold = key.to_lowercase();
        let all = self.sets_doc.as_ref()?.generated();
        let (_, v) = all.iter().find(|(k, _)| k.to_lowercase() == fold)?;
        serde_json::from_value(v.clone()).ok()
    }

    /// Generated items of the set, by set key (P5: a GEN-REPLACED loser is no member but keeps its set).
    fn generated_of(&self, v: &SetView) -> Vec<&TextureItem> {
        self.items
            .iter()
            .filter(|i| !v.key.is_empty() && i.set_key.as_deref() == Some(v.key.as_str()))
            .filter(|i| is_generated(&stem(&i.key)))
            .collect()
    }

    /// The set's generated files with their provenance, if recorded.
    pub fn generated_items(&self, set: &str) -> Vec<(String, Option<Provenance>)> {
        let Some(v) = self.view_of(set) else {
            return vec![];
        };
        self.generated_of(&v)
            .into_iter()
            .map(|i| (i.key.clone(), self.provenance(&i.key)))
            .collect()
    }

    /// Why `map` cannot be generated for `set`, the first reason in the brief's order, or `Ok`.
    pub fn gen_offer(&self, set: &str, map: GenMap) -> Result<(), String> {
        let v = self.view_of(set).ok_or_else(|| format!("no set {set}"))?;
        self.offer(&v, map)
    }

    fn offer(&self, v: &SetView, map: GenMap) -> Result<(), String> {
        if let Some(e) = self.sets_doc.as_ref().and_then(|d| d.error()) {
            return Err(format!("sdc-project.toml is read-only: {e}"));
        }
        if map != GenMap::Height {
            return Err(NOT_OFFERED.into());
        }
        if v.locks.contains(&SlotKind::Height.slot_number()) {
            return Err(SLOT_LOCKED.into());
        }
        let aliases = self.plan_aliases();
        let normal = self.normal_of(v, &aliases).ok_or(NO_NORMAL)?;
        let dims = self.normal_dims(&normal)?;
        if !dims.0.is_power_of_two() || !dims.1.is_power_of_two() {
            return Err(GenerateError::NotPowerOfTwo(dims.0, dims.1).to_string());
        }
        let shader = shader_of(v);
        match shader {
            Shader::Default | Shader::Parallax | Shader::Pbr | Shader::PbrLandscape => {}
            Shader::ComplexMaterial if self.target == engine::config::Target::Cs => {
                return Err(COMPLEX_CS.into())
            }
            Shader::ComplexMaterial => {
                let m = self
                    .real_members(v, &aliases)
                    .find(|(i, s)| s.as_deref() == Some("m") && !is_generated(&stem(&i.key)));
                if let Some((i, _)) = m {
                    return Err(format!(
                        "this set has a pre-packed _m ({}); its alpha already decides the height",
                        i.name
                    ));
                }
            }
            Shader::Multilayer => return Err(MULTILAYER.into()),
            Shader::Landscape => return Err(VANILLA_LANDSCAPE.into()),
            Shader::Glow | Shader::Envmap | Shader::Hair | Shader::Eye | Shader::Skin => {
                return Err(format!(
                    "a height map would switch this set from {} to Parallax",
                    shader.label()
                ))
            }
            Shader::AdvancedSkin | Shader::PbrGrass | Shader::ComplexGrass => {
                return Err(format!("{} reads no height map", shader.label()))
            }
        }
        let own = if shader == Shader::ComplexMaterial {
            "height"
        } else {
            "p"
        };
        let has_real = |name: &str| format!("this set already has a height map ({name})");
        let gens: Vec<(&TextureItem, String)> = self
            .generated_of(v)
            .into_iter()
            .filter_map(|i| Some((i, gen_role(&stem(&i.key))?)))
            .filter(|(_, r)| map_of(r) == Some(GenMap::Height))
            .collect();
        // The engine decided: a generated member it skipped GEN-REPLACED names its rival (P4).
        if let Some(r) = gens.iter().find_map(|(i, _)| replaced_by(i)) {
            return Err(has_real(&r));
        }
        // The other generated height kind writes the same `_p` on cs (spec-M6).
        if let Some((i, _)) = gens.iter().find(|(_, r)| r != own) {
            return Err(has_real(&i.name));
        }
        if gens.is_empty() {
            let gen_stem = format!("x_{own}_generated");
            let rival = self.real_members(v, &aliases).find(|(i, s)| {
                let canonical = s.as_ref().map_or_else(
                    || stem(&i.key),
                    |s| format!("{}_{s}", split_suffix_with(&stem(&i.key), &aliases).base),
                );
                let kind = match i.kind {
                    ItemKind::Dds => InputKind::Dds,
                    _ => InputKind::Image,
                };
                real_rival(&gen_stem, &canonical, kind, self.target)
            });
            if let Some((i, _)) = rival {
                return Err(has_real(&i.name));
            }
        }
        if let Some((i, _)) = gens.iter().find(|(i, _)| self.provenance(&i.key).is_none()) {
            return Err(format!(
                "{} exists but was not generated here; delete it to generate again",
                i.name
            ));
        }
        self.gen_size_of(v, map, dims, &aliases).map(|_| ())
    }

    fn normal_dims(&self, normal: &str) -> Result<(u32, u32), String> {
        self.normal_size(normal).ok_or_else(|| {
            let name = self.item(normal).map_or(normal, |i| i.name.as_str());
            format!("the normal map {name} cannot be read")
        })
    }

    /// The Size control of `map` (P6, OWNER-Q1). The dialog (Task 6) only renders it.
    pub fn gen_size(&self, set: &str, map: GenMap) -> Result<GenSize, String> {
        let v = self.view_of(set).ok_or_else(|| format!("no set {set}"))?;
        let aliases = self.plan_aliases();
        let normal = self.normal_of(&v, &aliases).ok_or(NO_NORMAL)?;
        let dims = self.normal_dims(&normal)?;
        self.gen_size_of(&v, map, dims, &aliases)
    }

    fn gen_size_of(
        &self,
        v: &SetView,
        map: GenMap,
        (nw, nh): (u32, u32),
        aliases: &BTreeMap<String, String>,
    ) -> Result<GenSize, String> {
        let free = |cap: u32, want: Option<u32>| {
            let s = sizes((nw, nh), cap);
            let default = want
                .filter(|w| s.contains(w))
                .unwrap_or_else(|| *s.last().unwrap_or(&nw.max(nh)));
            GenSize::Free { sizes: s, default }
        };
        if map != GenMap::Height {
            return Err(NOT_OFFERED.into());
        }
        match shader_of(v) {
            // OWNER-Q1: the `_m` alpha takes the size of the `_m` recipe's other sources, up to 2048.
            Shader::ComplexMaterial => match self.m_recipe_size(v, aliases) {
                None => Ok(free(HEIGHT_MAX_RECIPE, None)),
                Some((w, h)) => {
                    if nw < w
                        || nh < h
                        || u64::from(nw) * u64::from(h) != u64::from(nh) * u64::from(w)
                    {
                        Err(size_locked(
                            (w, h),
                            "height map",
                            format!("the normal is {nw}×{nh}"),
                        ))
                    } else if w.max(h) > HEIGHT_MAX_RECIPE {
                        Err(size_locked(
                            (w, h),
                            "height map",
                            format!("above the {HEIGHT_MAX_RECIPE} px limit"),
                        ))
                    } else {
                        Ok(GenSize::Locked(w.max(h)))
                    }
                }
            },
            // OWNER-Q1: a cs PBR `_p` may follow the albedo up to 2048 (Also's CM `_m` and landscape diffuse read it).
            Shader::Pbr | Shader::PbrLandscape => {
                let albedo = match v.slots.get(&SlotKind::Diffuse) {
                    Some(SlotFill::Own { item }) => self.normal_size(item).map(|(w, h)| w.max(h)),
                    _ => None,
                };
                let cap = albedo.map_or(HEIGHT_MAX, |a| a.clamp(HEIGHT_MAX, HEIGHT_MAX_RECIPE));
                Ok(free(cap, albedo))
            }
            _ => Ok(free(HEIGHT_MAX, None)),
        }
    }

    /// The size of the vanilla complex `_m` recipe's sources other than the height (P6), from the first readable one.
    fn m_recipe_size(&self, v: &SetView, aliases: &BTreeMap<String, String>) -> Option<(u32, u32)> {
        let pk = v
            .members
            .iter()
            .filter_map(|k| self.item(k))
            .filter(|i| i.kind == ItemKind::Packed)
            .find(|i| split_suffix_with(&stem(&i.key), aliases).suffix.as_deref() == Some("m"))?
            .job
            .as_ref()?
            .pack
            .as_ref()?;
        pk.sources
            .iter()
            .filter(|s| s.role != RawMap::Height)
            .find_map(|s| self.header_dims(&s.path))
    }

    /// The "Generate from normal map…" entry: a slot row's (`pre` = its map) or the set menu's (`None`, disabled
    /// only when every offered map is refused).
    pub fn gen_entry(&self, set: &str, pre: Option<GenMap>) -> MenuEntry {
        let maps = pre.map_or(OFFERED.to_vec(), |m| vec![m]);
        let reasons: Vec<Result<(), String>> =
            maps.iter().map(|&m| self.gen_offer(set, m)).collect();
        let disabled = match reasons.iter().any(Result::is_ok) {
            true => None,
            false => reasons.into_iter().find_map(Result::err),
        };
        let regen = self.generated_items(set).iter().any(|(k, p)| {
            p.is_some()
                && gen_role(&stem(k))
                    .and_then(|r| map_of(&r))
                    .is_some_and(|m| maps.contains(&m))
        });
        MenuEntry {
            action: MenuAction::Generated {
                set: set.to_string(),
                op: GenOp::Open(pre),
            },
            label: if regen {
                "Regenerate…"
            } else {
                "Generate from normal map…"
            }
            .into(),
            disabled,
        }
    }
}

/// The set's shader: the saved choice, else the engine's pick; with none yet, as `Default`.
fn shader_of(v: &SetView) -> Shader {
    v.shader_choice
        .or(v.shader.as_ref().map(|p| p.shader))
        .unwrap_or(Shader::Default)
}

// ---- Task 6: the dialog state, the preview and the Generate job ----

/// The Generate button's hover while nothing is ticked (spec-M2).
pub const NOTHING_TICKED: &str = "tick a map to generate";
/// The Generate button's hover while a preview job runs.
pub const PREVIEW_RUNNING: &str = "wait for the preview to finish";
/// The Generate button's hover while Generate runs.
pub const GENERATING: &str = "generating; wait for it to finish";
/// The `_p` choice on a complex-material set (D5).
pub const COMPLEX_USE_P: &str =
    "complex material reads height from the _m alpha; a _p would be unused";
/// Q11, D2: shown when the height is above `COST_WARN_ABOVE`.
pub const SIZE_WARNING: &str = "Above 512 px, parallax costs more GPU time for little visible gain";
/// The status line after Generate when `sdc-project.toml` held unsaved edits (P14).
pub const PROVENANCE_DEFERRED: &str = "provenance recorded; written with the next Save";
/// The preview's longer side at most (CO35).
pub const PREVIEW_MAX: u32 = 512;

/// The Generate dialog's state. It lives on `AppModel::gen`, not in `Dialog`, because `Dialog` is `Eq` and the
/// sliders are floats.
pub struct GenDialog {
    pub set: String,
    pub ticked: BTreeSet<GenMap>,
    pub opts: Opts,
    pub m_alpha: bool,
    pub preview: Option<Arc<GenResult>>,
    /// Per mille, written by the running job.
    pub progress: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
    /// The job in flight: its id and whether it is a preview.
    pub running: Option<(u64, bool)>,
    pub error: Option<String>,
    /// Set by the UI each frame from the sliders' `Response::dragged()`: no preview starts mid-drag.
    pub dragging: bool,
    /// Preview jobs started (tests).
    pub started: u32,
    next_id: u64,
    /// The opts of the last preview started.
    previewed: Option<Opts>,
    /// The running Generate's project root and normal key, taken when it started (its record goes there or nowhere).
    job: Option<(Option<PathBuf>, String)>,
}

/// What a job made: the maps, and for Generate the files it wrote with their sha256 and the sha256 of the normal it
/// read (hashed on the worker before the decode).
pub struct GenResult {
    pub maps: Generated,
    pub written: Vec<(GenMap, PathBuf, String)>,
    pub from_sha256: String,
}

/// A written file's item key: the normal's key with its file name swapped (they share a folder).
fn sibling_key(normal: &str, name: &str) -> String {
    match normal.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.to_string(),
    }
}

fn sha256(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(b))
}

/// `(w, h)` scaled so its longer side is `long`.
fn scaled((w, h): (u32, u32), long: u32) -> (u32, u32) {
    let big = w.max(h).max(1);
    let s = |v: u32| ((u64::from(v) * u64::from(long)) / u64::from(big)).max(1) as u32;
    (s(w), s(h))
}

fn cap_of(map: GenMap) -> u32 {
    match map {
        GenMap::Height => HEIGHT_MAX_RECIPE,
        GenMap::Ao | GenMap::Curvature => AO_MAX,
    }
}

/// Steps 3-5 of Generate (spec-S9): stop if cancelled (the last point where Cancel writes nothing); check every
/// target with the shared may-replace helper before writing any; write each with `write_atomic`. If a write fails,
/// undo this call's earlier writes: a file it created is removed, our own earlier file (Regenerate) gets its old
/// bytes back, and a file that already held the new bytes was never written (M4). `targets` are (path, bytes, the
/// recorded sha256 when the file is ours). Returns each target's sha256. Public as the test seam for the Cancel
/// window.
pub fn write_all(
    targets: &[(PathBuf, Vec<u8>, Option<String>)],
    cancel: &AtomicBool,
) -> Result<Vec<String>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let mut before = vec![];
    for (path, bytes, own) in targets {
        may_replace(path, bytes, |old| own.as_deref() == Some(sha256(old).as_str())).map_err(
            |k| match k {
                Kept::Foreign if own.is_some() => format!(
                    "{} was edited since it was generated, so it is not overwritten; delete or rename it first",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                Kept::Foreign => format!("{} already exists; nothing was written", path.display()),
                k => k.note(path),
            },
        )?;
        // What was there (absent, or bytes the helper let us replace), for the rollback.
        before.push(std::fs::read(path).ok());
    }
    // shortcut: a file created between the check above and the rename is replaced; upgrade with a hard-link
    // publish if that is ever seen.
    for (i, (path, bytes, _)) in targets.iter().enumerate() {
        if before[i].as_ref() == Some(bytes) {
            continue;
        }
        if let Err(e) = write_atomic(path, bytes) {
            let mut msg = format!("{}: {e}", path.display());
            for ((p, new, _), old) in targets[..i].iter().zip(&before) {
                let undo = match old {
                    Some(old) if old == new => continue,
                    Some(old) => write_atomic(p, old),
                    None => std::fs::remove_file(p),
                };
                if let Err(e) = undo {
                    msg.push_str(&format!("; {} could not be undone: {e}", p.display()));
                }
            }
            return Err(msg);
        }
    }
    Ok(targets.iter().map(|t| sha256(&t.1)).collect())
}

/// One job: decode the normal, generate, and for Generate encode and write the ticked maps.
fn run_job(
    normal: &Path,
    opts: &Opts,
    targets: Option<Vec<(GenMap, PathBuf, Option<String>)>>,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<GenResult, String> {
    // Hashed before the decode: a normal saved again meanwhile leaves the record stale, never falsely fresh (I2).
    let from_sha256 = match targets {
        Some(_) => crate::model::history::sha256_of(normal)
            .ok_or_else(|| format!("{} cannot be read", normal.display()))?,
        None => String::new(),
    };
    let (img, _) = engine::load::load_image(normal).map_err(|e| e.to_string())?;
    let maps = generate(&img, opts, progress, cancel).map_err(|e| e.to_string())?;
    let Some(targets) = targets else {
        return Ok(GenResult {
            maps,
            written: vec![],
            from_sha256,
        });
    };
    let mut files = vec![];
    for (map, path, own) in &targets {
        let g = match map {
            GenMap::Height => maps.height.as_ref(),
            GenMap::Ao => maps.ao.as_ref(),
            GenMap::Curvature => maps.curvature.as_ref(),
        };
        let bytes = g.map(png16).unwrap_or_default();
        if bytes.is_empty() {
            return Err(format!("no {map:?} map was made; nothing was written"));
        }
        files.push((path.clone(), bytes, own.clone()));
    }
    let shas = write_all(&files, cancel)?;
    let written = targets
        .into_iter()
        .zip(shas)
        .map(|((map, path, _), sha)| (map, path, sha))
        .collect();
    Ok(GenResult {
        maps,
        written,
        from_sha256,
    })
}

impl AppModel {
    /// Open the Generate dialog for `set`, with `pre` ticked. Regenerate (a recorded generated `pre` map) starts from
    /// its stored opts, with the neutral the set wants now and a size the normal still offers (spec-S3).
    pub fn open_gen(&mut self, set: &str, pre: Option<GenMap>) {
        if self
            .gen
            .as_ref()
            .is_some_and(|g| matches!(g.running, Some((_, false))))
        {
            // A cancelled Generate is still landing: its files get their provenance first.
            self.status_line = "Generate is still finishing; try again in a moment".into();
            return;
        }
        let Some(v) = self.view_of(set) else {
            return;
        };
        let aliases = self.plan_aliases();
        let normal = self.normal_of(&v, &aliases);
        let dims = normal.as_deref().and_then(|n| self.normal_size(n));
        let shader = shader_of(&v);
        let resolved = crate::model::auto_flags::resolved(v.pbr, &v.slots, &v.settings);
        let neutral = height_neutral(Some(shader), &resolved);
        let size_rule = self.gen_size_of_opt(&v, pre.unwrap_or(GenMap::Height), dims, &aliases);
        let stored = pre.and_then(|map| {
            self.generated_items(set).into_iter().find_map(|(k, p)| {
                (gen_role(&stem(&k)).and_then(|r| map_of(&r)) == Some(map)).then_some(p)?
            })
        });
        let (opts, m_alpha) = match stored {
            Some(p) => {
                let mut o = p.opts;
                if let (Some(h), Some(n)) = (o.height.as_mut(), neutral) {
                    h.neutral = n;
                }
                o.size = match size_rule {
                    Some(GenSize::Locked(s)) => s,
                    Some(GenSize::Free { sizes, .. }) => sizes
                        .iter()
                        .copied()
                        .filter(|&s| s <= o.size)
                        .max()
                        .or(sizes.first().copied())
                        .unwrap_or(o.size),
                    None => o.size,
                };
                (o, p.m_alpha)
            }
            None => {
                let only_ao = pre == Some(GenMap::Ao);
                let size = match size_rule {
                    Some(GenSize::Locked(s)) | Some(GenSize::Free { default: s, .. }) => s,
                    None => dims
                        .map(|d| {
                            *sizes(d, if only_ao { AO_MAX } else { HEIGHT_MAX })
                                .last()
                                .unwrap_or(&d.0)
                        })
                        .unwrap_or(0),
                };
                let opts = Opts {
                    size,
                    tiling: Tiling::Auto,
                    relief: 0.3,
                    green: normal.as_deref().and_then(|n| self.normal_green(n)),
                    height: Some(HeightOpts {
                        strength: 1.0,
                        neutral: neutral.unwrap_or(Neutral::Half),
                    }),
                    ao: None,
                    curvature: false,
                };
                (opts, shader == Shader::ComplexMaterial)
            }
        };
        self.gen = Some(GenDialog {
            set: set.to_string(),
            ticked: pre.into_iter().collect(),
            opts,
            m_alpha,
            preview: None,
            progress: Arc::default(),
            cancel: Arc::default(),
            running: None,
            error: None,
            dragging: false,
            started: 0,
            // Ids never repeat across dialogs, so a closed dialog's late preview is dropped (stale id).
            next_id: self.gen_ids,
            previewed: None,
            job: None,
        });
        self.dialog = Some(crate::model::Dialog::GenerateFromNormal(set.to_string()));
    }

    /// `gen_size_of` with the normal's size possibly unknown.
    fn gen_size_of_opt(
        &self,
        v: &SetView,
        map: GenMap,
        dims: Option<(u32, u32)>,
        aliases: &BTreeMap<String, String>,
    ) -> Option<GenSize> {
        self.gen_size_of(v, map, dims?, aliases).ok()
    }

    /// The green convention to solve with: an image normal's from its settings (`None` when it states none), a DDS
    /// normal's from its name (`_normalgl`, `_normaldx`; Note AW), else `None` (auto).
    fn normal_green(&self, normal: &str) -> Option<GreenConvention> {
        let i = self.item(normal)?;
        match &i.job {
            Some(j) if j.warnings.iter().any(|w| w.code == Code::InGreenUnstated) => None,
            Some(j) if j.profile.channels.flip_green => Some(GreenConvention::Opengl),
            Some(_) => Some(GreenConvention::Directx),
            None => match vocab_of(&stem(&i.key))?.role {
                VocabRole::Normal(c) => c,
                _ => None,
            },
        }
    }

    /// The dialog's normal: (item key, path).
    fn gen_normal(&self) -> Option<(String, PathBuf)> {
        let g = self.gen.as_ref()?;
        let key = self.set_normal(&g.set)?;
        let path = self.item(&key)?.input.clone();
        Some((key, path))
    }

    /// The ticked maps' output paths next to the normal, with their dimensions (D4).
    pub fn gen_paths(&self) -> Vec<(GenMap, PathBuf, (u32, u32))> {
        let (Some(g), Some((key, path))) = (self.gen.as_ref(), self.gen_normal()) else {
            return vec![];
        };
        let Some(dims) = self.normal_size(&key) else {
            return vec![];
        };
        let base = split_suffix_with(&stem(&key), &self.plan_aliases()).base;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        g.ticked
            .iter()
            .map(|&map| {
                let long = g.opts.size.min(cap_of(map)).min(dims.0.max(dims.1));
                (
                    map,
                    dir.join(output_name(&base, map, g.m_alpha)),
                    scaled(dims, long),
                )
            })
            .collect()
    }

    /// The disabled Generate button's hover: nothing ticked, or the job it waits for (M1).
    pub fn gen_generate_hover(&self) -> &'static str {
        match self.gen.as_ref().map(|g| (g.ticked.is_empty(), g.running)) {
            Some((false, Some((_, true)))) => PREVIEW_RUNNING,
            Some((false, Some((_, false)))) => GENERATING,
            _ => NOTHING_TICKED,
        }
    }

    /// The parallax cost warning while a `_p` height is ticked above `COST_WARN_ABOVE` (Q11, D2). Not for the `_m`
    /// alpha, whose size follows the `_m` at no extra parallax cost (OWNER-Q1).
    pub fn gen_size_warning(&self) -> Option<&'static str> {
        let g = self.gen.as_ref()?;
        (g.ticked.contains(&GenMap::Height) && !g.m_alpha && g.opts.size > COST_WARN_ABOVE)
            .then_some(SIZE_WARNING)
    }

    /// The read-only normal-format line (07 B3).
    pub fn gen_format_line(&self) -> String {
        let name = |c: GreenConvention| match c {
            GreenConvention::Opengl => "OpenGL",
            GreenConvention::Directx => "DirectX",
        };
        let Some(g) = self.gen.as_ref() else {
            return String::new();
        };
        if let Some(c) = g.opts.green {
            let has_job = self
                .gen_normal()
                .and_then(|(k, _)| self.item(&k).map(|i| i.job.is_some()))
                .unwrap_or(false);
            let from = if has_job {
                "from the normal's settings"
            } else {
                "from the file name"
            };
            return format!("Normal: {} ({from})", name(c));
        }
        match g.preview.as_ref().map(|p| &p.maps.report) {
            Some(r) if r.green_sure => format!("Normal: looks {} (detected)", name(r.green)),
            Some(_) => "Normal: green undetermined, assumed DirectX".into(),
            None => "Normal: green is detected with the preview".into(),
        }
    }

    /// Called each frame while the dialog shows: when nothing runs, the opts changed since the last preview and no
    /// slider is being dragged, start a preview job (at most `PREVIEW_MAX`, CO35).
    pub fn gen_frame(&mut self) {
        let normal = self.gen_normal();
        let long = normal
            .as_ref()
            .and_then(|(k, _)| self.normal_size(k))
            .map(|d| d.0.max(d.1));
        let tx = self.tx.clone();
        let Some(g) = self.gen.as_mut() else {
            return;
        };
        if g.running.is_some() || g.dragging || g.previewed.as_ref() == Some(&g.opts) {
            return;
        }
        g.previewed = Some(g.opts.clone());
        let Some((_, path)) = normal else {
            g.error = Some(NO_NORMAL.into());
            return;
        };
        let mut opts = g.opts.clone();
        opts.size = opts.size.min(PREVIEW_MAX).min(long.unwrap_or(PREVIEW_MAX));
        g.started += 1;
        self.spawn_gen(tx, path, opts, None);
    }

    /// Start a job on a worker; it lands as `WorkerMsg::Generated`.
    fn spawn_gen(
        &mut self,
        tx: std::sync::mpsc::Sender<WorkerMsg>,
        normal: PathBuf,
        opts: Opts,
        targets: Option<Vec<(GenMap, PathBuf, Option<String>)>>,
    ) {
        let Some(g) = self.gen.as_mut() else {
            return;
        };
        g.next_id += 1;
        self.gen_ids = g.next_id;
        let (id, preview) = (g.next_id, targets.is_none());
        g.running = Some((id, preview));
        g.progress.store(0, Ordering::Relaxed);
        let (progress, cancel) = (g.progress.clone(), g.cancel.clone());
        spawn_guarded(tx, "generate-from-normal", move |tx| {
            let result = or_send(
                tx,
                || run_job(&normal, &opts, targets, &progress, &cancel).map(Arc::new),
                |e| WorkerMsg::Generated {
                    id,
                    preview,
                    result: Err(format!("internal error: {e}")),
                },
            );
            let _ = tx.send(WorkerMsg::Generated {
                id,
                preview,
                result,
            });
        });
    }

    /// Generate: re-check each ticked map's offer, then write the maps on a worker. Nothing ticked: a no-op.
    pub fn gen_generate(&mut self) {
        let Some(g) = self.gen.as_ref() else {
            return;
        };
        if g.ticked.is_empty() || g.running.is_some() {
            return;
        }
        let (set, ticked) = (g.set.clone(), g.ticked.clone());
        let refused = ticked.iter().find_map(|&m| self.gen_offer(&set, m).err());
        let normal = self.gen_normal();
        // `ours` only for this set's recorded generated file (Regenerate, Task 7).
        let targets: Option<Vec<_>> = normal.as_ref().map(|(key, _)| {
            self.gen_paths()
                .into_iter()
                .map(|(map, p, _)| {
                    let name = p.file_name().unwrap_or_default().to_string_lossy();
                    let own = self.provenance(&sibling_key(key, &name)).map(|r| r.sha256);
                    (map, p, own)
                })
                .collect()
        });
        let tx = self.tx.clone();
        let g = self.gen.as_mut().expect("checked above");
        if let Some(e) = refused {
            g.error = Some(e);
            return;
        }
        let (Some((key, path)), Some(targets)) = (normal, targets) else {
            g.error = Some(NO_NORMAL.into());
            return;
        };
        let mut opts = g.opts.clone();
        if !ticked.contains(&GenMap::Height) {
            opts.height = None;
        }
        g.job = Some((self.root.clone(), key));
        g.error = None;
        g.cancel.store(false, Ordering::Relaxed);
        self.spawn_gen(tx, path, opts, Some(targets));
    }

    /// Cancel: stop the job and close. A Generate that already started writing still lands, to record its files.
    pub fn gen_cancel(&mut self) {
        if let Some(g) = self.gen.as_ref() {
            g.cancel.store(true, Ordering::Relaxed);
        }
        if !self.gen_writing() {
            self.gen = None;
        }
        if matches!(
            self.dialog,
            Some(crate::model::Dialog::GenerateFromNormal(_))
        ) {
            self.dialog = None;
        }
    }

    /// A Generate dialog job is in flight (owner answer 8: polled through `v2_jobs_pending`, never `busy`).
    pub fn gen_pending(&self) -> bool {
        self.gen.as_ref().is_some_and(|g| g.running.is_some())
    }

    pub(super) fn gen_landed(
        &mut self,
        id: u64,
        preview: bool,
        result: Result<Arc<GenResult>, String>,
    ) {
        let Some(g) = self.gen.as_mut() else {
            return;
        };
        if g.running != Some((id, preview)) {
            return;
        }
        g.running = None;
        let open = matches!(
            self.dialog,
            Some(crate::model::Dialog::GenerateFromNormal(_))
        );
        if preview {
            match result {
                Ok(r) => {
                    g.preview = Some(r);
                    g.error = None;
                }
                Err(e) => g.error = Some(e),
            }
            return;
        }
        let result = result.and_then(|r| match r.written.is_empty() {
            true => Err("nothing was written".to_string()),
            false => Ok(r),
        });
        let r = match result {
            Ok(r) => r,
            Err(e) if open => {
                g.error = Some(e);
                return;
            }
            Err(e) => {
                self.gen = None;
                // Cancelled with the dialog closed: a failure is still said, never dropped (M3).
                if e != "cancelled" {
                    self.gen_notice(format!("Generate from normal map failed: {e}"));
                }
                self.gen_after_job();
                return;
            }
        };
        let g = self.gen.take().expect("checked above");
        if open {
            self.dialog = None;
        }
        let (root, normal) = g.job.clone().unwrap_or_default();
        if root != self.root {
            // The project changed under the job: its record belongs to no open project (I1).
            let names: Vec<String> = r
                .written
                .iter()
                .map(|(_, p, _)| p.display().to_string())
                .collect();
            self.gen_notice(format!(
                "Generate wrote {}; its project was closed, so they are unrecorded",
                names.join(", ")
            ));
        } else {
            self.gen_record(&g, &normal, &r);
        }
        self.gen_after_job();
    }

    /// A Generate that writes is running: Open and Close wait for it (I1), as they wait for an export.
    pub(crate) fn gen_writing(&self) -> bool {
        self.gen
            .as_ref()
            .is_some_and(|g| matches!(g.running, Some((_, false))))
    }

    /// Close or open the folder once the Generate in its way has landed (`request_close` / `request_open` deferred
    /// them). An export or a held pass runs them itself when it ends.
    fn gen_after_job(&mut self) {
        if self.export.running.is_some() || self.half_size_waiting() {
            return;
        }
        match self.after_export.take() {
            Some(crate::model::AfterExport::Close) => self.close_ready = true,
            Some(crate::model::AfterExport::Open(root)) => {
                self.request_open(root);
            }
            None => {}
        }
    }

    /// Drop "provenance recorded; written with the next Save" once it is no longer true (a Save, another folder).
    pub(crate) fn drop_provenance_note(&mut self) {
        if self.status_note.as_deref() == Some(PROVENANCE_DEFERRED) {
            self.status_note = None;
        }
    }

    /// A warning on the status line and in Messages.
    fn gen_notice(&mut self, text: String) {
        self.status_line = text.clone();
        self.messages
            .notify(crate::model::messages::Severity::Warning, text);
    }

    /// Record the written files' provenance outside the undo history (the JSON hash records' rule, P14), add them
    /// to the set when its normal was moved in (the one undoable part), re-plan and select the first. The normal and
    /// its hash are the ones the job read (I1, I2).
    fn gen_record(&mut self, g: &GenDialog, normal: &str, r: &GenResult) {
        let (normal, written) = (normal.to_string(), &r.written);
        let from_sha256 = r.from_sha256.clone();
        let keys: Vec<String> = written
            .iter()
            .map(|(_, p, _)| {
                sibling_key(
                    &normal,
                    &p.file_name().unwrap_or_default().to_string_lossy(),
                )
            })
            .collect();
        let Some(d) = self.sets_doc.as_mut() else {
            return;
        };
        let was_dirty = d.is_dirty();
        for ((map, _, sha), key) in written.iter().zip(&keys) {
            let p = Provenance {
                from: normal.clone(),
                from_sha256: from_sha256.clone(),
                sha256: sha.clone(),
                kind: *map,
                m_alpha: g.m_alpha,
                opts: g.opts.clone(),
            };
            if let Ok(v) = serde_json::to_value(p) {
                d.set_generated(key, Some(&v));
            }
        }
        if self.save_record(was_dirty) {
            self.status_note = Some(PROVENANCE_DEFERRED.into());
        }
        let moved_in = self
            .sets_doc
            .as_ref()
            .and_then(|d| {
                let e = d.entries();
                let fold = normal.to_lowercase();
                e.iter()
                    .find(|e| engine::sets::same_id(&e.id, &g.set))
                    .map(|e| e.add.iter().any(|f| f.to_lowercase() == fold))
            })
            .unwrap_or(false);
        let before = self.sets_doc.as_ref().map(|d| d.text());
        if moved_in {
            let set = g.set.clone();
            let ks = keys.clone();
            self.edit_docs("Generate from normal map", None, true, move |_, s| {
                for k in &ks {
                    s.add_member(&set, k);
                }
            });
        }
        // `edit_docs` re-plans only when it changed something.
        if self.sets_doc.as_ref().map(|d| d.text()) == before {
            self.replan();
        }
        self.gen_select = keys.into_iter().next();
    }
}
