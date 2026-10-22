mod builder;
use builder::scenes::{free_chain, managed_clip};
use nif::anim::{
    apply_anim, clip_tracks, ctrl_time, free_tracks, sample_f32, sample_vec3, AnimValue, Target,
    Track,
};
use nif::header::Header;
use nif::keys::{Key, KeyData, KeyGroup};
use nif::material::MaterialState;
use nif::model::{read_model, NifModel};
use nif::props::{Alpha, LightingTrailer};
use nif::write::NifDoc;

fn patch(b: &[u8], block: u32, at: usize, bytes: &[u8]) -> Vec<u8> {
    let mut d = NifDoc::open(b).unwrap();
    d.patch(block, at, bytes).unwrap();
    d.bytes()
}

fn key<T>(time: f32, value: T) -> Key<T> {
    Key {
        time,
        value,
        forward: None,
        backward: None,
        tbc: None,
    }
}

fn group<T>(key_type: u32, keys: Vec<Key<T>>) -> KeyGroup<T> {
    KeyGroup {
        key_type: Some(key_type),
        keys,
    }
}

fn base_of(m: &NifModel, prop: u32) -> MaterialState {
    let mut s = MaterialState::builtin_default();
    s.params = m.lighting[&prop].lighting.params.clone();
    s
}

#[test]
fn ctrl_time_matches_nifskope_table() {
    // t, LOOP, REVERSE, CLAMP
    let rows: [(f32, f32, f32, f32); 9] = [
        (-1.0, 2.5, 2.5, 1.0),
        (0.0, 2.5, 1.5, 1.0),
        (0.25, 1.0, 1.0, 1.0),
        (0.5, 1.5, 1.5, 1.5),
        (1.25, 3.0, 3.0, 3.0),
        (1.5, 1.5, 2.5, 3.0),
        (1.875, 2.25, 1.75, 3.0),
        (2.25, 1.0, 1.0, 3.0),
        (2.5, 1.5, 1.5, 3.0),
    ];
    for (t, lp, rv, cl) in rows {
        assert_eq!(ctrl_time(t, 2.0, 0.5, 1.0, 3.0, 0x08), lp, "loop t={t}");
        assert_eq!(ctrl_time(t, 2.0, 0.5, 1.0, 3.0, 0x0A), rv, "reverse t={t}");
        assert_eq!(ctrl_time(t, 2.0, 0.5, 1.0, 3.0, 0x0C), cl, "clamp t={t}");
    }
    let back: [(u16, f32, f32); 10] = [
        (0x18, 0.0, 1.5),
        (0x18, 1.5, 2.5),
        (0x18, 1.875, 1.75),
        (0x18, 0.25, 3.0),
        (0x18, 1.25, 1.0),
        (0x1A, 0.0, 2.5),
        (0x1A, 1.875, 2.25),
        (0x1C, 0.0, 3.0),
        (0x1C, 2.5, 1.0),
        (0x1C, 0.5, 2.5),
    ];
    for (flags, t, want) in back {
        assert_eq!(
            ctrl_time(t, 2.0, 0.5, 1.0, 3.0, flags),
            want,
            "flags {flags:#x} t={t}"
        );
    }
    // start, stop, flags, t, expected
    let degenerate: [(f32, f32, u16, f32, f32); 9] = [
        (2.0, 2.0, 0x08, 5.0, 2.0),
        (2.0, 2.0, 0x0A, 5.0, 2.0),
        (2.0, 2.0, 0x0C, 5.0, 2.0),
        (2.0, 2.0, 0x0C, 2.0, 2.0),
        (3.0, 1.0, 0x08, 0.0, 3.0),
        (3.0, 1.0, 0x0A, 0.0, 3.0),
        (3.0, 1.0, 0x0C, 0.0, 3.0),
        (3.0, 1.0, 0x0C, 5.0, 1.0),
        (1.0, 3.0, 0x0E, 5.0, 3.0),
    ];
    for (start, stop, flags, t, want) in degenerate {
        assert_eq!(
            ctrl_time(t, 1.0, 0.0, start, stop, flags),
            want,
            "{start}..{stop} flags {flags:#x} t={t}"
        );
    }
}

