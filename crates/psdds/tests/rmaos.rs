//! `psdds_rmaos_encode` through the C exports (Task 7): the pack, fills, defaults and size rule are the engine's, and
//! a pair built here equals a real `Engine::plan` + `run` of the same pixels (M7).
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::image::Rgba32F;
use engine::pack::ChannelSource;
use engine::policy::*;
use image::{ImageBuffer, Luma};
use psdds::*;
use std::ffi::{c_char, CStr};
use std::mem::{offset_of, size_of};
use std::path::Path;
use std::sync::Arc;

fn empty_buf() -> PsddsBuf {
    PsddsBuf {
        data: std::ptr::null_mut(),
        len: 0,
    }
}

fn no_source() -> PsddsSource {
    PsddsSource {
        px: std::ptr::null(),
        width: 0,
        height: 0,
        channels: 0,
        channel: 0,
    }
}

fn src(px: &[f32], w: u32, channels: u32, channel: u32) -> PsddsSource {
    PsddsSource {
        px: px.as_ptr(),
        width: w,
        height: px.len() as u32 / w / channels,
        channels,
        channel,
    }
}

fn job() -> PsddsRmaosJob {
    PsddsRmaosJob {
        source: [no_source(), no_source(), no_source(), no_source()],
        fill: [-1; 4],
        quality: psdds_rmaos_default_quality(),
        mips: 1,
        normal: std::ptr::null(),
        normal_width: 0,
        normal_height: 0,
        normal_settings: psdds_defaults(1),
    }
}

/// What a call returned, with the buffers copied out and freed.
struct Out {
    rc: i32,
    rmaos: Vec<u8>,
    normal: Vec<u8>,
    empty: bool,
    defaulted: u32,
    baked: u32,
    note: String,
    err: String,
}

fn call(j: &PsddsRmaosJob) -> Out {
    let mut r = PsddsRmaosResult {
        rmaos: empty_buf(),
        normal: empty_buf(),
        defaulted: 99,
        baked: 99,
    };
    let mut note = [1 as c_char; 512];
    let mut err = [0 as c_char; 512];
    unsafe {
        let rc = psdds_rmaos_encode(
            j,
            &mut r,
            note.as_mut_ptr(),
            note.len(),
            err.as_mut_ptr(),
            err.len(),
        );
        let bytes = |b: &PsddsBuf| {
            if b.data.is_null() {
                vec![]
            } else {
                std::slice::from_raw_parts(b.data, b.len).to_vec()
            }
        };
        let out = Out {
            rc,
            rmaos: bytes(&r.rmaos),
            normal: bytes(&r.normal),
            empty: r.rmaos.data.is_null() && r.normal.data.is_null(),
            defaulted: r.defaulted,
            baked: r.baked,
            note: CStr::from_ptr(note.as_ptr()).to_string_lossy().into_owned(),
            err: CStr::from_ptr(err.as_ptr()).to_string_lossy().into_owned(),
        };
        psdds_free(r.rmaos);
        psdds_free(r.normal);
        out
    }
}

fn ok(j: &PsddsRmaosJob) -> Out {
    let o = call(j);
    assert_eq!(o.rc, 0, "{}", o.err);
    o
}

fn fails(j: &PsddsRmaosJob) -> String {
    let o = call(j);
    assert_ne!(o.rc, 0);
    assert!(o.empty, "buffers must be empty on error");
    o.err
}

/// The top-level RGBA8 of a DDS.
fn decode(dds: &[u8]) -> Vec<u8> {
    let (mut w, mut h, mut a) = (0, 0, 0);
    let mut out = empty_buf();
    let mut err = [0 as c_char; 256];
    unsafe {
        let rc = psdds_decode(
            dds.as_ptr(),
            dds.len(),
            &mut w,
            &mut h,
            &mut a,
            &mut out,
            err.as_mut_ptr(),
            err.len(),
        );
        assert_eq!(rc, 0);
        let v = std::slice::from_raw_parts(out.data, out.len).to_vec();
        psdds_free(out);
        v
    }
}

