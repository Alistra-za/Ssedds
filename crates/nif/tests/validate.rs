mod builder;
use builder::scenes;
use nif::edit::validate::{new_errors, validate, Level, Target, Warning};
use nif::edit::{apply, Op};
use nif::extra::ExtraValue;
use nif::fields::{locate, Field, Value};
use nif::header::Header;
use nif::model::read_model;

fn warnings(b: &[u8]) -> Vec<Warning> {
    validate(&read_model(b).unwrap(), None)
}

/// Writes `v` into `f` of `block`, bypassing the editor (the scenes hold fixed values).
fn put(mut b: Vec<u8>, block: usize, f: Field, v: Value) -> Vec<u8> {
    let h = Header::parse(&b).unwrap();
    let (rel, cur) = locate(&h, &b, block, f).unwrap();
    assert_eq!(cur.bytes().len(), v.bytes().len());
    let at = h.offsets()[block] + rel;
    b[at..at + v.bytes().len()].copy_from_slice(&v.bytes());
    b
}

fn fl(x: f32) -> Value {
    Value::F32(x.to_bits())
}

/// The one warning with this text, checked for level and target.
fn only(w: &[Warning], text: &str, level: Level) -> Warning {
    let hits: Vec<_> = w.iter().filter(|x| x.text == text).collect();
    assert_eq!(hits.len(), 1, "{text:?} in {w:#?}");
    assert_eq!(hits[0].level, level);
    hits[0].clone()
}

fn none(w: &[Warning], text: &str) {
    assert!(w.iter().all(|x| x.text != text), "{text:?} in {w:#?}");
}

#[test]
fn clean_scene_has_no_warnings() {
    assert_eq!(warnings(&scenes::lsp_shape(0, &[])), vec![]);
}

#[test]
fn soft_ranges_warn_outside_and_not_inside() {
    for (f, v, label, lo, hi) in [
        (
            Field::EmissiveMultiple,
            101.0,
            "Emissive Multiple 101",
            0,
            100,
        ),
        (Field::SpecularStrength, -1.0, "Specular Strength -1", 0, 10),
        (Field::Glossiness, 1000.0, "Glossiness 1000", 0, 999),
        (Field::Alpha, 128.5, "Alpha 128.5", 0, 128),
        (Field::Refraction, 1.5, "Refraction 1.5", 0, 1),
    ] {
        let w = warnings(&put(scenes::lsp_shape(0, &[]), 1, f, fl(v)));
        let x = only(&w, &format!("{label} is outside {lo}-{hi}"), Level::Warn);
        assert_eq!(x.target, Target::Field { block: 1, field: f });
        assert!(x.fix.is_none());
        let inside = warnings(&put(scenes::lsp_shape(0, &[]), 1, f, fl(lo as f32)));
        assert!(inside.is_empty(), "{inside:#?}");
    }
}

#[test]
fn trailer_ranges() {
    let w = warnings(&scenes::lsp_shape(1, &[11.0]));
    let x = only(&w, "Env scale 11 is outside 0-10", Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::Trailer(0)
        }
    );
    assert!(warnings(&scenes::lsp_shape(1, &[10.0])).is_empty());

    let w = warnings(&scenes::lsp_shape(11, &[4.0, 2.0, 1.0, 1.0, 1.0]));
    only(&w, "MLP thickness 4 is outside 5-500", Level::Warn);
    let x = only(&w, "MLP refraction scale 2 is outside 0-1", Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::Trailer(1)
        }
    );
    none(
        &warnings(&scenes::lsp_shape(11, &[5.0, 1.0, 1.0, 1.0, 1.0])),
        "MLP thickness 5 is outside 5-500",
    );

    let w = warnings(&scenes::lsp_shape(7, &[0.0, 1.0]));
    only(&w, "POM passes 0 is outside 1-320", Level::Warn);
    assert!(warnings(&scenes::lsp_shape(7, &[320.0, 1.0])).is_empty());
}

#[test]
fn clamp_mode_over_three_warns() {
    let b = put(
        scenes::lsp_shape(0, &[]),
        1,
        Field::ClampMode,
        Value::U32(4),
    );
    let w = warnings(&b);
    let x = only(&w, "Clamp mode 4 is not 0-3", Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::ClampMode
        }
    );
}

#[test]
fn shader_type_twenty_is_an_error_nineteen_is_fine() {
    let w = warnings(&scenes::lsp_shape(20, &[]));
    only(&w, "Shader type 20 is FO4 only", Level::Error);
    none(
        &warnings(&scenes::lsp_shape(19, &[])),
        "Shader type 20 is FO4 only",
    );
}

