//! Complex grass sub-type (design `F:\ai\dds\nifparseref\plan\esp\2026-10-12-complex-grass-design.md`).
use engine::analyse::code8;
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::image::Rgba32F;
use engine::policy::{
    availability, offered, AcceptSuggestions, Availability, JobStatus, ResolvedJob,
};
use image::{DynamicImage, ImageBuffer, Rgba};
use std::sync::Arc;
use tempfile::TempDir;

const TAG: &str = "file:**/g.png.subtype=complex_grass";
const CUTOUT: &str = "file:**/g.png.alpha.policy=cutout";
const GRASS_CODES: [Code; 5] = [
    Code::GrassShape,
    Code::GrassTopOpaque,
    Code::GrassNormals,
    Code::GrassDetectSource,
    Code::ValGrassDetect,
];

/// A `w` × 2`w` complex-grass source: top = colour with a ~30 % cutout alpha (all texels 0 or 255), bottom = unit
/// normals tilted along x with specular 12 in alpha. Its bottom-left texel is a unit vector.
fn grass(w: u32) -> Rgba32F {
    let mut img = Rgba32F::new(w, 2 * w, [0.0; 4]);
    for y in 0..2 * w {
        for x in 0..w {
            *img.at_mut(x, y) = if y < w {
                let a = if (x * 7 + y * 13) % 10 < 3 { 1.0 } else { 0.0 };
                [0.25, 0.5, 0.15, a]
            } else {
                let t = (x as f32 / w as f32 - 0.5) * 0.8;
                [t.sin() * 0.5 + 0.5, 0.5, t.cos() * 0.5 + 0.5, 12.0 / 255.0]
            };
        }
    }
    img
}

fn png(img: &Rgba32F) -> DynamicImage {
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(img.width, img.height, |x, y| {
        Rgba(img.at(x, y).map(code8))
    }))
}

/// Writes `img` as `in/textures/plants/<name>`.
fn put(d: &TempDir, name: &str, img: &Rgba32F) {
    let p = d.path().join("in/textures/plants").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    png(img).save(p).unwrap();
}

fn req(d: &TempDir, target: Target, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), target);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.on_existing = OnExisting::Overwrite;
    r.overrides = sets
        .iter()
        .map(|s| engine::overrides::parse_set(s).unwrap())
        .collect();
    r
}

fn plan_req(r: BatchRequest) -> Plan {
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap()
}

fn plan(d: &TempDir, target: Target, sets: &[&str]) -> Plan {
    plan_req(req(d, target, sets))
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn grass_codes(j: &ResolvedJob) -> Vec<Code> {
    codes(j)
        .into_iter()
        .filter(|c| GRASS_CODES.contains(c))
        .collect()
}

/// A clean source tagged by hand with its alpha meaning given: Ready, no grass code.
#[test]
fn a_clean_tagged_source_is_ready_with_no_grass_code() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
    assert_eq!(
        (j.profile.format, j.profile.srgb_tag()),
        (Format::Bc7, false)
    );
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
}

/// §4 first bullet (Review Focus 1): not exactly 1:2 or not a power of two is an error.
#[test]
fn shape_errors_are_grass_shape() {
    for (w, h) in [(32, 32), (64, 32), (48, 96)] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &Rgba32F::new(w, h, [0.5, 0.5, 1.0, 1.0]));
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
        let j = job(&p, "g.png");
        assert!(
            codes(j).contains(&Code::GrassShape),
            "{w}x{h}: {:?}",
            j.warnings
        );
        assert!(
            matches!(&j.status, JobStatus::Error { codes } if codes.contains(&Code::GrassShape)),
            "{w}x{h}: {:?}",
            j.status
        );
    }
}

/// §4 second bullet: the top half has no transparency.
#[test]
fn an_opaque_top_half_warns() {
    let d = tempfile::tempdir().unwrap();
    let mut img = grass(32);
    for px in &mut img.px[..32 * 32] {
        px[3] = 1.0;
    }
    put(&d, "g.png", &img);
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(grass_codes(job(&p, "g.png")), [Code::GrassTopOpaque]);
}

