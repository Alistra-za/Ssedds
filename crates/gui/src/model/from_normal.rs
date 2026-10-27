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
/// The maps the entries offer. Curvature is always offered: Track A's `curves` role exists (P10).
const OFFERED: [GenMap; 3] = [GenMap::Height, GenMap::Ao, GenMap::Curvature];
/// The Curvature box's hover: Track A's soft-light step is its only reader (a plain run reports PACK-UNUSED-INPUT).
pub const CURVATURE_HOVER: &str = "used by Also export as: Vanilla / Complex material";
/// Also export derives only cs PBR, PBR landscape and PBR grass sets (`also::derive_sets`), the curvature's one reader.
pub const NOT_PBR_CURVES: &str =
    "curvature is read only by Also export, which reads PBR sets under Community Shaders";
/// Also writes no diffuse for a set with no albedo, and the curvature only shades that diffuse.
pub const NO_ALBEDO: &str =
    "this set has no albedo; curvature only shades the diffuse Also export writes from it";
/// AO only for cs PBR sets: a loose `_ao` is PBR evidence (`suggest::is_pbr_set`), so it never reaches another set.
pub const NOT_PBR: &str =
    "AO is generated only for PBR sets under Community Shaders (a loose _ao would start an _rmaos)";
/// Shown at the top of the dialog for a set whose normal has an RNM bent layer, ticked or not (Q20, spec-M8).
pub const BENT_NOTE: &str =
    "This set has an RNM bent layer: a generated AO also changes its normal map, which is weighted by AO.";

fn dds_set(name: &str) -> String {
    format!("Also export skips a set with a DDS member ({name}), so nothing would read a curvature")
}

fn dds_rmaos(name: &str) -> String {
    format!("this set's _rmaos is a DDS ({name}), which is never re-encoded")
}

/// The dialog path's alias map, built once per plan, and its lookup counters (Task 6 review M6; tests read them
/// through `AppModel::gen_lookups`).
#[derive(Default)]
pub struct GenLookups {
    aliases: Option<(u64, Arc<BTreeMap<String, String>>)>,
    views: u32,
    alias_builds: u32,
}

/// What the dialog draws in one frame, from one set lookup (Task 6 review M6).
pub struct GenFacts {
    /// Each map the dialog lists, with its disabled reason.
    pub offers: Vec<(GenMap, Result<(), String>)>,
    /// The Size row: the free choice with the largest cap among the ticked maps, else a locked one.
    pub size: Result<GenSize, String>,
    /// Ticked maps whose size is fixed by their recipe, shown read-only.
    pub locked: Vec<(GenMap, u32)>,
    pub paths: Vec<(GenMap, PathBuf, (u32, u32))>,
    pub format_line: String,
    pub warning: Option<&'static str>,
    pub bent: bool,
    /// The normal's path and longer side, for the preview.
    pub normal: Option<(PathBuf, Option<u32>)>,
    /// The Size the free choice writes at (the dialog's Size within its offered sizes), for the combo's text.
    pub shown_size: u32,
}

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

/// The one size a recipe needs (P6): its sources' `(w, h)`, never upsampled from a smaller or other-aspect normal,
/// and within `cap`.
fn locked_to(
    (w, h): (u32, u32),
    (nw, nh): (u32, u32),
    cap: u32,
    what: &str,
) -> Result<GenSize, String> {
    if nw < w || nh < h || u64::from(nw) * u64::from(h) != u64::from(nh) * u64::from(w) {
        Err(size_locked(
            (w, h),
            what,
            format!("the normal is {nw}×{nh}"),
        ))
    } else if !w.is_power_of_two() || !h.is_power_of_two() {
        // The solve's FFT needs power-of-two sides (review M1).
        Err(size_locked((w, h), what, "not a power of two".into()))
    } else if w.max(h) > cap {
        Err(size_locked(
            (w, h),
            what,
            format!("above the {cap} px limit"),
        ))
    } else {
        Ok(GenSize::Locked(w.max(h)))
    }
}

/// The longer side a map is written at under the dialog's one Size: its locked size, else the Size within its own
/// offered sizes.
fn map_size(rule: &GenSize, size: u32) -> u32 {
    match rule {
        GenSize::Locked(s) => *s,
        GenSize::Free { sizes, .. } => sizes
            .iter()
            .copied()
            .filter(|&s| s <= size)
            .max()
            .or(sizes.first().copied())
            .unwrap_or(size),
    }
}

/// A job's per mille when call `part` (0-based) of `parts` reports `pm`: each call fills its share, so the line never
/// restarts (review M3).
pub fn overall_progress(pm: u32, part: u32, parts: u32) -> u32 {
    (part * 1000 + pm.min(1000)) / parts.max(1)
}

/// The dialog's name for a map.
pub fn map_label(map: GenMap) -> &'static str {
    match map {
        GenMap::Height => "Height",
        GenMap::Ao => "AO",
        GenMap::Curvature => "Curvature",
    }
}

/// A set's normal: (item key, path, size).
type NormalAt = (String, PathBuf, Option<(u32, u32)>);
/// A Generate target: (map, path, the recorded sha256 when the file is ours, its longer side).
type JobTarget = (GenMap, PathBuf, Option<String>, u32);

/// The dialog's set, the plan's alias map and the set's normal.
struct GenCtx {
    v: SetView,
    aliases: Arc<BTreeMap<String, String>>,
    normal: Option<NormalAt>,
}

/// The size warning for the paths about to be written: a `_p` height above `COST_WARN_ABOVE`.
fn warning_of(g: &GenDialog, paths: &[(GenMap, PathBuf, (u32, u32))]) -> Option<&'static str> {
    let big = paths
        .iter()
        .any(|(m, _, (w, h))| *m == GenMap::Height && (*w).max(*h) > COST_WARN_ABOVE);
    (big && !g.m_alpha).then_some(SIZE_WARNING)
}

impl AppModel {
    /// The suffix aliases the last plan matched (each job's `suffix_alias`), so member suffixes read canonical as
    /// the engine's (Task 2: `real_rival` takes a canonical stem).
    /// An alias names only a §6.1 suffix (never a raw role), so every aliased image is a slot file with a job.
    /// shortcut: an alias used only by DDS inputs (no job) is not seen; read the plan's preset registry here if a
    /// DDS `_m` or normal under a user alias ever needs it.
    /// Built once per plan (`items_rev`), not on every offer (Task 6 review M6).
    fn plan_aliases(&self) -> Arc<BTreeMap<String, String>> {
        let rev = self.items_rev();
        let mut c = self.gen_look.borrow_mut();
        if let Some((r, a)) = &c.aliases {
            if *r == rev {
                return a.clone();
            }
        }
        let a: Arc<BTreeMap<String, String>> = Arc::new(
            self.items
                .iter()
                .filter_map(|i| i.job.as_ref()?.suffix_alias.as_ref())
                .map(|a| (a.alias.clone(), a.canonical.clone()))
                .collect(),
        );
        c.alias_builds += 1;
        c.aliases = Some((rev, a.clone()));
        a
    }