#[test]
fn blend_modes_eleven_to_fifteen_read_as_one() {
    // flags: blend bit 0, src bits 1-4, dst bits 5-8.
    let w = warnings(&scenes::alpha_shape(1 | (11 << 1) | (15 << 5)));
    let t = |n: u16| format!("Blend mode {n} reads as ONE (invalid)");
    let x = only(&w, &t(11), Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::AlphaFlags
        }
    );
    only(&w, &t(15), Level::Warn);
    assert!(warnings(&scenes::alpha_shape(1 | (10 << 1) | (10 << 5))).is_empty());
}

#[test]
fn controller_rules() {
    let chain = scenes::free_chain(); // ctrls 1 (var 11) and 2 (var 20), flags 0x48, freq 1, start 0, stop 1
    assert!(warnings(&chain).is_empty());

    let w = warnings(&put(
        chain.clone(),
        1,
        Field::CtrlFlags,
        Value::U16(0x48 | 6),
    ));
    let x = only(&w, "Cycle type 3 is not 0-2", Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::CtrlFlags
        }
    );

    for f in [0.0, -1.0] {
        let w = warnings(&put(chain.clone(), 1, Field::Frequency, fl(f)));
        only(&w, "Frequency ≤ 0 freezes the controller", Level::Warn);
    }
    let w = warnings(&put(chain.clone(), 1, Field::StartTime, fl(2.0)));
    let x = only(&w, "Start is after stop", Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::StartTime
        }
    );

    let w = warnings(&put(chain.clone(), 1, Field::Variable, Value::U32(13)));
    let x = only(
        &w,
        "Variable 13 is not a known Skyrim value (kept)",
        Level::Warn,
    );
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::Variable
        }
    );
    // Known values are kept silently; 0 (refraction), 9 and 10 (Specular off here, B-P39/B-P41) and 20 (twice) warn.
    for v in [8, 11, 12, 21, 22, 23] {
        assert!(warnings(&put(chain.clone(), 1, Field::Variable, Value::U32(v))).is_empty());
    }
    for v in [0, 9, 10, 20] {
        let w = warnings(&put(chain.clone(), 1, Field::Variable, Value::U32(v)));
        assert_eq!(w.len(), 1, "{w:#?}");
        assert_eq!(w[0].target, Target::Block { block: 1 });
    }
}

#[test]
fn own_emit_off_with_emissive_colour_warns() {
    let b = put(
        scenes::lsp_shape(0, &[]),
        1,
        Field::EmissiveColor(1),
        fl(0.5),
    );
    let w = warnings(&b);
    only(&w, "No effect in game: Own_Emit is off", Level::Warn);
    // Own_Emit on, or a black colour: nothing.
    let on = put(b, 1, Field::Slsf1, Value::U32(1 << 22));
    none(&warnings(&on), "No effect in game: Own_Emit is off");
}

const EXT: &str = "External_Emittance does not match BSX ExternalEmit";

#[test]
fn external_emittance_mismatch_offers_a_bsx_fix() {
    let b = put(
        scenes::emit_scene("BSX", 0x83, true),
        2,
        Field::Slsf1,
        Value::U32(1 << 29),
    );
    let w = warnings(&b);
    let x = only(&w, EXT, Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 2,
            field: Field::Slsf1
        }
    );
    let Some(Op::EditExtra { value, base, .. }) = x.fix.clone() else {
        panic!("{x:#?}")
    };
    assert_eq!(
        (value, base),
        (ExtraValue::Bsx(0x283), ExtraValue::Bsx(0x83))
    );
    // The fix applies and clears the warning.
    let fixed = apply(&b, &[x.fix.unwrap()]).unwrap();
    none(&warnings(&fixed.bytes), EXT);
}

#[test]
fn bsx_bit_set_without_slsf_bit_clears_it() {
    let b = scenes::emit_scene("BSX", 0x283, true);
    let x = only(&warnings(&b), EXT, Level::Warn);
    let Some(Op::EditExtra { value, .. }) = x.fix else {
        panic!()
    };
    assert_eq!(value, ExtraValue::Bsx(0x83));
}

#[test]
fn external_emittance_match_or_no_bsx() {
    // Both clear, and both set: no warning.
    none(&warnings(&scenes::emit_scene("BSX", 0x83, true)), EXT);
    let both = put(
        scenes::emit_scene("BSX", 0x283, true),
        2,
        Field::Slsf1,
        Value::U32(1 << 29),
    );
    none(&warnings(&both), EXT);
    // No BSXFlags on the root: still a warning, but no fix.
    let b = put(
        scenes::emit_scene("BSX", 0x83, false),
        2,
        Field::Slsf1,
        Value::U32(1 << 29),
    );
    let x = only(&warnings(&b), EXT, Level::Warn);
    assert!(x.fix.is_none());
}

