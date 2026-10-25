//! The dialog's greying rules (spec + plan ruling R1), the quality lists (R4) and the profile overrides (R2, R3).
use engine::config::{
    AlphaPolicy, Backend, ColorContent, ColorTag, Effort, Format, MipFilter, MipKeyword, MipLevels,
    Profile, SearchRadius, Subtype, Target,
};
use engine::presets::PresetRegistry;
use psdds::settings::profile_for;
use psdds::{
    psdds_defaults, psdds_quality_count, psdds_quality_label, psdds_resolve, PsddsControls,
    PsddsSettings,
};
use std::ffi::CStr;

const BC1: u32 = 0;
const BC3: u32 = 1;
const BC4: u32 = 2;
const BC5: u32 = 3;
const BC7: u32 = 4;
const RGBA8: u32 = 5;
const FORMATS: [u32; 6] = [BC1, BC3, BC4, BC5, BC7, RGBA8];
const COLOUR: u32 = 0;
const NORMAL: u32 = 1;
const MASK: u32 = 2;
const FILTERS: [MipFilter; 4] = [
    MipFilter::Box,
    MipFilter::Tent,
    MipFilter::Mitchell,
    MipFilter::Kaiser,
];

fn s(format: u32, texture_type: u32, srgb: u32, filter: u32, quality: u32) -> PsddsSettings {
    PsddsSettings {
        format,
        texture_type,
        srgb,
        filter,
        mips: 1,
        quality,
    }
}

fn zero() -> PsddsSettings {
    s(99, 99, 99, 99, 99)
}

fn resolve(input: PsddsSettings) -> (i32, PsddsSettings, PsddsControls) {
    let mut e = zero();
    let mut c = PsddsControls {
        srgb_enabled: 9,
        filter_enabled: 9,
        quality_enabled: 9,
    };
    let rc = unsafe { psdds_resolve(&input, &mut e, &mut c) };
    (rc, e, c)
}

fn base(texture_type: u32) -> Profile {
    let st = [Subtype::Diffuse, Subtype::Normal, Subtype::GenericData][texture_type as usize];
    PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, st)
        .unwrap()
}

fn preset_filter(texture_type: u32) -> u32 {
    let f = base(texture_type).mips.filter;
    FILTERS.iter().position(|&m| m == f).unwrap() as u32
}

#[test]
fn normal_and_mask_grey_srgb_and_filter() {
    for ty in [NORMAL, MASK] {
        for f in FORMATS {
            let (rc, e, c) = resolve(s(f, ty, 1, 3, 0));
            assert_eq!(rc, 0);
            assert_eq!(
                (c.srgb_enabled, c.filter_enabled),
                (0, 0),
                "type {ty} format {f}"
            );
            assert_eq!(e.srgb, 0);
            assert_eq!(e.filter, preset_filter(ty));
            let p = profile_for(&s(f, ty, 1, 3, 0), false).unwrap();
            assert_eq!(p.color.tag, ColorTag::Unorm);
            assert_eq!(p.mips.filter, base(ty).mips.filter);
        }
    }
}

#[test]
fn colour_keeps_srgb_and_filter_editable() {
    for f in [BC1, BC3, BC7, RGBA8] {
        for (srgb, filter) in [(1, 2), (0, 3), (1, 0)] {
            let (rc, e, c) = resolve(s(f, COLOUR, srgb, filter, 0));
            assert_eq!(rc, 0);
            assert_eq!((c.srgb_enabled, c.filter_enabled), (1, 1), "format {f}");
            assert_eq!((e.srgb, e.filter), (srgb, filter));
            let p = profile_for(&s(f, COLOUR, srgb, filter, 0), false).unwrap();
            // The control sets both content and tag.
            let colour = if srgb == 1 {
                (ColorContent::Srgb, ColorTag::Srgb)
            } else {
                (ColorContent::Linear, ColorTag::Unorm)
            };
            assert_eq!((p.color.content, p.color.tag), colour);
            assert_eq!(p.mips.filter, FILTERS[filter as usize]);
        }
    }
}

