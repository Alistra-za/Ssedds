//! Task 9 (owner, 2026-10-13; DECISIONS Z11): when a texture's sub-type changes, its explicit format and tag that
//! the new sub-type does not allow are cleared, so the new type's default applies; a note says so.
mod support;
use engine::config::{ColorTag, Format, Subtype, Target};
use engine::policy::allowed_encodings;
use engine::shader::Shader;
use sdc_gui::model::bulk::MenuAction;
use sdc_gui::model::sets::set_views;
use sdc_gui::model::AppModel;
use serde_json::json;
use std::time::Duration;

const KEY: &str = "plants/g.png";

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn file(m: &AppModel, f: &str) -> Option<serde_json::Value> {
    m.project.as_ref().unwrap().file_value(KEY, f)
}

fn job_of(m: &AppModel) -> engine::policy::ResolvedJob {
    m.item(KEY).and_then(|i| i.job.clone()).expect("job")
}

/// The owner's case: `g.png` with `g_rmaos.png` is a TruePBR set (its albedo `pbr_albedo`); under the complex_grass
/// shader the albedo becomes complex grass, and `e` is written on it explicitly. Returns the model and the set id.
fn grass_with(dir: &std::path::Path, e: Option<(Format, ColorTag)>) -> (AppModel, String) {
    let t = dir.join("textures/plants");
    std::fs::create_dir_all(&t).unwrap();
    image::ImageBuffer::from_fn(32, 64, |_, y| {
        image::Rgba([64u8, 128, 38, if y % 3 == 0 { 255 } else { 0 }])
    })
    .save(t.join("g.png"))
    .unwrap();
    image::ImageBuffer::from_fn(32, 32, |_, _| image::Rgb([100u8, 0, 255]))
        .save(t.join("g_rmaos.png"))
        .unwrap();
    let mut m = support::opened(dir);
    m.accept = engine::policy::AcceptSuggestions::High;
    m.replan();
    idle(&mut m);
    assert_eq!(job_of(&m).profile.subtype, Subtype::PbrAlbedo);
    let set = set_views(&m)
        .into_iter()
        .find(|v| v.members.iter().any(|k| k == KEY))
        .expect("the set")
        .id;
    m.set_shader(&set, Some(Shader::ComplexGrass));
    idle(&mut m);
    assert_eq!(job_of(&m).profile.subtype, Subtype::ComplexGrass);
    if let Some((f, t)) = e {
        m.set_format(KEY, f, t);
        idle(&mut m);
    }
    m.encoding_note = None;
    (m, set)
}

const LINEAR: (Format, ColorTag) = (Format::Bc7, ColorTag::Unorm);

#[test]
fn switching_grass_to_pbr_resets_a_linear_albedo_and_says_so() {
    assert!(allowed_encodings(Subtype::ComplexGrass, Target::Cs).contains(&LINEAR));
    let d = tempfile::tempdir().unwrap();
    let (mut m, set) = grass_with(d.path(), Some(LINEAR));
    assert_eq!(file(&m, "format"), Some(json!("bc7")));
    m.set_shader(&set, Some(Shader::Pbr));
    idle(&mut m);
    let p = job_of(&m).profile;
    assert_ne!(p.subtype, Subtype::ComplexGrass);
    assert!(
        !allowed_encodings(p.subtype, m.target).contains(&LINEAR),
        "{:?} {:?}",
        p.subtype,
        m.target
    );
    assert_eq!(file(&m, "format"), None, "the explicit format is cleared");
    assert_eq!(file(&m, "color.tag"), None, "the explicit tag is cleared");
    assert_eq!((p.format, p.color.tag), (Format::Bc7, ColorTag::Srgb));
    let (line, _) = m.encoding_note.clone().expect("a note");
    assert!(
        line.starts_with("g.png: complex grass → pbr albedo"),
        "{line}"
    );
    assert!(line.contains("BC7 · sRGB"), "{line}");
    assert!(
        m.messages.entries().any(|e| e.text == line),
        "the status-line note is in the Messages log"
    );
}