#[test]
fn quadratic_is_hermite_with_unscaled_tangents() {
    let mk = |v: [f32; 4]| {
        let k = |t, v, f, b| Key {
            time: t,
            value: v,
            forward: Some(f),
            backward: Some(b),
            tbc: None,
        };
        (k(0.0, v[0], 100.0, 2.0), k(2.0, v[1], v[2], v[3]))
    };
    let (a, b) = mk([0.0, 1.0, 0.0, 100.0]);
    let g = group(2, vec![a, b]);
    assert_eq!(sample_f32(&g, 1.0), Some(0.75));
    assert_eq!(sample_f32(&g, 0.5), Some(0.4375));
    let v = group(
        2,
        g.keys
            .iter()
            .map(|k| Key {
                time: k.time,
                value: [k.value; 3],
                forward: k.forward.map(|f| [f; 3]),
                backward: k.backward.map(|f| [f; 3]),
                tbc: None,
            })
            .collect(),
    );
    assert_eq!(sample_vec3(&v, 1.0), Some([0.75; 3]));
}

#[test]
fn const_is_a_step() {
    let g = group(5, vec![key(0.0, 0.0), key(1.0, 5.0), key(2.0, 10.0)]);
    for (t, want) in [
        (0.9, 0.0),
        (1.0, 5.0),
        (1.99, 5.0),
        (2.0, 10.0),
        (3.0, 10.0),
    ] {
        assert_eq!(sample_f32(&g, t), Some(want), "t={t}");
    }
}

#[test]
fn before_first_and_after_last_clamp() {
    let g = group(1, vec![key(1.0, 10.0), key(3.0, 30.0)]);
    for (t, want) in [
        (0.0, 10.0),
        (1.0, 10.0),
        (2.0, 20.0),
        (3.0, 30.0),
        (9.0, 30.0),
    ] {
        assert_eq!(sample_f32(&g, t), Some(want), "t={t}");
    }
    let one = group(1, vec![key(5.0, 7.0)]);
    for t in [-1.0, 5.0, 100.0] {
        assert_eq!(sample_f32(&one, t), Some(7.0));
    }
    let empty: KeyGroup<f32> = KeyGroup {
        key_type: None,
        keys: vec![],
    };
    assert_eq!(sample_f32(&empty, 0.0), None);
    let v = group(1, vec![key(0.0, [0.0; 3]), key(2.0, [2.0, 4.0, 6.0])]);
    assert_eq!(sample_vec3(&v, 1.0), Some([1.0, 2.0, 3.0]));
}

#[test]
fn tbc_tension_flattens_the_tangents() {
    let mk = |tbc: [f32; 3]| {
        group(
            3,
            (0..3)
                .map(|i| Key {
                    tbc: Some(tbc),
                    ..key(i as f32, i as f32)
                })
                .collect(),
        )
    };
    assert_eq!(sample_f32(&mk([0.0; 3]), 0.25), Some(0.25));
    assert_eq!(sample_f32(&mk([1.0, 0.0, 0.0]), 0.25), Some(0.15625));
}