/// §4 third bullet (Review Focus 5): fewer than 99 % unit normals in the bottom half.
#[test]
fn bottom_normals_share_threshold() {
    for (bad, want) in [(5usize, vec![]), (21, vec![Code::GrassNormals])] {
        // 32 × 32 bottom half = 1024 texels: 5 is 0.5 %, 21 is 2.05 %. Zero vectors, never the corner texel.
        let d = tempfile::tempdir().unwrap();
        let mut img = grass(32);
        for i in 0..bad {
            img.px[32 * 32 + i] = [0.5, 0.5, 0.5, 12.0 / 255.0];
        }
        put(&d, "g.png", &img);
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
        assert_eq!(grass_codes(job(&p, "g.png")), want, "{bad} bad texels");
    }
}

/// §4 last bullet: the source's bottom-left texel already fails CS's 0.03 test.
#[test]
fn a_failing_source_corner_warns() {
    let d = tempfile::tempdir().unwrap();
    let mut img = grass(32);
    *img.at_mut(0, 63) = [0.5, 0.5, 0.75, 12.0 / 255.0]; // decodes to (0, 0, 0.5): |L-1| = 0.5
    put(&d, "g.png", &img);
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(grass_codes(j), [Code::GrassDetectSource]);
    let w = j
        .warnings
        .iter()
        .find(|w| w.code == Code::GrassDetectSource)
        .unwrap();
    assert!(w.message.contains("renormalised"), "{}", w.message);
}

/// Note AZ (reverses R1): under vanilla the tag stays, with no warning, and the file is not blocked.
#[test]
fn vanilla_tag_stays_complex_grass() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Vanilla, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
    assert!(!codes(j).contains(&Code::CfgUnavailable));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(
        availability(Target::Vanilla, Subtype::ComplexGrass),
        Availability::Available
    );
    assert!(!offered(Target::Vanilla, Subtype::ComplexGrass));
    assert!(!offered(Target::Cs, Subtype::ComplexGrass));
    assert!(offered(Target::Vanilla, Subtype::Diffuse));
}

/// R4, R7: BC7 UNORM only, and no resize.
#[test]
fn another_format_an_srgb_tag_or_a_resize_is_cfg_invalid() {
    for extra in [
        "file:**/g.png.format=bc3",
        "file:**/g.png.color.tag=srgb",
        "file:**/g.png.resize=half",
    ] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &grass(512)); // 512 x 1024: `half` applies (larger side > 512)
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT, extra]);
        let j = job(&p, "g.png");
        assert!(
            j.warnings
                .iter()
                .any(|w| w.code == Code::CfgInvalid && w.message.contains("complex_grass")),
            "{extra}: {:?}",
            j.warnings
        );
    }
}

/// X12 (2026-10-13): no alpha question for complex grass; varying alpha defaults to cutout (top half; the bottom
/// half is data).
#[test]
fn the_alpha_proposal_reads_the_top_half() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Cs, &[TAG]);
    let j = job(&p, "g.png");
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Cutout);
    let c = codes(j);
    assert!(
        !c.contains(&Code::LossAlphaData) && !c.contains(&Code::SugLowConfidence),
        "{c:?}"
    );
}

/// Review Focus 3: a tall texture that is not tagged complex grass gets no grass code.
#[test]
fn a_tall_diffuse_raises_no_grass_code() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "tall.png", &grass(32));
    let p = plan(&d, Target::Cs, &["file:**/tall.png.alpha.policy=cutout"]);
    let j = job(&p, "tall.png");
    assert_ne!(j.profile.subtype, Subtype::ComplexGrass);
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
}

fn run(d: &TempDir, target: Target, sets: &[&str]) -> (Vec<EngineEvent>, BatchSummary) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let e = Engine::new().unwrap();
    let p = e
        .plan(req(d, target, sets), &rep, &CancelToken::default())
        .unwrap();
    let s = e.run(p, &rep, &CancelToken::default()).unwrap();
    (sink.take(), s)
}

fn out(d: &TempDir) -> engine::dds::DdsFile {
    let b = std::fs::read(d.path().join("out/textures/plants/g.dds")).unwrap();
    engine::dds::read_dds(&b).unwrap()
}

