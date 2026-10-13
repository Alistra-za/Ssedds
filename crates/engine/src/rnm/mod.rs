//! M6 Track F2: RNM normal layers (spec `docs/superpowers/specs/2026-10-12-sdc-m6-f2-rnm-design.md`). Layer files
//! (`_bump`, `_detailnorm`, `_bent`, their masks, rnm.py aliases, `_n_`/`_cnr_` prefixes) join their set, are never
//! written on their own and are blended into the set's base normal before mips.
use crate::config::{LayerId, RnmSpec, RnmTarget};
use crate::group::{Vocab, VocabRole};
use crate::policy::SourceStamp;
use std::path::PathBuf;

/// What a layer file is (F2 spec §2.1): its layer, whether it is that layer's mask, and the base its name targets
/// (`_n_` → `N`, `_cnr_` → `Cnr`, none → `None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerTerm {
    pub layer: LayerId,
    pub mask: bool,
    pub prefix: Option<RnmTarget>,
}

impl LayerTerm {
    /// The canonical suffix the file counts as in its set: the owner's spelling (`bumpmask2`, `detailnormmask`),
    /// prefixed `n_`/`cnr_` when the name was.
    pub fn canonical(&self) -> String {
        let stem = match self.layer {
            LayerId::Bump | LayerId::Bump2 => "bump",
            LayerId::Detail | LayerId::Detail2 => "detailnorm",
            LayerId::Bent => "bent",
        };
        let two = matches!(self.layer, LayerId::Bump2 | LayerId::Detail2);
        let core = format!(
            "{stem}{}{}",
            if self.mask { "mask" } else { "" },
            if two { "2" } else { "" }
        );
        match self.prefix {
            Some(RnmTarget::N) => format!("n_{core}"),
            Some(RnmTarget::Cnr) => format!("cnr_{core}"),
            _ => core,
        }
    }
}

const fn lt(term: &'static str, layer: LayerId, mask: bool, prefix: Option<RnmTarget>) -> Vocab {
    Vocab {
        term,
        role: VocabRole::NormalLayer(LayerTerm {
            layer,
            mask,
            prefix,
        }),
    }
}

/// Each term three times: bare, `n_` and `cnr_` prefixed.
macro_rules! layer_vocab {
    ($($t:literal => $layer:ident $mask:literal),+ $(,)?) => {
        [$(
            lt($t, LayerId::$layer, $mask, None),
            lt(concat!("n_", $t), LayerId::$layer, $mask, Some(RnmTarget::N)),
            lt(concat!("cnr_", $t), LayerId::$layer, $mask, Some(RnmTarget::Cnr)),
        )+]
    };
}

/// F2 spec §2.1: the owner's ten suffixes, then rnm.py's aliases. Matched longest first ([`split_layer`]), before the
/// exporter vocabulary.
pub static LAYER_VOCABULARY: [Vocab; 51] = layer_vocab![
    "bump" => Bump false,
    "bump2" => Bump2 false,
    "bumpmask" => Bump true,
    "bumpmask2" => Bump2 true,
    "detailnorm" => Detail false,
    "detailnorm2" => Detail2 false,
    "detailnormmask" => Detail true,
    "detailnormmask2" => Detail2 true,
    "bent" => Bent false,
    "bentmask" => Bent true,
    "detail" => Detail false,
    "detail2" => Detail2 false,
    "detail_mask" => Detail true,
    "detail2_mask" => Detail2 true,
    "bump_mask" => Bump true,
    "bump2_mask" => Bump2 true,
    "bent_mask" => Bent true,
];

/// The layer term `stem` ends in (case-insensitive; the longest, so `_cnr_bump` beats `_bump`) and the base before
/// it. A stem that is only a term is left alone.
pub fn split_layer(stem: &str) -> Option<(String, &'static Vocab)> {
    let lower = stem.to_ascii_lowercase();
    LAYER_VOCABULARY
        .iter()
        .filter(|v| {
            let tail = format!("_{}", v.term);
            lower.ends_with(&tail) && lower.len() > tail.len()
        })
        .max_by_key(|v| v.term.len())
        // The tail is ASCII, so the cut falls on a char boundary even in non-ASCII stems.
        .map(|v| (stem[..stem.len() - v.term.len() - 1].to_string(), v))
}

/// The serialisable mirror of [`LayerTerm`], carried by `JobStatus::NormalLayer`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub struct LayerTermId {
    pub layer: LayerId,
    pub mask: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<RnmTarget>,
}

impl From<LayerTerm> for LayerTermId {
    fn from(t: LayerTerm) -> Self {
        LayerTermId {
            layer: t.layer,
            mask: t.mask,
            prefix: t.prefix,
        }
    }
}
impl From<LayerTermId> for LayerTerm {
    fn from(t: LayerTermId) -> Self {
        LayerTerm {
            layer: t.layer,
            mask: t.mask,
            prefix: t.prefix,
        }
    }
}

pub mod blend;
pub mod plan;

/// One input file of a blend, with the stamp `plan` read it at (checked by `run`, like `ResolvedJob::source`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RnmFile {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<SourceStamp>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RnmLayer {
    pub id: LayerId,
    pub file: RnmFile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<RnmFile>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RnmAo {
    pub file: RnmFile,
    /// 0 = a raw AO map's R, 2 = `_rmaos`/`_rfaos` B.
    pub channel: u8,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct RnmJob {
    pub layers: Vec<RnmLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ao: Option<RnmAo>,
}

impl RnmJob {
    /// The files the blend reads under `spec`, in a fixed order with a role: each layer in `LayerId::ORDER` order
    /// ("detail"), then its mask when used ("detail.mask"), then the AO when used ("bent.ao.r" for a raw `_ao`, "bent.ao.b" for `_rmaos`/`_rfaos`).
    pub fn inputs(&self, spec: &RnmSpec) -> Vec<(&RnmFile, String)> {
        let mut v = vec![];
        for id in LayerId::ORDER {
            let Some(l) = self.layers.iter().find(|l| l.id == id) else {
                continue;
            };
            v.push((&l.file, id.id().to_string()));
            if let (Some(m), true) = (&l.mask, spec.layer(id).mask) {
                v.push((m, format!("{}.mask", id.id())));
            }
        }
        let bent_ao = self.layers.iter().any(|l| l.id == LayerId::Bent)
            && spec.bent.mode == crate::config::BentMode::AoBlend
            && spec.bent.ao;
        if let (Some(a), true) = (&self.ao, bent_ao) {
            // The channel is part of the identity: the same bytes as `_ao` (R) and `_rmaos` (B) feed different data.
            let ch = if a.channel == 0 { "r" } else { "b" };
            v.push((&a.file, format!("bent.ao.{ch}")));
        }
        v
    }
}