#[test]
fn tbc_bias_and_continuity_on_an_asymmetric_curve() {
    // Keys (0,0), (1,2), (2,3), every key tbc [T 0, B 0.5, C -0.25].
    // out coefficients: (1+B)(1+C)/2 = 1.5*0.75/2 = 0.5625 on prev, (1-B)(1-C)/2 = 0.5*1.25/2 = 0.3125 on next.
    // in coefficients:  (1+B)(1-C)/2 = 1.5*1.25/2 = 0.9375 on prev, (1-B)(1+C)/2 = 0.5*0.75/2 = 0.1875 on next.
    // key 0: prev := next = 2         -> out0 = (0.5625 + 0.3125) * 2 = 1.75
    // key 1: prev = 2, next = 1       -> in1 = 0.9375*2 + 0.1875*1 = 2.0625; out1 = 0.5625*2 + 0.3125*1 = 1.4375
    // key 2: next := prev = 1         -> in2 = 0.9375 + 0.1875 = 1.125
    // Hermite at x = 0.5: h00 = 0.5, h01 = 0.5, h10 = 0.125, h11 = -0.125.
    // t = 0.5: 0*0.5 + 2*0.5 + 1.75*0.125 - 2.0625*0.125 = 1 + 0.21875 - 0.2578125 = 0.9609375
    // t = 1.5: 2*0.5 + 3*0.5 + 1.4375*0.125 - 1.125*0.125 = 2.5 + 0.1796875 - 0.140625 = 2.5390625
    let g = group(
        3,
        [(0.0, 0.0), (1.0, 2.0), (2.0, 3.0)]
            .into_iter()
            .map(|(t, v)| Key {
                tbc: Some([0.0, 0.5, -0.25]),
                ..key(t, v)
            })
            .collect(),
    );
    assert_eq!(sample_f32(&g, 0.5), Some(0.9609375));
    assert_eq!(sample_f32(&g, 1.5), Some(2.5390625));
}

#[test]
fn decreasing_times_use_the_largest_index_at_or_before_t() {
    // largest i with time <= 1.5 is key 2 (time 1); segment (1,100)-(3,300) at x 0.25 -> 150
    let g = group(
        1,
        vec![
            key(0.0, 0.0),
            key(2.0, 20.0),
            key(1.0, 100.0),
            key(3.0, 300.0),
        ],
    );
    assert_eq!(sample_f32(&g, 1.5), Some(150.0));
    assert_eq!(sample_f32(&g, 1.0), Some(100.0));
}

#[test]
fn null_interpolator_changes_nothing() {
    let b = patch(&free_chain(), 1, 26, &(-1i32).to_le_bytes());
    let m = read_model(&b).unwrap();
    let tracks = free_tracks(&m, &b, 0);
    assert_eq!(tracks[0].ctrl, 1);
    assert_eq!(tracks[0].value, AnimValue::None);
    assert_eq!(tracks[0].keys, None);
    let base = base_of(&m, 0);
    for t in [0.0, 0.5, 1.0] {
        assert_eq!(apply_anim(&base, &tracks[..1], t), base, "t={t}");
    }
    let b = patch(&free_chain(), 3, 0, &0xFF7F_FFFFu32.to_le_bytes());
    let b = patch(&b, 3, 4, &(-1i32).to_le_bytes());
    let m = read_model(&b).unwrap();
    let tracks = free_tracks(&m, &b, 0);
    assert_eq!(tracks[0].ctrl, 1);
    assert_eq!(tracks[0].value, AnimValue::None);
    assert_eq!(tracks[0].keys, None);
}

#[test]
fn managed_flag_without_manager_freezes_at_property_value() {
    let b = patch(&free_chain(), 1, 4, &0x68u16.to_le_bytes());
    let m = read_model(&b).unwrap();
    let tracks = free_tracks(&m, &b, 0);
    assert_eq!(tracks.iter().map(|t| t.ctrl).collect::<Vec<_>>(), [2]);
    let out = apply_anim(&base_of(&m, 0), &tracks, 0.5);
    assert_eq!(out.params.emissive_multiple, 1.0);
    assert_eq!(out.params.uv_offset[0], 0.5);

    let b = free_chain();
    let m = read_model(&b).unwrap();
    let tracks = free_tracks(&m, &b, 0);
    assert_eq!(tracks.len(), 2);
    let out = apply_anim(&base_of(&m, 0), &tracks, 0.5);
    assert_eq!(out.params.emissive_multiple, 0.5);

    let b = patch(&free_chain(), 1, 4, &0x40u16.to_le_bytes());
    let m = read_model(&b).unwrap();
    let tracks = free_tracks(&m, &b, 0);
    assert_eq!(tracks.iter().map(|t| t.ctrl).collect::<Vec<_>>(), [2]);
}