#[test]
fn srgb_forced_off_without_srgb_variant() {
    for f in [BC4, BC5] {
        let (rc, e, c) = resolve(s(f, COLOUR, 1, 2, 0));
        assert_eq!(rc, 0);
        assert_eq!(e.srgb, 0);
        assert_eq!(c.srgb_enabled, 0);
        assert_eq!(c.filter_enabled, 1);
        assert_eq!(e.filter, 2);
        let p = profile_for(&s(f, COLOUR, 1, 2, 0), false).unwrap();
        assert_eq!(
            (p.color.content, p.color.tag),
            (ColorContent::Linear, ColorTag::Unorm)
        );
    }
}

#[test]
fn stale_quality_falls_back_to_the_preset() {
    // Preset quality index per format for cs-quality: rgbcx 18, radius default, effort veryslow.
    let preset = |f: u32, ty: u32| {
        let b = base(ty);
        let radii = [SearchRadius::Default, SearchRadius::Max, SearchRadius::Fast];
        match f {
            BC1 | BC3 => b.encoder.rgbcx_level,
            BC4 | BC5 => radii
                .iter()
                .position(|&r| r == b.encoder.bc45_search_radius)
                .unwrap() as u32,
            BC7 => 4,
            _ => 0,
        }
    };
    for ty in [COLOUR, NORMAL, MASK] {
        // A BC7 Veryslow (4) remembered and the format switched to BC4.
        let (rc, e, _) = resolve(s(BC4, ty, 0, 0, 4));
        assert_eq!(rc, 0);
        assert_eq!(e.quality, preset(BC4, ty));
        for (f, q) in [(BC5, 3), (BC1, 19), (BC3, 99), (BC7, 7), (RGBA8, 7)] {
            let (rc, e, _) = resolve(s(f, ty, 0, 0, q));
            assert_eq!(
                (rc, e.quality),
                (0, preset(f, ty)),
                "format {f} quality {q}"
            );
            assert!(profile_for(&s(f, ty, 0, 0, q), false).is_ok());
        }
    }
}

#[test]
fn rgba8_greys_quality() {
    assert_eq!(psdds_quality_count(RGBA8), 0);
    assert!(psdds_quality_label(RGBA8, 0).is_null());
    // A quality index remembered from another format is not an error: the control is greyed and shows nothing.
    let (rc, e, c) = resolve(s(RGBA8, COLOUR, 1, 0, 7));
    assert_eq!(rc, 0);
    assert_eq!((c.quality_enabled, e.quality), (0, 0));
    for f in [BC1, BC3, BC4, BC5, BC7] {
        assert_eq!(resolve(s(f, COLOUR, 1, 0, 0)).2.quality_enabled, 1);
    }
}

fn labels(format: u32) -> Vec<String> {
    (0..psdds_quality_count(format))
        .map(|i| {
            let p = psdds_quality_label(format, i);
            assert!(!p.is_null());
            unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned()
        })
        .collect()
}

#[test]
fn quality_lists_are_pinned() {
    let effort = [
        "Ultrafast",
        "Veryfast",
        "Fast",
        "Basic",
        "Veryslow",
        "Slowest",
        "Max",
    ];
    // The GUI's rgbcx slider is 0..=RGBCX_MAX_LEVEL; psdds copies that range (it cannot name encoders-sys).
    let levels: Vec<String> = (0..=encoders_sys::RGBCX_MAX_LEVEL)
        .map(|n| n.to_string())
        .collect();
    let radius = ["Default", "Maximum", "Fast (preview)"];
    assert_eq!(labels(BC7), effort);
    assert_eq!(labels(BC1), levels);
    assert_eq!(labels(BC3), levels);
    assert_eq!(labels(BC4), radius);
    assert_eq!(labels(BC5), radius);
    assert!(labels(RGBA8).is_empty());
    for f in FORMATS {
        assert!(psdds_quality_label(f, psdds_quality_count(f)).is_null());
    }
    // And each index sets the engine field it names.
    let efforts = [
        Effort::Ultrafast,
        Effort::Veryfast,
        Effort::Fast,
        Effort::Basic,
        Effort::Veryslow,
        Effort::Slowest,
        Effort::Max,
    ];
    for (i, e) in efforts.iter().enumerate() {
        let p = profile_for(&s(BC7, COLOUR, 1, 0, i as u32), false).unwrap();
        assert_eq!(p.encoder.effort, *e);
    }
    for i in 0..=18 {
        assert_eq!(
            profile_for(&s(BC1, COLOUR, 1, 0, i), false)
                .unwrap()
                .encoder
                .rgbcx_level,
            i
        );
    }
    let radii = [SearchRadius::Default, SearchRadius::Max, SearchRadius::Fast];
    for (i, r) in radii.iter().enumerate() {
        let p = profile_for(&s(BC5, NORMAL, 0, 0, i as u32), false).unwrap();
        assert_eq!(p.encoder.bc45_search_radius, *r);
    }
}

