//! Helper textures Task 4 (Note DY10-DY12): a helper fills its target's alpha before the target is analysed, and the
//! run writes that alpha.
use engine::api::*;
use engine::config::*;
use engine::dds::read_dds;
use engine::decode::decode_level_rgba;
use engine::events::*;
use engine::helper::apply_fill;
use engine::load::load_image;
use engine::overrides::parse_set;
use engine::policy::*;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const N: u32 = 64;

fn at(d: &Path, name: &str) -> PathBuf {
    let p = d.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    p
}

fn rgb(d: &Path, name: &str) {
    ImageBuffer::from_fn(N, N, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90]))
        .save(at(d, name))
        .unwrap();
}

/// 16 px squares (whole BC blocks) of `lo` and `hi`.
fn checker_value(x: u32, y: u32, lo: u8, hi: u8) -> u8 {
    if (x / 16 + y / 16).is_multiple_of(2) {
        hi
    } else {
        lo
    }
}

fn checker(d: &Path, name: &str, lo: u8, hi: u8) {
    ImageBuffer::from_fn(N, N, |x, y| Luma([checker_value(x, y, lo, hi)]))
        .save(at(d, name))
        .unwrap();
}

/// RGBA with an alpha ramp along x.
fn rgba_ramp(d: &Path, name: &str) {
    ImageBuffer::from_fn(N, N, |x, y| Rgba([(y * 4) as u8, 60, 90, (x * 4) as u8]))
        .save(at(d, name))
        .unwrap();
}

fn toml(d: &Path, text: &str) {
    std::fs::write(d.join("in/sdc.toml"), text).unwrap();
}

fn req(d: &Path, target: Target, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.join("in")], d.join("out"), target);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    r
}

fn plan_of(r: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "helper-fill");
    let p = Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn plan(d: &Path, target: Target, sets: &[&str]) -> Plan {
    plan_of(req(d, target, sets)).0
}

fn run(p: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "helper-fill");
    let s = Engine::new()
        .unwrap()
        .run(p, &rep, &CancelToken::default())
        .unwrap();
    (s, sink.take())
}

fn job<'a>(p: &'a Plan, input: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.pack.is_none() && j.input.file_name().is_some_and(|n| n == input))
        .unwrap_or_else(|| panic!("no job for {input}: {:#?}", p.jobs))
}

fn out(d: &Path, name: &str) -> PathBuf {
    d.join("out/textures/a").join(name)
}

/// Mip 0 of a written DDS: its format and every texel's alpha code.
fn alpha(d: &Path, name: &str) -> (engine::dds::DdsPixelFormat, Vec<u8>) {
    let bytes = std::fs::read(out(d, name)).unwrap();
    let f = read_dds(&bytes).unwrap();
    let (w, h, r) = f.surfaces()[0].clone();
    let px = decode_level_rgba(f.format, w, h, &f.data[r], None).unwrap();
    (
        f.format,
        px.iter().map(|p| (p[3] * 255.0).round() as u8).collect(),
    )
}

fn assert_codes(got: &[u8], want: impl Fn(u32, u32) -> u8) {
    for (i, &g) in got.iter().enumerate() {
        let (x, y) = (i as u32 % N, i as u32 / N);
        let w = want(x, y);
        assert!(g.abs_diff(w) <= 1, "texel ({x}, {y}): alpha {g}, want {w}");
    }
}

const DIFFUSE: &str = "file:**/x.png.subtype=diffuse";

