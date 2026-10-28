//! Helper textures (DECISIONS Note DT): a grey map that fills a channel of another output of its set. This module
//! holds the hand-set roles, the fill targets and the four per-file `helper.*` selectors.
use crate::config::{LayerId, RawMap};
use crate::rnm::{LayerTerm, LayerTermId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod plan;

pub const ROLE_FIELD: &str = "helper.role";
pub const FILLS_FIELD: &str = "helper.fills";
pub const READ_FIELD: &str = "helper.read";
pub const INVERT_FIELD: &str = "helper.invert";

/// The four `helper.*` selectors.
pub const FIELDS: [&str; 4] = [ROLE_FIELD, FILLS_FIELD, READ_FIELD, INVERT_FIELD];

/// The raw-map roles a helper can be given by hand: the file-name roles plus the two weights no name carries.
const RAW_ROLES: [RawMap; 13] = {
    use RawMap::*;
    [
        Roughness,
        Metallic,
        Ao,
        Specular,
        Glossiness,
        Height,
        Opacity,
        Fuzz,
        Orm,
        Curves,
        Cavity,
        FuzzWeight,
        SubsurfaceWeight,
    ]
};

/// What a file is as a helper, when set by hand (`helper.role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperRole {
    // "none": an ordinary file, converted on its own.
    NotHelper,
    Raw(RawMap),
    // A normal layer; `prefix` is always `None` (the target is `rnm.target`).
    Layer(LayerTermId),
}

impl HelperRole {
    /// Every role, in menu order: Not a helper, the raw maps, the normal layers.
    pub fn all() -> Vec<HelperRole> {
        let layers = [
            LayerId::Bump,
            LayerId::Bump2,
            LayerId::Detail,
            LayerId::Detail2,
            LayerId::Bent,
        ]
        .into_iter()
        .flat_map(|layer| {
            [false, true].map(|mask| {
                HelperRole::Layer(LayerTermId {
                    layer,
                    mask,
                    prefix: None,
                })
            })
        });
        std::iter::once(HelperRole::NotHelper)
            .chain(RAW_ROLES.map(HelperRole::Raw))
            .chain(layers)
            .collect()
    }

    /// The `helper.role` value: `none`, a `RawMap` id, or a layer's canonical suffix without prefix.
    pub fn id(&self) -> String {
        match self {
            HelperRole::NotHelper => "none".into(),
            HelperRole::Raw(r) => r.id(),
            HelperRole::Layer(t) => LayerTerm {
                prefix: None,
                ..(*t).into()
            }
            .canonical(),
        }
    }

    pub fn parse(s: &str) -> Option<HelperRole> {
        HelperRole::all().into_iter().find(|r| r.id() == s)
    }

    pub fn label(&self) -> String {
        match self {
            HelperRole::NotHelper => "Not a helper".into(),
            HelperRole::Raw(r) => match r {
                RawMap::Fuzz => "Fuzz mask (skin)",
                RawMap::FuzzWeight => "Fuzz weight (PBR)",
                RawMap::SubsurfaceWeight => "Subsurface weight (PBR)",
                RawMap::Roughness => "Roughness",
                RawMap::Metallic => "Metallic",
                RawMap::Ao => "Ambient occlusion",
                RawMap::Specular => "Specular",
                RawMap::Glossiness => "Glossiness",
                RawMap::Height => "Height",
                RawMap::Opacity => "Opacity",
                RawMap::Orm => "ORM (occlusion, roughness, metallic)",
                RawMap::Curves => "Curvature",
                RawMap::Cavity => "Cavity",
                RawMap::Rmaos => "RMAOS",
            }
            .into(),
            HelperRole::Layer(t) => {
                let l = t.layer.label();
                let mut s = l[..1].to_uppercase() + &l[1..] + " normal";
                if t.mask {
                    s.push_str(" mask");
                }
                s
            }
        }
    }

    pub fn is_normal(&self) -> bool {
        matches!(self, HelperRole::Layer(_))
    }
}

/// Which channel of the helper is read (`helper.read`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HelperRead {
    R,
    G,
    B,
    A,
    Luma,
}

impl HelperRead {
    /// The `ChannelSource::Source.channel` it maps to; luma is [`crate::pack::LUMA_CHANNEL`].
    pub fn channel(self) -> u8 {
        match self {
            HelperRead::R => 0,
            HelperRead::G => 1,
            HelperRead::B => 2,
            HelperRead::A => 3,
            HelperRead::Luma => crate::pack::LUMA_CHANNEL,
        }
    }
}

/// The output whose alpha a helper fills: the diffuse, `_n`, `_f`, `_s` or `_cnr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaTarget {
    Diffuse,
    N,
    F,
    S,
    Cnr,
}

impl AlphaTarget {
    const ALL: [AlphaTarget; 5] = [
        AlphaTarget::Diffuse,
        AlphaTarget::N,
        AlphaTarget::F,
        AlphaTarget::S,
        AlphaTarget::Cnr,
    ];

    /// The output's suffix; `None` for the diffuse.
    fn suffix(self) -> Option<&'static str> {
        match self {
            AlphaTarget::Diffuse => None,
            AlphaTarget::N => Some("n"),
            AlphaTarget::F => Some("f"),
            AlphaTarget::S => Some("s"),
            AlphaTarget::Cnr => Some("cnr"),
        }
    }
}

/// A channel a helper fills (`helper.fills`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillTarget {
    Alpha(AlphaTarget),
    Packed { suffix: &'static str, channel: u8 },
}

