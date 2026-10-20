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
        (Field::EmissiveMultiple, 11.0, "Emissive Multiple 11", 0, 10),
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
    assert!(warnings(&scenes::lsp_shape(11, &[5.0, 1.0, 1.0, 1.0, 1.0])).is_empty());

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
    for v in [0, 8, 9, 10, 11, 12, 20, 21, 22, 23] {
        assert!(warnings(&put(chain.clone(), 1, Field::Variable, Value::U32(v))).is_empty());
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