#[test]
fn opacity_fills_bc1_diffuse_and_moves_it_to_an_alpha_format() {
    // A project rule makes every alpha-less file BC1, so the format follows the analysed alpha.
    const BC1_RULE: &str =
        "[[rules]]\nmatch = { alpha = \"none\" }\noverrides = { format = \"bc1\" }\n";
    let bare = tempfile::tempdir().unwrap();
    rgb(bare.path(), "x.png");
    toml(bare.path(), BC1_RULE);
    let (s, _) = run(plan(bare.path(), Target::Cs, &[DIFFUSE]));
    assert_eq!(s.converted, 1);
    let (f, _) = alpha(bare.path(), "x.dds");
    assert!(
        matches!(f, engine::dds::DdsPixelFormat::Bc1 { .. }),
        "{f:?}"
    );

    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_opacity.png", 0, 255);
    toml(d.path(), BC1_RULE);
    let p = plan(d.path(), Target::Cs, &[DIFFUSE, "alpha.policy=blend"]);
    let j = job(&p, "x.png");
    assert_eq!(j.status, JobStatus::Ready, "{:#?}", j.warnings);
    let fill = j.fill.as_ref().expect("x.png takes the fill");
    assert!(fill.source.path.ends_with("x_opacity.png"));
    assert!(fill.source.stamp.is_some());
    assert!(!fill.assigned);
    assert_eq!(
        j.provenance["fill.a"].to_string(),
        "source:x_opacity.png:R",
        "{:?}",
        j.provenance
    );
    assert!(p.jobs.iter().all(|j| !j.input.ends_with("x_opacity.png")));
    let (_, ev) = run(p);
    let (f, a) = alpha(d.path(), "x.dds");
    assert!(
        !matches!(f, engine::dds::DdsPixelFormat::Bc1 { .. }),
        "{f:?}"
    );
    assert_codes(&a, |x, y| checker_value(x, y, 0, 255));
    assert!(!out(d.path(), "x_opacity.dds").exists());
    let skip = ev
        .iter()
        .find(|e| {
            e.path
                .as_deref()
                .is_some_and(|p| p.ends_with("/x_opacity.png"))
        })
        .expect("x_opacity.png has a terminal event");
    match &skip.kind {
        EventKind::FileSkipped {
            kind: SkipKind::PackedSource,
            reason,
            ..
        } => assert!(
            reason.starts_with("filled into ") && reason.ends_with("/out/textures/a/x.dds"),
            "{reason}"
        ),
        k => panic!("{k:?}"),
    }
}

#[test]
fn fill_runs_before_alpha_policy() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    // Near-binary: every code is <= 5 or >= 250.
    checker(d.path(), "x_opacity.png", 3, 252);
    // The source alone has no alpha (`remove` by analysis); the filled image's analysis proposes `cutout`.
    let p = plan(d.path(), Target::Cs, &[DIFFUSE]);
    let j = job(&p, "x.png");
    assert!(j.fill.is_some());
    match &j.status {
        JobStatus::Unresolved { reasons, .. } => assert!(
            reasons.iter().any(|r| r.contains("proposal Cutout")),
            "{reasons:?}"
        ),
        s => panic!("{s:?}"),
    }
    let p = plan(d.path(), Target::Cs, &[DIFFUSE, "alpha.policy=cutout"]);
    let j = job(&p, "x.png");
    assert_eq!(j.status, JobStatus::Ready, "{:#?}", j.warnings);
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Cutout);
    assert!(j.profile.coverage_enabled());
    // Without the helper the same file's alpha is removed by analysis.
    toml(
        d.path(),
        "[files.\"textures/a/x_opacity\"]\nhelper.role = \"none\"\n",
    );
    let p = plan(d.path(), Target::Cs, &[DIFFUSE]);
    let j = job(&p, "x.png");
    assert!(j.fill.is_none());
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove);
}

/// Final review I1: a raw map the user typed to convert alone (`[files] subtype`, no `helper.*`) fills nothing.
#[test]
fn raw_map_typed_to_convert_alone_fills_nothing() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_opacity.png", 0, 255);
    toml(
        d.path(),
        "[files.\"textures/a/x_opacity.png\"]\nsubtype = \"diffuse\"\n",
    );
    let (p, plan_ev) = plan_of(req(d.path(), Target::Cs, &[DIFFUSE, "alpha.policy=blend"]));
    assert!(
        job(&p, "x.png").fill.is_none(),
        "{:#?}",
        job(&p, "x.png").fill
    );
    assert_eq!(
        job(&p, "x_opacity.png").profile.subtype,
        Subtype::Diffuse,
        "converted alone"
    );
    let (_, run_ev) = run(p);
    let t = terminal(plan_ev.iter().chain(&run_ev), "x_opacity.png");
    assert!(t.iter().all(|k| !filled_into(k, "/x.dds")), "{t:?}");
}

const OPACITY_ASSIGNED: &str = "[files.\"textures/a/x_opacity\"]\nhelper.fills = [\"diffuse.a\"]\n";
const RGBA8: [&str; 3] = [DIFFUSE, "alpha.policy=blend", "format=rgba8"];

