//! Cubemap sub-type and its `cubemap` profile section (cubemap support v1, DECISIONS Note DN).
mod cube_support;
use cube_support::*;
use engine::config::*;
use engine::cube::layout::{self, cells, detect, import_raw};
use engine::image::Rgba32F;
use engine::policy::allowed_encodings;
use engine::presets::PresetRegistry;

fn builtin_ids(reg: &PresetRegistry) -> Vec<String> {
    let mut ids: Vec<String> = reg.selectable().iter().map(|p| p.id.clone()).collect();
    ids.push("sdc-base".into());
    ids
}

#[test]
fn default_cubemap_section_is_not_serialised() {
    let reg = PresetRegistry::builtin().unwrap();
    for id in builtin_ids(&reg) {
        for t in [Target::Cs, Target::Vanilla] {
            for st in Subtype::ALL {
                let p = reg.profile(&id, t, st).unwrap();
                let v = serde_json::to_value(&p).unwrap();
                assert!(v.get("cubemap").is_none(), "{id}/{t:?}/{st:?}");
            }
        }
    }
}

#[test]
fn cubemap_profile_defaults() {
    let reg = PresetRegistry::builtin().unwrap();
    let p = reg
        .profile("cs-quality", Target::Cs, Subtype::Cubemap)
        .unwrap();
    assert_eq!(p.format, Format::Bc7);
    assert_eq!(
        (p.color.content, p.color.tag),
        (ColorContent::Srgb, ColorTag::Unorm)
    );
    assert_eq!(p.alpha.policy, AlphaPolicy::Remove);
    assert_eq!(p.mips.levels, MipLevels::Keyword(MipKeyword::Full));
    assert_eq!(p.category, Category::Color);
}

#[test]
fn cubemap_encodings_are_unorm_bc1_bc3_bc7_rgba8() {
    for t in [Target::Cs, Target::Vanilla] {
        assert_eq!(
            allowed_encodings(Subtype::Cubemap, t),
            vec![
                (Format::Bc1, ColorTag::Unorm),
                (Format::Bc3, ColorTag::Unorm),
                (Format::Bc7, ColorTag::Unorm),
                (Format::Rgba8, ColorTag::Unorm),
            ],
            "{t:?}"
        );
    }
}

#[test]
fn detect_exact_ratios_only() {
    assert_eq!(detect(1024, 768), Some(CubeLayout::Hcross));
    assert_eq!(detect(768, 1024), Some(CubeLayout::Vcross));
    assert_eq!(detect(1536, 256), Some(CubeLayout::Hstrip));
    assert_eq!(detect(256, 1536), Some(CubeLayout::Vstrip));
    for (w, h) in [
        (512, 512),
        (2048, 1024),
        (4096, 2049),
        (1025, 768),
        (6145, 1024),
    ] {
        assert_eq!(detect(w, h), None, "{w}x{h}");
    }
}

#[test]
fn every_raw_layout_round_trips_the_arrow_cube() {
    let faces = arrow_faces(64);
    let cases = [
        (CubeLayout::Hcross, hcross(&faces)),
        (CubeLayout::Vcross, vcross(&faces, true)),
        (CubeLayout::Hstrip, hstrip(&faces)),
        (CubeLayout::Vstrip, vstrip(&faces)),
    ];
    for (layout, img) in cases {
        let got = import_raw(&img, layout, &CubemapSpec::default(), true).unwrap();
        assert!(got == faces, "{layout:?}");
        assert_eq!(cells(layout, img.width, img.height).unwrap().0, 64);
    }
}

#[test]
fn vcross_negz_rotate_toggle() {
    let faces = arrow_faces(32);
    let spec = CubemapSpec::default();
    assert!(import_raw(&vcross(&faces, true), CubeLayout::Vcross, &spec, true).unwrap() == faces);
    let spec = CubemapSpec {
        vcross_negz_rotate: false,
        ..CubemapSpec::default()
    };
    assert!(import_raw(&vcross(&faces, false), CubeLayout::Vcross, &spec, true).unwrap() == faces);
    // Left on, the unrotated -Z comes out turned.
    let got = import_raw(
        &vcross(&faces, false),
        CubeLayout::Vcross,
        &CubemapSpec::default(),
        true,
    )
    .unwrap();
    assert!(got[5] == turn180(&faces[5]));
}

#[test]
fn face_rotate_and_flip_correct_a_turned_face() {
    let faces = arrow_faces(32);
    let n = 32;
    // A clockwise quarter turn sends the top (white) row to the right column.
    let mut turned = faces.clone();
    let mut t = Rgba32F::new(n, n, [0.0; 4]);
    for y in 0..n {
        for x in 0..n {
            *t.at_mut(x, y) = faces[2].at(y, n - 1 - x);
        }
    }
    assert_eq!(t.at(n - 1, 5), [1.0; 4]);
    turned[2] = t;
    let mut spec = CubemapSpec::default();
    spec.face_rotate[2] = 270;
    assert!(import_raw(&hstrip(&turned), CubeLayout::Hstrip, &spec, false).unwrap() == faces);

    let mut flipped = faces.clone();
    for y in 0..n {
        for x in 0..n {
            *flipped[3].at_mut(x, y) = faces[3].at(n - 1 - x, y);
        }
    }
    assert!(flipped[3] != faces[3]);
    let mut spec = CubemapSpec::default();
    spec.face_flip[3] = FaceFlip::H;
    assert!(import_raw(&hstrip(&flipped), CubeLayout::Hstrip, &spec, false).unwrap() == faces);
    // And a vertical flip is its own inverse on a v-flipped face.
    let mut vflipped = faces.clone();
    for y in 0..n {
        for x in 0..n {
            *vflipped[0].at_mut(x, y) = faces[0].at(x, n - 1 - y);
        }
    }
    let mut spec = CubemapSpec::default();
    spec.face_flip[0] = FaceFlip::V;
    assert!(import_raw(&hstrip(&vflipped), CubeLayout::Hstrip, &spec, false).unwrap() == faces);
}

#[test]
fn photo_shaped_like_a_cross_is_refused_when_auto() {
    let (w, h) = (64, 48);
    let mut seed = 12345u32;
    let mut img = Rgba32F::new(w, h, [0.0; 4]);
    for p in &mut img.px {
        for c in &mut p[..3] {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *c = (seed >> 24) as f32 / 255.0;
        }
        p[3] = 1.0;
    }
    let spec = CubemapSpec::default();
    let e = import_raw(&img, CubeLayout::Hcross, &spec, true).unwrap_err();
    assert!(e.contains("cubemap.layout"), "{e}");
    assert!(import_raw(&img, CubeLayout::Hcross, &spec, false).is_ok());
    // Faces is not a single-image layout.
    assert!(import_raw(&img, CubeLayout::Faces, &spec, false).is_err());
}

#[test]
fn face_size_rule() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg
        .profile("cs-quality", Target::Cs, Subtype::Cubemap)
        .unwrap();
    assert_eq!(layout::face_size(&p, CubeLayout::Hcross, (600, 450)), 128);
    p.cubemap.face_size = Some(64);
    assert_eq!(layout::face_size(&p, CubeLayout::Hcross, (600, 450)), 64);
}

#[test]
fn face_tags_parse_both_spellings() {
    assert_eq!(layout::face_tag("chrome_e_posx"), Some(("chrome_e", 0)));
    assert_eq!(layout::face_tag("chrome_e_NZ"), Some(("chrome_e", 5)));
    assert_eq!(layout::face_tag("chrome_e"), None);
    assert_eq!(layout::face_tag("_px"), None);
}
