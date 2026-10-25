//! `memory::pack_rmaos` packs in-memory channels by the engine's own rule: the result must equal a real file run
//! of the same pixels, and the refusals must be the engine's texts.
use engine::api::*;
use engine::config::*;
use engine::events::*;
use engine::memory::{encode_image, pack_rmaos, Channel};
use engine::pack::*;
use engine::policy::*;
use engine::presets::PresetRegistry;
use image::{ImageBuffer, Luma};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const N: u32 = 64;

fn rough_px() -> Vec<f32> {
    (0..N * N).map(|i| ((i % N) * 4) as f32 / 255.0).collect()
}
fn ao_px() -> Vec<f32> {
    (0..N * N)
        .map(|i| (255 - (i / N) * 4) as f32 / 255.0)
        .collect()
}
fn ch(px: &[f32], w: u32) -> Channel<'_> {
    Channel {
        px,
        width: w,
        height: px.len() as u32 / w,
        channels: 1,
        channel: 0,
    }
}
fn profile() -> Profile {
    PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, Subtype::Rmaos)
        .unwrap()
}
fn save(dir: &Path, name: &str, px: &[f32]) -> PathBuf {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(N, N, |x, y| {
        Luma([(px[(y * N + x) as usize] * 255.0).round() as u8])
    })
    .save(&p)
    .unwrap();
    p
}

/// The `_rmaos` job as `Engine::plan` builds it, its channels from the recipe rule rather than by hand.
fn plan(dir: &Path, rough: &Path, ao: &Path) -> Plan {
    let registry = PresetRegistry::builtin().unwrap();
    let raw = [
        RawInput {
            path: rough.to_path_buf(),
            role: RawMap::Roughness,
        },
        RawInput {
            path: ao.to_path_buf(),
            role: RawMap::Ao,
        },
    ];
    let (recipes, _) = recipes_for_with(Target::Cs, &raw, PackRecipe::Rmaos, false);
    let profile = profile();
    let (channels, _) = resolve_channels(&recipes[0], &profile.packing);
    let sources = recipes[0]
        .sources()
        .into_iter()
        .map(|(path, role)| PackSource {
            stamp: SourceStamp::of(&path),
            path,
            role,
        })
        .collect();
    let mut request = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    request.on_existing = OnExisting::IfChanged;
    let job = ResolvedJob {
        input: rough.to_path_buf(),
        output: dir.join("out/iron_rmaos.dds"),
        game_path: "iron_rmaos.png".into(),
        set_key: "out/iron".into(),
        profile,
        provenance: Default::default(),
        suggestions: vec![],
        warnings: vec![],
        status: JobStatus::Ready,
        suffix_alias: None,
        source: None,
        pack: Some(PackJob {
            kind: RecipeKind::Rmaos,
            channels,
            gray: GraySource::R,
            sources,
        }),
        bake: None,
        rnm: None,
        derive: None,
        shader: None,
    };
    Plan {
        request,
        registry,
        preset_id: "cs-quality".into(),
        target: Target::Cs,
        jobs: vec![job],
        skipped: vec![],
        failed: vec![],
        peak_admitted_bytes: 0,
        cancelled: vec![],
        shaders: Default::default(),
        nvtt: None,
        derive_own: Default::default(),
    }
}

#[test]
fn memory_pack_equals_a_file_run() {
    let d = tempfile::tempdir().unwrap();
    let (r, a) = (rough_px(), ao_px());
    let rp = save(d.path(), "iron_r.png", &r);
    let ap = save(d.path(), "iron_ao.png", &a);
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "mem-rmaos");
    let s = Engine::new()
        .unwrap()
        .run(plan(d.path(), &rp, &ap), &rep, &CancelToken::default())
        .unwrap();
    assert_eq!(s.converted, 1, "{:#?}", sink.take());
    let file = std::fs::read(d.path().join("out/iron_rmaos.dds")).unwrap();
    let (img, defaulted) = pack_rmaos(
        [Some(ch(&r, N)), None, Some(ch(&a, N)), None],
        [None; 4],
        &profile(),
    )
    .unwrap();
    assert_eq!(defaulted, vec![1, 3]);
    assert_eq!(encode_image(img, &profile()).unwrap(), file);
}

#[test]
fn multi_channel_sources_equal_single_channel_ones() {
    let (r, a) = (rough_px(), ao_px());
    let mix: Vec<f32> = (0..(N * N) as usize)
        .flat_map(|i| [a[i], 0.5, r[i]])
        .collect();
    let c = |channel| Channel {
        px: &mix,
        width: N,
        height: N,
        channels: 3,
        channel,
    };
    let (m, _) = pack_rmaos([Some(c(2)), None, Some(c(0)), None], [None; 4], &profile()).unwrap();
    let (s, _) = pack_rmaos(
        [Some(ch(&r, N)), None, Some(ch(&a, N)), None],
        [None; 4],
        &profile(),
    )
    .unwrap();
    assert_eq!(m, s);
}

#[test]
fn a_fill_replaces_the_default() {
    let r = rough_px();
    let (img, defaulted) = pack_rmaos(
        [Some(ch(&r, N)), None, None, None],
        [None, Some(255), None, None],
        &profile(),
    )
    .unwrap();
    assert!(img.px.iter().all(|p| p[1] == 1.0));
    assert_eq!(defaulted, vec![2, 3]);
}

#[test]
fn specular_alone_has_nothing_to_pack() {
    let s = vec![0.5f32; 16];
    let e = pack_rmaos([None, None, None, Some(ch(&s, 4))], [None; 4], &profile()).unwrap_err();
    assert!(e.contains("nothing to pack"), "{e}");
}

#[test]
fn sizes_must_match() {
    let (a, b) = (vec![0.5f32; 64], vec![0.5f32; 256]);
    let e = pack_rmaos(
        [Some(ch(&a, 8)), None, Some(ch(&b, 16)), None],
        [None; 4],
        &profile(),
    )
    .unwrap_err();
    assert_eq!(e, "sources differ in size: roughness 8x8, ao 16x16");
}

#[test]
fn out_of_range_values_are_clamped() {
    let wild = vec![-1.0f32, 2.0, f32::NAN, 0.25];
    let clamped = vec![0.0f32, 1.0, 0.0, 0.25];
    let p = |v: &[f32]| {
        pack_rmaos([Some(ch(v, 2)), None, None, None], [None; 4], &profile())
            .unwrap()
            .0
    };
    assert_eq!(p(&wild), p(&clamped));
}

#[test]
fn a_bad_channel_is_refused() {
    let v = vec![0.5f32; 12];
    let bad = Channel {
        px: &v,
        width: 2,
        height: 2,
        channels: 3,
        channel: 3,
    };
    let want = "roughness: invalid source image or channel";
    assert_eq!(
        pack_rmaos([Some(bad), None, None, None], [None; 4], &profile()).unwrap_err(),
        want
    );
    let short = Channel {
        px: &v[..5],
        width: 2,
        height: 2,
        channels: 1,
        channel: 0,
    };
    assert_eq!(
        pack_rmaos([Some(short), None, None, None], [None; 4], &profile()).unwrap_err(),
        want
    );
}

#[test]
fn a_profile_fill_survives_an_unset_argument() {
    let r = rough_px();
    let mut p = profile();
    p.packing.fill.metallic = Some(255);
    let (img, defaulted) = pack_rmaos([Some(ch(&r, N)), None, None, None], [None; 4], &p).unwrap();
    assert!(img.px.iter().all(|px| px[1] == 1.0));
    assert_eq!(defaulted, vec![2, 3]);
}