#[test]
fn assigned_opacity_replaces_varying_alpha() {
    let d = tempfile::tempdir().unwrap();
    rgba_ramp(d.path(), "x.png");
    checker(d.path(), "x_opacity.png", 0, 255);
    toml(d.path(), OPACITY_ASSIGNED);
    let p = plan(d.path(), Target::Cs, &RGBA8);
    let j = job(&p, "x.png");
    assert!(j.fill.as_ref().is_some_and(|f| f.assigned));
    run(p);
    assert_codes(&alpha(d.path(), "x.dds").1, |x, y| {
        checker_value(x, y, 0, 255)
    });
}

#[test]
fn suggested_opacity_leaves_varying_alpha() {
    let d = tempfile::tempdir().unwrap();
    rgba_ramp(d.path(), "x.png");
    checker(d.path(), "x_opacity.png", 0, 255);
    let p = plan(d.path(), Target::Cs, &RGBA8);
    assert!(job(&p, "x.png").fill.is_none());
    run(p);
    assert_codes(&alpha(d.path(), "x.dds").1, |x, _| (x * 4) as u8);
}

#[test]
fn specular_fills_vanilla_n_alpha() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_fn(N, N, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 255]))
        .save(at(d.path(), "x_n.png"))
        .unwrap();
    checker(d.path(), "x_spec.png", 40, 200);
    let p = plan(d.path(), Target::Vanilla, &["format=rgba8"]);
    let j = job(&p, "x_n.png");
    assert!(j.fill.is_some(), "{:#?}", p.jobs);
    assert_eq!(j.status, JobStatus::Ready, "{:#?}", j.warnings);
    run(p);
    assert_codes(&alpha(d.path(), "x_n.dds").1, |x, y| {
        checker_value(x, y, 40, 200)
    });
    assert!(!out(d.path(), "x_spec.dds").exists());
}

#[test]
fn inverted_luma_read() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    let px = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 200u8];
    ImageBuffer::from_fn(N, N, |x, y| Rgb(px(x, y)))
        .save(at(d.path(), "x_opacity.png"))
        .unwrap();
    toml(
        d.path(),
        "[files.\"textures/a/x_opacity\"]\nhelper.read = \"luma\"\nhelper.invert = true\n",
    );
    let p = plan(d.path(), Target::Cs, &RGBA8);
    let j = job(&p, "x.png");
    assert_eq!(
        j.provenance["fill.a"].to_string(),
        "source:1-x_opacity.png:luma",
        "{:?}",
        j.provenance
    );
    run(p);
    assert_codes(&alpha(d.path(), "x.dds").1, |x, y| {
        let [r, g, b] = px(x, y).map(|c| f32::from(c) / 255.0);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        ((1.0 - luma) * 255.0).round() as u8
    });
}

#[test]
fn sixteen_bit_helper_fills_eight_bit_target() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    ImageBuffer::from_fn(N, N, |x, _| Luma([(x * 1000) as u16]))
        .save(at(d.path(), "x_opacity.png"))
        .unwrap();
    let p = plan(d.path(), Target::Cs, &RGBA8);
    let j = job(&p, "x.png");
    let f = j.fill.as_ref().expect("x.png takes the fill");
    assert!(f.source.stamp.is_some());
    assert_eq!(j.status, JobStatus::Ready, "{:#?}", j.warnings);
    let (mut img, mut info) = load_image(&j.input).unwrap();
    assert_eq!(info.bit_depth, 8);
    apply_fill(&mut img, &mut info, f).unwrap();
    assert_eq!(info.bit_depth, 8);
    assert!(!info.is_float);
    assert!(info.has_alpha);
    assert_eq!(info.channels, 4);
    assert!((img.px[5][3] - 5000.0 / 65535.0).abs() < 1e-4);
}

