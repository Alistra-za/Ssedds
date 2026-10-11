//! The slot rewrite (`applyOnePatchSlots`, 01 §2.3 slot table) and the missing-texture check (01 §1.4).

use crate::json::flag;
use serde_json::{Map, Value};

/// `applyOnePatchSlots` [Src 1.2.0 TP:865-965]. `slots` are PGPatcher's lower-cased strings; `mp` is the
/// entry's `matchedPath` (an empty one changes nothing).
pub(crate) fn apply_slots(slots: &mut [String; 9], e: &Map<String, Value>, mp: &str) {
    if mp.is_empty() {
        return;
    }
    if !flag(e, "lock_diffuse") {
        slots[0] = format!("{mp}.dds");
    }
    if !flag(e, "lock_normal") {
        slots[1] = format!("{mp}_n.dds");
    }
    if e.contains_key("emissive") && !flag(e, "lock_emissive") {
        slots[2] = if flag(e, "emissive") {
            format!("{mp}_g.dds")
        } else {
            String::new()
        };
    }
    if e.contains_key("parallax") && !flag(e, "lock_parallax") {
        slots[3] = if flag(e, "parallax") {
            format!("{mp}_p.dds")
        } else {
            String::new()
        };
    }
    // Slot 4: the `cubemap` value verbatim, otherwise always cleared (also with lock_cubemap).
    slots[4] = match e.get("cubemap").and_then(Value::as_str) {
        Some(c) if !flag(e, "lock_cubemap") => c.to_string(),
        _ => String::new(),
    };
    if !flag(e, "lock_rmaos") {
        slots[5] = format!("{mp}_rmaos.dds");
    }
    if !flag(e, "lock_cnr") {
        let fuzz_tex = e
            .get("fuzz")
            .and_then(Value::as_object)
            .is_some_and(|f| flag(f, "texture"));
        slots[6] = if fuzz_tex {
            format!("{mp}_f.dds")
        } else if flag(e, "coat_normal") {
            format!("{mp}_cnr.dds")
        } else {
            String::new()
        };
    }
    if !flag(e, "lock_subsurface") {
        let s = flag(e, "subsurface_foliage") || flag(e, "subsurface") || flag(e, "coat_diffuse");
        slots[7] = if s {
            format!("{mp}_s.dds")
        } else {
            String::new()
        };
    }
    // slot1..slot8 last: lower-cased, "textures\" prepended unless present, "" clears.
    for (i, slot) in slots.iter_mut().enumerate().take(8) {
        if let Some(v) = e.get(&format!("slot{}", i + 1)).and_then(Value::as_str) {
            let mut v = v.to_ascii_lowercase();
            if !v.is_empty() && !v.starts_with("textures\\") {
                v.insert_str(0, "textures\\");
            }
            *slot = v;
        }
    }
}

