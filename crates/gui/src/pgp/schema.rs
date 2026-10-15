//! The PGPatcher PBR schema keys and how the GUI treats each (M3b spec §5.1–5.2). `KEYS` lists the entry keys in
//! schema order (the writer writes in this order), then the nested `glint.*` and `fuzz.*` keys.
pub const SCHEMA_JSON: &str = include_str!("../../assets/pbr.schema.json");
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/hakasapl/PGPatcher/main/schema/pbr.schema.json";

/// Never written: removed by PGPatcher 2.0.0.
pub const REMOVED: &[&str] = &["pbr", "env_mapping", "env_map_scale", "env_map_scale_mult"];
/// Not offered: `delete` is a later round (spec §9); `texture`, `match_diffuse` and `match_normal` are the partial
/// ends-with matches, no longer offered (M6 F1 spec §3.1, DECISIONS V2). The writer adds `match_diffuse` itself next to a
/// rename (DECISIONS AA8).
pub const NOT_EXPOSED: &[&str] = &["texture", "delete", "match_diffuse", "match_normal"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// On/off, written only when on. The fixed `default` block supplies `false` for emissive, parallax, subsurface.
    OnlyTrue,
    /// Not set / on / off; written only when set.
    Tri,
    /// A number, kept inside its range (F1 spec §2, DECISIONS V5).
    Number(NumRange),
    Text,
    /// 0–255 RGB in the GUI; written as 0–1.
    Rgb,
    /// As `Rgb`, written with a fourth component 1.0.
    Rgba,
    /// A slot command (`slotN`).
    Slot,
    /// `glint` / `fuzz`: on/off; when on, the object with its `<key>.*` values is written.
    Object,
}

/// The values sdc lets a number key take (F1 spec §2, DECISIONS V5). Neither the CS True PBR wiki nor PGPatcher
/// documents limits; these keep values from going negative, and the 0–1 factors from going above 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumRange {
    /// 0 or more: roughness values, multipliers and levels, and every key that fits no class.
    Min0,
    /// 0 to 1: strengths, weights, opacities.
    Unit,
    /// No bounds: `glint.log_microfacet_density` is a log value and may be negative (controller ruling).
    Any,
    /// A whole number from 0 to this maximum: the `alphaprop_*` keys (DECISIONS AG27). Written as a JSON integer.
    Int(u32),
}

impl NumRange {
    pub fn bounds(self) -> (f64, f64) {
        match self {
            NumRange::Min0 => (0.0, f64::INFINITY),
            NumRange::Unit => (0.0, 1.0),
            NumRange::Any => (f64::NEG_INFINITY, f64::INFINITY),
            NumRange::Int(max) => (0.0, f64::from(max)),
        }
    }

    pub fn clamp(self, x: f64) -> f64 {
        let (lo, hi) = self.bounds();
        x.clamp(lo, hi)
    }

    /// The note a typed value outside the range shows next to its box.
    pub fn note(self) -> String {
        match self {
            NumRange::Min0 => "clamped to ≥ 0".into(),
            NumRange::Unit => "clamped to 0–1".into(),
            NumRange::Any => String::new(),
            NumRange::Int(max) => format!("clamped to a whole number 0-{max}"),
        }
    }

    /// A stored `alphaprop_*` value as a whole number 0 to `max`: JSON `4844.0` reads as 4844 (the writer and the
    /// validator accept it); a negative, fractional or too large one is `None` (shown as invalid, not as "not set").
    pub fn whole(v: &serde_json::Value, max: u32) -> Option<u32> {
        let x = v.as_f64()?;
        (x.fract() == 0.0 && (0.0..=f64::from(max)).contains(&x)).then_some(x as u32)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Match,
    Material,
    Features,
    Emissive,
    Parallax,
    Subsurface,
    Multilayer,
    Glint,
    Fuzz,
    Vertex,
    /// The NiAlphaProperty keys; the card draws them as one section, not as a key grid.
    Alpha,
    Locks,
    Uv,
    Slots,
}

/// How a number key drags and reads (Task 14b, DECISIONS AG121): `speed` per pixel, `decimals` shown, and `log` when
/// the useful values span orders of magnitude. egui 0.36's `DragValue` has no logarithmic mode, so a log key scales
/// its speed with the value instead (`speed_at`). Only the feel and the display change: the range clamps and the
/// stored values are the key's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragSpec {
    pub speed: f64,
    pub decimals: usize,
    pub log: bool,
}

impl DragSpec {
    pub const fn new(speed: f64, decimals: usize, log: bool) -> Self {
        Self {
            speed,
            decimals,
            log,
        }
    }

    /// The speed at value `v`: a log key drags at least 1% of its value per pixel, never below `speed`.
    pub fn speed_at(self, v: f64) -> f64 {
        if self.log {
            self.speed.max(v.abs() * 0.01)
        } else {
            self.speed
        }
    }