/// A CRC-32 (PNG chunks).
fn crc(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// An 8-bit palette PNG (the `image` crate writes none): entry `i` has R = 4i, and a tRNS alpha of 7 for every entry.
fn palette_png(path: &Path) {
    use std::io::Write;
    let chunk = |out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let body: Vec<u8> = ty.iter().chain(data).copied().collect();
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    };
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = N.to_be_bytes().to_vec();
    ihdr.extend(N.to_be_bytes());
    ihdr.extend([8, 3, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    let plte: Vec<u8> = (0..N as u8).flat_map(|i| [i * 4, 30, 60]).collect();
    chunk(&mut png, b"PLTE", &plte);
    chunk(&mut png, b"tRNS", &[7; N as usize]);
    let raw: Vec<u8> = (0..N)
        .flat_map(|_| std::iter::once(0).chain(0..N as u8))
        .collect();
    let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
    z.write_all(&raw).unwrap();
    chunk(&mut png, b"IDAT", &z.finish().unwrap());
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).unwrap();
}

#[test]
fn palette_helper_with_trns_reads_r() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    palette_png(&at(d.path(), "x_opacity.png"));
    let (img, _) = load_image(&at(d.path(), "x_opacity.png")).unwrap();
    assert!((img.px[0][3] - 7.0 / 255.0).abs() < 1e-6, "the tRNS alpha");
    let p = plan(d.path(), Target::Cs, &RGBA8);
    assert!(job(&p, "x.png").fill.is_some());
    run(p);
    assert_codes(&alpha(d.path(), "x.dds").1, |x, _| (x * 4) as u8);
}

#[test]
fn fill_size_changed_since_planning_fails_at_load() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    checker(d.path(), "x_opacity.png", 0, 255);
    let p = plan(d.path(), Target::Cs, &RGBA8);
    let j = job(&p, "x.png");
    let f = j.fill.as_ref().expect("x.png takes the fill");
    ImageBuffer::from_fn(32, 32, |_, _| Luma([9u8]))
        .save(&f.source.path)
        .unwrap();
    let (mut img, mut info) = load_image(&j.input).unwrap();
    let e = apply_fill(&mut img, &mut info, f).unwrap_err();
    assert!(e.contains("size changed since planning"), "{e}");
    // The run's stamp check names the helper.
    let (s, ev) = run(p);
    assert_eq!(s.converted, 0);
    assert!(ev.iter().any(|e| match &e.kind {
        EventKind::FileFailed { error, .. } =>
            error.contains("x_opacity.png") && error.contains("changed since planning"),
        _ => false,
    }));
}

#[test]
fn cube_never_takes_a_fill() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_fn(384, 64, |x, y| Rgb([(x % 256) as u8, (y * 4) as u8, 90]))
        .save(at(d.path(), "chrome_e.png"))
        .unwrap();
    checker(d.path(), "chrome_opacity.png", 0, 255);
    let p = plan(d.path(), Target::Cs, &[]);
    let j = p
        .jobs
        .iter()
        .find(|j| j.cube.is_some())
        .unwrap_or_else(|| panic!("no cube job: {:#?}", p.jobs));
    assert!(j.fill.is_none());
}

/// Owner data (`SDC_HELPER_SKIN_DIR`, read only): every PNG there fills a generated diffuse of its size.
#[test]
#[ignore]
fn owner_skin_f_files_plan() {
    let Ok(dir) = std::env::var("SDC_HELPER_SKIN_DIR") else {
        eprintln!("SDC_HELPER_SKIN_DIR unset; skipped");
        return;
    };
    let mut seen = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let src = e.unwrap().path();
        if !src
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("png"))
        {
            continue;
        }
        let d = tempfile::tempdir().unwrap();
        let (w, h) = image::image_dimensions(&src).unwrap();
        ImageBuffer::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 90]))
            .save(at(d.path(), "x.png"))
            .unwrap();
        std::fs::copy(&src, at(d.path(), "x_h.png")).unwrap();
        toml(
            d.path(),
            "[files.\"textures/a/x_h\"]\nhelper.role = \"opacity\"\nhelper.fills = [\"diffuse.a\"]\n",
        );
        let p = plan(d.path(), Target::Cs, &[DIFFUSE, "alpha.policy=blend"]);
        let j = job(&p, "x.png");
        assert_eq!(
            j.status,
            JobStatus::Ready,
            "{}: {:#?}",
            src.display(),
            j.warnings
        );
        let f = j.fill.as_ref().expect("x.png takes the fill");
        let (mut img, mut info) = load_image(&j.input).unwrap();
        apply_fill(&mut img, &mut info, f).unwrap();
        assert!(
            img.px.iter().any(|p| p[3] < 1.0),
            "{}: alpha is all 1.0",
            src.display()
        );
        seen += 1;
    }
    assert!(seen > 0, "no PNG in SDC_HELPER_SKIN_DIR");
}

