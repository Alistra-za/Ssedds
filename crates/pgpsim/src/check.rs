//! PGPatcher 2.x reads a patch key only when it has its JSON type and ignores it otherwise [Src 2.2.0 TP:52-57,
//! 744-1235]; the accessors in `json.rs` do that. The match keys are still read with nlohmann `get<std::string>()`,
//! which throws [Src 2.2.0 TP:242, 256, 265, 280, 534, 594, 608-609, 618]: those are the only type failures left.
use crate::json::{flag, GLINT_KEYS};
use serde_json::{Map, Value};

/// The number keys, for `range_warnings` only.
const NUM_KEYS: &[&str] = &[
    "smooth_angle",
    "auto_uv",
    "vertex_color_lum_mult",
    "vertex_color_sat_mult",
    "specular_level",
    "roughness_scale",
    "subsurface_opacity",
    "displacement_scale",
    "emissive_scale",
    "uv_scale",
    "coat_specular_level",
    "coat_roughness",
    "coat_strength",
    "inner_uv_scale",
];

/// The match keys 2.x still reads without a type check.
const MATCH_KEYS: &[&str] = &[
    "texture",
    "match_normal",
    "match_diffuse",
    "rename",
    "path_contains",
    "nif_filter",
    "match1",
    "match2",
    "match3",
    "match4",
    "match5",
    "match6",
    "match7",
    "match8",
];

fn crashes(k: &str, t: &str) -> String {
    format!("{}: {k} is not a {t}", crate::CRASHES)
}

/// The first match key that is not a string (a load or `shouldApply` throw).
pub(crate) fn match_key_error(e: &Map<String, Value>) -> Option<String> {
    MATCH_KEYS
        .iter()
        .find(|k| e.get(**k).is_some_and(|v| !v.is_string()))
        .map(|k| crashes(k, "string"))
}

/// A multilayer entry's `coat_color` that is an object of 3+ keys: `size() >= 3` passes, then `[0]` throws
/// [Src 2.2.0 TP:1108-1110; measured T6].
pub(crate) fn coat_error(e: &Map<String, Value>) -> Option<String> {
    let throws = flag(e, "multilayer")
        && e.get("coat_color")
            .and_then(Value::as_object)
            .is_some_and(|o| o.len() >= 3);
    throws.then(|| crashes("coat_color", "array"))
}

/// PGPatcher 2.2 `alphaprop_*` whole-number keys and their maxima. Never in `NUM_KEYS`: 2.2 ignores a bad value
/// (`unsignedFromJSON`) where 1.2.0 threw, so they never make a `Skip`.
const INT_KEYS: [(&str, u32); 2] = [("alphaprop_flags", 65535), ("alphaprop_thresh", 255)];

/// PGPatcher 2.2 `unsignedFromJSON`: `Some` only for a JSON number that is a whole value in 0..=max (an integral
/// float counts). Booleans, strings, fractions, negatives and out-of-range values are `None` (ignored, never thrown).
pub(crate) fn uint_in(v: &Value, max: u32) -> Option<u32> {
    if !v.is_number() {
        return None;
    }
    v.as_f64()
        .filter(|f| f.fract() == 0.0 && (0.0..=f64::from(max)).contains(f))
        .map(|f| f as u32)
}

/// The 0–1 factors among the number keys (sdc M6 F1 spec §2). Mirrors `NumRange::Unit` in sdc-gui's `pgp/schema.rs`;
/// the GUI test `pgpsim_warns_on_exactly_the_gui_ranges` keeps the two in step.
const UNIT_KEYS: [&str; 3] = ["subsurface_opacity", "coat_strength", "fuzz.weight"];

