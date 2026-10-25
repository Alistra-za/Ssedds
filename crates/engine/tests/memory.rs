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