/// Every terminal event (skip, failure, done) for the input named `name`, from the plan and run events.
fn terminal<'a>(ev: impl IntoIterator<Item = &'a EngineEvent>, name: &str) -> Vec<&'a EventKind> {
    ev.into_iter()
        .filter(|e| {
            e.path
                .as_deref()
                .is_some_and(|p| p.ends_with(&format!("/{name}")))
        })
        .map(|e| &e.kind)
        .filter(|k| {
            matches!(
                k,
                EventKind::FileSkipped { .. }
                    | EventKind::FileFailed { .. }
                    | EventKind::FileDone { .. }
            )
        })
        .collect()
}

fn filled_into(k: &EventKind, out: &str) -> bool {
    matches!(k, EventKind::FileSkipped { kind: SkipKind::PackedSource, reason, .. }
        if reason.starts_with("filled into ") && reason.ends_with(out))
}

/// Review 4 I1: an assigned helper of another size fails its target; the helper still ends with that output.
#[test]
fn size_mismatch_helper_ends_with_its_target() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "x.png");
    ImageBuffer::from_fn(32, 32, |_, _| Luma([9u8]))
        .save(at(d.path(), "x_opacity.png"))
        .unwrap();
    toml(d.path(), OPACITY_ASSIGNED);
    let (p, plan_ev) = plan_of(req(d.path(), Target::Cs, &RGBA8));
    assert!(matches!(job(&p, "x.png").status, JobStatus::Error { .. }));
    let (_, run_ev) = run(p);
    let t = terminal(plan_ev.iter().chain(&run_ev), "x_opacity.png");
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(filled_into(t[0], "/x.dds"), "{t:?}");
}

struct HookVar(&'static str);

impl Drop for HookVar {
    fn drop(&mut self) {
        std::env::remove_var(self.0);
    }
}

/// Review 4 I1: a target that fails at the fill step has no job; its helper still ends with that output.
#[test]
fn fill_step_failure_still_ends_the_helper() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "fillboom.png");
    checker(d.path(), "fillboom_opacity.png", 0, 255);
    // The fill step is named by the output (pass 1 by the input), so only it panics.
    std::env::set_var("SDC_TEST_PANIC_ON_PLAN", "fillboom.dds@load");
    let _hook = HookVar("SDC_TEST_PANIC_ON_PLAN");
    let (p, plan_ev) = plan_of(req(
        d.path(),
        Target::Cs,
        &["file:**/fillboom.png.subtype=diffuse", "alpha.policy=blend"],
    ));
    assert!(p.jobs.iter().all(|j| !j.input.ends_with("fillboom.png")));
    assert_eq!(p.failed.len(), 1, "{:?}", p.failed);
    let (_, run_ev) = run(p);
    let all: Vec<&EngineEvent> = plan_ev.iter().chain(&run_ev).collect();
    let t = terminal(all.iter().copied(), "fillboom_opacity.png");
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(filled_into(t[0], "/fillboom.dds"), "{t:?}");
    assert!(matches!(
        terminal(all.iter().copied(), "fillboom.png")[..],
        [EventKind::FileFailed { .. }]
    ));
}