fn tr(target: Target, value: AnimValue) -> Track {
    Track {
        ctrl: 1,
        target,
        flags: 0x48,
        freq: 1.0,
        phase: 0.0,
        start: 0.0,
        stop: 1.0,
        value,
        keys: None,
    }
}

#[test]
fn apply_anim_maps_each_target() {
    use AnimValue::{Vec3, F32};
    use Target::{AlphaRef, Lscc, Lscf};
    let mut base = MaterialState::builtin_default();
    base.params.trailer = LightingTrailer::EnvMap { scale: 1.0 };
    base.alpha = Some(Alpha {
        flags: 0x1ED,
        threshold: 0,
    });
    let run = |base: &MaterialState, tracks: &[Track]| apply_anim(base, tracks, 0.5);
    let one = |t: Target, v: AnimValue, edit: &dyn Fn(&mut MaterialState)| {
        let mut want = base.clone();
        edit(&mut want);
        assert_eq!(run(&base, &[tr(t, v)]), want, "{t:?} {v:?}");
    };
    one(Lscf(20), F32(0.25), &|s| s.params.uv_offset[0] = 0.25);
    one(Lscf(22), F32(0.25), &|s| s.params.uv_offset[1] = 0.25);
    one(Lscf(21), F32(0.25), &|s| s.params.uv_scale[0] = 0.25);
    one(Lscf(23), F32(0.25), &|s| s.params.uv_scale[1] = 0.25);
    one(Lscf(11), F32(2.5), &|s| s.params.emissive_multiple = 2.5);
    one(Lscc(1), Vec3([0.1, 0.2, 0.3]), &|s| {
        s.params.emissive_color = [0.1, 0.2, 0.3]
    });
    one(Lscf(9), F32(42.0), &|s| s.params.glossiness = 42.0);
    one(Lscf(10), F32(3.0), &|s| s.params.specular_strength = 3.0);
    one(Lscc(0), Vec3([0.1, 0.2, 0.3]), &|s| {
        s.params.specular_color = [0.1, 0.2, 0.3]
    });
    one(Lscf(12), F32(0.5), &|s| s.params.alpha = 0.5);
    one(Lscf(8), F32(0.75), &|s| {
        s.params.trailer = LightingTrailer::EnvMap { scale: 0.75 }
    });
    let mut plain = base.clone();
    plain.params.trailer = LightingTrailer::None;
    assert_eq!(run(&plain, &[tr(Lscf(8), F32(0.75))]), plain);
    let thr = |v: f32| run(&base, &[tr(AlphaRef, F32(v))]).alpha.unwrap().threshold;
    assert_eq!(thr(127.6), 128);
    assert_eq!(thr(300.0), 255);
    assert_eq!(thr(-4.0), 0);
    assert_eq!(thr(f32::NAN), 0);
    // a nonzero base shows NaN leaves the threshold alone (NaN as u8 would be 0)
    let mut seven = base.clone();
    seven.alpha = Some(Alpha {
        flags: 0x1ED,
        threshold: 7,
    });
    assert_eq!(run(&seven, &[tr(AlphaRef, F32(f32::NAN))]), seven);
    let mut no_alpha = base.clone();
    no_alpha.alpha = None;
    assert_eq!(run(&no_alpha, &[tr(AlphaRef, F32(100.0))]), no_alpha);
    for (t, v) in [
        (Lscf(0), F32(5.0)),
        (Lscf(3), F32(5.0)),
        (Lscf(11), Vec3([5.0; 3])),
        (Lscc(1), F32(5.0)),
        (AlphaRef, Vec3([5.0; 3])),
    ] {
        assert_eq!(run(&base, &[tr(t, v)]), base, "{t:?} {v:?}");
    }
    let two = [tr(Lscf(11), F32(1.0)), tr(Lscf(11), F32(2.0))];
    assert_eq!(run(&base, &two).params.emissive_multiple, 2.0);
}

