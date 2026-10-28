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
        cube: None,
        fill: None,
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

/// One rule for out-of-range values: a float roughness file (below 0 and above 1) packs and encodes as the same
/// floats from memory; the engine clamps neither (the host clamps memory input, M8).
#[test]
fn out_of_range_float_sources_match_the_file_path() {
    let d = tempfile::tempdir().unwrap();
    let r: Vec<f32> = (0..N * N)
        .map(|i| -0.5 + 2.0 * (i % N) as f32 / (N - 1) as f32)
        .collect();
    let a = ao_px();
    let rp = d.path().join("in/iron_roughness.exr");
    std::fs::create_dir_all(rp.parent().unwrap()).unwrap();
    image::DynamicImage::ImageRgb32F(ImageBuffer::from_fn(N, N, |x, y| {
        let v = r[(y * N + x) as usize];
        image::Rgb([v, v, v])
    }))
    .save(&rp)
    .unwrap();
    let ap = save(d.path(), "iron_ao.png", &a);
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "mem-rmaos-float");
    let s = Engine::new()
        .unwrap()
        .run(plan(d.path(), &rp, &ap), &rep, &CancelToken::default())
        .unwrap();
    assert_eq!(s.converted, 1, "{:#?}", sink.take());
    let file = std::fs::read(d.path().join("out/iron_rmaos.dds")).unwrap();
    let (img, _) = pack_rmaos(
        [Some(ch(&r, N)), None, Some(ch(&a, N)), None],
        [None; 4],
        &profile(),
    )
    .unwrap();
    assert!(img.px.iter().any(|p| p[0] < 0.0) && img.px.iter().any(|p| p[0] > 1.0));
    assert!(encode_image(img, &profile()).unwrap() == file);
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

// ---- Task 6: the normal-into-roughness bake from memory (encode_rmaos_set) ----

use engine::bake::plan::{decide_bake, mark_normal_baking, BakeDecision};
use engine::codes::Code;
use engine::image::Rgba32F;
use engine::memory::encode_rmaos_set;

/// A bumpy RGB normal on the 8-bit grid (as the bake goldens' `bumpy`), so the PNG and the memory image hold the
/// same values; its footprints fall far below `NOISE_FLOOR_L`.
fn bumpy_u8(w: u32) -> Vec<u8> {
    let b = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
    (0..w * w)
        .flat_map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let nx = 0.6 * (x * 1.3 + y * 0.7).sin();
            let ny = 0.6 * (x * 0.9 - y * 1.1).cos();
            let nz = (1.0 - nx * nx - ny * ny).sqrt();
            [b(nx), b(ny), b(nz)]
        })
        .collect()
}
fn normal_img(w: u32) -> Rgba32F {
    let raw = bumpy_u8(w);
    let mut img = Rgba32F::new(w, w, [0.0, 0.0, 0.0, 1.0]);
    for (p, c) in img.px.iter_mut().zip(raw.as_chunks::<3>().0) {
        let [r, g, b] = c.map(|v| f32::from(v) / 255.0);
        *p = [r, g, b, 1.0];
    }
    img
}
fn pbr_normal_profile() -> Profile {
    PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, Subtype::PbrNormal)
        .unwrap()
}

struct Batch {
    plan_n: ResolvedJob,
    plan_rmaos: ResolvedJob,
    n: Vec<u8>,
    rmaos: Vec<u8>,
}

/// A real `Engine::plan` + `Engine::run` of `iron_n.png` (nw x nw) beside `iron_roughness.png` and `iron_ao.png` (64x64),
/// target cs, preset cs-quality, no user config.
fn batch(nw: u32) -> Batch {
    batch_with(nw, &[])
}