/// Meshes UI T5b: heel data on the root warns (the game ignores it there); on a later shape it warns that the first
/// shape wins; the first shape's own heel data is fine.
#[test]
fn heel_data_off_a_shape_or_on_a_later_shape_warns() {
    use nif::edit::validate::{HEEL_EARLIER_WINS, HEEL_NOT_ON_SHAPE};
    let w = warnings(&scenes::heel_scene());
    assert_eq!(
        only(&w, HEEL_NOT_ON_SHAPE, Level::Warn).target,
        Target::Extra { block: 6 }
    );
    assert_eq!(
        only(&w, HEEL_EARLIER_WINS, Level::Warn).target,
        Target::Extra { block: 5 }
    );
}

/// T5b review Minors 4 and 7 (controller ruling): a shape the root does not reach does not count as "earlier", and
/// heel data on any block that is not a shape (here an unlinked NiNode) warns.
#[test]
fn heel_data_on_an_orphan_shape_or_a_node() {
    use nif::edit::validate::{HEEL_EARLIER_WINS, HEEL_NOT_ON_SHAPE};
    let w = warnings(&scenes::heel_orphan_scene());
    none(&w, HEEL_EARLIER_WINS);
    assert_eq!(
        only(&w, HEEL_NOT_ON_SHAPE, Level::Warn).target,
        Target::Extra { block: 6 }
    );
}

#[test]
fn bsxflags_must_be_named_bsx() {
    let w = warnings(&scenes::emit_scene("Other", 0x83, true));
    let x = only(&w, "BSXFlags must be named BSX", Level::Error);
    assert_eq!(x.target, Target::Extra { block: 0 });
    none(
        &warnings(&scenes::emit_scene("BSX", 0x83, true)),
        "BSXFlags must be named BSX",
    );
}

#[test]
fn pbr_shape_has_no_warning() {
    let b = put(
        scenes::lsp_shape(0, &[]),
        1,
        Field::Slsf2,
        Value::U32(1 << 23),
    );
    assert!(warnings(&b).is_empty());
}

fn path_warnings(p: &str) -> Vec<Warning> {
    warnings(&scenes::one_shape_set(&[p]))
}

const REL: &str = "Not relative to Data";

#[test]
fn texture_path_clean_and_empty_are_valid() {
    assert!(path_warnings("textures\\a.dds").is_empty());
    assert!(path_warnings("Textures\\A.DDS").is_empty());
    assert!(path_warnings("").is_empty());
}

#[test]
fn texture_path_forward_slash() {
    let w = path_warnings("textures/a.dds");
    let x = only(&w, "Uses / instead of \\", Level::Warn);
    assert_eq!(x.target, Target::Slot { set: 1, slot: 0 });
    assert_eq!(w.len(), 1);
}

#[test]
fn texture_path_absolute() {
    let w = path_warnings("C:\\mods\\textures\\a.dds");
    only(&w, "Absolute path", Level::Warn);
    only(&w, REL, Level::Warn);
    none(&path_warnings("\\a.dds"), "Uses / instead of \\");
    only(
        &path_warnings("\\textures\\a.dds"),
        "Absolute path",
        Level::Warn,
    );
    none(&path_warnings("textures\\a.dds"), "Absolute path");
}

#[test]
fn texture_path_fix_cuts_before_last_textures() {
    let b = scenes::one_shape_set(&["D:\\Mods\\Pack\\Textures\\sub\\a.dds"]);
    let x = only(&warnings(&b), REL, Level::Warn);
    let Some(Op::SetTexturePath {
        slot, path, base, ..
    }) = x.fix.clone()
    else {
        panic!("{x:#?}")
    };
    assert_eq!((slot, path.as_str()), (0, "Textures\\sub\\a.dds"));
    assert_eq!(base, "D:\\Mods\\Pack\\Textures\\sub\\a.dds");
    let fixed = apply(&b, &[x.fix.unwrap()]).unwrap();
    assert!(warnings(&fixed.bytes).is_empty());
}

#[test]
fn texture_path_fix_removes_leading_data() {
    let x = only(&path_warnings("Data\\textures\\a.dds"), REL, Level::Warn);
    let Some(Op::SetTexturePath { path, .. }) = x.fix else {
        panic!()
    };
    assert_eq!(path, "textures\\a.dds");
}

#[test]
fn texture_path_without_a_textures_component_has_no_fix() {
    let x = only(&path_warnings("a.dds"), REL, Level::Warn);
    assert!(x.fix.is_none());
    let x = only(&path_warnings("mytextures\\a.dds"), REL, Level::Warn);
    assert!(x.fix.is_none());
}

#[test]
fn texture_path_extension() {
    let x = only(
        &path_warnings("textures\\a.png"),
        "Not a .dds file",
        Level::Warn,
    );
    assert_eq!(x.target, Target::Slot { set: 1, slot: 0 });
}