/// sdc's value ranges as warnings (sdc M6 F1 spec §2): every number key below 0, and a 0–1 factor above 1. PGPatcher
/// itself has no limits, so this is never a `Skip`. Values of the wrong type are `check_types`' business.
/// `glint.log_microfacet_density` (a log value) and the removed `env_map_*` keys have no range.
pub fn range_warnings(entry: &Value) -> Vec<String> {
    let Some(e) = entry.as_object() else {
        return vec![];
    };
    let mut nums: Vec<(String, f64)> = NUM_KEYS
        .iter()
        .filter_map(|k| Some((k.to_string(), e.get(*k)?.as_f64()?)))
        .collect();
    for (obj, keys) in [("glint", &GLINT_KEYS[..]), ("fuzz", &["weight"][..])] {
        if let Some(o) = e.get(obj).and_then(Value::as_object) {
            nums.extend(
                keys.iter()
                    .filter(|k| **k != "log_microfacet_density")
                    .filter_map(|k| Some((format!("{obj}.{k}"), o.get(*k)?.as_f64()?))),
            );
        }
    }
    let mut out: Vec<String> = nums
        .into_iter()
        .filter_map(|(k, x)| {
            let unit = UNIT_KEYS.contains(&k.as_str());
            let hi = if unit { 1.0 } else { f64::INFINITY };
            (!(0.0..=hi).contains(&x)).then(|| {
                let range = if unit { "0–1" } else { "≥ 0" };
                format!("{k} is {x}, outside {range}")
            })
        })
        .collect();
    // A number PGPatcher 2.2 would ignore; strings and booleans are ignored too but are not sdc's range business.
    for (k, max) in INT_KEYS {
        if let Some(v) = e
            .get(k)
            .filter(|v| v.is_number() && uint_in(v, max).is_none())
        {
            out.push(format!(
                "{k} is {v}, outside 0–{max} (PGPatcher ignores it)"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{range_warnings, uint_in};
    use serde_json::json;

    #[test]
    fn ranges_warn_below_zero_and_above_one_for_the_factors() {
        assert!(range_warnings(
            &json!({"roughness_scale": 0.0, "coat_strength": 1.0, "fuzz": {"weight": 0.5}})
        )
        .is_empty());
        let w = range_warnings(&json!({
            "roughness_scale": -1, "coat_strength": 1.5, "specular_level": 9.0,
            "glint": {"microfacet_roughness": -0.1, "log_microfacet_density": -4}, "fuzz": {"weight": 2}
        }));
        assert_eq!(w.len(), 4, "{w:?}");
        assert!(w
            .iter()
            .any(|m| m.contains("coat_strength") && m.contains("0–1")));
        assert!(w
            .iter()
            .any(|m| m.contains("glint.microfacet_roughness") && m.contains("≥ 0")));
        assert!(
            range_warnings(&json!({"roughness_scale": "x"})).is_empty(),
            "a wrong type is check_types' business"
        );
        assert!(range_warnings(&json!(3)).is_empty());
        // Final review Minor 9: the removed env_map_* keys have no range.
        assert!(range_warnings(&json!({"env_map_scale": -1, "env_map_scale_mult": -2})).is_empty());
    }

    #[test]
    fn uint_in_is_unsigned_from_json() {
        assert_eq!(uint_in(&json!(0), 255), Some(0));
        assert_eq!(uint_in(&json!(255), 255), Some(255));
        assert_eq!(
            uint_in(&json!(8.0), 255),
            Some(8),
            "an integral float counts"
        );
        for bad in [
            json!(256),
            json!(-1),
            json!(1.5),
            json!(-0.5),
            json!("8"),
            json!(true),
            json!(null),
        ] {
            assert_eq!(uint_in(&bad, 255), None, "{bad}");
        }
    }

    #[test]
    fn alphaprop_numbers_outside_their_range_warn_and_other_types_do_not() {
        let w = range_warnings(&json!({"alphaprop_thresh": 300}));
        assert_eq!(
            w,
            ["alphaprop_thresh is 300, outside 0–255 (PGPatcher ignores it)"]
        );
        assert_eq!(range_warnings(&json!({"alphaprop_flags": -1})).len(), 1);
        assert_eq!(range_warnings(&json!({"alphaprop_flags": 1.5})).len(), 1);
        assert!(
            range_warnings(&json!({"alphaprop_flags": 65535, "alphaprop_thresh": 0})).is_empty()
        );
        assert!(
            range_warnings(&json!({"alphaprop_thresh": "8", "alphaprop_flags": true})).is_empty()
        );
    }
}