#[test]
fn clip_tracks_bind_by_ref_then_by_strings() {
    let b = patch(&managed_clip(), 10, 0, &0.25f32.to_le_bytes());
    let b = patch(&b, 11, 0, &0.75f32.to_le_bytes());
    let b = patch(&b, 8, 6, &0.5f32.to_le_bytes());
    let m = read_model(&b).unwrap();
    let a = m.sequences.iter().find(|s| s.block == 2).unwrap();
    assert_eq!((a.cycle, a.frequency, a.start, a.stop), (0, 1.0, 0.0, 1.0));
    let want = Track {
        ctrl: 7,
        target: Target::Lscf(11),
        flags: 0x08,
        freq: 1.0,
        phase: 0.0,
        start: 0.0,
        stop: 1.0,
        value: AnimValue::F32(0.25),
        keys: None,
    };
    assert_eq!(clip_tracks(&m, &b, 2, 6), [want.clone(), want.clone()]);
    let bt = clip_tracks(&m, &b, 3, 6);
    assert_eq!(
        bt,
        [Track {
            value: AnimValue::F32(0.75),
            ..want.clone()
        }]
    );
    // C-P32: with no clip playing, the managed controller holds A's value (A is listed first, no keys)
    assert_eq!(
        free_tracks(&m, &b, 6),
        [Track {
            freq: 0.0,
            stop: 0.0,
            ..want.clone()
        }]
    );

    let strings = Header::parse(&b).unwrap().strings;
    let idx = |s: &[u8]| strings.iter().position(|x| x == s).unwrap() as i32;
    let b = patch(&b, 5, 0, &idx(b"shape").to_le_bytes());
    let b = patch(&b, 2, 16, &(-1i32).to_le_bytes());
    let m = read_model(&b).unwrap();
    assert_eq!(clip_tracks(&m, &b, 2, 6), [want.clone(), want]);
    let b = patch(&b, 2, 33, &idx(b"").to_le_bytes());
    let m = read_model(&b).unwrap();
    assert_eq!(clip_tracks(&m, &b, 2, 6).len(), 1);
}

#[test]
fn string_binding_compares_raw_name_bytes() {
    // "shape" -> "shap\xFF" in the header string table (first occurrence; the palette's copy comes later)
    let mut b = managed_clip();
    let at = b.windows(9).position(|w| w == b"\x05\0\0\0shape").unwrap();
    b[at + 8] = 0xFF;
    let strings = Header::parse(&b).unwrap().strings;
    let idx = strings.iter().position(|x| x == b"shap\xFF").unwrap() as i32;
    let b = patch(&b, 5, 0, &idx.to_le_bytes());
    let b = patch(&b, 2, 16, &(-1i32).to_le_bytes());
    let m = read_model(&b).unwrap();
    let a = &m.sequences.iter().find(|s| s.block == 2).unwrap().blocks[0];
    assert_eq!(a.node.as_deref(), Some(&b"shap\xFF"[..]));
    // CB 0 binds by strings, CB 1 by ref
    assert_eq!(clip_tracks(&m, &b, 2, 6).len(), 2);
}

const REAL: [&str; 5] = [
    "meshes/animobjects/dlc01bloodpotion.nif",
    "meshes/animobjects/ms12whitephialrepaired.nif",
    "meshes/_resourcepack/effects/rp_fxambsandpile01animated.nif",
    "meshes/creationclub/bgssse067/effects/lavabubbles01.nif",
    "meshes/architecture/solitude/clutter/slighthousefire.nif",
];

/// The emissive multiple the free preview (no clip playing) shows on `prop` at time `t`.
fn idle_emissive(b: &[u8], prop: u32, t: f32) -> f32 {
    let m = read_model(b).unwrap();
    apply_anim(&base_of(&m, prop), &free_tracks(&m, b, prop), t)
        .params
        .emissive_multiple
}