fn mip_count(dds: &[u8]) -> u32 {
    u32::from_le_bytes(dds[28..32].try_into().unwrap()).max(1)
}

fn near(a: u8, b: u8) -> bool {
    a.abs_diff(b) <= 2
}

#[test]
fn layouts_match_the_header() {
    assert_eq!(size_of::<PsddsSettings>(), 32);
    assert_eq!(size_of::<PsddsSource>(), 24);
    assert_eq!(offset_of!(PsddsSource, width), 8);
    assert_eq!(offset_of!(PsddsSource, channel), 20);
    assert_eq!(size_of::<PsddsRmaosJob>(), 168);
    assert_eq!(offset_of!(PsddsRmaosJob, fill), 96);
    assert_eq!(offset_of!(PsddsRmaosJob, quality), 112);
    assert_eq!(offset_of!(PsddsRmaosJob, mips), 116);
    assert_eq!(offset_of!(PsddsRmaosJob, normal), 120);
    assert_eq!(offset_of!(PsddsRmaosJob, normal_width), 128);
    assert_eq!(offset_of!(PsddsRmaosJob, normal_settings), 136);
    assert_eq!(size_of::<PsddsRmaosResult>(), 40);
    assert_eq!(offset_of!(PsddsRmaosResult, normal), 16);
    assert_eq!(offset_of!(PsddsRmaosResult, defaulted), 32);
    assert_eq!(offset_of!(PsddsRmaosResult, baked), 36);
}

/// (a) roughness only: the other channels take the engine's neutral defaults.
#[test]
fn roughness_only_takes_defaults() {
    let r = vec![0.25f32; 64];
    let mut j = job();
    j.source[0] = src(&r, 8, 1, 0);
    let o = ok(&j);
    assert_eq!(o.defaulted, 0b1110);
    assert_eq!(o.baked, 0);
    assert!(o.normal.is_empty());
    let px = decode(&o.rmaos);
    assert!(near(px[0], 64) && near(px[1], 0) && near(px[2], 255) && near(px[3], 255));
}

/// (b) a fill replaces the default and clears its bit.
#[test]
fn a_fill_sets_the_channel() {
    let r = vec![0.25f32; 64];
    let mut j = job();
    j.source[0] = src(&r, 8, 1, 0);
    j.fill[1] = 255;
    let o = ok(&j);
    assert_eq!(o.defaulted & 0b10, 0);
    assert!(near(decode(&o.rmaos)[1], 255));
}

/// (c) the engine's refusal text.
#[test]
fn specular_alone_has_nothing_to_pack() {
    let s = vec![0.5f32; 64];
    let mut j = job();
    j.source[3] = src(&s, 8, 1, 0);
    assert!(fails(&j).contains("nothing to pack"));
}

/// (d) the packer's size text, unchanged.
#[test]
fn sizes_must_match() {
    let (r, a) = (vec![0.5f32; 64], vec![0.5f32; 256]);
    let mut j = job();
    j.source[0] = src(&r, 8, 1, 0);
    j.source[2] = src(&a, 16, 1, 0);
    assert_eq!(fails(&j), "sources differ in size: roughness 8x8, ao 16x16");
}

/// (e) an out-of-range quality is the profile's.
#[test]
fn out_of_range_quality_is_the_profiles() {
    let r: Vec<f32> = (0..64).map(|i| i as f32 / 63.0).collect();
    let mut j = job();
    j.source[0] = src(&r, 8, 1, 0);
    let d = ok(&j).rmaos;
    j.quality = 99;
    assert!(ok(&j).rmaos == d);
}

/// (f) M3/M4: the size rule is check_size's, before the source is read.
#[test]
fn an_oversize_source_gets_the_size_text() {
    let r = vec![0.5f32; 16385];
    let mut j = job();
    j.source[0] = src(&r, 16385, 1, 0);
    let mut e = [0 as c_char; 256];
    unsafe { psdds_check_size(16385, 1, e.as_mut_ptr(), e.len()) };
    let want = unsafe { CStr::from_ptr(e.as_ptr()) }.to_str().unwrap();
    assert_eq!(fails(&j), want);
    // The normal too.
    let r = vec![0.5f32; 64];
    let n = [0.5f32; 4];
    let mut j = job();
    j.source[0] = src(&r, 8, 1, 0);
    j.normal = n.as_ptr();
    (j.normal_width, j.normal_height) = (16385, 1);
    assert_eq!(fails(&j), want);
}