/// §3.5, R6: one BC7 UNORM file, 64 x 128 down to 1 x 2 (7 levels).
#[test]
fn a_tagged_grass_texture_is_one_bc7_unorm_file_down_to_one_by_two() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(64));
    let (_, s) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(s.converted, 1);
    let f = out(&d);
    assert_eq!(f.format.as_format(), Some((Format::Bc7, false)));
    assert_eq!((f.width, f.height, f.mip_count), (64, 128, 7));
}

fn decoded_corner(f: &engine::dds::DdsFile) -> [u8; 4] {
    use engine::decode::{decode_level, Decoded};
    let (w, h, r) = f.levels[0].clone();
    match decode_level(f.format, w, h, &f.data[r], None).unwrap() {
        Decoded::Ldr(px) => px[((h - 1) * w) as usize],
        Decoded::Hdr(_) => panic!("LDR expected"),
    }
}

fn corner_err(t: [u8; 4]) -> f32 {
    engine::grass::unit_err([t[0], t[1], t[2]].map(|c| f32::from(c) / 255.0))
}

fn file_done(events: &[EngineEvent]) -> engine::validate::ValidationSummary {
    events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileDone { validation, .. } => Some(validation.clone()),
            _ => None,
        })
        .expect("file_done")
}

/// §3.6: the written file's decoded bottom-left texel is within the guard.
#[test]
fn the_written_corner_passes_the_guard() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(64));
    let (events, _) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert!(file_done(&events).passed);
    let e = corner_err(decoded_corner(&out(&d)));
    assert!(e < 0.02, "{e}");
}

/// §3.6: a corner no BC7 encode can bring to unit length is the named error.
#[test]
fn a_corner_no_encode_can_fix_is_val_grass_detect() {
    use engine::dds::{dds_bytes, resolve_header, DdsDesc};
    use engine::encode::{encode_level, EncodeCtx};
    use engine::quantize::EncoderInput;
    use engine::validate::{validate, ValidationInput};
    let mut p = engine::presets::PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, Subtype::ComplexGrass)
        .unwrap();
    p.alpha.policy = AlphaPolicy::Cutout;
    for (grey, want) in [(false, false), (true, true)] {
        let mut px = vec![[200u8, 180, 90, 255]; 16];
        px.extend(vec![[128u8, 128, 255, 12]; 16]);
        if grey {
            px[28] = [128, 128, 128, 12]; // texel (0, 7): length ~0
        }
        let input = EncoderInput::Ldr(px);
        let level = encode_level(&input, 4, 8, &p, &EncodeCtx::default()).unwrap();
        let desc = DdsDesc {
            width: 4,
            height: 8,
            format: Format::Bc7,
            srgb: false,
            header: resolve_header(Format::Bc7, false, p.header),
        };
        let bytes = dds_bytes(&desc, &[level]).unwrap();
        let r = validate(&ValidationInput {
            bytes: &bytes,
            profile: &p,
            encoder_input: Some(std::slice::from_ref(&input)),
            base_coverage: None,
            source_peak: None,
            bc1_model: encoders_sys::Bc1Model::Ideal,
            expected_levels: Some(1),
        });
        let named = r.warnings.iter().any(|w| w.code == Code::ValGrassDetect);
        assert_eq!(named, want, "grey {grey}: {:?}", r.warnings);
        assert_eq!(r.summary.passed, !want);
    }
}

/// Review Focus 2: a source without an alpha channel resolves `remove`, encodes opaque in both halves and passes.
#[test]
fn an_rgb_grass_source_encodes_opaque_and_passes() {
    let d = tempfile::tempdir().unwrap();
    let rgb = image::DynamicImage::ImageRgb8(png(&grass(32)).to_rgb8());
    let p = d.path().join("in/textures/plants/g.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    rgb.save(&p).unwrap();
    let plan = plan(&d, Target::Cs, &[TAG]);
    let j = job(&plan, "g.png");
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove);
    assert_eq!(grass_codes(j), [Code::GrassTopOpaque]);
    let (events, s) = run(&d, Target::Cs, &[TAG]);
    assert_eq!(s.converted, 1);
    let v = file_done(&events);
    assert!(v.passed, "{v:?}");
    // `exact_opaque` is off for this sub-type, so the `remove` guarantee is 254 (mode-6 p-bit), as for diffuse.
    assert!(decoded_corner(&out(&d))[3] >= 254);
}

