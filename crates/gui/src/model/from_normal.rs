//! v2 Track B (Note CO): "Generate from normal map…". The offers and their disabled reasons, the set's normal, the
//! offered sizes and the `[generated]` provenance records. The dialog and the jobs are Task 6.
use super::bulk::{MenuAction, MenuEntry};
use super::checks::{self, HeightNeutral};
use super::items::{ItemKind, TextureItem};
use super::sets::{SetView, SlotFill, SlotKind, SLOT_LOCKED};
use super::shell::STAT_TTL;
use super::AppModel;
use crate::pgp::settings::Settings;
use engine::codes::Code;
use engine::config::RawMap;
use engine::discover::InputKind;
use engine::from_normal::{sizes, GenerateError, Neutral, Opts, HEIGHT_MAX, HEIGHT_MAX_RECIPE};
use engine::group::{is_generated, real_rival, split_generated, split_suffix_with};
use engine::policy::SourceStamp;
use engine::shader::Shader;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
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
