//! `memory::encode_image` must write the bytes the file pipeline writes for the same pixels and profile. The
//! reference is a real batch run (PNG on disk, `Engine::plan` + `Engine::run`, as `golden.rs` drives it) with every
//! field pinned in a temp `sdc.toml`; no user config or presets folder is passed, so nothing under %APPDATA% or
//! %LOCALAPPDATA% is read or written.
use engine::api::*;
use engine::codes::Code;
use engine::config::{
    AlphaPolicy, Backend, ColorContent, ColorTag, Effort, Format, MipFilter, MipKeyword, MipLevels,
    Subtype, Target,
};
use engine::events::*;
use engine::image::Rgba32F;
use engine::memory::encode_image;
use engine::presets::PresetRegistry;
use std::sync::Arc;

struct Case {
    name: &'static str,
    subtype: Subtype,
    format: Format,
    content: ColorContent,
    tag: ColorTag,
    policy: AlphaPolicy,
    backend: Backend,
    alpha: bool,
}

fn gradient(alpha: bool) -> Rgba32F {
    let (w, h) = (64u32, 32u32);
    let mut img = Rgba32F::new(w, h, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            let c = |v: u32| (v % 256) as f32 / 255.0;
            let a = if alpha { c(255 - x * 3) } else { 1.0 };
            img.px[(y * w + x) as usize] = [c(x * 4), c(y * 8), c(x * 2 + y * 3), a];
        }
    }
    img
}

fn save_png(img: &Rgba32F, path: &std::path::Path, alpha: bool) {
    let b = |v: f32| (v * 255.0).round() as u8;
    if alpha {
        let raw: Vec<u8> = img.px.iter().flat_map(|p| p.map(b)).collect();
        image::RgbaImage::from_raw(img.width, img.height, raw)
            .unwrap()
            .save(path)
            .unwrap();
    } else {
        let raw: Vec<u8> = img
            .px
            .iter()
            .flat_map(|p| [b(p[0]), b(p[1]), b(p[2])])
            .collect();
        image::RgbImage::from_raw(img.width, img.height, raw)
            .unwrap()
            .save(path)
            .unwrap();
    }
}