    /// The set `set` for the Generate path; counted (tests).
    fn gen_view(&self, set: &str) -> Option<SetView> {
        self.gen_look.borrow_mut().views += 1;
        self.view_of(set)
    }

    /// (set lookups, alias map builds) on the Generate path so far (tests: Task 6 review M6).
    pub fn gen_lookups(&self) -> (u32, u32) {
        let c = self.gen_look.borrow();
        (c.views, c.alias_builds)
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
        self.gen_view(set)
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
    /// Parsed once per (sets document, items) revision, as `generated_info` (final review M4).
    pub fn provenance(&self, key: &str) -> Option<Provenance> {
        let d = self.sets_doc.as_ref()?;
        let rev = (d.rev(), self.items_rev());
        let mut c = self.gen_info.borrow_mut();
        let all = match &c.provs {
            Some((r, all)) if *r == rev => all.clone(),
            _ => {
                let all: Provs = Arc::new(
                    d.generated()
                        .into_iter()
                        .filter_map(|(k, v)| {
                            Some((k.to_lowercase(), serde_json::from_value(v).ok()?))
                        })
                        .collect(),
                );
                c.provs = Some((rev, all.clone()));
                c.prov_builds += 1;
                all
            }
        };
        all.get(&key.to_lowercase()).cloned()
    }

    /// `[generated]` parses for `provenance` (tests).
    pub fn gen_prov_builds(&self) -> u32 {
        self.gen_info.borrow().prov_builds
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
        let Some(v) = self.gen_view(set) else {
            return vec![];
        };
        self.generated_of(&v)
            .into_iter()
            .map(|i| (i.key.clone(), self.provenance(&i.key)))
            .collect()
    }

    /// Why `map` cannot be generated for `set`, the first reason in the brief's order, or `Ok`.
    pub fn gen_offer(&self, set: &str, map: GenMap) -> Result<(), String> {
        let v = self.gen_view(set).ok_or_else(|| format!("no set {set}"))?;
        self.offer(&v, map, &self.plan_aliases())
    }

    fn offer(
        &self,
        v: &SetView,
        map: GenMap,
        aliases: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if let Some(e) = self.sets_doc.as_ref().and_then(|d| d.error()) {
            return Err(format!("sdc-project.toml is read-only: {e}"));
        }
        // Curvature fills no slot (P20), so no slot lock holds it.
        let slot = match map {
            GenMap::Height => Some(SlotKind::Height),
            GenMap::Ao => Some(SlotKind::Rmaos),
            GenMap::Curvature => None,
        };
        if slot.is_some_and(|s| v.locks.contains(&s.slot_number())) {
            return Err(SLOT_LOCKED.into());
        }
        let normal = self.normal_of(v, aliases).ok_or(NO_NORMAL)?;
        let dims = self.normal_dims(&normal)?;
        if !dims.0.is_power_of_two() || !dims.1.is_power_of_two() {
            return Err(GenerateError::NotPowerOfTwo(dims.0, dims.1).to_string());
        }
        let shader = shader_of(v);
        let (own, what) = match map {
            GenMap::Ao => {
                self.ao_shader_and_rmaos(v, shader, aliases)?;
                ("ao", "an AO source")
            }
            GenMap::Curvature => {
                self.curves_reader(v, shader, aliases)?;
                ("curves", "a curvature map")
            }
            GenMap::Height => {
                self.height_shader(v, shader, aliases)?;
                let own = if shader == Shader::ComplexMaterial {
                    "height"
                } else {
                    "p"
                };
                (own, "a height map")
            }
        };
        let has_real = |name: &str| format!("this set already has {what} ({name})");
        let gens: Vec<(&TextureItem, String)> = self
            .generated_of(v)
            .into_iter()
            .filter_map(|i| Some((i, gen_role(&stem(&i.key))?)))
            .filter(|(_, r)| map_of(r) == Some(map))
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
            let rival = self.real_members(v, aliases).find(|(i, s)| {
                let canonical = s.as_ref().map_or_else(
                    || stem(&i.key),
                    |s| format!("{}_{s}", split_suffix_with(&stem(&i.key), aliases).base),
                );
                let kind = match i.kind {
                    ItemKind::Dds => InputKind::Dds,
                    _ => InputKind::Image,
                };
                real_rival(&gen_stem, &canonical, kind, self.target)
                    && !self.own_packed_export(v, i)
            });
            if let Some((i, _)) = rival {
                return Err(has_real(&i.name));
            }
            // Note CO103: a pre-packed image whose B holds AO, read from the plan's analysis as the engine reads it
            // (none, e.g. a skipped image: the real map wins). CO107/CO108: as `pack::plan::prepacked_b`, an aliased
            // image or one beside a loose R/G/A source is never filled, so it blocks with its plain name.
            if let Some((i, s)) = self
                .prepacked_rmaos(v, aliases)
                .filter(|_| map == GenMap::Ao)
            {
                let aliased = engine::group::split_suffix(&stem(&i.key)).1 != s;
                if aliased || self.loose_rmaos_source(v, aliases) {
                    return Err(has_real(&i.name));
                }
                if i.analysis
                    .as_ref()
                    .is_none_or(|a| engine::pack::b_has_content(&a.channels[2]))
                {
                    return Err(has_real(&format!("{} (B)", i.name)));
                }
            }
        }
        if let Some((i, _)) = gens.iter().find(|(i, _)| self.provenance(&i.key).is_none()) {
            return Err(format!(
                "{} exists but was not generated here; delete it to generate again",
                i.name
            ));
        }
        self.gen_size_of(v, map, dims, aliases).map(|_| ())
    }

    /// The height's shader rules (Task 5).
    fn height_shader(
        &self,
        v: &SetView,
        shader: Shader,
        aliases: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        match shader {
            Shader::Default | Shader::Parallax | Shader::Pbr | Shader::PbrLandscape => Ok(()),
            Shader::ComplexMaterial if self.target == engine::config::Target::Cs => {
                Err(COMPLEX_CS.into())
            }
            Shader::ComplexMaterial => {
                let m = self
                    .real_members(v, aliases)
                    .find(|(i, s)| s.as_deref() == Some("m") && !is_generated(&stem(&i.key)));
                match m {
                    Some((i, _)) => Err(format!(
                        "this set has a pre-packed _m ({}); its alpha already decides the height",
                        i.name
                    )),
                    None => Ok(()),
                }
            }
            Shader::Multilayer => Err(MULTILAYER.into()),
            Shader::Landscape => Err(VANILLA_LANDSCAPE.into()),
            Shader::Glow | Shader::Envmap | Shader::Hair | Shader::Eye | Shader::Skin => {
                Err(format!(
                    "a height map would switch this set from {} to Parallax",
                    shader.label()
                ))
            }
            Shader::AdvancedSkin | Shader::PbrGrass | Shader::ComplexGrass => {
                Err(format!("{} reads no height map", shader.label()))
            }
        }
    }