/// [`batch`] with `--set` overrides.
fn batch_with(nw: u32, sets: &[&str]) -> Batch {
    let d = tempfile::tempdir().unwrap();
    let (r, a) = (rough_px(), ao_px());
    save(d.path(), "iron_roughness.png", &r);
    save(d.path(), "iron_ao.png", &a);
    image::RgbImage::from_raw(nw, nw, bumpy_u8(nw))
        .unwrap()
        .save(d.path().join("in/iron_n.png"))
        .unwrap();
    let mut req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    req.preset = Some("cs-quality".into());
    req.on_existing = OnExisting::Overwrite;
    // As the bake tests plan a set: High-confidence sub-types (`_n` in a PBR set: pbr_normal) are taken.
    req.accept = AcceptSuggestions::High;
    // The 96x96 case: acknowledged, it gates the write and changes no byte.
    req.allow = [Code::DimNpot].into();
    req.overrides = sets
        .iter()
        .map(|s| engine::overrides::parse_set(s).unwrap())
        .collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "mem-rmaos-bake");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let find = |st: Subtype| {
        plan.jobs
            .iter()
            .find(|j| j.profile.subtype == st)
            .unwrap_or_else(|| panic!("no {st:?} job: {:#?}", plan.jobs))
            .clone()
    };
    let (plan_n, plan_rmaos) = (find(Subtype::PbrNormal), find(Subtype::Rmaos));
    let s = e.run(plan, &rep, &CancelToken::default()).unwrap();
    assert_eq!(s.converted, 2, "{:#?}", sink.take());
    Batch {
        n: std::fs::read(&plan_n.output).unwrap(),
        rmaos: std::fs::read(&plan_rmaos.output).unwrap(),
        plan_n,
        plan_rmaos,
    }
}

fn memory_set(nw: Option<u32>, p: &Profile) -> engine::memory::RmaosSet {
    let (r, a) = (rough_px(), ao_px());
    encode_rmaos_set(
        [Some(ch(&r, N)), None, Some(ch(&a, N)), None],
        [None; 4],
        p,
        nw.map(|w| (normal_img(w), pbr_normal_profile())),
    )
    .unwrap()
}

fn json<T: serde::Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).unwrap()
}

