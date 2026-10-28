//! Pure maths for the expanded preview (M3a spec §6.1): pan/zoom in uv space, the difference heatmap, readouts.
use crate::model::preview::PreviewOut;
use engine::image::Rgba32F;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Channel {
    #[default]
    Rgba,
    R,
    G,
    B,
    A,
    AlphaMask,
}

impl Channel {
    /// The channel number the shader takes.
    pub fn shader_id(self) -> u32 {
        self as u32
    }
    /// The channel with this `shader_id`, `None` for a number that names none.
    pub fn from_id(id: u32) -> Option<Channel> {
        [
            Channel::Rgba,
            Channel::R,
            Channel::G,
            Channel::B,
            Channel::A,
            Channel::AlphaMask,
        ]
        .into_iter()
        .find(|c| c.shader_id() == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    SideBySide,
    Split,
}

impl Layout {
    /// The name in `gui.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Layout::SideBySide => "side-by-side",
            Layout::Split => "split",
        }
    }
    pub fn from_name(s: &str) -> Option<Layout> {
        [Layout::SideBySide, Layout::Split]
            .into_iter()
            .find(|l| l.name() == s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    pub channel: Channel,
    pub layout: Layout,
    /// Where the divider sits in `Layout::Split`, 0..1 of the viewport's width.
    pub split: f32,
    /// Show the stored values as linear data (the "UNORM" view).
    pub view_linear: bool,
    pub heatmap: bool,
    /// The absolute mip index.
    pub mip: usize,
    /// Screen pixels per texel of the shown mip: 1.0 is 1:1.
    pub zoom: f32,
    /// The uv at the viewport centre.
    pub center: [f32; 2],
    /// Exposure in EV for HDR formats, applied as `2^EV` before the tone map; 0 on every texture change.
    pub exposure: f32,
    /// The heatmap's full scale: the difference drawn white.
    pub full_scale: f32,
    /// The magnifier lens follows the cursor.
    pub lens: bool,
    /// Repeat the picture 3×3 to check seams.
    pub tile: bool,
    /// The overlay painted over the source pane (Note CL); not saved in `gui.toml`, like the heatmap (AG59).
    pub overlay: Option<crate::model::overlay::OverlayKind>,
    /// Name each face on a cube's cross ("Face labels", cube items only; Note DT16).
    pub face_labels: bool,
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            channel: Channel::Rgba,
            layout: Layout::SideBySide,
            split: 0.5,
            view_linear: false,
            heatmap: false,
            mip: 0,
            zoom: 1.0,
            center: [0.5, 0.5],
            exposure: 0.0,
            full_scale: FULL_SCALE_DEFAULT,
            lens: false,
            tile: false,
            overlay: None,
            face_labels: true,
        }
    }
}

const ZOOM_RANGE: (f32, f32) = (1.0 / 64.0, 64.0);
pub const EXPOSURE_RANGE: (f32, f32) = (-8.0, 8.0);
/// About 25 codes of 255 is white.
pub const FULL_SCALE_DEFAULT: f32 = 0.1;
pub const FULL_SCALE_RANGE: (f32, f32) = (0.005, 0.5);
/// The lens: its width in points and its magnification over the view.
pub const LENS_PX: f32 = 160.0;
pub const LENS_ZOOM: f32 = 8.0;

/// What the shader multiplies an HDR value by before the tone map.
pub fn exposure_factor(ev: f32) -> f32 {
    ev.exp2()
}

/// Where `pos` sits in a pane at `min` of `size`, as 0..1 of each side.
pub fn pane_fraction(pos: [f32; 2], min: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [
        (pos[0] - min[0]) / size[0].max(1.0),
        (pos[1] - min[1]) / size[1].max(1.0),
    ]
}

/// The uv that is sampled for `uv`: itself inside the picture, wrapped into it over the 3×3 when tiled, `None` off
/// the picture. Mirrors the shader.
pub fn tile_uv(uv: [f32; 2], tile: bool) -> Option<[f32; 2]> {
    let (lo, hi) = if tile { (-1.0, 2.0) } else { (0.0, 1.0) };
    if uv.iter().any(|&u| !(lo..hi).contains(&u)) {
        return None;
    }
    Some(if tile { uv.map(|u| u - u.floor()) } else { uv })
}

impl ViewState {
    /// The uv under the point `s` (0..1 of the pane).
    pub fn uv_at(&self, viewport: [f32; 2], mip_size: (u32, u32), s: [f32; 2]) -> [f32; 2] {
        let (offset, scale) = self.uv_rect(viewport, mip_size);
        [offset[0] + s[0] * scale[0], offset[1] + s[1] * scale[1]]
    }