/// (g) a fill outside -1..255.
#[test]
fn a_fill_out_of_range_is_refused() {
    let r = vec![0.5f32; 64];
    let mut j = job();
    j.source[0] = src(&r, 8, 1, 0);
    j.fill[2] = 300;
    assert_eq!(fails(&j), "fill for ao is 300, not -1 or 0..255");
}

/// (h) mips 0 writes the top level only.
#[test]
fn no_mips_is_one_level() {
    let r = vec![0.5f32; 64];
    let mut j = job();
    j.source[0] = src(&r, 8, 1, 0);
    assert!(mip_count(&ok(&j).rmaos) > 1);
    j.mips = 0;
    assert_eq!(mip_count(&ok(&j).rmaos), 1);
    j.mips = 2;
    assert_eq!(fails(&j), "mips 2 out of range 0..2");
}

/// (i) one 4-channel image feeding all four roles equals four 1-channel images.
#[test]
fn multi_channel_equals_single_channel() {
    let plane = |k: usize| -> Vec<f32> {
        (0..64)
            .map(|i| ((i * (k + 3)) % 64) as f32 / 63.0)
            .collect()
    };
    let planes: Vec<Vec<f32>> = (0..4).map(plane).collect();
    let inter: Vec<f32> = (0..64)
        .flat_map(|i| (0..4).map(move |k| (i, k)))
        .map(|(i, k)| planes[k][i])
        .collect();
    let mut j = job();
    for k in 0..4 {
        j.source[k] = src(&inter, 8, 4, k as u32);
    }
    let a = ok(&j).rmaos;
    for (k, p) in planes.iter().enumerate() {
        j.source[k] = src(p, 8, 1, 0);
    }
    assert!(ok(&j).rmaos == a);
}

/// Two roles on one buffer with different channel counts (the second a smaller view of it) equal two separate
/// buffers: the shared clamped copy is keyed on the whole view, not on `px` alone.
#[test]
fn views_of_one_buffer_are_not_merged() {
    let buf: Vec<f32> = (0..64 * 4).map(|i| (i % 97) as f32 / 96.0).collect();
    let mut j = job();
    j.source[0] = src(&buf, 8, 4, 1);
    j.source[2] = src(&buf[..64 * 2], 8, 2, 1);
    assert_eq!(j.source[0].px, j.source[2].px);
    let shared = ok(&j).rmaos;
    let (a, b) = (buf.clone(), buf[..64 * 2].to_vec());
    j.source[0] = src(&a, 8, 4, 1);
    j.source[2] = src(&b, 8, 2, 1);
    assert!(ok(&j).rmaos == shared);
}

/// (j) a channel the image does not have.
#[test]
fn a_bad_channel_is_refused() {
    let r = vec![0.5f32; 64 * 3];
    let mut j = job();
    j.source[1] = src(&r, 8, 3, 3);
    assert_eq!(fails(&j), "metallic channel 3 is not in a 3-channel image");
    j.source[1] = src(&r, 8, 3, 0);
    j.source[1].channels = 5;
    assert_eq!(fails(&j), "metallic channel 0 is not in a 5-channel image");
}

/// A bumpy RGB normal on the 8-bit grid (the bake goldens' `bumpy`), so the PNG and the floats hold the same values.
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
fn normal_rgba(w: u32) -> Vec<f32> {
    bumpy_u8(w)
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|c| {
            [c[0], c[1], c[2]]
                .map(|v| f32::from(v) / 255.0)
                .into_iter()
                .chain([1.0])
        })
        .collect()
}

const N: u32 = 64;
fn rough_px() -> Vec<f32> {
    (0..N * N).map(|i| ((i % N) * 4) as f32 / 255.0).collect()
}
fn ao_px() -> Vec<f32> {
    (0..N * N)
        .map(|i| (255 - (i / N) * 4) as f32 / 255.0)
        .collect()
}