fn serde_name<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn encode_image_matches_the_file_pipeline() {
    use AlphaPolicy::*;
    let cases = [
        Case {
            name: "d_bc7",
            subtype: Subtype::Diffuse,
            format: Format::Bc7,
            content: ColorContent::Srgb,
            tag: ColorTag::Srgb,
            policy: Remove,
            backend: Backend::Bc7e,
            alpha: false,
        },
        Case {
            name: "d_bc1",
            subtype: Subtype::Diffuse,
            format: Format::Bc1,
            content: ColorContent::Linear,
            tag: ColorTag::Unorm,
            policy: Remove,
            backend: Backend::Rgbcx,
            alpha: false,
        },
        Case {
            name: "n_bc5",
            subtype: Subtype::Normal,
            format: Format::Bc5,
            content: ColorContent::Linear,
            tag: ColorTag::Unorm,
            policy: Remove,
            backend: Backend::Rgbcx,
            alpha: false,
        },
        Case {
            name: "g_bc4",
            subtype: Subtype::GenericData,
            format: Format::Bc4,
            content: ColorContent::Linear,
            tag: ColorTag::Unorm,
            policy: Remove,
            backend: Backend::Rgbcx,
            alpha: false,
        },
        Case {
            name: "d_rgba8",
            subtype: Subtype::Diffuse,
            format: Format::Rgba8,
            content: ColorContent::Srgb,
            tag: ColorTag::Srgb,
            policy: Blend,
            backend: Backend::Uncompressed,
            alpha: true,
        },
    ];
    let reg = PresetRegistry::builtin().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    std::fs::create_dir_all(&input).unwrap();
    let mut toml = String::new();
    let mut expected = Vec::new();
    for c in &cases {
        let img = gradient(c.alpha);
        save_png(&img, &input.join(format!("{}.png", c.name)), c.alpha);
        // The profile, built from the preset, then pinned field by field ...
        let mut p = reg.profile("cs-quality", Target::Cs, c.subtype).unwrap();
        p.format = c.format;
        p.color.content = c.content;
        p.color.tag = c.tag;
        p.alpha.policy = c.policy;
        p.mips.levels = MipLevels::Keyword(MipKeyword::Full);
        p.mips.filter = MipFilter::Box;
        p.encoder.effort = Effort::Fast;
        p.encoder.backend = c.backend;
        // ... and the same pins in the project config for the file run.
        toml += &format!(
            "[files.\"{}.png\"]\nsubtype = \"{}\"\nformat = \"{}\"\ncolor.content = \"{}\"\ncolor.tag = \"{}\"\n\
             alpha.policy = \"{}\"\nmips.levels = \"full\"\nmips.filter = \"box\"\nencoder.effort = \"fast\"\n\
             encoder.backend = \"{}\"\n\n",
            c.name,
            serde_name(&c.subtype),
            serde_name(&c.format),
            serde_name(&c.content),
            serde_name(&c.tag),
            serde_name(&c.policy),
            serde_name(&c.backend),
        );
        expected.push((c.name, encode_image(img, &p).unwrap(), p));
    }
    std::fs::write(input.join("sdc.toml"), toml).unwrap();

    let mut req = BatchRequest::new(vec![input.clone()], dir.path().join("out"), Target::Cs);
    req.preset = Some("cs-quality".into());
    req.on_existing = OnExisting::Overwrite;
    // Acknowledgements for the blocking checks the gradients trip (sRGB on a non-PBR diffuse, BC5 normal); they
    // gate the write and change no encoded byte.
    req.allow = [Code::TagSrgbNonpbr, Code::FmtBc5Normal].into();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "memory");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    e.run(plan, &rep, &CancelToken::default()).unwrap();
    let events = sink.take();

    for (name, bytes, p) in expected {
        // The pins reproduce the profile exactly, so equal bytes are a real comparison.
        let planned = events
            .iter()
            .find_map(|ev| match &ev.kind {
                EventKind::FilePlanned { job }
                    if ev
                        .path
                        .as_deref()
                        .is_some_and(|q| q.ends_with(&format!("{name}.png"))) =>
                {
                    Some(serde_json::to_value(&**job).unwrap())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name} was not planned"));
        assert_eq!(
            planned["profile"],
            serde_json::to_value(&p).unwrap(),
            "{name}: profile"
        );
        let file = std::fs::read(dir.path().join("out").join(format!("{name}.dds")))
            .unwrap_or_else(|e| {
                panic!(
                    "{name}: {e}; {:?}",
                    std::fs::read_dir(dir.path().join("out"))
                        .map(|d| d.map(|x| x.unwrap().path()).collect::<Vec<_>>())
                )
            });
        assert!(
            file == bytes,
            "{name}: {} vs {} bytes, content differs",
            file.len(),
            bytes.len()
        );
    }
}

/// A Diffuse cs-quality profile with no mips and the given format; `encode_image_with` takes the source transfer.
fn flat_profile(format: Format) -> engine::config::Profile {
    let reg = PresetRegistry::builtin().unwrap();
    let mut p = reg
        .profile("cs-quality", Target::Cs, Subtype::Diffuse)
        .unwrap();
    p.format = format;
    p.mips.levels = MipLevels::Keyword(MipKeyword::None);
    p.alpha.policy = AlphaPolicy::Remove;
    if format == Format::Bc6hUf16 {
        p.encoder.backend = Backend::IspcBc6h;
        p.color.tag = ColorTag::NotApplicable;
    }
    p
}

fn decode_top(bytes: &[u8]) -> Vec<[f32; 4]> {
    let dds = engine::dds::read_dds(bytes).unwrap();
    let (w, h, r) = dds.levels[0].clone();
    engine::decode::decode_level_rgba(dds.format, w, h, &dds.data[r], None).unwrap()
}

#[test]
fn linear_transfer_skips_the_srgb_decode() {
    use engine::memory::encode_image_with;
    use engine::ops::{srgb_to_linear, SourceTransfer};
    let flat = |v: f32| Rgba32F::new(4, 4, [v, v, v, 1.0]);
    let p = flat_profile(Format::Rgba8);
    // Linear light 0.2140... is sRGB 0.5, so it is the code Stored 0.5 gives (rounding may differ by one).
    let lin =
        encode_image_with(flat(srgb_to_linear(0.5)), &p, None, SourceTransfer::Linear).unwrap();
    let sto = encode_image_with(flat(0.5), &p, None, SourceTransfer::Stored).unwrap();
    let (a, b) = (decode_top(&lin)[0][0], decode_top(&sto)[0][0]);
    assert!((a - b).abs() <= 1.0 / 255.0 + 1e-6, "{a} vs {b}");
    assert!((b - 0.5).abs() <= 1.0 / 255.0 + 1e-6, "{b}");
    // Exact ends are identical.
    for v in [0.0, 1.0] {
        let l = encode_image_with(flat(v), &p, None, SourceTransfer::Linear).unwrap();
        let s = encode_image_with(flat(v), &p, None, SourceTransfer::Stored).unwrap();
        assert!(l == s, "{v}");
    }
    // Stored is encode_image.
    assert!(sto == encode_image(flat(0.5), &p).unwrap());
    // HDR: Linear keeps 4.0 through BC6H.
    let hp = flat_profile(Format::Bc6hUf16);
    let hdr = encode_image_with(flat(4.0), &hp, None, SourceTransfer::Linear).unwrap();
    let got = decode_top(&hdr)[0][0];
    assert!((got - 4.0).abs() <= 0.08, "{got}");
}