/// Design §6: the owner's example (2048 x 4096, bottom-left texel (127, 126, 255, 10)) encodes, the guard passes and the
/// decoded bottom-left texel passes CS's 0.03 test. Owner data, read only (copied into a temp dir):
/// `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p engine --test complex_grass -- --ignored`
#[test]
#[ignore]
fn owner_example_encodes_and_stays_complex() {
    std::env::var("SDC_ACHERUS")
        .expect("owner-data test: set SDC_ACHERUS (it marks the owner's machine)");
    let src = std::path::Path::new(
        r"F:\ai\dds\complexgrass\textures\landscape\grass\grassfieldtundra.png",
    );
    assert!(src.is_file(), "{} is missing", src.display());
    let d = tempfile::tempdir().unwrap();
    let dst = d.path().join("in/textures/plants/g.png");
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    std::fs::copy(src, &dst).unwrap();
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
    let (img, info) = engine::load::load_image(&dst).unwrap();
    let g = engine::analyse::analyse(&img, &info).grass.unwrap();
    drop(img);
    assert!(g.corner_err < 0.001 && g.bottom_unit_share > 0.999, "{g:?}");
    let (events, s) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(s.converted, 1);
    assert!(file_done(&events).passed);
    let f = out(&d);
    assert_eq!(f.format.as_format(), Some((Format::Bc7, false)));
    assert_eq!((f.width, f.height, f.mip_count), (2048, 4096, 12));
    let e = corner_err(decoded_corner(&f));
    assert!(e < 0.03, "decoded corner |L-1| = {e}");
    eprintln!(
        "owner example: decoded corner {:?}, |L-1| = {e:.5}",
        decoded_corner(&f)
    );
}

fn plan_gras(d: &TempDir, target: Target, gras: bool, sets: &[&str]) -> Plan {
    let mut r = req(d, target, sets);
    if gras {
        r.gras_diffuses = ["textures/plants/g".to_string()].into();
    }
    plan_req(r)
}

/// §5: all four conditions hold: a high-confidence complex_grass, decided under --accept-suggestions high.
#[test]
fn all_four_conditions_tag_complex_grass() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan_gras(&d, Target::Cs, true, &[]);
    let j = job(&p, "g.png");
    assert_eq!(
        (j.suggestions[0].subtype, j.suggestions[0].confidence),
        (Subtype::ComplexGrass, engine::suggest::Confidence::High)
    );
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
}

/// §5 (Review Focus 3): each condition false alone leaves the existing classification.
#[test]
fn each_condition_false_alone_leaves_the_classification() {
    let mut bad_corner = grass(32);
    *bad_corner.at_mut(0, 63) = [0.5, 0.5, 0.5, 12.0 / 255.0];
    let mut bad_bottom = grass(32);
    for i in 0..21 {
        bad_bottom.px[32 * 32 + i] = [0.5, 0.5, 0.5, 12.0 / 255.0];
    }
    let square = Rgba32F::new(32, 32, [0.25, 0.5, 0.15, 1.0]);
    for (what, img, gras) in [
        ("no GRAS record", grass(32), false),
        ("not 1:2", square, true),
        ("corner fails 0.03", bad_corner, true),
        ("bottom < 99 % unit", bad_bottom, true),
    ] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &img);
        let j = job(&plan_gras(&d, Target::Cs, gras, &[]), "g.png").clone();
        assert!(
            j.suggestions
                .iter()
                .all(|s| s.subtype != Subtype::ComplexGrass),
            "{what}: {:?}",
            j.suggestions
        );
        assert_ne!(j.profile.subtype, Subtype::ComplexGrass, "{what}");
    }
}

/// §5 last paragraph: a user tag always wins, both ways.
#[test]
fn a_user_tag_wins_both_ways() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan_gras(&d, Target::Cs, true, &["file:**/g.png.subtype=diffuse"]);
    assert_eq!(job(&p, "g.png").profile.subtype, Subtype::Diffuse);
    let p = plan_gras(&d, Target::Cs, false, &[TAG]);
    assert_eq!(job(&p, "g.png").profile.subtype, Subtype::ComplexGrass);
}