fn pair_job<'a>(r: &'a [f32], a: &'a [f32], n: &'a [f32], nw: u32) -> PsddsRmaosJob {
    let mut j = job();
    j.source[0] = src(r, N, 1, 0);
    j.source[2] = src(a, N, 1, 0);
    j.normal = n.as_ptr();
    (j.normal_width, j.normal_height) = (nw, nw);
    j
}

/// (k) a 64x64 normal beside a 64x64 roughness bakes.
#[test]
fn a_pair_bakes() {
    let (r, a, n) = (rough_px(), ao_px(), normal_rgba(64));
    let o = ok(&pair_job(&r, &a, &n, 64));
    assert_eq!(o.baked, 1);
    assert_eq!(o.note, "");
    assert!(!o.normal.is_empty());
}

/// (l) the normal settings must be Normal's.
#[test]
fn a_colour_normal_is_refused() {
    let (r, a, n) = (rough_px(), ao_px(), normal_rgba(64));
    let mut j = pair_job(&r, &a, &n, 64);
    j.normal_settings = psdds_defaults(0);
    assert_eq!(fails(&j), "normal_settings must use texture type Normal");
}

/// (m) sizes that cannot bake: the engine's note.
#[test]
fn a_96_normal_does_not_bake() {
    let (r, a, n) = (rough_px(), ao_px(), normal_rgba(96));
    let o = ok(&pair_job(&r, &a, &n, 96));
    assert_eq!(o.baked, 0);
    assert!(o.note.starts_with("variance bake off:"), "{}", o.note);
    assert!(!o.normal.is_empty());
}

/// No normal: the engine's own note, as-is (M5).
#[test]
fn no_normal_notes_why() {
    let r = rough_px();
    let mut j = job();
    j.source[0] = src(&r, N, 1, 0);
    let o = ok(&j);
    assert_eq!(o.note, engine::bake::plan::NO_NORMAL);
}

/// (n) DC36: discard_alpha changes nothing in the pair; 2 is refused.
#[test]
fn discard_alpha_has_no_effect_on_the_pair() {
    let (n, spec4): (Vec<f32>, Vec<f32>) = (
        normal_rgba(64),
        (0..N * N * 4).map(|i| (i % 251) as f32 / 250.0).collect(),
    );
    let mut j = job();
    j.source[0] = src(&spec4, N, 4, 0);
    j.source[3] = src(&spec4, N, 4, 3);
    j.normal = n.as_ptr();
    (j.normal_width, j.normal_height) = (64, 64);
    let a = ok(&j);
    j.normal_settings.discard_alpha = 1;
    let b = ok(&j);
    assert!(a.rmaos == b.rmaos && a.normal == b.normal);
    assert_eq!(a.defaulted & 0b1000, 0, "specular lands in A");
    j.normal_settings.discard_alpha = 2;
    fails(&j);
    // DH: alpha_mode is checked the same way and changes nothing either.
    j.normal_settings.discard_alpha = 0;
    j.normal_settings.alpha_mode = 1;
    let c = ok(&j);
    assert!(a.rmaos == c.rmaos && a.normal == c.normal);
    j.normal_settings.alpha_mode = 2;
    fails(&j);
}

/// (o) DC34: the Normal defaults carry no sRGB.
#[test]
fn normal_defaults_have_no_srgb() {
    assert_eq!(psdds_defaults(1).srgb, 0);
}

/// NaN and out-of-range values are clamped (M8) like psdds_encode's input.
#[test]
fn sources_and_normal_are_clamped() {
    let (r, a, n) = (rough_px(), ao_px(), normal_rgba(64));
    let base = ok(&pair_job(&r, &a, &n, 64));
    let wild = |v: &[f32]| -> Vec<f32> {
        v.iter()
            .map(|&x| match () {
                _ if x == 0.0 => f32::NAN,
                _ if x == 1.0 => 7.0,
                _ => x,
            })
            .collect()
    };
    let (r2, a2, n2) = (wild(&r), wild(&a), wild(&n));
    let o = ok(&pair_job(&r2, &a2, &n2, 64));
    assert!(o.rmaos == base.rmaos && o.normal == base.normal);
}

