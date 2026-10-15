//! The pixel readout of the expanded preview (item 1, DECISIONS AG54-AG56): the texel under the cursor, read from
//! the CPU images the preview already holds (the source chain and the decoded encode), never from the GPU.
use crate::model::pin::decoded_side;
use crate::model::preview::SourceState;
use crate::model::view::{channel_error, Channel};
use crate::model::AppModel;
use engine::config::{Category, Format};

/// One side's image at the shown mip.
#[derive(Clone, Copy)]
pub struct Side<'a> {
    pub width: u32,
    pub height: u32,
    pub px: &'a [[f32; 4]],
}

impl Side<'_> {
    fn at(&self, (x, y): (u32, u32)) -> Option<[f32; 4]> {
        (x < self.width && y < self.height)
            .then(|| self.px.get((y * self.width + x) as usize).copied())
            .flatten()
    }
}

#[derive(Clone, Copy)]
pub struct ReadoutOpts {
    pub channel: Channel,
    /// The UNORM view: the stored values are read as linear data.
    pub view_linear: bool,
    /// An HDR format: the values are linear floats with no 0-255 form.
    pub hdr: bool,
    /// A normal sub-type: `Some(two_channel)`, where a two-channel encode (BC5, RG8) rebuilds z as the shader does.
    pub normal: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalReadout {
    pub source: Option<[f32; 3]>,
    pub encoded: Option<[f32; 3]>,
    /// The encoded vector's length.
    pub length: Option<f32>,
    /// The angle between the source and encoded vectors, in degrees.
    pub angle_deg: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PixelReadout {
    pub texel: (u32, u32),
    pub mip: usize,
    pub source: Option<[f32; 4]>,
    pub encoded: Option<[f32; 4]>,
    /// |source − encoded| per channel.
    pub error: Option<[f32; 4]>,
    /// The error the channel measures, as the heatmap does.
    pub max_error: Option<f32>,
    pub normal: Option<NormalReadout>,
    /// While comparing with a pin (DECISIONS AG68): the pinned side of the texel and its |pinned - encoded|.
    /// `Some(None)` is a pinned encode without this mip.
    pub pinned: Option<Option<PinnedReadout>>,
    /// What the numbers are.
    pub space: &'static str,
    channels: &'static [usize],
    hdr: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PinnedReadout {
    pub value: [f32; 4],
    /// |pinned - encoded| per channel, `None` without an encoded side.
    pub error: Option<[f32; 4]>,
}

const NAMES: [&str; 4] = ["R", "G", "B", "A"];

fn channels(c: Channel) -> &'static [usize] {
    match c {
        Channel::Rgba => &[0, 1, 2, 3],
        Channel::R => &[0],
        Channel::G => &[1],
        Channel::B => &[2],
        Channel::A | Channel::AlphaMask => &[3],
    }
}

fn len3(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The source and encoded normals of a texel (stored 0..1 to −1..1), the encoded length and the angle error.
pub fn normal_readout(
    source: Option<[f32; 4]>,
    encoded: Option<[f32; 4]>,
    two_channel: bool,
) -> NormalReadout {
    let unpack = |p: [f32; 4]| [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
    let src = source.map(unpack);
    let enc = encoded.map(|p| {
        let mut v = unpack(p);
        if two_channel {
            v[2] = (1.0 - v[0] * v[0] - v[1] * v[1]).max(0.0).sqrt();
        }
        v
    });
    let angle = src.zip(enc).and_then(|(a, b)| {
        let l = len3(a) * len3(b);
        (l > 1e-12).then(|| {
            let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            (dot / l).clamp(-1.0, 1.0).acos().to_degrees()
        })
    });
    NormalReadout {
        source: src,
        encoded: enc,
        length: enc.map(len3),
        angle_deg: angle,
    }
}

/// The readout of `texel` at absolute mip `mip`, from each side's image at that mip.
pub fn texel_readout(
    source: Option<Side>,
    encoded: Option<Side>,
    mip: usize,
    texel: (u32, u32),
    o: ReadoutOpts,
) -> PixelReadout {
    let s = source.and_then(|s| s.at(texel));
    let e = encoded.and_then(|e| e.at(texel));
    let error = s
        .zip(e)
        .map(|(s, e)| std::array::from_fn(|c| (s[c] - e[c]).abs()));
    let space = if o.hdr {
        "linear HDR values"
    } else if o.view_linear {
        "stored values, read as linear (UNORM)"
    } else {
        "stored values, sRGB-encoded"
    };
    PixelReadout {
        texel,
        mip,
        source: s,
        encoded: e,
        error,
        max_error: error.map(|e| channel_error(e, o.channel)),
        normal: o.normal.map(|two| normal_readout(s, e, two)),
        pinned: None,
        space,
        channels: channels(o.channel),
        hdr: o.hdr,
    }
}

fn vec3(v: Option<[f32; 3]>) -> String {
    v.map_or("—".into(), |v| {
        format!("({:.3}, {:.3}, {:.3})", v[0], v[1], v[2])
    })
}

impl PixelReadout {
    /// Adds the pinned side (`None`: the pinned encode lacks this mip).
    pub fn with_pinned(mut self, pinned: Option<Side>) -> PixelReadout {
        self.pinned = Some(pinned.and_then(|p| p.at(self.texel)).map(|value| {
            PinnedReadout {
                value,
                error: self
                    .encoded
                    .map(|e| std::array::from_fn(|c| (value[c] - e[c]).abs())),
            }
        }));
        self
    }

    /// The readout as lines: the texel and colour space, each side, the error, and the normal.
    pub fn lines(&self) -> Vec<String> {
        let (x, y) = self.texel;
        let mut out = vec![format!(
            "Texel ({x}, {y}) at mip {}: {}",
            self.mip, self.space
        )];
        let side = |name: &str, p: Option<[f32; 4]>| match p {
            None => format!("{name}: —"),
            Some(p) => {
                let parts: Vec<String> = self
                    .channels
                    .iter()
                    .map(|&c| {
                        if self.hdr {
                            format!("{} {:.4}", NAMES[c], p[c])
                        } else {
                            let code = (p[c].clamp(0.0, 1.0) * 255.0).round() as u8;
                            format!("{} {code} {:.4}", NAMES[c], p[c])
                        }
                    })
                    .collect();
                format!("{name}: {}", parts.join("  "))
            }
        };
        out.push(side("Source", self.source));
        if let Some(p) = &self.pinned {
            out.push(side("Pinned", p.as_ref().map(|p| p.value)));
        }
        out.push(side("Encoded", self.encoded));
        out.push(match (self.error, self.max_error) {
            (Some(e), Some(m)) => {
                let parts: Vec<String> = self
                    .channels
                    .iter()
                    .map(|&c| format!("{} {:.4}", NAMES[c], e[c]))
                    .collect();
                format!("|Δ|: {}  max {m:.4}", parts.join("  "))
            }
            _ => "|Δ|: —".into(),
        });
        if let Some(p) = &self.pinned {
            out.push(match p.as_ref().and_then(|p| p.error) {
                Some(e) => {
                    let parts: Vec<String> = self
                        .channels
                        .iter()
                        .map(|&c| format!("{} {:.4}", NAMES[c], e[c]))
                        .collect();
                    format!("|Δ| pinned vs encoded: {}", parts.join("  "))
                }
                None => "|Δ| pinned vs encoded: —".into(),
            });
        }
        if let Some(n) = &self.normal {
            out.push(format!(
                "Normal: source {}  encoded {}  length {}  angle error {}",
                vec3(n.source),
                vec3(n.encoded),
                n.length.map_or("—".into(), |l| format!("{l:.4}")),
                n.angle_deg.map_or("—".into(), |a| format!("{a:.2}°")),
            ));
        }
        out
    }
}

impl AppModel {
    /// The readout of `texel` at absolute mip `mip` of the expanded preview, under the current view. The source
    /// side is the one the result was encoded from (the loaded source before any encode).
    pub fn preview_texel(&self, mip: usize, texel: (u32, u32)) -> PixelReadout {
        let shown = self.preview.encoded.shown();
        let source = match shown {
            Some(o) => {
                let s = &o.result.source;
                mip.checked_sub(s.first_level).and_then(|i| s.levels.get(i))
            }
            None => match &self.preview.source {
                SourceState::Ready(s) => {
                    mip.checked_sub(s.first_level).and_then(|i| s.levels.get(i))
                }
                _ => None,
            },
        }
        .map(|l| Side {
            width: l.width,
            height: l.height,
            px: &l.px,
        });
        let encoded = shown.and_then(|o| {
            let (w, h) = o.result.source.dims.get(mip).copied()?;
            let px = o
                .decoded
                .get(mip.checked_sub(o.result.source.first_level)?)?;
            Some(Side {
                width: w,
                height: h,
                px,
            })
        });
        let format = shown.map(|o| o.result.format);
        let job = self
            .expanded
            .as_deref()
            .and_then(|k| self.item(k))
            .and_then(|i| i.job.as_ref());
        let normal = job
            .filter(|j| j.profile.subtype.category() == Category::Normal)
            .map(|_| matches!(format, Some(Format::Bc5 | Format::Rg8)));
        let hdr = format
            .or(job.map(|j| j.profile.format))
            .is_some_and(Format::is_hdr);
        let r = texel_readout(
            source,
            encoded,
            mip,
            texel,
            ReadoutOpts {
                channel: self.view.channel,
                view_linear: self.view.view_linear,
                hdr,
                normal,
            },
        );
        match self.compared_pin() {
            Some(pin) => r.with_pinned(decoded_side(&pin.out, mip)),
            None => r,
        }
    }

    /// The images of the left and right sides at absolute mip `mip`: the source (or, while comparing with a pin, the
    /// pinned encode) and the current encode.
    pub fn compare_sides(&self, mip: usize) -> (Option<Side<'_>>, Option<Side<'_>>) {
        let shown = self.preview.encoded.shown();
        let right = shown.and_then(|o| decoded_side(o, mip));
        if let Some(pin) = self.compared_pin() {
            return (decoded_side(&pin.out, mip), right);
        }
        let left = shown.and_then(|o| {
            let s = &o.result.source;
            let l = s.levels.get(mip.checked_sub(s.first_level)?)?;
            Some(Side {
                width: l.width,
                height: l.height,
                px: &l.px,
            })
        });
        (left, right)
    }
}