/// `animated(Some(1), 2)` with its shader chain converted into one clip `name` (LINEAR (0, 0) -> (1, 1) per
/// controller), the clip data's first key of Emissive multiple (4) set to 3.0 (its interpolator's Value stays 0).
fn converted(name: &str) -> Vec<u8> {
    use nif::edit::{anchor_role, apply, Op, Role};
    let b = builder::scenes::animated(Some(1), 2);
    let at = anchor_role(&read_model(&b).unwrap(), Some(2), Role::Shader).unwrap();
    let r = apply(
        &b,
        &[Op::ConvertChainToClips {
            at,
            name: name.into(),
        }],
    )
    .unwrap();
    let m = read_model(&r.bytes).unwrap();
    assert!(m.managed(4), "Emissive multiple is in the clip");
    let cb = m.sequences[0]
        .blocks
        .iter()
        .find(|cb| cb.controller == 4)
        .unwrap();
    let interp = &m.interps[&(cb.interp as u32)];
    assert_eq!(interp.value, [0.0]);
    // NiFloatData: count, key type, then key 0's time and value
    patch(&r.bytes, interp.data as u32, 12, &3.0f32.to_le_bytes())
}

/// C-P32 (IDLE-NM): with no clip playing, a managed controller holds the first key of its clip, not the property
/// value and not the clip interpolator's own Value.
#[test]
fn a_managed_controller_with_no_playing_clip_holds_its_clips_first_key() {
    let o = converted("Glow");
    let base = base_of(&read_model(&o).unwrap(), 3)
        .params
        .emissive_multiple;
    assert_ne!(base, 3.0);
    for t in [0.0, 0.5, 1.0, 7.25] {
        assert_eq!(idle_emissive(&o, 3, t), 3.0, "t={t}");
    }
}

/// C-P34 (a), AUTO-SI: under Free chains a SpecialIdle clip (ASCII case-insensitive) plays with its own timing.
#[test]
fn a_special_idle_clip_plays_under_free_chains() {
    for name in ["SpecialIdle", "specialIDLE"] {
        let o = converted(name);
        // keys (0, 3) -> (1, 1), Loop over 0..1
        for (t, want) in [(0.0, 3.0), (0.5, 2.0), (1.25, 2.5)] {
            assert_eq!(idle_emissive(&o, 3, t), want, "{name} t={t}");
        }
        let m = read_model(&o).unwrap();
        let tr = free_tracks(&m, &o, 3);
        assert!(tr.iter().all(|t| t.freq == 1.0 && t.stop == 1.0), "{tr:?}");
    }
}

/// C-P34 (a): a controller bound by SpecialIdle and by another clip listed before it plays SpecialIdle.
#[test]
fn special_idle_wins_over_an_earlier_listed_clip() {
    use nif::edit::{anchor_role, apply, Op, Role};
    let b = patch(&managed_clip(), 10, 0, &0.25f32.to_le_bytes());
    let b = patch(&b, 11, 0, &0.75f32.to_le_bytes());
    let at = anchor_role(&read_model(&b).unwrap(), None, Role::Root).unwrap();
    let r = apply(
        &b,
        &[Op::RenameSequence {
            at,
            name: "B".into(),
            nth: 0,
            new_name: "SpecialIdle".into(),
        }],
    )
    .unwrap();
    assert_eq!(idle_emissive(&r.bytes, 6, 0.5), 0.75);
    let m = read_model(&r.bytes).unwrap();
    let tr = free_tracks(&m, &r.bytes, 6);
    assert_eq!(tr.len(), 1);
    assert_eq!((tr[0].freq, tr[0].start, tr[0].stop), (1.0, 0.0, 1.0));
}