    /// The texel of the shown mip under the point `s` (0..1 of the pane); `None` off the picture.
    pub fn texel_at(
        &self,
        viewport: [f32; 2],
        mip_size: (u32, u32),
        s: [f32; 2],
    ) -> Option<(u32, u32)> {
        let uv = tile_uv(self.uv_at(viewport, mip_size, s), self.tile)?;
        let t = |u: f32, n: u32| ((u * n as f32) as u32).min(n.saturating_sub(1));
        Some((t(uv[0], mip_size.0), t(uv[1], mip_size.1)))
    }

    /// `(uv_offset, uv_scale)` of the lens at the point `s`: [`LENS_PX`] at [`LENS_ZOOM`]× the view, centred on the
    /// texel under the point. Off the picture the texel clamps to its edge; tiled, it wraps.
    pub fn lens_uv(
        &self,
        viewport: [f32; 2],
        mip_size: (u32, u32),
        s: [f32; 2],
    ) -> ([f32; 2], [f32; 2]) {
        let uv = self.uv_at(viewport, mip_size, s);
        let wrapped = if self.tile {
            uv.map(|u| u - u.floor())
        } else {
            uv
        };
        let size = [mip_size.0.max(1) as f32, mip_size.1.max(1) as f32];
        let centre = |u: f32, n: f32| ((u * n).floor().clamp(0.0, n - 1.0) + 0.5) / n;
        let c = [centre(wrapped[0], size[0]), centre(wrapped[1], size[1])];
        let scale = [
            LENS_PX / (LENS_ZOOM * self.zoom * size[0]),
            LENS_PX / (LENS_ZOOM * self.zoom * size[1]),
        ];
        ([c[0] - scale[0] / 2.0, c[1] - scale[1] / 2.0], scale)
    }

    /// `(uv_offset, uv_scale)` mapping the viewport's [0, 1]² to texture uv.
    pub fn uv_rect(&self, viewport: [f32; 2], mip_size: (u32, u32)) -> ([f32; 2], [f32; 2]) {
        let scale = [
            viewport[0] / (self.zoom * mip_size.0 as f32),
            viewport[1] / (self.zoom * mip_size.1 as f32),
        ];
        let offset = [
            self.center[0] - scale[0] / 2.0,
            self.center[1] - scale[1] / 2.0,
        ];
        (offset, scale)
    }