#[test]
fn an_explicit_encoding_the_new_type_allows_is_kept() {
    let d = tempfile::tempdir().unwrap();
    let srgb = (Format::Bc7, ColorTag::Srgb);
    let (mut m, set) = grass_with(d.path(), Some(srgb));
    m.set_shader(&set, Some(Shader::Pbr));
    idle(&mut m);
    assert!(allowed_encodings(job_of(&m).profile.subtype, m.target).contains(&srgb));
    assert_eq!(file(&m, "format"), Some(json!("bc7")));
    assert_eq!(file(&m, "color.tag"), Some(json!("srgb")));
    assert_eq!(m.encoding_note, None);
}

#[test]
fn a_texture_in_a_locked_slot_is_untouched_and_reported() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, set) = grass_with(d.path(), Some(LINEAR));
    assert_eq!(
        m.run_menu(MenuAction::SlotLock {
            set: set.clone(),
            n: 1,
            on: true
        }),
        None
    );
    idle(&mut m);
    m.set_shader(&set, Some(Shader::Pbr));
    idle(&mut m);
    assert_eq!(file(&m, "format"), Some(json!("bc7")));
    assert_eq!(file(&m, "color.tag"), Some(json!("unorm")));
    let (line, _) = m.encoding_note.clone().expect("a note");
    assert!(line.contains("slot locked"), "{line}");
}

#[test]
fn one_undo_restores_the_type_and_the_cleared_encoding() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, set) = grass_with(d.path(), Some(LINEAR));
    let steps = m.history.undo.len();
    m.set_shader(&set, Some(Shader::Pbr));
    idle(&mut m);
    assert_eq!(file(&m, "format"), None);
    assert_eq!(
        m.history.undo.len(),
        steps + 1,
        "the cleanup is part of the shader step"
    );
    m.undo();
    idle(&mut m);
    assert_eq!(job_of(&m).profile.subtype, Subtype::ComplexGrass);
    assert_eq!(file(&m, "format"), Some(json!("bc7")));
    assert_eq!(file(&m, "color.tag"), Some(json!("unorm")));
    assert_eq!(m.encoding_note, None, "the undo dismissed the note");
    assert!(m.can_redo(), "the undo did not start a new step");
    m.redo();
    idle(&mut m);
    assert_eq!(file(&m, "format"), None);
}

#[test]
fn the_cleanup_costs_one_extra_plan() {
    let flip = |e| {
        let d = tempfile::tempdir().unwrap();
        let (mut m, set) = grass_with(d.path(), e);
        let g = m.generation();
        m.set_shader(&set, Some(Shader::Pbr));
        idle(&mut m);
        m.generation() - g
    };
    let (plain, cleaned) = (flip(None), flip(Some(LINEAR)));
    assert!(
        cleaned == plain + 1,
        "{plain} plans without, {cleaned} with"
    );
}

/// `name.png` with `name_rmaos.png` under `textures/plants`: a TruePBR set once High suggestions are accepted.
fn pbr_pair(dir: &std::path::Path, name: &str) {
    let t = dir.join("textures/plants");
    std::fs::create_dir_all(&t).unwrap();
    image::ImageBuffer::from_fn(32, 64, |_, y| {
        image::Rgba([64u8, 128, 38, if y % 3 == 0 { 255 } else { 0 }])
    })
    .save(t.join(format!("{name}.png")))
    .unwrap();
    image::ImageBuffer::from_fn(32, 32, |_, _| image::Rgb([100u8, 0, 255]))
        .save(t.join(format!("{name}_rmaos.png")))
        .unwrap();
}

/// Review M6: two textures re-typed by one bulk edit give one count line, each listed on hover, in one step.
#[test]
fn several_textures_give_one_count_line_with_each_on_hover() {
    use sdc_gui::model::bulk::BulkOp;
    let d = tempfile::tempdir().unwrap();
    pbr_pair(d.path(), "g");
    pbr_pair(d.path(), "h");
    let mut m = support::opened(d.path());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.replan();
    idle(&mut m);
    let keys = vec!["plants/g.png".to_string(), "plants/h.png".to_string()];
    for k in &keys {
        m.set_field(k, "subtype", json!("complex_grass"));
        m.set_format(k, Format::Bc7, ColorTag::Unorm);
    }
    idle(&mut m);
    let steps = m.history.undo.len();
    m.apply_bulk_edit(&keys, "subtype", BulkOp::Set(json!("pbr_albedo")));
    idle(&mut m);
    assert_eq!(
        m.history.undo.len(),
        steps + 1,
        "one step for the edit and the cleanup"
    );
    for k in &keys {
        assert_eq!(
            m.project.as_ref().unwrap().file_value(k, "format"),
            None,
            "{k}"
        );
    }
    let (line, hover) = m.encoding_note.clone().expect("a note");
    assert!(
        line.starts_with("Encoding follows type: 2 textures"),
        "{line}"
    );
    assert!(
        hover.contains("g.png: complex grass → pbr albedo"),
        "{hover}"
    );
    assert!(
        hover.contains("h.png: complex grass → pbr albedo"),
        "{hover}"
    );
}

