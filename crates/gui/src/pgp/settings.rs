//! A set's (or the defaults') JSON settings, by schema key path (`glint.screen_space_scale`). Colours are 0–255
//! integer arrays here; the writer converts them (DECISIONS P10). Features and their mutual exclusion (spec §5.4).
use serde_json::Value;

pub type Settings = std::collections::BTreeMap<String, Value>;

/// Glint values a freshly enabled glint starts from (spec §4.3; TruePBR-Manager's widget values, which it then
/// overwrote with zeros).
pub const GLINT_DEFAULTS: [(&str, f64); 4] = [
    ("glint.screen_space_scale", 1.5),
    ("glint.log_microfacet_density", 40.0),
    ("glint.microfacet_roughness", 0.015),
    ("glint.density_randomization", 2.0),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Feature {
    Emissive,
    Parallax,
    Subsurface,
    Hair,
    Multilayer,
    Glint,
    Fuzz,
}

/// The settings key that turns each feature on.
pub const FEATURE_KEYS: [(Feature, &str); 7] = [
    (Feature::Emissive, "emissive"),
    (Feature::Parallax, "parallax"),
    (Feature::Subsurface, "subsurface"),
    (Feature::Hair, "hair"),
    (Feature::Multilayer, "multilayer"),
    (Feature::Glint, "glint"),
    (Feature::Fuzz, "fuzz"),
];

pub fn feature_key(f: Feature) -> &'static str {
    FEATURE_KEYS
        .iter()
        .find(|(g, _)| *g == f)
        .map(|(_, k)| *k)
        .expect("every feature has a key")
}

pub fn feature_on(s: &Settings, f: Feature) -> bool {
    s.get(feature_key(f))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Pairs that cannot both be on (owner, 2026-10-10). The relation is symmetric.
const EXCLUSIVE: [(Feature, Feature); 10] = {
    use Feature::*;
    [
        (Hair, Multilayer),
        (Hair, Glint),
        (Hair, Fuzz),
        (Hair, Emissive),
        (Hair, Parallax),
        (Hair, Subsurface),
        (Multilayer, Subsurface),
        (Multilayer, Glint),
        (Multilayer, Fuzz),
        (Glint, Fuzz),
    ]
};

/// The feature, currently on, that rules `f` out; `None` when `f` may be turned on. The control for `f` is greyed
/// with "off: <feature> is on" while this is `Some`.
pub fn blocked_by(s: &Settings, f: Feature) -> Option<Feature> {
    EXCLUSIVE.iter().find_map(|&(a, b)| {
        if a == f && feature_on(s, b) {
            Some(b)
        } else if b == f && feature_on(s, a) {
            Some(a)
        } else {
            None
        }
    })
}

/// `own` over `defaults` (the JSON `default` block applies to every entry that does not set the key).
pub fn effective(defaults: &Settings, own: &Settings) -> Settings {
    let mut out = defaults.clone();
    out.extend(own.iter().map(|(k, v)| (k.clone(), v.clone())));
    out
}