#[test]
fn texture_exists_callback() {
    let m = read_model(&scenes::one_shape_set(&["Textures\\Dir/A.DDS", ""])).unwrap();
    let seen = std::cell::RefCell::new(vec![]);
    let f = |p: &str| {
        seen.borrow_mut().push(p.to_string());
        false
    };
    let w = validate(&m, Some(&f));
    only(&w, "Not found in the scanned mods or BSAs", Level::Warn);
    // The canonical key, and never for the empty slot.
    assert_eq!(*seen.borrow(), vec!["textures/dir/a.dds".to_string()]);
    let yes = |_: &str| true;
    none(
        &validate(&m, Some(&yes)),
        "Not found in the scanned mods or BSAs",
    );
    none(&validate(&m, None), "Not found in the scanned mods or BSAs");
}

#[test]
fn unused_texture_sets_are_not_checked() {
    let mut n = builder::Nif::new(100);
    n.block("BSShaderTextureSet", scenes::texset(&["bad.png"]));
    assert!(warnings(&n.bytes()).is_empty());
}

#[test]
fn untouched_source_errors_are_not_new() {
    let src = warnings(&scenes::emit_scene("Other", 0x83, true));
    assert_eq!(src.iter().filter(|w| w.level == Level::Error).count(), 1);
    // Same file edited elsewhere: the source error is not new; warnings never count.
    let edited = warnings(&put(
        scenes::emit_scene("Other", 0x83, true),
        2,
        Field::Glossiness,
        fl(5000.0),
    ));
    assert!(new_errors(&src, &edited).is_empty());
    // A new error is reported, a clean source reports every error.
    let bad = warnings(&scenes::lsp_shape(20, &[]));
    let n = new_errors(&src, &bad);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].text, "Shader type 20 is FO4 only");
    assert_eq!(new_errors(&[], &edited).len(), 1);
}

const NEEDS_MLP: &str =
    "Shader type 11 needs the Multi_Layer_Parallax flag (SLSF2 24) without PBR.";
const MLP_NOT_11: &str = "Multi_Layer_Parallax (SLSF2 24) is set but the shader type is not 11.";
const FIT_SLOPE: &str = "Fit_Slope (SLSF2 12) does nothing without the PBR flag.";

fn slsf_scene(shader_type: u32, slsf2: u32) -> Vec<Warning> {
    let trailer: &[f32] = if shader_type == 11 {
        &[5.0, 1.0, 1.0, 1.0, 1.0]
    } else {
        &[]
    };
    let b = put(
        scenes::lsp_shape(shader_type, trailer),
        1,
        Field::Slsf2,
        Value::U32(slsf2),
    );
    warnings(&b)
}

#[test]
fn slsf_type_11_without_mlp_flag_warns_when_not_pbr() {
    let w = slsf_scene(11, 0);
    let x = only(&w, NEEDS_MLP, Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::Slsf2
        }
    );
    none(&slsf_scene(11, 1 << 24), NEEDS_MLP);
    none(&slsf_scene(11, 1 << 23), NEEDS_MLP);
}

#[test]
fn slsf_mlp_flag_on_other_type_warns() {
    for (t, f2) in [(0, 1 << 24), (0, (1 << 24) | (1 << 23))] {
        let x = only(&slsf_scene(t, f2), MLP_NOT_11, Level::Warn);
        assert_eq!(
            x.target,
            Target::Field {
                block: 1,
                field: Field::Slsf2
            }
        );
    }
    none(&slsf_scene(11, 1 << 24), MLP_NOT_11);
}

#[test]
fn slsf_fit_slope_without_pbr_warns() {
    let x = only(&slsf_scene(0, 1 << 12), FIT_SLOPE, Level::Warn);
    assert_eq!(
        x.target,
        Target::Field {
            block: 1,
            field: Field::Slsf2
        }
    );
    none(&slsf_scene(0, (1 << 12) | (1 << 23)), FIT_SLOPE);
}

#[test]
fn slsf_vanilla_lighting_bits_do_not_warn() {
    for t in [0, 11] {
        let f2 = (1 << 25) | (1 << 26) | (1 << 27) | (1 << 30) | if t == 11 { 1 << 24 } else { 0 };
        let w = slsf_scene(t, f2);
        assert!(w.is_empty(), "{w:#?}");
    }
    // PBR type 11 without bit 24 is common.
    assert!(slsf_scene(11, 1 << 23).is_empty());
}

// ---- v2 B4: authored keys ----

use nif::keys::{Key, KeyData, KeyGroup};

/// New errors after a SetKeys of `keys` (type `key_type`) on var 11 of `keyed_pair(Share::No, [1, 2])`.
fn key_errors(key_type: Option<u32>, keys: Vec<Key<f32>>) -> Vec<Warning> {
    key_errors_on(
        scenes::keyed_pair(scenes::Share::No, [1, 2]),
        key_type,
        keys,
    )
}