/// Note CU5 (CR14 parked): two SpecialIdle clips, or two managers, binding one controller both play under Free chains;
/// the track listed last wins (managers in block order, each in its list order).
#[test]
fn the_later_special_idle_track_wins() {
    use nif::edit::{anchor_role, apply, Op, Role};
    let b = patch(&managed_clip(), 10, 0, &0.25f32.to_le_bytes());
    let b = patch(&b, 11, 0, &0.75f32.to_le_bytes());
    let at = anchor_role(&read_model(&b).unwrap(), None, Role::Root).unwrap();
    let r = apply(
        &b,
        &[Op::RenameSequence {
            at,
            name: "B".into(),
            nth: 0,
            new_name: "SpecialIdle".into(),
        }],
    )
    .unwrap();
    // A gets B's Name index too: Rename refuses a duplicate name
    let h = Header::parse(&r.bytes).unwrap();
    let name_b = r.bytes[h.offsets()[3]..h.offsets()[3] + 4].to_vec();
    let both = patch(&r.bytes, 2, 0, &name_b);
    let tracks = |b: &[u8]| free_tracks(&read_model(b).unwrap(), b, 6).len();
    assert!(tracks(&both) > tracks(&r.bytes), "A plays too");
    assert_eq!(idle_emissive(&both, 6, 0.5), 0.75, "B is listed last");
    // a second manager listing [B, A] after the first one's [A, B]: A is last
    let h = Header::parse(&both).unwrap();
    let (s, n) = (h.offsets()[1], h.sizes[1] as usize);
    let mut body = both[s..s + n].to_vec();
    body[31..39].copy_from_slice(&[3i32.to_le_bytes(), 2i32.to_le_bytes()].concat());
    let mut d = NifDoc::open(&both).unwrap();
    d.append(b"NiControllerManager", body);
    let two = d.bytes();
    assert!(
        tracks(&two) > tracks(&both),
        "the second manager plays its clips too"
    );
    assert_eq!(
        idle_emissive(&two, 6, 0.5),
        0.25,
        "the second manager's last clip"
    );
}

/// C-P32: the first sequence in the manager's list that binds the controller gives the held value (an interpolator
/// without keys holds its Value); without a manager (G2) the property value stays.
#[test]
fn the_first_manager_listed_clip_gives_the_held_value() {
    let b = patch(&managed_clip(), 10, 0, &0.25f32.to_le_bytes());
    let b = patch(&b, 11, 0, &0.75f32.to_le_bytes());
    assert_eq!(idle_emissive(&b, 6, 0.5), 0.25, "A is listed first");
    // manager body: 26 controller bytes, cumulative u8, count u32, then the sequence refs
    let ba = patch(
        &b,
        1,
        31,
        &[3i32.to_le_bytes(), 2i32.to_le_bytes()].concat(),
    );
    assert_eq!(idle_emissive(&ba, 6, 0.5), 0.75, "B is listed first");

    let p = builder::scenes::managed_pair();
    let m = read_model(&p).unwrap();
    assert!(m.managed(5) && !m.sequences.is_empty());
    assert!(free_tracks(&m, &p, 4).is_empty(), "no manager: G2");
}

