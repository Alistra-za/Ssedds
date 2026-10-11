//! The PGPatcher PBR schema keys and how the GUI treats each (M3b spec §5.1–5.2). `KEYS` lists the entry keys in
//! schema order (the writer writes in this order), then the nested `glint.*` and `fuzz.*` keys.
pub const SCHEMA_JSON: &str = include_str!("../../assets/pbr.schema.json");
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/hakasapl/PGPatcher/main/schema/pbr.schema.json";

/// Never written: removed by PGPatcher 2.0.0.
pub const REMOVED: &[&str] = &["pbr", "env_mapping", "env_map_scale", "env_map_scale_mult"];
/// Never written: `delete` is a later round (spec §9); `texture` is the old name of `match_diffuse`.
pub const NOT_EXPOSED: &[&str] = &["texture", "delete"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// On/off, written only when on. The fixed `default` block supplies `false` for emissive, parallax, subsurface.
    OnlyTrue,
    /// Not set / on / off; written only when set.
    Tri,
    Number,
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
    Locks,
    Uv,
    Slots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    pub key: &'static str,
    pub kind: KeyKind,
    pub group: Group,
    /// Shown in the Advanced drawer rather than the main view.
    pub advanced: bool,
    pub label: &'static str,
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
    }
}

use Group as G;
use KeyKind as K;

pub const KEYS: &[KeySpec] = &[
    k("match_diffuse", K::Text, G::Match, false, "Match diffuse"),
    k("match_normal", K::Text, G::Match, true, "Match normal"),
    k("path_contains", K::Text, G::Match, true, "Path contains"),
    k("match1", K::Text, G::Match, true, "Match slot 1 (diffuse)"),
    k("match2", K::Text, G::Match, true, "Match slot 2 (normal)"),
    k("match3", K::Text, G::Match, true, "Match slot 3 (glow)"),
    k("match4", K::Text, G::Match, true, "Match slot 4 (parallax)"),
    k("match5", K::Text, G::Match, true, "Match slot 5 (cubemap)"),
    k("match6", K::Text, G::Match, true, "Match slot 6 (env mask)"),
    k(
        "match7",
        K::Text,
        G::Match,
        true,
        "Match slot 7 (multilayer)",
    ),
    k(
        "match8",
        K::Text,
        G::Match,
        true,
        "Match slot 8 (backlight)",
    ),
    k("nif_filter", K::Text, G::Match, true, "Mesh path contains"),
    k("rename", K::Text, G::Match, false, "Rename"),
    k("smooth_angle", K::Number, G::Uv, true, "Smooth angle"),
    k("auto_uv", K::Number, G::Uv, true, "Auto UV"),
    k("uv_scale", K::Number, G::Uv, true, "UV scale"),
    k("vertex_colors", K::Tri, G::Vertex, true, "Vertex colours"),
    k(
        "vertex_color_lum_mult",
        K::Number,
        G::Vertex,
        true,
        "Vertex colour luminance ×",
    ),
    k(
        "vertex_color_sat_mult",
        K::Number,
        G::Vertex,
        true,
        "Vertex colour saturation ×",
    ),
    k("zbuffer_write", K::Tri, G::Vertex, true, "Z-buffer write"),
    k("lock_diffuse", K::Tri, G::Locks, true, "Lock diffuse"),
    k("lock_normal", K::Tri, G::Locks, true, "Lock normal"),
    k("emissive", K::OnlyTrue, G::Features, false, "Emissive"),
    k("lock_emissive", K::Tri, G::Locks, true, "Lock emissive"),
    k(
        "emissive_scale",
        K::Number,
        G::Emissive,
        false,
        "Emissive scale",
    ),
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
        K::Number,
        G::Parallax,
        false,
        "Displacement scale",
    ),
    k("cubemap", K::Text, G::Uv, true, "Cubemap path"),
    k("lock_cubemap", K::Tri, G::Locks, true, "Lock cubemap"),
    k("lock_rmaos", K::Tri, G::Locks, true, "Lock RMAOS"),
    k(
        "specular_level",
        K::Number,
        G::Material,
        false,
        "Specular level",
    ),
    k(
        "roughness_scale",
        K::Number,
        G::Material,
        false,
        "Roughness scale",
    ),
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
        K::Number,
        G::Subsurface,
        false,
        "Subsurface opacity",
    ),
    k("lock_subsurface", K::Tri, G::Locks, true, "Lock subsurface"),
    k("hair", K::OnlyTrue, G::Features, false, "Hair"),
    k(
        "multilayer",
        K::OnlyTrue,
        G::Features,
        false,
        "Multilayer (coat)",
    ),
    k("coat_color", K::Rgb, G::Multilayer, true, "Coat colour"),
    k(
        "coat_specular_level",
        K::Number,
        G::Multilayer,
        false,
        "Coat specular level",
    ),
    k(
        "coat_roughness",
        K::Number,
        G::Multilayer,
        false,
        "Coat roughness",
    ),
    k(
        "coat_strength",
        K::Number,
        G::Multilayer,
        false,
        "Coat strength",
    ),
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
    k(
        "inner_uv_scale",
        K::Number,
        G::Multilayer,
        true,
        "Inner UV scale",
    ),
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
        K::Number,
        G::Glint,
        false,
        "Screen-space scale",
    ),
    k(
        "glint.log_microfacet_density",
        K::Number,
        G::Glint,
        false,
        "Log microfacet density",
    ),
    k(
        "glint.microfacet_roughness",
        K::Number,
        G::Glint,
        false,
        "Microfacet roughness",
    ),
    k(
        "glint.density_randomization",
        K::Number,
        G::Glint,
        false,
        "Density randomisation",
    ),
    k(
        "fuzz.texture",
        K::OnlyTrue,
        G::Fuzz,
        false,
        "Fuzz texture (_f)",
    ),
    k("fuzz.color", K::Rgb, G::Fuzz, false, "Fuzz colour"),
    k("fuzz.weight", K::Number, G::Fuzz, false, "Fuzz weight"),
];

pub fn spec(key: &str) -> Option<&'static KeySpec> {
    KEYS.iter().find(|k| k.key == key)
}