/// Review M6: a field-locked Format value is kept and reported, never cleared. No field lock covers `format`
/// today, so the rule is checked directly.
#[test]
fn a_field_locked_format_is_kept_and_reported() {
    use sdc_gui::model::encoding_follow::{verdict, Verdict};
    let t = Target::Cs;
    assert_eq!(
        verdict(t, Subtype::PbrAlbedo, false, LINEAR, true, None),
        Verdict::Clear
    );
    assert_eq!(
        verdict(
            t,
            Subtype::PbrAlbedo,
            false,
            LINEAR,
            true,
            Some("locked here")
        ),
        Verdict::Kept("locked here".into())
    );
    assert_eq!(
        verdict(t, Subtype::PbrAlbedo, false, LINEAR, false, None),
        Verdict::Keep,
        "nothing explicit"
    );
    assert_eq!(
        verdict(
            t,
            Subtype::PbrAlbedo,
            false,
            (Format::Bc7, ColorTag::Srgb),
            true,
            None
        ),
        Verdict::Keep,
        "still allowed"
    );
}

#[test]
fn undo_after_the_encoding_cleanup_names_the_step_that_caused_it() {
    let d = tempfile::tempdir().unwrap();
    let (mut m, set) = grass_with(d.path(), Some(LINEAR));
    m.set_shader(&set, Some(Shader::Pbr));
    idle(&mut m);
    assert_eq!(file(&m, "format"), None, "the cleanup ran");
    // The cleanup folded into the Shader step: no step of its own.
    assert_eq!(m.undo_label(), Some("Shader"));
    m.undo();
    assert!(
        m.status_line.starts_with("Undid: Shader."),
        "{}",
        m.status_line
    );
}

/// Trace-debug #31 (Note BS13): an explicit R8 on a texture re-typed to a grey `skin_wet` is allowed and kept; on a
/// colour one it is cleared.
#[test]
fn a_grey_skin_wet_keeps_an_explicit_r8() {
    use sdc_gui::model::encoding_follow::{verdict, Verdict};
    let r8 = *engine::policy::allowed_encodings_for(Subtype::SkinWet, Target::Cs, true)
        .iter()
        .find(|(f, _)| *f == Format::R8)
        .unwrap();
    let st = Subtype::SkinWet;
    assert_eq!(verdict(Target::Cs, st, true, r8, true, None), Verdict::Keep);
    assert_eq!(
        verdict(Target::Cs, st, false, r8, true, None),
        Verdict::Clear
    );
}

/// Note FC3 (supersedes Z11's status line): a cleared format is an Info row per texture, with Undo while its step is
/// the last one; it leaves "ready" alone.
#[test]
fn a_cleared_format_is_an_info_row_with_undo() {
    use sdc_gui::model::bottom::{counts_as_ready_problem, problems, At, Fix, Level};
    let d = tempfile::tempdir().unwrap();
    let (mut m, set) = grass_with(d.path(), Some(LINEAR));
    m.set_shader(&set, Some(Shader::Pbr));
    idle(&mut m);
    // Note FC1's "types set automatically" row is the opening's, not this edit's.
    let rows: Vec<_> = problems(&m)
        .iter()
        .filter(|p| p.level == Level::Info && !p.text.contains("set automatically"))
        .cloned()
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].at, At::Project);
    assert!(
        rows[0]
            .text
            .starts_with("g.png: complex grass → pbr albedo"),
        "{}",
        rows[0].text
    );
    assert_eq!(rows[0].fix, Some(Fix::UndoLast));
    assert!(!counts_as_ready_problem(&rows[0]));
    m.undo();
    idle(&mut m);
    assert!(!problems(&m).iter().any(|p| p.level == Level::Info));
    assert_eq!(
        file(&m, "format"),
        Some(json!("bc7")),
        "the undo restored it"
    );
}
