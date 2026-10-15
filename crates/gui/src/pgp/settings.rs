//! A set's JSON settings, by schema key path (`glint.screen_space_scale`). Colours are float arrays (AG133;
//! older 0–255 integer arrays read as value/255, `rgb_of`). Features and their mutual exclusion (spec §5.4).
use serde_json::Value;

pub type Settings = std::collections::BTreeMap<String, Value>;

/// Glint values a freshly enabled glint starts from (spec §4.3; TruePBR-Manager's widget values, which it then
/// overwrote with zeros). Three moved in Task 14b (DECISIONS AG123): a value outside the observed p1-p99 band is now the
/// observed median.
pub const GLINT_DEFAULTS: [(&str, f64); 4] = [
    ("glint.screen_space_scale", 1.5),
    ("glint.log_microfacet_density", 10.0),
    ("glint.microfacet_roughness", 1.0),
    ("glint.density_randomization", 5.0),
];

/// What a freshly enabled Fuzz starts from (AG133, owner 2026-10-15). In the owner's mod JSONs `fuzz.color` is
/// [0.08, 0.08, 0.08] 231 times, [0.04, 0.04, 0.04] 110 times and [1.25, 1.25, 1.25] 39 times.
pub const FUZZ_COLOR_DEFAULT: [f64; 3] = [0.08, 0.08, 0.08];
pub const FUZZ_WEIGHT_DEFAULT: f64 = 1.0;

/// The value an unset number key of an enabled feature object starts from (the card shows it, the writer writes it).
pub fn number_default(key: &str) -> Option<f64> {
    GLINT_DEFAULTS
        .iter()
        .chain(&[("fuzz.weight", FUZZ_WEIGHT_DEFAULT)])
        .find(|(k, _)| *k == key)
        .map(|(_, x)| *x)
}

/// The colour an unset colour key starts from: only `fuzz.color` has one.
pub fn rgb_default(key: &str) -> Option<[f64; 3]> {
    (key == "fuzz.color").then_some(FUZZ_COLOR_DEFAULT)
}

/// The largest a colour channel may be (AG133): above 1.0 is real (fuzz.color 1.25 in 39 mod JSONs).
pub const RGB_MAX: f64 = 10.0;

/// A stored colour's channels as floats (AG133). Colours are stored as floats now; a colour whose channels are all
/// integers is the 0-255 form older projects stored, and reads as value/255. `None`: not an array of three or more.
pub fn rgb_of(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array().filter(|a| a.len() >= 3)?;
    let legacy = rgb_is_legacy(v);
    Some([0, 1, 2].map(|i| {
        let x = a[i].as_f64().unwrap_or(0.0);
        if legacy {
            x / 255.0
        } else {
            x
        }
    }))
}

/// Every channel is an integer: the 0-255 form of projects before AG133.
pub fn rgb_is_legacy(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|a| a.iter().take(3).all(|c| c.is_i64() || c.is_u64()))
}

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
    (Feature::Glint, "glint_on"),
    (Feature::Fuzz, "fuzz_on"),
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

/// The TruePBR card's Subsurface selector (owner, 2026-10-14; DECISIONS AG101): `subsurface` is Standard,
/// `subsurface_foliage` is Foliage, and both stored is a conflict that is shown, never rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubMode {
    Off,
    Standard,
    Foliage,
    Conflict,
}

pub const FOLIAGE_KEY: &str = "subsurface_foliage";

pub fn sub_mode(s: &Settings) -> SubMode {
    let on = |k| s.get(k).and_then(Value::as_bool) == Some(true);
    match (on("subsurface"), on(FOLIAGE_KEY)) {
        (false, false) => SubMode::Off,
        (true, false) => SubMode::Standard,
        (false, true) => SubMode::Foliage,
        (true, true) => SubMode::Conflict,
    }
}

/// Whether the "Subsurface settings" group (colour, opacity) applies: both kinds read `SubsurfaceColor` and
/// `Thickness` (CS `PBR.hlsli:174-187`). Hair and multilayer rule it out.
pub fn subsurface_group_open(s: &Settings) -> bool {
    sub_mode(s) != SubMode::Off && blocked_by(s, Feature::Subsurface).is_none()
}

/// The settings edits for choosing `mode` (`None` removes the key). It never stores both keys. Off stores an
/// explicit `subsurface: false` while the set has an `_s` (`auto_s`), so the auto value stays off (AJ8); the writer
/// still writes no `false` (OnlyTrue).
pub fn sub_edits(mode: SubMode, auto_s: bool) -> Vec<(&'static str, Option<Value>)> {
    let t = Some(Value::Bool(true));
    match mode {
        SubMode::Standard => vec![("subsurface", t), (FOLIAGE_KEY, None)],
        SubMode::Foliage => vec![(FOLIAGE_KEY, t), ("subsurface", None)],
        _ => vec![
            ("subsurface", auto_s.then_some(Value::Bool(false))),
            (FOLIAGE_KEY, None),
        ],
    }
}