    /// The value as the box shows it: at most `decimals` places, no trailing zeros beyond one (`1.0`, `0.04`).
    pub fn format(self, v: f64) -> String {
        if self.decimals == 0 {
            return format!("{v:.0}");
        }
        let p = 10f64.powi(self.decimals as i32);
        format!("{:?}", (v * p).round() / p)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeySpec {
    pub key: &'static str,
    pub kind: KeyKind,
    pub group: Group,
    /// Shown in the Advanced drawer rather than the main view.
    pub advanced: bool,
    pub label: &'static str,
    /// Drag speed and display of a number key; unused by the other kinds.
    pub drag: DragSpec,
}

impl KeySpec {
    pub const fn with(mut self, drag: DragSpec) -> Self {
        self.drag = drag;
        self
    }
}

const fn k(
    key: &'static str,
    kind: KeyKind,
    group: Group,
    advanced: bool,
    label: &'static str,
) -> KeySpec {
    KeySpec {
        key,
        kind,
        group,
        advanced,
        label,
        drag: DragSpec::new(0.01, 4, false),
    }
}

use Group as G;
use KeyKind as K;
const MIN0: KeyKind = K::Number(NumRange::Min0);
const UNIT: KeyKind = K::Number(NumRange::Unit);
const ANY: KeyKind = K::Number(NumRange::Any);

pub const KEYS: &[KeySpec] = &[
    k("path_contains", K::Text, G::Match, true, "Path contains"),
    // match1–match8 are drawn by the Matching header only (F1 spec §3.2), so no group shows them.
    k("match1", K::Text, G::Match, false, "Match slot 1 (diffuse)"),
    k("match2", K::Text, G::Match, false, "Match slot 2 (normal)"),
    k("match3", K::Text, G::Match, false, "Match slot 3 (glow)"),
    k(
        "match4",
        K::Text,
        G::Match,
        false,
        "Match slot 4 (parallax)",
    ),
    k("match5", K::Text, G::Match, false, "Match slot 5 (cubemap)"),
    k(
        "match6",
        K::Text,
        G::Match,
        false,
        "Match slot 6 (env mask)",
    ),
    k(
        "match7",
        K::Text,
        G::Match,
        false,
        "Match slot 7 (multilayer)",
    ),
    k(
        "match8",
        K::Text,
        G::Match,
        false,
        "Match slot 8 (backlight)",
    ),
    k("nif_filter", K::Text, G::Match, true, "Mesh path contains"),
    k("rename", K::Text, G::Match, true, "Rename"),
    k("smooth_angle", MIN0, G::Uv, true, "Smooth angle").with(DragSpec::new(0.5, 1, false)),
    // shortcut: no field data, retune when seen
    k("auto_uv", MIN0, G::Uv, true, "Auto UV").with(DragSpec::new(0.01, 3, false)),
    // shortcut: no field data, retune when seen
    k("uv_scale", MIN0, G::Uv, true, "UV scale").with(DragSpec::new(0.01, 3, true)),
    k("vertex_colors", K::Tri, G::Vertex, true, "Vertex colours"),
    k(
        "vertex_color_lum_mult",
        MIN0,
        G::Vertex,
        true,
        "Vertex colour luminance ×",
    )
    .with(DragSpec::new(0.005, 3, false)),
    k(
        "vertex_color_sat_mult",
        MIN0,
        G::Vertex,
        true,
        "Vertex colour saturation ×",
    )
    .with(DragSpec::new(0.005, 2, false)),
    k("zbuffer_write", K::Tri, G::Vertex, true, "Z-buffer write"),
    // shortcut: no field data, retune when seen
    k(
        "alphaprop_flags",
        K::Number(NumRange::Int(65535)),
        G::Alpha,
        false,
        "Alpha flags",
    )
    .with(DragSpec::new(16.0, 0, false)),
    // shortcut: no field data, retune when seen
    k(
        "alphaprop_thresh",
        K::Number(NumRange::Int(255)),
        G::Alpha,
        false,
        "Alpha test threshold",
    )
    .with(DragSpec::new(1.0, 0, false)),
    k(
        "alphaprop_create",
        K::OnlyTrue,
        G::Alpha,
        false,
        "Create NiAlphaProperty if missing",
    ),
    k(
        "alphaprop_delete",
        K::OnlyTrue,
        G::Alpha,
        false,
        "Delete NiAlphaProperty",
    ),
    k("lock_diffuse", K::Tri, G::Locks, true, "Lock diffuse"),
    k("lock_normal", K::Tri, G::Locks, true, "Lock normal"),
    k("emissive", K::OnlyTrue, G::Features, false, "Emissive"),
    k("lock_emissive", K::Tri, G::Locks, true, "Lock emissive"),
    k("emissive_scale", MIN0, G::Emissive, false, "Emissive scale")
        .with(DragSpec::new(0.02, 2, true)),
    k(
        "emissive_color",
        K::Rgba,
        G::Emissive,
        false,
        "Emissive colour",
    ),
    k("parallax", K::OnlyTrue, G::Features, false, "Parallax"),
    k("lock_parallax", K::Tri, G::Locks, true, "Lock parallax"),
    k(
        "displacement_scale",
        MIN0,
        G::Parallax,
        false,
        "Displacement scale",
    )
    .with(DragSpec::new(0.01, 2, true)),
    k("cubemap", K::Text, G::Uv, true, "Cubemap path"),
    k("lock_cubemap", K::Tri, G::Locks, true, "Lock cubemap"),
    k("lock_rmaos", K::Tri, G::Locks, true, "Lock RMAOS"),
    k("specular_level", MIN0, G::Material, false, "Specular level")
        .with(DragSpec::new(0.001, 5, true)),
    k(
        "roughness_scale",
        MIN0,
        G::Material,
        false,
        "Roughness scale",
    )
    .with(DragSpec::new(0.01, 2, false)),
    k("subsurface", K::OnlyTrue, G::Features, false, "Subsurface"),
    k(
        "subsurface_foliage",
        K::OnlyTrue,
        G::Subsurface,
        false,
        "Foliage subsurface",
    ),
    k(
        "subsurface_color",
        K::Rgb,
        G::Subsurface,
        false,
        "Subsurface colour",
    ),
    k(
        "subsurface_opacity",
        UNIT,
        G::Subsurface,
        false,
        "Subsurface opacity",
    )
    .with(DragSpec::new(0.005, 2, false)),
    k("lock_subsurface", K::Tri, G::Locks, true, "Lock subsurface"),
    k("hair", K::OnlyTrue, G::Features, false, "Hair"),
    k(
        "multilayer",
        K::OnlyTrue,
        G::Features,
        false,
        "Multilayer (coat)",
    ),
    k("coat_color", K::Rgb, G::Multilayer, false, "Coat colour"),
    k(
        "coat_specular_level",
        MIN0,
        G::Multilayer,
        false,
        "Coat specular level",
    )
    .with(DragSpec::new(0.001, 4, true)),
    k(
        "coat_roughness",
        MIN0,
        G::Multilayer,
        false,
        "Coat roughness",
    )
    .with(DragSpec::new(0.005, 2, false)),
    k("coat_strength", UNIT, G::Multilayer, false, "Coat strength")
        .with(DragSpec::new(0.005, 2, false)),
    k(
        "coat_diffuse",
        K::OnlyTrue,
        G::Multilayer,
        false,
        "Coat diffuse",
    ),
    k(
        "coat_normal",
        K::OnlyTrue,
        G::Multilayer,
        false,
        "Coat normal (_cnr)",
    ),
    k(
        "coat_parallax",
        K::OnlyTrue,
        G::Multilayer,
        false,
        "Coat parallax",
    ),
    k("lock_cnr", K::Tri, G::Locks, true, "Lock coat normal"),
    // shortcut: no field data, retune when seen
    k(
        "inner_uv_scale",
        MIN0,
        G::Multilayer,
        false,
        "Inner UV scale",
    )
    .with(DragSpec::new(0.01, 3, true)),
    k("glint", K::Object, G::Features, false, "Glint"),
    k("fuzz", K::Object, G::Features, false, "Fuzz"),
    k("slot1", K::Slot, G::Slots, false, "Slot 1"),
    k("slot2", K::Slot, G::Slots, false, "Slot 2"),
    k("slot3", K::Slot, G::Slots, false, "Slot 3"),
    k("slot4", K::Slot, G::Slots, false, "Slot 4"),
    k("slot5", K::Slot, G::Slots, false, "Slot 5"),
    k("slot6", K::Slot, G::Slots, false, "Slot 6"),
    k("slot7", K::Slot, G::Slots, false, "Slot 7"),
    k("slot8", K::Slot, G::Slots, false, "Slot 8"),
    k(
        "glint.screen_space_scale",
        MIN0,
        G::Glint,
        false,
        "Screen-space scale",
    )
    .with(DragSpec::new(0.02, 2, false)),
    k(
        "glint.log_microfacet_density",
        ANY,
        G::Glint,
        false,
        "Log microfacet density",
    )
    .with(DragSpec::new(0.1, 3, false)),
    k(
        "glint.microfacet_roughness",
        MIN0,
        G::Glint,
        false,
        "Microfacet roughness",
    )
    .with(DragSpec::new(0.005, 3, true)),
    k(
        "glint.density_randomization",
        MIN0,
        G::Glint,
        false,
        "Density randomisation",
    )
    .with(DragSpec::new(1.0, 3, true)),
    k(
        "fuzz.texture",
        K::OnlyTrue,
        G::Fuzz,
        false,
        "Fuzz texture (_f)",
    ),
    k("fuzz.color", K::Rgb, G::Fuzz, false, "Fuzz colour"),
    k("fuzz.weight", UNIT, G::Fuzz, false, "Fuzz weight").with(DragSpec::new(0.005, 2, false)),
];

pub fn spec(key: &str) -> Option<&'static KeySpec> {
    KEYS.iter().find(|k| k.key == key)
}