/// The owner's "exactly as in a project export": a chaiNNer pair equals the batch's `_n` and `_rmaos`.
#[test]
fn a_baking_pair_equals_the_batch_at_1_to_1_and_2_to_1() {
    for (nw, shift) in [(64u32, 0i32), (128, 1)] {
        let b = batch(nw);
        let bake = b.plan_rmaos.bake.as_ref().expect("the _rmaos must bake");
        assert_eq!(bake.shift, shift);
        // Profile check (R12): psdds's choice is the batch's.
        let mut np = pbr_normal_profile();
        mark_normal_baking(&mut np);
        assert_eq!(json(&b.plan_n.profile), json(&np), "{nw}: _n profile");
        // Task 5 carry: the planned channels and PACK-DEFAULTED are pack_rmaos's.
        let pk = b.plan_rmaos.pack.as_ref().unwrap();
        let roles: Vec<String> = pk
            .channels
            .iter()
            .map(|c| match c {
                ChannelSource::Source { role, .. } => role.id().to_string(),
                ChannelSource::Default { value } => format!("default {value}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(roles, ["roughness", "default 0", "ao", "default 1"], "{nw}");
        assert_eq!(pk.gray, profile().channels.grayscale_source);
        let planned_defaults: Vec<String> = b
            .plan_rmaos
            .warnings
            .iter()
            .filter(|w| w.code == Code::PackDefaulted)
            .map(|w| w.data["channel"].as_str().unwrap().to_string())
            .collect();
        let m = memory_set(Some(nw), &b.plan_rmaos.profile);
        let names = ["R", "G", "B", "A"];
        let mem_defaults: Vec<String> = m.defaulted.iter().map(|&i| names[i].to_string()).collect();
        assert_eq!(planned_defaults, mem_defaults, "{nw}: PACK-DEFAULTED");
        assert!(m.baked, "{nw}");
        assert_eq!(m.bake_note, None, "{nw}");
        assert!(
            m.normal.as_deref() == Some(&b.n[..]),
            "{nw}: _n bytes differ"
        );
        assert!(m.rmaos == b.rmaos, "{nw}: _rmaos bytes differ");
        // The bake really changed the bytes.
        assert!(
            memory_set(None, &b.plan_rmaos.profile).rmaos != b.rmaos,
            "{nw}: bake had no effect"
        );
        // The one policy-layer difference: the High-confidence alpha suggestion ("alpha is constant 255", the
        // defaulted specular) takes `alpha.policy = remove`. It changes no byte: the registry profile (`data`) writes
        // the same `_rmaos`.
        let mut rp = profile();
        rp.alpha.policy = AlphaPolicy::Remove;
        assert_eq!(
            json(&b.plan_rmaos.profile),
            json(&rp),
            "{nw}: _rmaos profile"
        );
        let r = memory_set(Some(nw), &profile());
        assert!(r.rmaos == b.rmaos, "{nw}: registry-profile _rmaos differs");
        // Task 7: the set takes that step itself, so the profile it encodes with is the planned one.
        assert_eq!(r.rmaos_profile, b.plan_rmaos.profile, "{nw}");
        assert_eq!(m.normal_profile.as_ref(), Some(&b.plan_n.profile), "{nw}");
    }
}

#[test]
fn a_size_that_cannot_bake_notes_why_and_writes_the_unbaked_rmaos() {
    let b = batch(96);
    assert!(b.plan_rmaos.bake.is_none());
    let m = memory_set(Some(96), &profile());
    assert!(!m.baked);
    let note = m.bake_note.unwrap();
    assert!(note.starts_with("variance bake off:"), "{note}");
    assert!(m.rmaos == memory_set(None, &profile()).rmaos);
    assert!(m.rmaos == b.rmaos && m.normal.as_deref() == Some(&b.n[..]));
}

#[test]
fn no_normal_writes_no_n_and_notes_why() {
    let m = memory_set(None, &profile());
    assert!(m.normal.is_none() && !m.baked);
    assert!(m.bake_note.unwrap().starts_with("no _n"));
}

#[test]
fn an_srgb_member_never_bakes() {
    let mut p = profile();
    p.color.content = ColorContent::Srgb;
    let m = memory_set(Some(64), &p);
    assert!(!m.baked && m.normal.is_some());
    assert!(m.bake_note.unwrap().contains("sRGB"));
    assert!(m.rmaos == memory_set(None, &p).rmaos);
}

/// M5 as amended: without mips the batch still bakes (mip 0 changes when the `_n` is larger), and so does memory.
#[test]
fn no_mips_bakes_as_the_batch_does() {
    for (nw, shift) in [(128u32, 1i32), (64, 0)] {
        let b = batch_with(nw, &["subtype:rmaos.mips.levels=none"]);
        assert_eq!(
            b.plan_rmaos.bake.as_ref().map(|j| j.shift),
            Some(shift),
            "{nw}"
        );
        let p = &b.plan_rmaos.profile;
        assert_eq!(p.mips.levels, MipLevels::Keyword(MipKeyword::None));
        let m = memory_set(Some(nw), p);
        assert!(m.baked && m.bake_note.is_none(), "{nw}");
        assert!(
            m.rmaos == b.rmaos && m.normal.as_deref() == Some(&b.n[..]),
            "{nw}"
        );
        // Mip 0 changes only when the `_n` is larger.
        assert_eq!(memory_set(None, p).rmaos != b.rmaos, shift > 0, "{nw}");
    }
}

/// A set that cannot pack fails before the `_n` is encoded: the normal profile here would fail its own encode.
#[test]
fn a_bad_set_fails_before_any_encode() {
    let mut np = pbr_normal_profile();
    np.drop_top_mip = true;
    let s = vec![0.5f32; (N * N) as usize];
    let run = |src: [Option<Channel>; 4]| {
        encode_rmaos_set(
            src,
            [None; 4],
            &profile(),
            Some((normal_img(N), np.clone())),
        )
        .unwrap_err()
    };
    assert!(run([None, None, None, None]).contains("nothing to pack"));
    assert!(run([None, None, None, Some(ch(&s, N))]).contains("nothing to pack"));
    let bad = Channel {
        px: &s,
        width: N,
        height: N,
        channels: 1,
        channel: 1,
    };
    assert_eq!(
        run([Some(bad), None, None, None]),
        "roughness: invalid source image or channel"
    );
    let small = vec![0.5f32; 16];
    assert!(run([Some(ch(&s, N)), None, Some(ch(&small, 4)), None])
        .starts_with("sources differ in size"));
    // The same normal profile does reach its encode once the set is valid.
    assert!(run([Some(ch(&s, N)), None, None, None]).contains("drop_top_mip"));
}

/// R6: a `_n` that opts out never bakes (reachable only through `decide_bake`: memory has no provenance).
#[test]
fn an_opted_out_normal_never_bakes() {
    assert!(matches!(
        decide_bake(true, (64, 64), &profile(), (64, 64)),
        BakeDecision::Never
    ));
    assert!(matches!(
        decide_bake(false, (64, 64), &profile(), (64, 64)),
        BakeDecision::Bake { shift: 0 }
    ));
}