#[test]
fn out_of_range_field_is_refused() {
    let ok = s(BC7, COLOUR, 1, 0, 4);
    assert_eq!(resolve(ok).0, 0);
    let bad = [
        PsddsSettings { format: 6, ..ok },
        PsddsSettings {
            texture_type: 3,
            ..ok
        },
        PsddsSettings { srgb: 2, ..ok },
        PsddsSettings { filter: 4, ..ok },
        PsddsSettings { mips: 2, ..ok },
    ];
    for b in bad {
        assert_eq!(resolve(b).0, -1, "{b:?}");
        assert!(profile_for(&b, false).is_err(), "{b:?}");
    }
    let mut e = zero();
    let mut c = PsddsControls {
        srgb_enabled: 0,
        filter_enabled: 0,
        quality_enabled: 0,
    };
    unsafe {
        assert_eq!(psdds_resolve(std::ptr::null(), &mut e, &mut c), -1);
        assert_eq!(psdds_resolve(&ok, std::ptr::null_mut(), &mut c), -1);
        assert_eq!(psdds_resolve(&ok, &mut e, std::ptr::null_mut()), -1);
    }
    assert_eq!(psdds_quality_count(6), 0);
    assert!(psdds_quality_label(6, 0).is_null());
}

#[test]
fn defaults_follow_the_preset() {
    for ty in [COLOUR, NORMAL, MASK] {
        let d = psdds_defaults(ty);
        let b = base(ty);
        assert_eq!(d.format, BC7);
        assert_eq!(d.texture_type, ty);
        assert_eq!(d.srgb, u32::from(ty == COLOUR));
        assert_eq!(d.filter, preset_filter(ty));
        assert_eq!(d.mips, 1);
        assert_eq!(
            profile_for(&d, false).unwrap().encoder.effort,
            b.encoder.effort
        );
        assert_eq!(resolve(d), (0, d, resolve(d).2));
    }
}

#[test]
fn profile_overrides_follow_r2_r3() {
    let backends = [
        Backend::Rgbcx,
        Backend::Rgbcx,
        Backend::Rgbcx,
        Backend::Rgbcx,
        Backend::Bc7e,
        Backend::Uncompressed,
    ];
    let formats = [
        Format::Bc1,
        Format::Bc3,
        Format::Bc4,
        Format::Bc5,
        Format::Bc7,
        Format::Rgba8,
    ];
    for ty in [COLOUR, NORMAL, MASK] {
        for f in FORMATS {
            for has_alpha in [false, true] {
                let mut set = psdds_defaults(ty);
                set.format = f;
                set.quality = 0;
                set.mips = 0;
                let p = profile_for(&set, has_alpha).unwrap();
                let b = base(ty);
                assert_eq!(p.subtype, b.subtype);
                assert_eq!(p.format, formats[f as usize]);
                assert_eq!(p.encoder.backend, backends[f as usize]);
                assert_eq!(p.mips.levels, MipLevels::Keyword(MipKeyword::None));
                // Normal and Linear mask: sRGB greyed Off → linear content, UNORM tag.
                if ty != COLOUR {
                    assert_eq!(
                        (p.color.content, p.color.tag),
                        (ColorContent::Linear, ColorTag::Unorm)
                    );
                }
                let keeps = has_alpha && matches!(f, BC3 | BC7 | RGBA8);
                let policy = if keeps {
                    AlphaPolicy::Blend
                } else {
                    AlphaPolicy::Remove
                };
                assert_eq!(
                    p.alpha.policy, policy,
                    "type {ty} format {f} alpha {has_alpha}"
                );
                assert!(p.resize.is_none() && !p.drop_top_mip);
            }
        }
    }
}