/// The 01 §1.4 check [Src 1.2.0 TP:348-375]: every non-empty slot after the slot pass must be a file in the
/// VFS. Returns the first that is not. `exists` receives the lower-cased string.
pub(crate) fn first_missing(slots: &[String; 9], exists: &dyn Fn(&str) -> bool) -> Option<String> {
    slots
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .find(|s| !exists(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map, Value};
    use std::cell::RefCell;

    const MP: &str = "textures\\pbr\\landscape\\rock01";

    fn obj(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            other => panic!("not an object: {other}"),
        }
    }

    fn source() -> [String; 9] {
        [
            "textures\\landscape\\rock01_d.dds",
            "textures\\landscape\\rock01_n.dds",
            "textures\\landscape\\rock01_g.dds",
            "textures\\landscape\\rock01_p.dds",
            "textures\\cubemaps\\shiny_e.dds",
            "textures\\landscape\\rock01_m.dds",
            "textures\\landscape\\rock01_i.dds",
            "textures\\landscape\\rock01_b.dds",
            "textures\\landscape\\rock01_x.dds",
        ]
        .map(String::from)
    }

    fn mp(suffix: &str) -> String {
        format!("{MP}{suffix}")
    }

    fn run(entry: Value) -> [String; 9] {
        let mut s = source();
        apply_slots(&mut s, &obj(entry), MP);
        s
    }

    #[test]
    fn bare_entry_writes_diffuse_normal_rmaos_and_clears_4_6_7() {
        let src = source();
        let want = [
            mp(".dds"),
            mp("_n.dds"),
            src[2].clone(),
            src[3].clone(),
            String::new(),
            mp("_rmaos.dds"),
            String::new(),
            String::new(),
            src[8].clone(),
        ];
        assert_eq!(run(json!({})), want);
    }

    #[test]
    fn emissive_and_parallax_follow_their_keys() {
        let on = run(json!({"emissive": true, "parallax": true}));
        assert_eq!(on[2], mp("_g.dds"));
        assert_eq!(on[3], mp("_p.dds"));
        let off = run(json!({"emissive": false, "parallax": false}));
        assert_eq!(off[2], "");
        assert_eq!(off[3], "");
    }

    #[test]
    fn locks_keep_the_source_but_not_the_cubemap() {
        let got = run(json!({
            "lock_diffuse": true, "lock_normal": true, "emissive": true, "lock_emissive": true,
            "parallax": false, "lock_parallax": true, "lock_rmaos": true, "coat_normal": true,
            "lock_cnr": true, "subsurface": true, "lock_subsurface": true, "lock_cubemap": true
        }));
        let mut want = source();
        want[4] = String::new();
        assert_eq!(got, want);
    }

    #[test]
    fn cubemap_is_verbatim_unless_locked() {
        assert_eq!(
            run(json!({"cubemap": "Textures\\Cubemaps\\Ice_E.dds"}))[4],
            "Textures\\Cubemaps\\Ice_E.dds"
        );
        assert_eq!(
            run(json!({"cubemap": "x.dds", "lock_cubemap": true}))[4],
            ""
        );
    }

    #[test]
    fn slot_6_cnr_and_fuzz_texture_fuzz_wins() {
        assert_eq!(run(json!({"coat_normal": true}))[6], mp("_cnr.dds"));
        assert_eq!(
            run(json!({"coat_normal": true, "fuzz": {"texture": true}}))[6],
            mp("_f.dds")
        );
        assert_eq!(run(json!({"fuzz": {"texture": false}}))[6], "");
        assert_eq!(run(json!({"fuzz": {}}))[6], "");
    }

    #[test]
    fn slot_7_s_from_any_of_three_keys() {
        for k in ["subsurface_foliage", "subsurface", "coat_diffuse"] {
            assert_eq!(run(json!({ k: true }))[7], mp("_s.dds"), "{k}");
        }
        assert_eq!(
            run(json!({"subsurface": false, "coat_diffuse": false}))[7],
            ""
        );
    }

    #[test]
    fn slot_n_overrides_last_lower_cased_with_prefix_and_empty_clears() {
        let got = run(json!({
            "emissive": true,
            "slot1": "Landscape\\Custom_D.dds",
            "slot3": "",
            "slot5": "textures\\black.dds",
            "slot8": "TEXTURES\\X_S.dds",
            "slot9": "textures\\nine.dds"
        }));
        assert_eq!(got[0], "textures\\landscape\\custom_d.dds");
        assert_eq!(got[2], "");
        assert_eq!(got[4], "textures\\black.dds");
        assert_eq!(got[7], "textures\\x_s.dds");
        assert_eq!(got[8], source()[8]);
    }

    #[test]
    fn empty_prefix_changes_nothing() {
        let mut s = source();
        apply_slots(&mut s, &obj(json!({"slot1": "a.dds"})), "");
        assert_eq!(s, source());
    }

    #[test]
    fn missing_check_asks_every_non_empty_slot_lower_cased() {
        let mut slots = run(json!({}));
        slots[4] = "Textures\\Cubemaps\\Ice_E.dds".to_string();
        let seen = RefCell::new(Vec::new());
        let exists = |p: &str| {
            seen.borrow_mut().push(p.to_string());
            true
        };
        assert_eq!(first_missing(&slots, &exists), None);
        let src = source();
        let want = vec![
            mp(".dds"),
            mp("_n.dds"),
            src[2].clone(),
            src[3].clone(),
            "textures\\cubemaps\\ice_e.dds".to_string(),
            mp("_rmaos.dds"),
            src[8].clone(),
        ];
        assert_eq!(*seen.borrow(), want);
    }

    #[test]
    fn missing_check_reports_the_first_missing_slot() {
        let slots = run(json!({}));
        let rmaos = mp("_rmaos.dds");
        let exists = |p: &str| p != rmaos && p != source()[8];
        assert_eq!(first_missing(&slots, &exists), Some(rmaos.clone()));
    }
}