    /// AO's `NOT_PBR` (Q21; PbrGrass reads `_rmaos` too) and `DDS_RMAOS`. Part of the offer itself, so it holds from
    /// the `_rmaos` row, the set menu and Regenerate alike (08e-7).
    fn ao_shader_and_rmaos(
        &self,
        v: &SetView,
        shader: Shader,
        aliases: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let pbr = matches!(
            shader,
            Shader::Pbr | Shader::PbrLandscape | Shader::PbrGrass
        );
        if self.target != engine::config::Target::Cs || !pbr {
            return Err(NOT_PBR.into());
        }
        let dds = self.real_members(v, aliases).find(|(i, s)| {
            matches!(s.as_deref(), Some("rmaos" | "rfaos"))
                && i.kind == ItemKind::Dds
                && !self.own_packed_export(v, i)
        });
        match dds {
            Some((i, _)) => Err(dds_rmaos(&i.name)),
            None => Ok(()),
        }
    }

    /// Curvature's reader is Also export alone, so it is offered only where `also::derive_sets` derives the set: the
    /// cs target, a PBR, PBR landscape or PBR grass shader, and no DDS member (derive skips such a set whole,
    /// DERIVE-DDS-SOURCE). The albedo rule is the size rule's (`NO_ALBEDO`).
    fn curves_reader(
        &self,
        v: &SetView,
        shader: Shader,
        aliases: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let pbr = matches!(
            shader,
            Shader::Pbr | Shader::PbrLandscape | Shader::PbrGrass
        );
        if self.target != engine::config::Target::Cs || !pbr {
            return Err(NOT_PBR_CURVES.into());
        }
        match self
            .real_members(v, aliases)
            .find(|(i, _)| i.kind == ItemKind::Dds)
        {
            Some((i, _)) => Err(dds_set(&i.name)),
            None => Ok(()),
        }
    }

    /// The set's pre-packed `_rmaos`/`_rfaos` image, if any.
    fn prepacked_rmaos<'a>(
        &'a self,
        v: &'a SetView,
        aliases: &'a BTreeMap<String, String>,
    ) -> Option<(&'a TextureItem, Option<String>)> {
        self.real_members(v, aliases).find(|(i, s)| {
            i.kind == ItemKind::Image && matches!(s.as_deref(), Some("rmaos" | "rfaos"))
        })
    }

    /// A loose roughness, glossiness, metallic, specular, fuzz or `_orm` image in the set (the engine's `lone` rule).
    fn loose_rmaos_source(&self, v: &SetView, aliases: &BTreeMap<String, String>) -> bool {
        self.real_members(v, aliases).any(|(i, _)| {
            i.kind == ItemKind::Image
                && matches!(
                    vocab_of(&stem(&i.key)).map(|v| v.role),
                    Some(VocabRole::PackingInput(
                        RawMap::Roughness
                            | RawMap::Glossiness
                            | RawMap::Metallic
                            | RawMap::Specular
                            | RawMap::Fuzz
                            | RawMap::Orm
                    ))
                )
        })
    }

    /// A DDS that is the set's own packed output exported in place (a PBR set under `textures/pbr/`, or one with no
    /// prefix, exported into its own folder): an earlier export, not a pre-packed `_rmaos` or a rival (Review
    /// Focus 1; the engine's `same_file` rule, CO8/CO10).
    fn own_packed_export(&self, v: &SetView, i: &TextureItem) -> bool {
        i.kind == ItemKind::Dds
            && v.members
                .iter()
                .filter_map(|k| self.item(k))
                .filter(|p| p.kind == ItemKind::Packed)
                .filter_map(|p| p.job.as_ref())
                .any(|j| self.same_file_cached(&i.input, &j.output))
    }

    /// `dds::same_file` (a canonicalize per path), kept `STAT_TTL` per pair so an open dialog does not stat every
    /// frame (final review M4), as `header_dims` does.
    fn same_file_cached(&self, a: &Path, b: &Path) -> bool {
        let now = Instant::now();
        let key = (a.to_path_buf(), b.to_path_buf());
        let mut c = self.gen_info.borrow_mut();
        if let Some((at, same)) = c.same.get(&key) {
            if now.saturating_duration_since(*at) < STAT_TTL {
                return *same;
            }
        }
        let same = engine::dds::same_file(a, b);
        c.same.insert(key, (now, same));
        same
    }

    fn normal_dims(&self, normal: &str) -> Result<(u32, u32), String> {
        self.normal_size(normal).ok_or_else(|| {
            let name = self.item(normal).map_or(normal, |i| i.name.as_str());
            format!("the normal map {name} cannot be read")
        })
    }

    /// The Size control of `map` (P6, OWNER-Q1). The dialog (Task 6) only renders it.
    pub fn gen_size(&self, set: &str, map: GenMap) -> Result<GenSize, String> {
        let v = self.gen_view(set).ok_or_else(|| format!("no set {set}"))?;
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
        match map {
            // P6: AO packs into `_rmaos` B, so it takes the size of the recipe's other sources.
            GenMap::Ao => {
                // A pre-packed image is the recipe's R/G/A source before any AO exists (Note CO103).
                let size = self
                    .recipe_size(v, aliases, "rmaos", RawMap::Ao)
                    .or_else(|| {
                        self.prepacked_rmaos(v, aliases)
                            .and_then(|(i, _)| self.header_dims(&i.input))
                    });
                return match size {
                    None => Ok(free(AO_MAX, None)),
                    Some(s) => locked_to(s, (nw, nh), AO_MAX, "AO"),
                };
            }
            // P6: Also's diffuse reads the albedo and the curvature together, so it takes the albedo's size.
            GenMap::Curvature => {
                // No albedo: Also writes no diffuse, so nothing would read it (CO116).
                return match self.albedo_size(v) {
                    None => Err(NO_ALBEDO.into()),
                    Some(s) => locked_to(s, (nw, nh), AO_MAX, "curvature map"),
                };
            }
            GenMap::Height => {}
        }
        match shader_of(v) {
            // OWNER-Q1: the `_m` alpha takes the size of the `_m` recipe's other sources, up to 2048.
            Shader::ComplexMaterial => match self.recipe_size(v, aliases, "m", RawMap::Height) {
                None => Ok(free(HEIGHT_MAX_RECIPE, None)),
                Some(s) => locked_to(s, (nw, nh), HEIGHT_MAX_RECIPE, "height map"),
            },
            // OWNER-Q1: a cs PBR `_p` may follow the albedo up to 2048 (Also's CM `_m` and landscape diffuse read it).
            Shader::Pbr | Shader::PbrLandscape => {
                let albedo = self.albedo_size(v).map(|(w, h)| w.max(h));
                let cap = albedo.map_or(HEIGHT_MAX, |a| a.clamp(HEIGHT_MAX, HEIGHT_MAX_RECIPE));
                Ok(free(cap, albedo))
            }
            _ => Ok(free(HEIGHT_MAX, None)),
        }
    }

    /// The size of the set's own albedo (its Diffuse slot's file). A borrowed diffuse does not count: Also reads only
    /// the set's own jobs (`also::derive_sets`), so for it the set has no albedo (CO116).
    fn albedo_size(&self, v: &SetView) -> Option<(u32, u32)> {
        match v.slots.get(&SlotKind::Diffuse) {
            Some(SlotFill::Own { item }) => self.normal_size(item),
            _ => None,
        }
    }

    /// The size of the set's packed `suffix` recipe's sources other than `own` (the generated map's role), from the
    /// first readable one (P6).
    fn recipe_size(
        &self,
        v: &SetView,
        aliases: &BTreeMap<String, String>,
        suffix: &str,
        own: RawMap,
    ) -> Option<(u32, u32)> {
        let pk = v
            .members
            .iter()
            .filter_map(|k| self.item(k))
            .filter(|i| i.kind == ItemKind::Packed)
            .find(|i| split_suffix_with(&stem(&i.key), aliases).suffix.as_deref() == Some(suffix))?
            .job
            .as_ref()?
            .pack
            .as_ref()?;
        pk.sources
            .iter()
            .filter(|s| s.role != own)
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
    /// The running call of a Generate job that makes its maps in several `generate` calls: `part << 8 | parts`.
    pub part: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
    /// The job in flight: its id and whether it is a preview.
    pub running: Option<(u64, bool)>,
    pub error: Option<String>,
    /// Set by the UI each frame from the sliders' `Response::dragged()`: no preview starts mid-drag.
    pub dragging: bool,
    /// Preview jobs started (tests).
    pub started: u32,
    /// Which open this is; the UI clears the preview zoom when it changes.
    pub opened: u64,
    next_id: u64,
    /// The opts of the last preview started.
    previewed: Option<Opts>,
    /// The running Generate's project root and normal key, taken when it started (its record goes there or nowhere).
    job: Option<(Option<PathBuf>, String)>,
}