/// Review 4 I1: a derive (Also export) replaces the filled albedo; its helper is a member of the derived set and
/// ends once, and no derived job takes a fill.
#[test]
fn derive_replacing_the_target_ends_the_helper() {
    use engine::derive::recipe::LegacyParams;
    use engine::derive::{DeriveMode, DeriveRequest};
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("in/textures/pbr/arm");
    std::fs::create_dir_all(&dir).unwrap();
    let px = |name: &str, v: [u8; 4]| {
        ImageBuffer::from_pixel(16, 16, Rgba(v))
            .save(dir.join(name))
            .unwrap()
    };
    px("steel.png", [200, 200, 200, 255]);
    px("steel_n.png", [128, 128, 255, 255]);
    px("steel_rmaos.png", [128, 0, 255, 255]);
    ImageBuffer::from_fn(16, 16, |x, _| Luma([(x * 16) as u8]))
        .save(dir.join("steel_opacity.png"))
        .unwrap();
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("pbr"), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    // The filled albedo's alpha needs a meaning, or the set is undecided and not derived.
    r.overrides = vec![parse_set("alpha.policy=blend").unwrap()];
    let (cs, _) = plan_of(r);
    assert!(
        job(&cs, "steel.png").fill.is_some(),
        "the albedo takes the fill: {:#?}",
        cs.jobs
    );
    let sets = engine::also::derive_sets(&cs, &Default::default()).0;
    assert!(
        sets.iter()
            .any(|s| s.members.iter().any(|m| m.ends_with("steel_opacity.png"))),
        "{sets:#?}"
    );
    let mut r = BatchRequest::new(
        vec![d.path().join("in")],
        d.path().join("vanilla"),
        Target::Vanilla,
    );
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    // The filled albedo's alpha needs a meaning, or the set is undecided and not derived.
    r.overrides = vec![parse_set("alpha.policy=blend").unwrap()];
    r.derive = Some(DeriveRequest {
        mode: DeriveMode::Vanilla,
        params: LegacyParams::default(),
        sets,
        drop: vec![],
        scanned: false,
    });
    let (p, plan_ev) = plan_of(r);
    assert!(p.jobs.iter().any(|j| j.derive.is_some()), "{:#?}", p.jobs);
    assert!(p
        .jobs
        .iter()
        .all(|j| j.derive.is_none() || j.fill.is_none()));
    let (_, run_ev) = run(p);
    let t = terminal(plan_ev.iter().chain(&run_ev), "steel_opacity.png");
    assert_eq!(t.len(), 1, "{t:?}");
}

/// Review 4 I2: a rule on the filled alpha turns `alpha.exr_straight` on; the plan analyses the image the run loads
/// (straight RGB 0.6, not 1.2 un-premultiplied by the source's 0.5 alpha).
#[test]
fn exr_decode_setting_changed_by_the_fill_is_reloaded() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_pixel(N, N, Rgba([0.6f32, 0.6, 0.6, 0.5]))
        .save(at(d.path(), "x.exr"))
        .unwrap();
    ImageBuffer::from_pixel(N, N, Luma([255u8]))
        .save(at(d.path(), "x_opacity.png"))
        .unwrap();
    toml(
        d.path(),
        &format!(
            "{OPACITY_ASSIGNED}\n[[rules]]\nmatch = {{ alpha = \"constant255\" }}\noverrides = {{ alpha = {{ exr_straight = true }} }}\n"
        ),
    );
    let p = plan(
        d.path(),
        Target::Cs,
        &["file:**/x.exr.subtype=diffuse", "alpha.policy=blend"],
    );
    let j = job(&p, "x.exr");
    assert!(j.fill.is_some());
    assert!(j.profile.alpha.exr_straight);
    let clamp: Vec<_> = j
        .warnings
        .iter()
        .filter(|w| w.code == engine::codes::Code::LossClampHdr)
        .collect();
    assert!(clamp.is_empty(), "analysed un-premultiplied: {clamp:?}");
}

/// Review 4 I1: a batch cancelled as the fill step starts lists the target as cancelled; its helper still ends once.
#[test]
fn fill_step_cancel_still_ends_the_helper() {
    let d = tempfile::tempdir().unwrap();
    rgb(d.path(), "fillcancel.png");
    checker(d.path(), "fillcancel_opacity.png", 0, 255);
    // Named by the target's output, which only the fill step matches.
    std::env::set_var("SDC_TEST_CANCEL_ON", "fillcancel.dds");
    let _hook = HookVar("SDC_TEST_CANCEL_ON");
    let (p, plan_ev) = plan_of(req(
        d.path(),
        Target::Cs,
        &[
            "file:**/fillcancel.png.subtype=diffuse",
            "alpha.policy=blend",
        ],
    ));
    assert!(p.jobs.iter().all(|j| !j.input.ends_with("fillcancel.png")));
    assert!(
        p.cancelled.iter().any(|c| c.ends_with("fillcancel.png")),
        "{:?}",
        p.cancelled
    );
    let (s, run_ev) = run(p);
    assert!(s.cancelled.iter().any(|c| c.ends_with("fillcancel.png")));
    let t = terminal(plan_ev.iter().chain(&run_ev), "fillcancel_opacity.png");
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(filled_into(t[0], "/fillcancel.dds"), "{t:?}");
}