/// The packed outputs and how many channels a helper may fill in each (`_p` has only R).
const PACKED: [(&str, u8); 4] = [("rmaos", 4), ("rfaos", 4), ("m", 4), ("p", 1)];
const CHANNELS: [&str; 4] = ["r", "g", "b", "a"];

impl FillTarget {
    pub fn all() -> Vec<FillTarget> {
        let packed = PACKED.into_iter().flat_map(|(suffix, n)| {
            (0..n).map(move |channel| FillTarget::Packed { suffix, channel })
        });
        AlphaTarget::ALL
            .map(FillTarget::Alpha)
            .into_iter()
            .chain(packed)
            .collect()
    }

    /// `diffuse.a`, `n.a`, `rmaos.r`, …
    pub fn id(&self) -> String {
        match self {
            FillTarget::Alpha(a) => format!("{}.a", a.suffix().unwrap_or("diffuse")),
            FillTarget::Packed { suffix, channel } => {
                format!("{suffix}.{}", CHANNELS[usize::from(*channel)])
            }
        }
    }

    pub fn parse(s: &str) -> Option<FillTarget> {
        FillTarget::all().into_iter().find(|t| t.id() == s)
    }

    /// "rock alpha", "rock_n alpha", "rock_rmaos R" for the set `set_base`.
    pub fn label(&self, set_base: &str) -> String {
        match self {
            FillTarget::Alpha(a) => match a.suffix() {
                None => format!("{set_base} alpha"),
                Some(s) => format!("{set_base}_{s} alpha"),
            },
            FillTarget::Packed { suffix, channel } => format!(
                "{set_base}_{suffix} {}",
                CHANNELS[usize::from(*channel)].to_uppercase()
            ),
        }
    }
}

/// What a file's `helper.*` selectors hand-set. `Default` = nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelperSel {
    pub role: Option<HelperRole>,
    pub fills: Option<Vec<FillTarget>>,
    pub read: Option<HelperRead>,
    pub invert: bool,
}

impl HelperSel {
    /// A helper role (other than Not a helper) or fills was set by hand.
    pub fn assigned(&self) -> bool {
        self.role.is_some_and(|r| r != HelperRole::NotHelper) || self.fills.is_some()
    }
}

/// How a helper fills its target's alpha (Note DT): the source, the channel read (`None` = R, Ruling 9), whether it
/// is inverted, and whether it was hand-set (assigned replaces any alpha; suggested fills only an empty one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FillJob {
    pub source: crate::pack::PackSource,
    pub read: Option<HelperRead>,
    pub invert: bool,
    pub assigned: bool,
}

impl FillJob {
    /// `a:<read>` plus `:inv` when inverted: what the fill adds to its output's identity.
    pub fn identity(&self) -> String {
        let read = match self.read.unwrap_or(HelperRead::R) {
            HelperRead::R => "r",
            HelperRead::G => "g",
            HelperRead::B => "b",
            HelperRead::A => "a",
            HelperRead::Luma => "luma",
        };
        format!("a:{read}{}", if self.invert { ":inv" } else { "" })
    }
}

/// Writes the helper's channel into `img`'s alpha (Note DT): the plan analyses and the run encodes the same filled
/// image (Note M6). `info` keeps the target's bit depth and float-ness (Ruling 10).
pub fn apply_fill(
    img: &mut crate::image::Rgba32F,
    info: &mut crate::image::SourceInfo,
    f: &FillJob,
) -> Result<(), String> {
    let (src, _) = crate::load::load_image(&f.source.path).map_err(|e| e.to_string())?;
    if (src.width, src.height) != (img.width, img.height) {
        return Err(format!(
            "{}: size changed since planning; re-run",
            f.source.path.display()
        ));
    }
    let k = f.read.unwrap_or(HelperRead::R).channel();
    for (o, q) in img.px.iter_mut().zip(&src.px) {
        let v = match k {
            crate::pack::LUMA_CHANNEL => crate::pack::assemble::luma(q),
            k => q[usize::from(k)],
        };
        o[3] = (if f.invert { 1.0 - v } else { v }).clamp(0.0, 1.0);
    }
    info.has_alpha = true;
    info.channels = 4;
    Ok(())
}

/// `helper.fills`'s value: a list of target ids.
pub fn fills_value(v: &Value) -> Option<Vec<FillTarget>> {
    v.as_array()?
        .iter()
        .map(|t| FillTarget::parse(t.as_str()?))
        .collect()
}

/// Why `v` is no valid value of the `helper.*` selector `field`; `None` when it is valid or `field` is no helper key.
pub fn value_error(field: &str, v: &Value) -> Option<String> {
    let ok = match field {
        ROLE_FIELD => v.as_str().and_then(HelperRole::parse).is_some(),
        FILLS_FIELD => fills_value(v).is_some(),
        READ_FIELD => serde_json::from_value::<HelperRead>(v.clone()).is_ok(),
        INVERT_FIELD => v.is_boolean(),
        _ => return None,
    };
    (!ok).then(|| match field {
        ROLE_FIELD => {
            let ids: Vec<String> = HelperRole::all().iter().map(|r| r.id()).collect();
            format!("helper.role takes one of: {}", ids.join(", "))
        }
        FILLS_FIELD => {
            let ids: Vec<String> = FillTarget::all().iter().map(|t| t.id()).collect();
            format!("helper.fills takes a list of: {}", ids.join(", "))
        }
        READ_FIELD => "helper.read takes r, g, b, a or luma".into(),
        _ => "helper.invert takes true or false".into(),
    })
}