/// Same, on the keyed_pair-shaped file `b`.
fn key_errors_on(b: Vec<u8>, key_type: Option<u32>, keys: Vec<Key<f32>>) -> Vec<Warning> {
    use nif::edit::validate::validate_ops;
    use nif::edit::{anchor_role, Outcome, Owner, Role};
    let m = read_model(&b).unwrap();
    let role = Role::Ctrl {
        owner: Owner::Shader,
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(11),
        nth: 0,
    };
    let base = nif::keys::read_data(&Header::parse(&b).unwrap(), &b, 6).unwrap();
    let ops = vec![Op::SetKeys {
        at: anchor_role(&m, Some(0), role).unwrap(),
        clip: None,
        clip_name: None,
        keys: KeyData::Float(KeyGroup { key_type, keys }),
        base,
    }];
    let r = apply(&b, &ops).unwrap();
    assert_eq!(
        r.outcomes,
        vec![Outcome::Applied],
        "validation never refuses"
    );
    let mut edited = warnings(&r.bytes);
    edited.extend(validate_ops(&ops));
    new_errors(&warnings(&b), &edited)
}

fn k(time: f32, extra: Option<f32>, tbc: bool) -> Key<f32> {
    Key {
        time,
        value: 1.0,
        forward: extra,
        backward: extra,
        tbc: tbc.then_some([0.0, 0.0, 0.0]),
    }
}

fn error(text: &str) -> Vec<Warning> {
    vec![Warning {
        level: Level::Error,
        target: Target::Block { block: 2 },
        text: text.into(),
        fix: None,
    }]
}

#[test]
fn decreasing_times_block_export() {
    let lin = |t: &[f32]| t.iter().map(|&t| k(t, None, false)).collect();
    assert_eq!(
        key_errors(Some(1), lin(&[0.0, 2.0, 1.0])),
        error("Key times must not decrease")
    );
    assert_eq!(key_errors(Some(1), lin(&[0.0, 1.0, 1.0])), vec![]);
    let tbc = vec![k(0.0, None, true), k(1.0, None, true)];
    assert_eq!(
        key_errors(Some(3), tbc),
        error("Only linear and quadratic keys can be authored")
    );
    let quad = vec![k(0.0, Some(0.5), false), k(1.0, Some(0.5), false)];
    assert_eq!(key_errors(Some(2), quad), vec![]);
    assert_eq!(key_errors(None, vec![]), vec![]);
}

/// Ruling B-P29: NaN or infinite authored times, values and tangents block export.
#[test]
fn non_finite_keys_block_export() {
    let finite = || error("Key values must be finite numbers");
    let lin = |t: &[f32]| t.iter().map(|&t| k(t, None, false)).collect();
    // NaN hides the decrease from every adjacent comparison
    assert_eq!(key_errors(Some(1), lin(&[0.0, f32::NAN, -1.0])), finite());
    assert_eq!(key_errors(Some(1), lin(&[0.0, f32::INFINITY])), finite());
    assert_eq!(
        key_errors(Some(1), lin(&[f32::NEG_INFINITY, f32::INFINITY])),
        finite()
    );
    let mut v = k(0.0, None, false);
    v.value = f32::NAN;
    assert_eq!(key_errors(Some(1), vec![v]), finite());
    let mut q = k(0.0, Some(0.5), false);
    q.backward = Some(f32::INFINITY);
    assert_eq!(key_errors(Some(2), vec![q]), finite());

    // an untouched source curve with a NaN value is never checked
    let mut b = scenes::keyed_pair(scenes::Share::No, [1, 2]);
    let at = Header::parse(&b).unwrap().offsets()[7] + 12; // data 7, key 0 value
    b[at..at + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert_eq!(key_errors_on(b, Some(1), lin(&[0.0, 1.0])), vec![]);
}

/// Only the keys that are exported are checked: a SetKeys that a later SetKeys on the same (controller, clip)
/// replaced never errors; another clip's SetKeys still does.
#[test]
fn superseded_set_keys_are_not_checked() {
    use nif::edit::validate::validate_ops;
    use nif::edit::{Anchor, Role};
    let op = |clip: Option<u32>, times: &[f32]| Op::SetKeys {
        at: Anchor {
            block: 2,
            kind: "BSLightingShaderPropertyFloatController".into(),
            shape: None,
            role: Role::Shape,
        },
        clip,
        clip_name: None,
        keys: KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: times.iter().map(|&t| k(t, None, false)).collect(),
        }),
        base: KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: vec![],
        }),
    };
    let bad = op(None, &[0.0, -1.0]);
    assert_eq!(validate_ops(&[bad.clone(), op(None, &[0.0, 1.0])]), vec![]);
    assert_eq!(
        validate_ops(&[bad, op(Some(8), &[0.0, 1.0])]),
        error("Key times must not decrease")
    );
    let nan = op(None, &[0.0, f32::NAN]);
    assert_eq!(validate_ops(&[nan, op(None, &[0.0, 1.0])]), vec![]);
}