fn loose(root: &std::path::Path, rel: &str) -> Option<Vec<u8>> {
    std::fs::read_dir(root.join("mods"))
        .ok()?
        .flatten()
        .find_map(|m| std::fs::read(m.path().join(rel)).ok())
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// The key a sample at `keys[i].time` must return: the first key at the first time, else the last key at that time.
fn expected_at<T: Copy>(keys: &[Key<T>], i: usize) -> T {
    let t = keys[i].time;
    if t <= keys[0].time {
        return keys[0].value;
    }
    keys.iter().rev().find(|k| k.time == t).unwrap().value
}

/// The fields `target` writes, for the finiteness check.
fn written(s: &MaterialState, target: Target) -> Vec<f32> {
    let p = &s.params;
    match target {
        Target::Lscf(20) => vec![p.uv_offset[0]],
        Target::Lscf(22) => vec![p.uv_offset[1]],
        Target::Lscf(21) => vec![p.uv_scale[0]],
        Target::Lscf(23) => vec![p.uv_scale[1]],
        Target::Lscf(11) => vec![p.emissive_multiple],
        Target::Lscc(1) => p.emissive_color.to_vec(),
        Target::Lscf(9) => vec![p.glossiness],
        Target::Lscf(10) => vec![p.specular_strength],
        Target::Lscc(0) => p.specular_color.to_vec(),
        Target::Lscf(12) => vec![p.alpha],
        Target::Lscf(8) => match p.trailer {
            LightingTrailer::EnvMap { scale } => vec![scale],
            _ => vec![],
        },
        _ => vec![],
    }
}

#[test]
#[ignore]
fn real_curves_evaluate() {
    let Some(root) = std::env::var_os("SDC_ACHERUS") else {
        println!("skipped: set SDC_ACHERUS");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let (mut found, mut keyed) = (0, 0);
    for rel in REAL {
        let Some(b) = loose(&root, rel) else {
            println!("MISSING {rel}");
            continue;
        };
        found += 1;
        let m = read_model(&b).unwrap();
        let mut props: Vec<(u32, MaterialState)> = vec![];
        for (&p, l) in &m.lighting {
            if !l.prop.ctrls.is_empty() {
                props.push((p, base_of(&m, p)));
            }
        }
        for (&p, a) in &m.alphas {
            if !a.prop.ctrls.is_empty() {
                let mut s = MaterialState::builtin_default();
                s.alpha = Some(a.alpha);
                props.push((p, s));
            }
        }
        let mut line = vec![];
        for (p, base) in props {
            let tracks = free_tracks(&m, &b, p);
            for tr in &tracks {
                let (kt, n, finite) = match &tr.keys {
                    Some(KeyData::Float(g)) if !g.keys.is_empty() => {
                        for i in 0..g.keys.len() {
                            let got = sample_f32(g, g.keys[i].time).unwrap();
                            let want = expected_at(&g.keys, i);
                            assert!(same(got, want), "{rel} ctrl {} key {i}", tr.ctrl);
                        }
                        let fin = g.keys.iter().all(|k| {
                            k.value.is_finite()
                                && k.forward.is_none_or(f32::is_finite)
                                && k.backward.is_none_or(f32::is_finite)
                        });
                        (g.key_type, g.keys.len(), fin)
                    }
                    Some(KeyData::Pos(g)) if !g.keys.is_empty() => {
                        for i in 0..g.keys.len() {
                            let got = sample_vec3(g, g.keys[i].time).unwrap();
                            let want = expected_at(&g.keys, i);
                            assert!(
                                (0..3).all(|c| same(got[c], want[c])),
                                "{rel} ctrl {} key {i}",
                                tr.ctrl
                            );
                        }
                        let ok = |v: &[f32; 3]| v.iter().all(|x| x.is_finite());
                        let fin = g.keys.iter().all(|k| {
                            ok(&k.value)
                                && k.forward.as_ref().is_none_or(ok)
                                && k.backward.as_ref().is_none_or(ok)
                        });
                        (g.key_type, g.keys.len(), fin)
                    }
                    _ => {
                        let fin = match tr.value {
                            AnimValue::F32(v) => v.is_finite(),
                            AnimValue::Vec3(v) => v.iter().all(|x| x.is_finite()),
                            AnimValue::None => true,
                        };
                        (None, 0, fin)
                    }
                };
                if n > 0 {
                    keyed += 1;
                }
                line.push(format!(
                    "prop {p} ctrl {} {:?} type {kt:?} keys {n}",
                    tr.ctrl, tr.target
                ));
                if !finite {
                    continue;
                }
                let (lo, hi) = (tr.start - 1.0, 2.0 * tr.stop + 1.0);
                for s in 0..64 {
                    let t = lo + (hi - lo) * s as f32 / 63.0;
                    let out = apply_anim(&base, std::slice::from_ref(tr), t);
                    let vals = written(&out, tr.target);
                    assert!(
                        vals.iter().all(|v| v.is_finite()),
                        "{rel} ctrl {} t={t}: {vals:?}",
                        tr.ctrl
                    );
                }
            }
        }
        println!("{rel}: {} tracks; {}", line.len(), line.join("; "));
    }
    assert!(found > 0, "no real file found");
    assert!(keyed > 0, "no track had keys");
}