    /// Zoom by `factor` about `cursor_uv`: the texel under the cursor stays where it is on screen.
    pub fn zoom_at(&mut self, factor: f32, cursor_uv: [f32; 2]) {
        let z = (self.zoom * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let f = self.zoom / z;
        for (c, cursor) in self.center.iter_mut().zip(cursor_uv) {
            *c = cursor + (*c - cursor) * f;
        }
        self.zoom = z;
    }

    /// Drag the picture by `delta` screen pixels.
    pub fn pan_pixels(&mut self, delta: [f32; 2], _viewport: [f32; 2], mip_size: (u32, u32)) {
        self.center[0] -= delta[0] / (self.zoom * mip_size.0 as f32);
        self.center[1] -= delta[1] / (self.zoom * mip_size.1 as f32);
    }

    /// Show the whole mip (all nine copies when tiled), centred.
    pub fn fit(&mut self, viewport: [f32; 2], mip_size: (u32, u32)) {
        let n = if self.tile { 3.0 } else { 1.0 };
        let z = (viewport[0] / (n * mip_size.0 as f32)).min(viewport[1] / (n * mip_size.1 as f32));
        if z.is_finite() && z > 0.0 {
            self.zoom = z.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        }
        self.center = [0.5, 0.5];
    }

    /// One screen pixel per texel.
    pub fn one_to_one(&mut self) {
        self.zoom = 1.0;
    }

    /// Keep `mip` within the previewed mips: `first` up to, not including, `end`.
    pub fn clamp_mip(&mut self, first: usize, end: usize) {
        self.mip = self.mip.clamp(first, end.saturating_sub(1).max(first));
    }

    /// Switch to another mip, keeping the same part of the picture in view (the zoom is per texel of the shown mip).
    pub fn set_mip(&mut self, mip: usize, old_size: (u32, u32), new_size: (u32, u32)) {
        self.mip = mip;
        let z = self.zoom * old_size.0 as f32 / new_size.0.max(1) as f32;
        if z.is_finite() && z > 0.0 {
            self.zoom = z.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        }
    }
}

/// |source − decoded| of `channel` (RGB: the largest of the three), as a black → red → yellow → white ramp.
pub fn heatmap(
    source: &Rgba32F,
    decoded: &[[f32; 4]],
    channel: Channel,
    full_scale: f32,
) -> (u32, u32, Vec<u8>) {
    heatmap_between(
        source.width,
        source.height,
        &source.px,
        decoded,
        channel,
        full_scale,
    )
}

/// The same ramp for |a − b| of two `width × height` images (the source against an encode, or the pinned encode
/// against the current one).
pub fn heatmap_between(
    width: u32,
    height: u32,
    a: &[[f32; 4]],
    b: &[[f32; 4]],
    channel: Channel,
    full_scale: f32,
) -> (u32, u32, Vec<u8>) {
    let mut out = Vec::with_capacity(b.len() * 4);
    for (s, d) in a.iter().zip(b) {
        let e: [f32; 4] = std::array::from_fn(|c| (s[c] - d[c]).abs());
        let t = (channel_error(e, channel) / full_scale.max(f32::MIN_POSITIVE)).clamp(0.0, 1.0);
        out.extend(ramp(t));
        out.push(255);
    }
    (width, height, out)
}

/// The error `channel` measures: that channel, alpha for the mask, the largest of R, G and B for RGBA.
pub fn channel_error(e: [f32; 4], channel: Channel) -> f32 {
    match channel {
        Channel::R => e[0],
        Channel::G => e[1],
        Channel::B => e[2],
        Channel::A | Channel::AlphaMask => e[3],
        Channel::Rgba => e[0].max(e[1]).max(e[2]),
    }
}

/// The heatmap colour of `t` (0..1 of full scale): black → red → yellow → white.
pub fn ramp(t: f32) -> [u8; 3] {
    let r = (t * 3.0).clamp(0.0, 1.0);
    let g = (t * 3.0 - 1.0).clamp(0.0, 1.0);
    let b = (t * 3.0 - 2.0).clamp(0.0, 1.0);
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

/// The heatmap of absolute mip `mip` of `out`, against the source it was encoded from. `None` when that mip has no
/// source side (a DDS input) or the sides disagree in size.
pub fn heatmap_of(
    out: &PreviewOut,
    mip: usize,
    channel: Channel,
    full_scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    let i = mip.checked_sub(out.result.source.first_level)?;
    let src = out.result.source.levels.get(i)?;
    let dec = out.decoded.get(i)?;
    (dec.len() == src.px.len()).then(|| heatmap(src, dec, channel, full_scale))
}

/// The heatmap of absolute mip `mip` of `cur` against the same mip of the pinned encode `pin`. `None` when either
/// lacks that mip or the two disagree in size.
pub fn heatmap_vs_pin(
    cur: &PreviewOut,
    pin: &PreviewOut,
    mip: usize,
    channel: Channel,
    full_scale: f32,
) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = *cur.result.source.dims.get(mip)?;
    let a = cur
        .decoded
        .get(mip.checked_sub(cur.result.source.first_level)?)?;
    let b = pin
        .decoded
        .get(mip.checked_sub(pin.result.source.first_level)?)?;
    let same_size = pin.result.source.dims.get(mip) == Some(&(w, h));
    (same_size && a.len() == b.len()).then(|| heatmap_between(w, h, b, a, channel, full_scale))
}

/// One line naming every mip left out because it is above the preview limit.
pub fn skipped_note(dims: &[(u32, u32)], first: usize) -> Option<String> {
    (first > 0).then(|| {
        let names: Vec<String> = dims[..first.min(dims.len())]
            .iter()
            .enumerate()
            .map(|(i, (w, h))| format!("mip {i} ({w}×{h})"))
            .collect();
        format!(
            "{} not previewed: above the preview limit",
            names.join(", ")
        )
    })
}

pub struct Readouts {
    pub lines: Vec<(String, String)>,
}

/// The readouts of M3a spec §6.4 for absolute mip `mip`. The validation lists index from the first mip kept.
pub fn readouts(out: &PreviewOut, mip: usize) -> Readouts {
    let r = &out.result;
    let v = &r.validation;
    let i = mip.saturating_sub(r.source.first_level);
    let mut lines = vec![];
    if let Some(last) = v.alpha.last() {
        lines.push(("Smallest-mip alpha".into(), last.min.to_string()));
    }
    if let Some(a) = v.alpha.get(i) {
        let share = 1.0 - a.eq255 as f64 / a.pixels.max(1) as f64;
        lines.push(("α ≠ 255".into(), format!("{:.1} %", share * 100.0)));
    }
    if let Some(c) = v.coverage.get(i) {
        lines.push(("Coverage".into(), format!("{:.1} %", c * 100.0)));
    }
    if let Some(n) = r.normal_len.get(i) {
        let text = if n.mean_err.is_nan() || n.max_err.is_nan() {
            "—".to_string()
        } else {
            format!("mean {:.4}, max {:.4}", n.mean_err, n.max_err)
        };
        lines.push(("Normal length error".into(), text));
    }
    if let Some(p) = v.bc6h_peak_ratio {
        lines.push(("BC6H peak ratio".into(), format!("{p:.3}")));
    }
    if let Some(p) = v.min_mip_psnr_db {
        lines.push(("Minimum mip PSNR".into(), format!("{p:.1} dB")));
    }
    Readouts { lines }
}