/// B-P41 (final review minor 6): AddController's frequency, start and stop are authored like its keys.
#[test]
fn add_controller_times_must_be_finite() {
    use nif::edit::validate::validate_ops;
    use nif::edit::{Anchor, Role};
    let op = |freq: f32, start: f32, stop: f32| Op::AddController {
        at: Anchor {
            block: 1,
            kind: "BSLightingShaderProperty".into(),
            shape: None,
            role: Role::Shader,
        },
        class: "BSLightingShaderPropertyFloatController".into(),
        var: Some(11),
        keys: KeyData::Float(KeyGroup {
            key_type: Some(1),
            keys: vec![],
        }),
        flags: 0x48,
        freq,
        start,
        stop,
    };
    assert_eq!(validate_ops(&[op(1.0, 0.0, 1.0)]), vec![]);
    for (f, a, b) in [
        (f32::NAN, 0.0, 1.0),
        (1.0, f32::INFINITY, 1.0),
        (1.0, 0.0, f32::NEG_INFINITY),
    ] {
        let w = validate_ops(&[op(f, a, b)]);
        assert_eq!(w.len(), 1, "{w:#?}");
        assert_eq!(w[0].text, "Controller times must be finite numbers");
        assert_eq!(w[0].level, Level::Error);
        assert_eq!(w[0].target, Target::Block { block: 1 });
    }
}

// ---- B-P39: guards from the B9 in-game controller check ----

const ENV_PBR: &str = "Env map scale crashes the game on PBR materials.";
const SPEC_OFF: &str = "This controller has no effect: Specular is off when the mesh loads.";
const TEST_OFF: &str = "This controller has no effect: alpha test is off.";
const REFRACTION: &str = "Refraction strength has no visible effect in game.";
const SLSF1_SPECULAR: u32 = 1;
const PBR: u32 = 1 << 23;
const LSCF: &str = "BSLightingShaderPropertyFloatController";
const LSCC: &str = "BSLightingShaderPropertyColorController";

/// BSLSP (0, slsf1 0x8000_0000 = Specular off, slsf2 1) with a chain of `ctrls` (class, var) at blocks 1, 2, ...
fn chain_of(ctrls: &[(&str, u32)]) -> Vec<u8> {
    let mut n = builder::Nif::new(100);
    n.block(
        "BSLightingShaderProperty",
        scenes::lsp_body(1, &[], 1, &[1.0]),
    );
    for (i, (class, var)) in ctrls.iter().enumerate() {
        let next = if i + 1 < ctrls.len() {
            i as i32 + 2
        } else {
            -1
        };
        let tail = builder::W::default().i32(-1).u32(*var);
        n.block(class, scenes::ctrl_body(next, 0x48, 0, tail));
    }
    n.bytes()
}

#[test]
fn env_map_scale_on_pbr_is_an_error_that_is_always_new() {
    let b = put(scenes::free_chain(), 1, Field::Variable, Value::U32(8));
    none(&warnings(&b), ENV_PBR);
    let pbr = put(b, 0, Field::Slsf2, Value::U32(1 | PBR));
    let w = warnings(&pbr);
    let x = only(&w, ENV_PBR, Level::Error);
    assert_eq!(x.target, Target::Block { block: 1 });
    // A source that already had it still blocks: the game crashes either way.
    let n = new_errors(&w, &w);
    assert_eq!(n.len(), 1, "{n:#?}");
    assert_eq!(n[0].text, ENV_PBR);
}

#[test]
fn specular_controllers_without_the_specular_flag_warn() {
    // B-P41 (B9 I6-R10Z, CM22): glossiness does nothing there too.
    for (class, var) in [(LSCF, 9), (LSCF, 10), (LSCC, 0)] {
        let b = chain_of(&[(class, var)]);
        let x = only(&warnings(&b), SPEC_OFF, Level::Warn);
        assert_eq!(x.target, Target::Block { block: 1 });
        let on = put(
            b.clone(),
            0,
            Field::Slsf1,
            Value::U32(0x8000_0000 | SLSF1_SPECULAR),
        );
        none(&warnings(&on), SPEC_OFF);
        // PBR reads these as specular level, roughness scale and coat / subsurface colour.
        none(
            &warnings(&put(b, 0, Field::Slsf2, Value::U32(1 | PBR))),
            SPEC_OFF,
        );
    }
    // Emissive colour is not in the rule.
    none(&warnings(&chain_of(&[(LSCC, 1)])), SPEC_OFF);
}

#[test]
fn testref_without_alpha_test_warns() {
    let b = scenes::alpha_testref(); // flags 0x00ED: blend on, test off
    let x = only(&warnings(&b), TEST_OFF, Level::Warn);
    assert_eq!(x.target, Target::Block { block: 1 });
    let on = put(b, 0, Field::AlphaFlags, Value::U16(0x00ED | 1 << 9));
    none(&warnings(&on), TEST_OFF);
}