/// What a job made: the maps, and for Generate the files it wrote with their sha256 and the opts each was made with
/// (its own size), and the sha256 of the normal it read (hashed on the worker before the decode).
pub struct GenResult {
    pub maps: Generated,
    pub written: Vec<(GenMap, PathBuf, String, Opts)>,
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

/// One job: decode the normal, generate, and for Generate encode and write the ticked maps. `targets` are (map, path,
/// the recorded sha256 when the file is ours, its longer side): maps of one size come from one `generate` call.
fn run_job(
    normal: &Path,
    opts: &Opts,
    targets: Option<Vec<JobTarget>>,
    progress: &AtomicU32,
    part: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<GenResult, String> {
    // Hashed before the decode: a normal saved again meanwhile leaves the record stale, never falsely fresh (I2).
    let from_sha256 = match targets {
        Some(_) => crate::model::history::sha256_of(normal)
            .ok_or_else(|| format!("{} cannot be read", normal.display()))?,
        None => String::new(),
    };
    let (img, _) = engine::load::load_image(normal).map_err(|e| e.to_string())?;
    let run = |o: &Opts| generate(&img, o, progress, cancel).map_err(|e| e.to_string());
    let Some(targets) = targets else {
        return Ok(GenResult {
            maps: run(opts)?,
            written: vec![],
            from_sha256,
        });
    };
    let sizes: BTreeSet<u32> = targets.iter().map(|t| t.3).collect();
    let mut first = None;
    let mut made = vec![];
    // shortcut: up to three sizes (height, AO and curvature each at their own, CO113) solve up to three times; pass per-map
    // sizes into `generate` if that cost shows. Each call reports its part, so the progress line runs once (`overall_progress`).
    let parts = sizes.len() as u32;
    for (i, size) in sizes.into_iter().enumerate() {
        part.store(((i as u32) << 8) | parts, Ordering::Relaxed);
        let has = |m: GenMap| targets.iter().any(|t| t.3 == size && t.0 == m);
        let o = Opts {
            size,
            height: opts.height.clone().filter(|_| has(GenMap::Height)),
            ao: opts.ao.clone().filter(|_| has(GenMap::Ao)),
            curvature: opts.curvature && has(GenMap::Curvature),
            ..opts.clone()
        };
        let maps = run(&o)?;
        for (map, path, own, _) in targets.iter().filter(|t| t.3 == size) {
            let g = match map {
                GenMap::Height => maps.height.as_ref(),
                GenMap::Ao => maps.ao.as_ref(),
                GenMap::Curvature => maps.curvature.as_ref(),
            };
            let bytes = g.map(png16).unwrap_or_default();
            if bytes.is_empty() {
                return Err(format!("no {map:?} map was made; nothing was written"));
            }
            made.push((*map, path.clone(), own.clone(), bytes, o.clone()));
        }
        first.get_or_insert(maps);
    }
    let files: Vec<_> = made
        .iter()
        .map(|(_, p, own, b, _)| (p.clone(), b.clone(), own.clone()))
        .collect();
    let shas = write_all(&files, cancel)?;
    let written = made
        .into_iter()
        .zip(shas)
        .map(|((map, path, _, _, o), sha)| (map, path, sha, o))
        .collect();
    Ok(GenResult {
        maps: first.ok_or("nothing to generate")?,
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
        // The set card's "Regenerate…" (no map preselected) reopens the first recorded map it may make again, with its
        // record (review I1, spec Goal 7).
        let pre = pre.or_else(|| {
            self.generated_items(set)
                .into_iter()
                .filter(|(_, p)| p.is_some())
                .filter_map(|(k, _)| gen_role(&stem(&k)).and_then(|r| map_of(&r)))
                .find(|&m| self.gen_offer(set, m).is_ok())
        });
        let Some(v) = self.gen_view(set) else {
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
        let (mut opts, m_alpha) = match stored {
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
        // A record of one map may lack the other's settings; AO's are held only while AO is ticked (`gen_tick`).
        opts.height.get_or_insert(HeightOpts {
            strength: 1.0,
            neutral: neutral.unwrap_or(Neutral::Half),
        });
        opts.ao = match pre {
            Some(GenMap::Ao) => Some(opts.ao.unwrap_or_default()),
            _ => None,
        };
        // Kept with the tick, like AO's settings.
        opts.curvature = pre == Some(GenMap::Curvature);
        self.gen_opens += 1;
        self.gen = Some(GenDialog {
            opened: self.gen_opens,
            set: set.to_string(),
            ticked: pre.into_iter().collect(),
            opts,
            m_alpha,
            preview: None,
            progress: Arc::default(),
            part: Arc::new(AtomicU32::new(1)),
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

    /// The dialog's set, alias map and normal (item key, path, size), looked up once (Task 6 review M6).
    fn gen_ctx(&self) -> Option<GenCtx> {
        let g = self.gen.as_ref()?;
        let v = self.gen_view(&g.set)?;
        let aliases = self.plan_aliases();
        let normal = self.normal_of(&v, &aliases).and_then(|key| {
            let path = self.item(&key)?.input.clone();
            let dims = self.normal_size(&key);
            Some((key, path, dims))
        });
        Some(GenCtx { v, aliases, normal })
    }

    /// The dialog's normal: (item key, path).
    fn gen_normal(&self) -> Option<(String, PathBuf)> {
        let (key, path, _) = self.gen_ctx()?.normal?;
        Some((key, path))
    }

    /// Each ticked map's size rule (P6, OWNER-Q1).
    fn gen_rules(&self, c: &GenCtx) -> Vec<(GenMap, Result<GenSize, String>)> {
        let (Some(g), Some((_, _, Some(dims)))) = (self.gen.as_ref(), c.normal.as_ref()) else {
            return vec![];
        };
        g.ticked
            .iter()
            .map(|&m| (m, self.gen_size_of(&c.v, m, *dims, &c.aliases)))
            .collect()
    }

    /// The ticked maps' output paths next to the normal, with their dimensions (D4): each map at its own size under
    /// the one Size (per-map caps).
    pub fn gen_paths(&self) -> Vec<(GenMap, PathBuf, (u32, u32))> {
        self.gen_ctx()
            .map(|c| self.paths_in(&c, &self.gen_rules(&c)))
            .unwrap_or_default()
    }

    fn paths_in(
        &self,
        c: &GenCtx,
        rules: &[(GenMap, Result<GenSize, String>)],
    ) -> Vec<(GenMap, PathBuf, (u32, u32))> {
        let (Some(g), Some((key, path, Some(dims)))) = (self.gen.as_ref(), c.normal.as_ref())
        else {
            return vec![];
        };
        let base = split_suffix_with(&stem(key), &c.aliases).base;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        rules
            .iter()
            .map(|(map, rule)| {
                let long = match rule {
                    Ok(r) => map_size(r, g.opts.size),
                    Err(_) => g.opts.size.min(cap_of(*map)),
                }
                .min(dims.0.max(dims.1));
                (
                    *map,
                    dir.join(output_name(&base, *map, g.m_alpha)),
                    scaled(*dims, long),
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

    /// The parallax cost warning while a `_p` height is written above `COST_WARN_ABOVE` (Q11, D2). Not for the `_m`
    /// alpha, whose size follows the `_m` at no extra parallax cost (OWNER-Q1).
    pub fn gen_size_warning(&self) -> Option<&'static str> {
        warning_of(self.gen.as_ref()?, &self.gen_paths())
    }

    /// The read-only normal-format line (07 B3).
    pub fn gen_format_line(&self) -> String {
        let c = self.gen_ctx();
        self.format_line_of(
            c.as_ref()
                .and_then(|c| c.normal.as_ref())
                .map(|n| n.0.as_str()),
        )
    }

    fn format_line_of(&self, normal: Option<&str>) -> String {
        let name = |c: GreenConvention| match c {
            GreenConvention::Opengl => "OpenGL",
            GreenConvention::Directx => "DirectX",
        };
        let Some(g) = self.gen.as_ref() else {
            return String::new();
        };
        if let Some(c) = g.opts.green {
            let has_job = normal
                .and_then(|k| self.item(k).map(|i| i.job.is_some()))
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

    /// Everything the dialog draws this frame, from one set lookup and the plan's alias map (Task 6 review M6).
    pub fn gen_facts(&self) -> GenFacts {
        let c = self.gen_ctx();
        let offers = OFFERED
            .iter()
            .map(|&m| {
                let r = match &c {
                    Some(c) => self.offer(&c.v, m, &c.aliases),
                    None => Err(NO_NORMAL.into()),
                };
                (m, r)
            })
            .collect();
        let rules = c.as_ref().map(|c| self.gen_rules(c)).unwrap_or_default();
        let paths = c
            .as_ref()
            .map(|c| self.paths_in(c, &rules))
            .unwrap_or_default();
        let locked = rules
            .iter()
            .filter_map(|(m, r)| match r {
                Ok(GenSize::Locked(s)) => Some((*m, *s)),
                _ => None,
            })
            .collect();
        // The Size row: the free rule with the most sizes (the larger cap); else the first rule.
        let size = rules
            .iter()
            .filter_map(|(_, r)| match r {
                Ok(f @ GenSize::Free { sizes, .. }) => Some((sizes.len(), f.clone())),
                _ => None,
            })
            .max_by_key(|(n, _)| *n)
            .map(|(_, f)| Ok(f))
            .or_else(|| rules.first().map(|(_, r)| r.clone()))
            .unwrap_or_else(|| match &c {
                Some(c) => match &c.normal {
                    Some((_, _, Some(d))) => self.gen_size_of(&c.v, GenMap::Height, *d, &c.aliases),
                    _ => Err(NO_NORMAL.into()),
                },
                None => Err(NO_NORMAL.into()),
            });
        let normal = c.as_ref().and_then(|c| c.normal.as_ref());
        let want = self.gen.as_ref().map_or(0, |g| g.opts.size);
        GenFacts {
            shown_size: match &size {
                Ok(r @ GenSize::Free { .. }) => map_size(r, want),
                _ => want,
            },
            offers,
            size,
            locked,
            warning: self.gen.as_ref().and_then(|g| warning_of(g, &paths)),
            paths,
            format_line: self.format_line_of(normal.map(|n| n.0.as_str())),
            bent: normal.is_some_and(|n| self.bent_of(&n.0)),
            normal: normal.map(|(_, p, d)| (p.clone(), d.map(|d| d.0.max(d.1)))),
        }
    }

    /// Tick or untick a map. AO's settings start at their defaults (Q17) when AO is ticked, and go when it is not, so
    /// a tick changes the preview's opts and starts a preview.
    pub fn gen_tick(&mut self, map: GenMap, on: bool) {
        // A map's default size (its locked size, else the largest it offers), for "only AO (or Curvature) ticked"
        // (CO95, review M2).
        let c = self.gen_ctx();
        let size_of = |m: GenMap| {
            let c = c.as_ref()?;
            let d = c.normal.as_ref()?.2?;
            match self.gen_size_of(&c.v, m, d, &c.aliases).ok()? {
                GenSize::Locked(s) | GenSize::Free { default: s, .. } => Some(s),
            }
        };
        let solo = [GenMap::Ao, GenMap::Curvature].map(|m| (m, size_of(m)));
        // The map's record, when the set has one: its own settings load with the tick (final review M2, Goal 7).
        let rec = self
            .gen
            .as_ref()
            .filter(|_| on)
            .and_then(|g| {
                self.generated_items(&g.set).into_iter().find_map(|(k, p)| {
                    (gen_role(&stem(&k)).and_then(|r| map_of(&r)) == Some(map)).then_some(p)?
                })
            })
            .map(|p| p.opts);
        let Some(g) = self.gen.as_mut() else {
            return;
        };
        if on {
            g.ticked.insert(map);
        } else {
            g.ticked.remove(&map);
        }
        match map {
            GenMap::Ao => {
                g.opts.ao = on.then(|| {
                    rec.and_then(|r| r.ao)
                        .or(g.opts.ao.take())
                        .unwrap_or_default()
                })
            }
            GenMap::Curvature => g.opts.curvature = on,
            // Only the strength: the neutral is the set's current one (spec-S3).
            GenMap::Height => {
                if let (Some(h), Some(s)) = (
                    g.opts.height.as_mut(),
                    rec.and_then(|r| r.height).map(|h| h.strength),
                ) {
                    h.strength = s;
                }
            }
        }
        for (m, s) in solo {
            if let (true, Some(s)) = (g.ticked.len() == 1 && g.ticked.contains(&m), s) {
                g.opts.size = s;
            }
        }
    }

    /// Whether the set's normal has an RNM bent layer (Q20).
    pub fn has_bent_layer(&self, set: &str) -> bool {
        self.set_normal(set).is_some_and(|n| self.bent_of(&n))
    }

    fn bent_of(&self, normal: &str) -> bool {
        self.item(normal)
            .and_then(|i| i.job.as_ref()?.rnm.as_ref())
            .is_some_and(|r| {
                r.layers
                    .iter()
                    .any(|l| l.id == engine::config::LayerId::Bent)
            })
    }

    /// The preview's AO summary line.
    pub fn gen_ao_summary(&self) -> Option<String> {
        let r = &self.gen.as_ref()?.preview.as_ref()?.maps.report;
        Some(format!(
            "AO: mean {:.2}, {:.0} % of texels below 0.5",
            r.ao_mean?,
            r.ao_dark_share? * 100.0
        ))
    }

    /// The running job's per mille over all its `generate` calls.
    pub fn gen_progress(&self) -> u32 {
        self.gen.as_ref().map_or(0, |g| {
            let p = g.part.load(Ordering::Relaxed);
            overall_progress(g.progress.load(Ordering::Relaxed), p >> 8, p & 0xff)
        })
    }

    /// The opts of the last preview started (tests: the preview AO quality).
    pub fn gen_preview_opts(&self) -> Option<Opts> {
        self.gen.as_ref()?.previewed.clone()
    }

    /// Called each frame while the dialog shows: when nothing runs, the preview's opts changed since the last preview
    /// and no slider is being dragged, start a preview job (at most `PREVIEW_MAX`, CO35; AO at 8 slices × 8 steps).
    pub fn gen_frame(&mut self) {
        let normal = self
            .gen_ctx()
            .and_then(|c| c.normal)
            .map(|(_, p, d)| (p, d.map(|d| d.0.max(d.1))));
        self.gen_frame_at(normal);
    }

    /// `gen_frame` with the normal's path and longer side already looked up (`GenFacts::normal`).
    pub fn gen_frame_at(&mut self, normal: Option<(PathBuf, Option<u32>)>) {
        let tx = self.tx.clone();
        let Some(g) = self.gen.as_mut() else {
            return;
        };
        if g.running.is_some() || g.dragging {
            return;
        }
        let long = normal.as_ref().and_then(|n| n.1);
        let mut opts = g.opts.clone();
        opts.size = opts.size.min(PREVIEW_MAX).min(long.unwrap_or(PREVIEW_MAX));
        if !g.ticked.contains(&GenMap::Ao) {
            opts.ao = None;
        }
        // Review M4: no height preview while Height is unticked (the solve and its fit line stay).
        if !g.ticked.contains(&GenMap::Height) {
            opts.height = None;
        }
        if let Some(a) = opts.ao.as_mut() {
            (a.slices, a.steps) = (8, 8);
        }
        if g.previewed.as_ref() == Some(&opts) {
            return;
        }
        g.previewed = Some(opts.clone());
        let Some((path, _)) = normal else {
            g.error = Some(NO_NORMAL.into());
            return;
        };
        g.started += 1;
        self.spawn_gen(tx, path, opts, None);
    }

    /// Start a job on a worker; it lands as `WorkerMsg::Generated`.
    fn spawn_gen(
        &mut self,
        tx: std::sync::mpsc::Sender<WorkerMsg>,
        normal: PathBuf,
        opts: Opts,
        targets: Option<Vec<JobTarget>>,
    ) {
        let Some(g) = self.gen.as_mut() else {
            return;
        };
        g.next_id += 1;
        self.gen_ids = g.next_id;
        let (id, preview) = (g.next_id, targets.is_none());
        g.running = Some((id, preview));
        g.progress.store(0, Ordering::Relaxed);
        g.part.store(1, Ordering::Relaxed);
        let (progress, part, cancel) = (g.progress.clone(), g.part.clone(), g.cancel.clone());
        spawn_guarded(tx, "generate-from-normal", move |tx| {
            let result = or_send(
                tx,
                || run_job(&normal, &opts, targets, &progress, &part, &cancel).map(Arc::new),
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
                .map(|(map, p, (w, h))| {
                    let name = p.file_name().unwrap_or_default().to_string_lossy();
                    let own = self.provenance(&sibling_key(key, &name)).map(|r| r.sha256);
                    (map, p, own, w.max(h))
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
        if !ticked.contains(&GenMap::Ao) {
            opts.ao = None;
        }
        opts.curvature = ticked.contains(&GenMap::Curvature);
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
        self.generate_landed(open, result);
        // Every way a Generate ends runs the close or open that waited for it, exactly once (fix round 2, N1).
        self.gen_after_job();
    }

    /// The end of a Generate job: its error, or its record (or the notice that it has none).
    fn generate_landed(&mut self, open: bool, result: Result<Arc<GenResult>, String>) {
        let result = result.and_then(|r| match r.written.is_empty() {
            true => Err("nothing was written".to_string()),
            false => Ok(r),
        });
        let r = match result {
            Ok(r) => r,
            Err(e) if open => {
                if let Some(g) = self.gen.as_mut() {
                    g.error = Some(e);
                }
                return;
            }
            Err(e) => {
                self.gen = None;
                // Cancelled with the dialog closed: a failure is still said, never dropped (M3).
                if e != "cancelled" {
                    self.gen_notice(format!("Generate from normal map failed: {e}"));
                }
                return;
            }
        };
        let Some(g) = self.gen.take() else {
            return;
        };
        if open {
            self.dialog = None;
        }
        let (root, normal) = g.job.clone().unwrap_or_default();
        if root != self.root {
            // The project changed under the job: its record belongs to no open project (I1).
            let names: Vec<String> = r
                .written
                .iter()
                .map(|(_, p, _, _)| p.display().to_string())
                .collect();
            self.gen_notice(format!(
                "Generate wrote {}; its project was closed, so they are unrecorded",
                names.join(", ")
            ));
        } else {
            self.gen_record(&g, &normal, &r);
        }
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
            .map(|(_, p, _, _)| {
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
        for ((map, _, sha, opts), key) in written.iter().zip(&keys) {
            let p = Provenance {
                from: normal.clone(),
                from_sha256: from_sha256.clone(),
                sha256: sha.clone(),
                kind: *map,
                m_alpha: g.m_alpha,
                opts: opts.clone(),
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

// ---- Task 7: provenance on the slot row, stale, Delete generated file ----

/// What a slot row shows of a generated file (owner answers 2 and 7): "Generated from {from}" on the File cell's
/// hover and as the Flags marker's name, and `stale` on the row's dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenInfo {
    /// The normal's file name (not `Provenance::from`, which is its item key).
    pub from: String,
    /// GEN-STALE's reason.
    pub stale: Option<String>,
    /// The real map that replaced it (the engine's GEN-REPLACED, P4).
    pub replaced_by: Option<String>,
}

pub const NORMAL_GONE: &str = "the normal map is gone";
pub const READS_NO_HEIGHT: &str =
    "this set's shader reads no height map; the generated file is unused";
pub const DELETE_ENTRY: &str = "Delete generated file…";

/// `AppModel::gen_info`: each normal's sha256 by path with the stamp it was read at, the hashes taken (tests), and
/// each item's `GenInfo` with the (sets document, items) revisions it was built at.
/// `[generated]` parsed, by lowercase item key.
type Provs = Arc<std::collections::HashMap<String, Provenance>>;

#[derive(Default)]
pub struct GenInfoCache {
    hashes: std::collections::HashMap<PathBuf, (Option<SourceStamp>, String)>,
    hashed: u32,
    infos: std::collections::HashMap<String, ((u64, u64), Option<GenInfo>)>,
    /// `[generated]` parsed once per (sets document, items) revision, by lowercase key (final review M4), and the
    /// rebuilds (tests).
    provs: Option<((u64, u64), Provs)>,
    prov_builds: u32,
    /// `own_packed_export`'s `same_file` answers by (DDS, packed output), re-checked after `STAT_TTL`.
    same: std::collections::HashMap<(PathBuf, PathBuf), (Instant, bool)>,
}

/// One file in the "Delete generated file" box: its texts and hash are read when it opens; on Delete the file must
/// still have that hash (spec-S8: the user's confirm covers the file as the box described it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteGenerated {
    pub key: String,
    pub name: String,
    pub path: PathBuf,
    /// "Delete {name}? …", with the whole name (CE19).
    pub body: String,
    /// Set only when the file cannot be read; it is then not deleted.
    pub refused: Option<String>,
    /// The file's sha256 when the box opened.
    pub seen: Option<String>,
}

fn neutral_label(n: Neutral) -> &'static str {
    match n {
        Neutral::Half => "0.5",
        Neutral::White => "1.0",
    }
}

impl AppModel {
    /// The slot row's facts about generated item `item`: `None` for an item that is not a recorded generated file.
    /// Cached per item on the sets document and items revisions, so a frame neither hashes nor rebuilds (ui-rules
    /// 9.1, 9.4).
    pub fn generated_info(&self, item: &str) -> Option<GenInfo> {
        let rev = (
            self.sets_doc
                .as_ref()
                .map_or(0, crate::sets_doc::SetsDoc::rev),
            self.items_rev(),
        );
        let hit = self
            .gen_info
            .borrow()
            .infos
            .get(item)
            .filter(|(r, _)| *r == rev)
            .map(|(_, i)| i.clone());
        if let Some(i) = hit {
            return i;
        }
        let info = self.gen_info_now(item);
        self.gen_info
            .borrow_mut()
            .infos
            .insert(item.to_string(), (rev, info.clone()));
        info
    }

    /// A packed output's generated source (P20: the `_rmaos` row of a generated AO): the first channel read from a
    /// recorded generated file.
    pub fn generated_feed(&self, packed: &str) -> Option<GenInfo> {
        super::pack_fills::fill_rows(self, packed)
            .into_iter()
            .find_map(|r| match r {
                super::pack_fills::FillRow::FromFile { key, .. } => self.generated_info(&key),
                _ => None,
            })
    }

    /// Normal hashes taken for the stale check (tests).
    pub fn gen_hashes(&self) -> u32 {
        self.gen_info.borrow().hashed
    }

    fn gen_info_now(&self, key: &str) -> Option<GenInfo> {
        let i = self.item(key).filter(|_| is_generated(&stem(key)))?;
        let p = self.provenance(key)?;
        let normal = self.item(&p.from);
        let from = normal.map_or_else(
            || {
                Path::new(&p.from)
                    .file_name()
                    .map_or(p.from.clone(), |n| n.to_string_lossy().into_owned())
            },
            |n| n.name.clone(),
        );
        Some(GenInfo {
            stale: self.stale_reason(i, &p, normal, &from),
            from,
            replaced_by: replaced_by(i),
        })
    }

    /// GEN-STALE's reason: the normal is gone or changed (its hash), the set's shader reads no height, or it wants
    /// the other neutral (D7, spec-M4, P8).
    fn stale_reason(
        &self,
        i: &TextureItem,
        p: &Provenance,
        normal: Option<&TextureItem>,
        name: &str,
    ) -> Option<String> {
        let Some(n) = normal else {
            return Some(NORMAL_GONE.into());
        };
        // An image normal's stamp is its job's (`TextureItem::stamp` is `None` for every item with a job, code-S2).
        let stamp = n.job.as_ref().map_or(n.stamp, |j| j.source);
        match self.normal_sha(&n.input, stamp) {
            // Review M3: unreadable is not "changed" (Task 5's wording).
            None => return Some(format!("the normal map {name} cannot be read")),
            Some(h) if h != p.from_sha256 => {
                return Some(format!(
                    "generated from an earlier {name}: Regenerate… to update it"
                ))
            }
            Some(_) => {}
        }
        if p.kind != GenMap::Height {
            return None;
        }
        let views = super::sets::views(self);
        let v = views
            .iter()
            .find(|v| i.set_key.as_deref() == Some(v.key.as_str()))?;
        let shader = v.shader_choice.or(v.shader.as_ref().map(|s| s.shader));
        let resolved = super::auto_flags::resolved(v.pbr, &v.slots, &v.settings);
        let neutral = height_neutral(shader, &resolved);
        let unused = if p.m_alpha {
            shader != Some(Shader::ComplexMaterial)
        } else {
            neutral.is_none()
                || shader.is_some_and(|s| !super::sets::usable_slots(s).contains(&SlotKind::Height))
        };
        if unused {
            return Some(READS_NO_HEIGHT.into());
        }
        let (was, now) = (p.opts.height.as_ref()?.neutral, neutral?);
        (was != now).then(|| {
            format!(
                "generated for a height neutral of {}; this set now wants {}",
                neutral_label(was),
                neutral_label(now)
            )
        })
    }

    /// The normal's sha256, read once per (path, stamp). The stamps come from the plan, so a frame never reads the
    /// disk; the next plan after an edit re-stamps it.
    /// shortcut: hashed on the UI thread (a 4k PNG takes tens of ms, once per change); move to a worker if it stalls.
    fn normal_sha(&self, path: &Path, stamp: Option<SourceStamp>) -> Option<String> {
        let mut c = self.gen_info.borrow_mut();
        if let Some((s, h)) = c.hashes.get(path) {
            if *s == stamp {
                return Some(h.clone());
            }
        }
        c.hashed += 1;
        // A read that failed is not kept: the next rebuild tries again.
        let h = crate::model::history::sha256_of(path)?;
        c.hashes.insert(path.to_path_buf(), (stamp, h.clone()));
        Some(h)
    }

    /// The texture's message list (the settings panel's): its warnings, its check results, and GEN-STALE as Info,
    /// which is never pushed into `TextureItem::warnings` (owner answers 2 and 7).
    pub fn item_messages(&self, key: &str) -> Vec<engine::codes::Warning> {
        let mut ws = self
            .item(key)
            .map(|i| i.warnings.clone())
            .unwrap_or_default();
        ws.extend(self.check_warnings(key).iter().cloned());
        ws.extend(
            self.generated_info(key)
                .and_then(|g| g.stale)
                .map(|r| engine::codes::Warning::new(Code::GenStale, r)),
        );
        ws
    }

    fn project_read_only(&self) -> Option<String> {
        self.sets_doc
            .as_ref()
            .and_then(|d| d.error())
            .map(|e| format!("sdc-project.toml is read-only: {e}"))
    }

    /// The slot row's "Delete generated file…": the Height row filled by a generated file, or whose set has a
    /// generated height the engine replaced (P5: found by set key).
    pub(crate) fn delete_slot_entry(&self, v: &SetView, kind: SlotKind) -> Option<MenuEntry> {
        let of_map = |map: GenMap| {
            self.generated_of(v)
                .into_iter()
                .filter(move |i| gen_role(&stem(&i.key)).and_then(|r| map_of(&r)) == Some(map))
        };
        let key = match kind {
            SlotKind::Height => match v.slots.get(&kind) {
                Some(SlotFill::Own { item }) if is_generated(&stem(item)) => Some(item.clone()),
                _ => None,
            }
            .or_else(|| {
                of_map(GenMap::Height)
                    .find(|i| replaced_by(i).is_some())
                    .map(|i| i.key.clone())
            }),
            // P20: a generated AO fills no row; the `_rmaos` row it feeds (or that replaced it) offers its Delete.
            SlotKind::Rmaos => of_map(GenMap::Ao).next().map(|i| i.key.clone()),
            _ => None,
        }?;
        Some(MenuEntry {
            action: MenuAction::Generated {
                set: v.id.clone(),
                op: GenOp::Delete(key),
            },
            label: DELETE_ENTRY.into(),
            disabled: self.project_read_only(),
        })
    }

    /// The set card's "Delete generated file" submenu: one entry per generated file of the set (P20), by name.
    pub fn delete_generated_entries(&self, set: &str) -> Vec<MenuEntry> {
        let Some(v) = self.gen_view(set) else {
            return vec![];
        };
        let off = self.project_read_only();
        self.generated_of(&v)
            .into_iter()
            .map(|i| MenuEntry {
                action: MenuAction::Generated {
                    set: set.to_string(),
                    op: GenOp::Delete(i.key.clone()),
                },
                label: i.name.clone(),
                disabled: off.clone(),
            })
            .collect()
    }

    /// Open the "Delete generated file" box for item `key` (CE3: user-confirmed). Its hash is read here, once.
    pub fn ask_delete_generated(&mut self, key: &str) {
        let Some(i) = self.item(key) else {
            return;
        };
        let p = self.provenance(key);
        let now = crate::model::history::sha256_of(&i.input);
        let (body, refused) = match &p {
            None => (
                format!(
                    "Delete {}? Tessalith has no record of generating it.",
                    i.name
                ),
                now.is_none().then(|| format!("{} cannot be read", i.name)),
            ),
            Some(p) => {
                let from = self
                    .item(&p.from)
                    .map_or(p.from.as_str(), |n| n.name.as_str());
                let body = format!("Delete {}? It was generated from {from}.", i.name);
                match &now {
                    Some(s) if *s == p.sha256 => (body, None),
                    Some(_) => (format!("{body} It was edited since."), None),
                    None => (body, Some(format!("{} cannot be read", i.name))),
                }
            }
        };
        let row = DeleteGenerated {
            key: key.to_string(),
            name: i.name.clone(),
            path: i.input.clone(),
            body,
            refused,
            seen: now,
        };
        self.dialog = Some(crate::model::Dialog::ConfirmDeleteGenerated(Box::new(row)));
    }

    /// "Delete …" in the box: delete each file that is still the one Tessalith wrote (its hash is checked again
    /// now), then drop its record outside the undo history (P14's save-if-clean rule) and re-plan. Not undoable: the
    /// confirm box is the safeguard.
    pub fn confirm_delete_generated(&mut self) {
        let Some(crate::model::Dialog::ConfirmDeleteGenerated(r)) = self.dialog.take() else {
            return;
        };
        // Review M5: the record could not be dropped, so nothing is deleted.
        if let Some(why) = self.project_read_only() {
            self.gen_notice(why);
            return;
        }
        if r.refused.is_some() {
            return;
        }
        if crate::model::history::sha256_of(&r.path) != r.seen {
            self.gen_notice(format!("{} changed since; it was not deleted", r.name));
            return;
        }
        if let Err(e) = std::fs::remove_file(&r.path) {
            self.gen_notice(format!("{}: {e}", r.path.display()));
            return;
        }
        let k = &r.key;
        if let Some(d) = self.sets_doc.as_mut() {
            let was_dirty = d.is_dirty();
            d.set_generated(k, None);
            // Review M4: a file moved in with its normal leaves the set's `add` too (no SET-EDIT-STALE).
            for e in d
                .entries()
                .iter()
                .filter(|e| e.add.iter().any(|f| engine::sets::same_id(f, k)))
            {
                d.remove_member(&e.id, k);
            }
            if self.save_record(was_dirty) {
                self.status_note = Some(PROVENANCE_DEFERRED.into());
            }
        }
        self.replan();
    }
}