/// Note AZ (reverses R1): a GRAS diffuse is auto-tagged under vanilla too.
#[test]
fn vanilla_is_auto_tagged() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan_gras(&d, Target::Vanilla, true, &[]);
    let j = job(&p, "g.png");
    assert_eq!(
        (j.suggestions[0].subtype, j.suggestions[0].confidence),
        (Subtype::ComplexGrass, engine::suggest::Confidence::High)
    );
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
}

/// Plan `rel` (under `in/`) with an explicit set shader for its set, and the GRAS flag on or off.
fn plan_shader(rel: &str, shader: Option<engine::shader::Shader>, gras: bool) -> (TempDir, Plan) {
    let d = tempfile::tempdir().unwrap();
    let key = rel.trim_end_matches(".png");
    let p = d.path().join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    png(&grass(32)).save(p).unwrap();
    let mut r = req(&d, Target::Cs, &[]);
    if gras {
        r.gras_diffuses = [key.to_string()].into();
    }
    if let Some(s) = shader {
        r.set_shaders = [(key.to_string(), s)].into();
    }
    let plan = plan_req(r);
    (d, plan)
}

/// 2026-10-13: the set's complex_grass shader makes its colour member complex grass, over a landscape path too.
#[test]
fn a_set_shader_complex_grass_tags_its_member() {
    use engine::shader::Shader;
    for rel in ["textures/plants/g.png", "textures/landscape/grass/g.png"] {
        let (_d, p) = plan_shader(rel, Some(Shader::ComplexGrass), false);
        let j = job(&p, "g.png");
        assert_eq!(j.profile.subtype, Subtype::ComplexGrass, "{rel}");
        assert_eq!(
            j.provenance
                .get("subtype")
                .map(ToString::to_string)
                .as_deref(),
            Some("shader:complex_grass"),
            "{rel}"
        );
    }
}

/// 2026-10-13: any other explicit set shader keeps a GRAS-listed diffuse an ordinary texture.
#[test]
fn another_explicit_set_shader_is_not_grass() {
    use engine::shader::Shader;
    let (_d, p) = plan_shader(
        "textures/landscape/grass/g.png",
        Some(Shader::Landscape),
        true,
    );
    assert_eq!(job(&p, "g.png").profile.subtype, Subtype::LandscapeDiffuse);
    for s in [Shader::Default, Shader::Envmap] {
        let (_d, p) = plan_shader("textures/plants/g.png", Some(s), true);
        assert_ne!(
            job(&p, "g.png").profile.subtype,
            Subtype::ComplexGrass,
            "{s:?}"
        );
    }
}

/// An auto shader plus a GRAS-listed diffuse is still grass, and the set auto-picks complex_grass.
#[test]
fn an_auto_shader_with_a_gras_diffuse_is_grass() {
    let (_d, p) = plan_shader("textures/plants/g.png", None, true);
    assert_eq!(job(&p, "g.png").profile.subtype, Subtype::ComplexGrass);
    assert_eq!(
        p.shaders.values().next().map(|s| s.shader),
        Some(engine::shader::Shader::ComplexGrass)
    );
}

/// Audit REPORT #4 (Note BC): the bottom half's alpha is specular data, so a specular alternating 100/160 around
/// the cutout threshold (50 % passing at mip 0, drifting at lower mips) raises no VAL-COVERAGE; the top half is
/// preserved in `grass::build`.
#[test]
fn a_varying_specular_raises_no_val_coverage() {
    let d = tempfile::tempdir().unwrap();
    let mut img = grass(64);
    for y in 64..128 {
        for x in 0..64 {
            img.at_mut(x, y)[3] = if (x + y) % 2 == 0 { 100.0 } else { 160.0 } / 255.0;
        }
    }
    put(&d, "g.png", &img);
    let (events, s) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(s.converted, 1);
    let codes: Vec<Code> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) => Some(w.code),
            _ => None,
        })
        .collect();
    assert!(!codes.contains(&Code::ValCoverage), "{codes:?}");
}