#[test]
fn refraction_strength_controller_warns() {
    let w = warnings(&chain_of(&[(LSCF, 0)]));
    let x = only(&w, REFRACTION, Level::Warn);
    assert_eq!(x.target, Target::Block { block: 1 });
    assert!(w.iter().all(|w| w.level == Level::Warn));
}

#[test]
fn two_controllers_on_one_variable_warn_on_the_one_that_does_not_play() {
    let w = warnings(&chain_of(&[(LSCF, 11), (LSCF, 20), (LSCF, 11)]));
    let x = only(
        &w,
        "Two controllers drive Emissive multiple: only the last one plays.",
        Level::Warn,
    );
    assert_eq!(x.target, Target::Block { block: 1 });
    // Same variable number, other class: different values.
    assert!(warnings(&chain_of(&[(LSCF, 0), (LSCC, 0)]))
        .iter()
        .all(|w| !w.text.starts_with("Two controllers")));
    // The label is the PBR one.
    let pbr = put(
        chain_of(&[(LSCF, 9), (LSCF, 9)]),
        0,
        Field::Slsf2,
        Value::U32(1 | PBR),
    );
    only(
        &warnings(&pbr),
        "Two controllers drive Specular level: only the last one plays.",
        Level::Warn,
    );
}

/// B-P40 (B9 I6-R10Z): the specular permutation is chosen at load, so a strength of 0 then stays non-specular.
#[test]
fn specular_controllers_on_zero_strength_warn() {
    const ZERO: &str = "This controller has no effect: specular strength is 0 when the mesh loads.";
    for (class, var) in [(LSCF, 9), (LSCF, 10), (LSCC, 0)] {
        let on = put(
            chain_of(&[(class, var)]),
            0,
            Field::Slsf1,
            Value::U32(0x8000_0000 | SLSF1_SPECULAR),
        );
        none(&warnings(&on), ZERO);
        let zero = put(on, 0, Field::SpecularStrength, fl(0.0));
        let x = only(&warnings(&zero), ZERO, Level::Warn);
        assert_eq!(x.target, Target::Block { block: 1 });
        none(&warnings(&zero), SPEC_OFF);
        none(
            &warnings(&put(zero, 0, Field::Slsf2, Value::U32(1 | PBR))),
            ZERO,
        );
    }
    // Flag off and strength 0: the flag warning alone.
    let off = put(chain_of(&[(LSCF, 10)]), 0, Field::SpecularStrength, fl(0.0));
    only(&warnings(&off), SPEC_OFF, Level::Warn);
    none(&warnings(&off), ZERO);
}

#[test]
fn emissive_multiple_up_to_100_is_quiet() {
    // Real mods use values up to 100 (corpus max); 0-10 was nif.xml's editing range, not a game limit.
    let w = warnings(&put(
        scenes::lsp_shape(0, &[]),
        1,
        Field::EmissiveMultiple,
        fl(42.0),
    ));
    assert!(w.is_empty(), "{w:#?}");
}

/// v2 C2 (F10): AddSequence track keys get the same three Errors as AddController, on the track's controller.
#[test]
fn add_sequence_track_keys_are_checked() {
    use nif::edit::validate::validate_ops;
    use nif::edit::{Anchor, ClipTrack, Role};
    let op = |key_type: u32, keys: Vec<Key<f32>>| Op::AddSequence {
        at: Anchor {
            block: 0,
            kind: "NiNode".into(),
            shape: None,
            role: Role::Root,
        },
        name: "Open".into(),
        cycle: 2,
        freq: 1.0,
        start: 0.0,
        stop: 1.0,
        tracks: vec![ClipTrack {
            at: Anchor {
                block: 2,
                kind: "BSLightingShaderPropertyFloatController".into(),
                shape: Some("S".into()),
                role: Role::Shape,
            },
            keys: KeyData::Float(KeyGroup {
                key_type: Some(key_type),
                keys,
            }),
        }],
    };
    let lin = |t: &[f32]| t.iter().map(|&t| k(t, None, false)).collect();
    assert_eq!(validate_ops(&[op(1, lin(&[0.0, 1.0]))]), vec![]);
    assert_eq!(
        validate_ops(&[op(1, lin(&[0.0, 2.0, 1.0]))]),
        error("Key times must not decrease")
    );
    let tbc = vec![k(0.0, None, true), k(1.0, None, true)];
    assert_eq!(
        validate_ops(&[op(3, tbc)]),
        error("Only linear and quadratic keys can be authored")
    );
    let mut v = k(0.0, None, false);
    v.value = f32::NAN;
    assert_eq!(
        validate_ops(&[op(1, vec![v])]),
        error("Key values must be finite numbers")
    );
}

// ---- Note CS: SLSF1 bits that need a shader type (owner) ----