// ---- M7: the pair equals a real batch run ----

fn save(dir: &Path, name: &str, px: &[f32]) {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(N, N, |x, y| {
        Luma([(px[(y * N + x) as usize] * 255.0).round() as u8])
    })
    .save(&p)
    .unwrap();
}

struct Batch {
    plan_n: ResolvedJob,
    plan_rmaos: ResolvedJob,
    n: Vec<u8>,
    rmaos: Vec<u8>,
}

/// `Engine::plan` + `run` of `iron_n.png` (nw x nw) beside `iron_roughness.png` and `iron_ao.png`, target cs, preset
/// cs-quality, High suggestions accepted, DIM-NPOT allowed (96x96).
fn batch(nw: u32) -> Batch {
    let d = tempfile::tempdir().unwrap();
    save(d.path(), "iron_roughness.png", &rough_px());
    save(d.path(), "iron_ao.png", &ao_px());
    image::RgbImage::from_raw(nw, nw, bumpy_u8(nw))
        .unwrap()
        .save(d.path().join("in/iron_n.png"))
        .unwrap();
    let mut req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    req.preset = Some("cs-quality".into());
    req.on_existing = OnExisting::Overwrite;
    req.accept = AcceptSuggestions::High;
    req.allow = [Code::DimNpot].into();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "psdds-rmaos");
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

/// M7: psdds's `_n` and `_rmaos` profiles are the planned ones, the planned channels and PACK-DEFAULTED are the
/// pack's, and the C export's bytes are the batch's.
#[test]
fn a_pair_equals_the_batch() {
    let (r, a) = (rough_px(), ao_px());
    for nw in [64u32, 96] {
        let b = batch(nw);
        // The profiles psdds_rmaos_encode builds, as the engine uses them.
        let rp = psdds::rmaos::rmaos_profile(psdds_rmaos_default_quality(), 1).unwrap();
        let np = psdds::rmaos::normal_profile(&psdds_defaults(1)).unwrap();
        let ch = |px| engine::memory::Channel {
            px,
            width: N,
            height: N,
            channels: 1,
            channel: 0,
        };
        let nf = normal_rgba(nw);
        let img = Rgba32F {
            width: nw,
            height: nw,
            px: nf.as_chunks::<4>().0.to_vec(),
        };
        let set = engine::memory::encode_rmaos_set(
            [Some(ch(&r)), None, Some(ch(&a)), None],
            [None; 4],
            &rp,
            Some((img, np)),
        )
        .unwrap();
        assert_eq!(
            set.rmaos_profile, b.plan_rmaos.profile,
            "{nw}: _rmaos profile"
        );
        assert_eq!(
            set.normal_profile.as_ref(),
            Some(&b.plan_n.profile),
            "{nw}: _n profile"
        );
        // The planned channels and PACK-DEFAULTED are the pack's.
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
        assert_eq!(pk.gray, rp.channels.grayscale_source);
        let planned: Vec<&str> = b
            .plan_rmaos
            .warnings
            .iter()
            .filter(|w| w.code == Code::PackDefaulted)
            .map(|w| w.data["channel"].as_str().unwrap())
            .collect();
        let names = ["R", "G", "B", "A"];
        let mem: Vec<&str> = set.defaulted.iter().map(|&i| names[i]).collect();
        assert_eq!(planned, mem, "{nw}: PACK-DEFAULTED");
        // The C export writes the batch's bytes.
        let o = ok(&pair_job(&r, &a, &nf, nw));
        let bits: u32 = set.defaulted.iter().map(|&i| 1 << i).sum();
        assert_eq!(o.defaulted, bits);
        assert_eq!(o.baked, u32::from(b.plan_rmaos.bake.is_some()), "{nw}");
        assert!(o.rmaos == b.rmaos, "{nw}: _rmaos bytes differ");
        assert!(o.normal == b.n, "{nw}: _n bytes differ");
    }
}