const SKIN_TINT: &str =
    "Skin tint flag needs the Skin Tint shader type: the game crashes otherwise.";
const FACEGEN_DETAIL: &str =
    "FaceGen detail flag needs the Face Tint or Skin Tint shader type: the mesh renders black otherwise.";
const FACE_TINT: &str =
    "Face Tint shader type needs the FaceGen detail flag: the face renders wrong otherwise.";

/// `lsp_shape(shader_type)` with SLSF1 = `slsf1`.
fn typed(shader_type: u32, slsf1: u32) -> Vec<u8> {
    let trailer: &[f32] = if shader_type == 5 { &[1.0; 3] } else { &[] };
    put(
        scenes::lsp_shape(shader_type, trailer),
        1,
        Field::Slsf1,
        Value::U32(slsf1),
    )
}

#[test]
fn type_bound_flags_are_errors_with_a_one_click_fix_that_always_blocks() {
    let slsf1 = 1 << 31;
    for (ty, bits, text, value) in [
        (0, 1 << 21, SKIN_TINT, 0),
        (4, (1 << 21) | (1 << 10), SKIN_TINT, 0),
        (0, 1 << 10, FACEGEN_DETAIL, 0),
        (2, 1 << 10, FACEGEN_DETAIL, 0),
        (4, 0, FACE_TINT, 1 << 10),
    ] {
        let b = typed(ty, slsf1 | bits);
        let w = warnings(&b);
        let x = only(&w, text, Level::Error);
        assert_eq!(
            x.target,
            Target::Field {
                block: 1,
                field: Field::Slsf1
            }
        );
        let Some(Op::SetBits {
            field,
            mask,
            value: v,
            base,
            ..
        }) = x.fix.clone()
        else {
            panic!("{x:#?}")
        };
        let bit = if text == SKIN_TINT { 1 << 21 } else { 1 << 10 };
        assert_eq!(
            (field, mask, v, base),
            (Field::Slsf1, bit, value, slsf1 | bits)
        );
        // The fix applies and clears it; an untouched source still blocks export.
        let fixed = apply(&b, &[x.fix.unwrap()]).unwrap();
        none(&warnings(&fixed.bytes), text);
        assert!(
            new_errors(&w, &w).iter().any(|n| n.text == text),
            "{ty} {text}"
        );
    }
    // The matching pairs are clean.
    // CS3: Skin Tint may carry the detail flag (9 mod shapes, fine in game).
    for (ty, bits) in [
        (5, 1 << 21),
        (4, 1 << 10),
        (5, 0),
        (0, 0),
        (5, 1 << 10),
        (5, (1 << 10) | (1 << 21)),
    ] {
        let w = warnings(&typed(ty, slsf1 | bits));
        for t in [SKIN_TINT, FACEGEN_DETAIL, FACE_TINT] {
            none(&w, t);
        }
    }
}

#[test]
fn a_type_change_fixes_the_flags_it_needs() {
    use nif::edit::validate::type_flag_errors;
    let f = |ty, slsf1| -> Vec<(u32, u32)> {
        type_flag_errors(ty, slsf1)
            .into_iter()
            .map(|e| (e.0, e.1))
            .collect()
    };
    assert_eq!(f(0, 1 << 21), vec![(1 << 21, 0)]);
    assert_eq!(f(4, 1 << 21), vec![(1 << 21, 0), (1 << 10, 1 << 10)]);
    assert_eq!(f(5, 1 << 10), vec![]);
    assert_eq!(f(1, 1 << 10), vec![(1 << 10, 0)]);
    assert_eq!(f(5, 1 << 21), vec![]);
    let notice: Vec<_> = type_flag_errors(0, 1 << 21)
        .into_iter()
        .map(|e| e.3)
        .collect();
    assert_eq!(
        notice,
        vec!["Skin tint flag cleared: it needs the Skin Tint shader type."]
    );
}

/// S-P2: shape "S" (0) on a clean BSLSP (1), plus an orphan BSLSP (2) of type 0 with the skin tint flag.
fn orphan_skin_tint() -> Vec<u8> {
    let mut n = builder::Nif::new(100);
    let s = n.string("S");
    n.block("BSTriShape", scenes::bstri_named(s, 1, -1));
    n.block(
        "BSLightingShaderProperty",
        scenes::lsp_body(0, &[], -1, &[]),
    );
    n.block(
        "BSLightingShaderProperty",
        scenes::lsp_body(0, &[], -1, &[]),
    );
    put(
        n.bytes(),
        2,
        Field::Slsf1,
        Value::U32((1 << 31) | (1 << 21)),
    )
}

#[test]
fn an_orphan_shader_with_a_type_bound_flag_is_not_an_error() {
    let w = warnings(&orphan_skin_tint());
    none(&w, SKIN_TINT);
    assert!(new_errors(&w, &w).is_empty(), "{w:#?}");
}
