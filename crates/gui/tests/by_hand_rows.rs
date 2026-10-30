//! Note FC10 (theme 10): remedies that told the user to fix something by hand are Problems rows with a button. Each
//! button writes on its own click only, never when the row is drawn.
mod support;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use image::{ImageBuffer, Rgb, Rgba};
use sdc_gui::model::bottom::{problems, At, Fix, Level, Problem};
use sdc_gui::model::shell::ShellRequest;
use sdc_gui::model::AppModel;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::*;

const WAIT: Duration = Duration::from_secs(120);

fn harness(mut m: AppModel) -> Harness<'static, AppModel> {
    m.open_problems(None);
    let h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn idle(m: &mut AppModel) {
    assert!(m.wait_idle(WAIT), "the model did not settle");
}

fn row(m: &AppModel, f: impl Fn(&Fix) -> bool) -> Option<Problem> {
    problems(m)
        .iter()
        .find(|p| p.fix.as_ref().is_some_and(&f))
        .cloned()
}

fn click(h: &mut Harness<'static, AppModel>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

/// Focuses the button and presses Enter, then Space (FC5: a button that writes to disk answers a pointer click only).
fn press_keys_on(h: &mut Harness<'static, AppModel>, label: &str) {
    for key in [egui::Key::Enter, egui::Key::Space] {
        h.get_by_role_and_label(Role::Button, label).focus();
        h.run();
        h.key_press(key);
        h.run();
    }
}

/// Clears a test file's read-only flag on drop, so a failed assert does not leave it set.
#[cfg(windows)]
struct ReadOnly(PathBuf);

#[cfg(windows)]
impl ReadOnly {
    fn set(&self, on: bool) {
        let mut p = std::fs::metadata(&self.0).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(on);
        std::fs::set_permissions(&self.0, p).unwrap();
    }
}

#[cfg(windows)]
impl Drop for ReadOnly {
    fn drop(&mut self) {
        if let Ok(m) = std::fs::metadata(&self.0) {
            let mut p = m.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(false);
            let _ = std::fs::set_permissions(&self.0, p);
        }
    }
}

/// Item 27 (Review Focus 3): a refused save leaves a row; drawing it changes nothing, its button clears the flag.
#[cfg(windows)]
#[test]
fn clear_read_only_writes_only_on_click() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    m.save().unwrap();
    let file = m.root.clone().unwrap().join("sdc.toml");
    let guard = ReadOnly(file.clone());
    let flagged = || std::fs::metadata(&file).unwrap().permissions().readonly();
    guard.set(true);
    m.set_field(
        "landscape/rock01.png",
        "alpha.policy",
        json!(engine::config::AlphaPolicy::Remove),
    );
    idle(&mut m);
    assert!(m.save().is_err());
    assert_eq!(
        m.dialog, None,
        "the refusal is a row, not a dead-end dialog"
    );
    let r = row(&m, |f| matches!(f, Fix::ClearReadOnly(_))).expect("a read-only row");
    assert_eq!((r.level, &r.at), (Level::Warning, &At::Project));
    assert_eq!(r.fix, Some(Fix::ClearReadOnly(file.clone())));
    let mut h = harness(m);
    h.run();
    h.run();
    assert!(flagged(), "drawing the row writes nothing");
    press_keys_on(&mut h, "Clear read-only flag");
    assert!(
        flagged(),
        "Enter and Space on the focused button write nothing"
    );
    click(&mut h, "Clear read-only flag");
    assert!(!flagged(), "the click cleared the flag");
    drop(guard);
    assert!(
        h.state().is_dirty(),
        "the unsaved edit is kept, not reloaded away"
    );
    assert!(h.state_mut().save().is_ok());
    assert!(row(h.state(), |f| matches!(f, Fix::ClearReadOnly(_))).is_none());
}

/// P8: a bad `sdc-project.toml` is a red row naming the line, with `Open in editor` (the OS opener) and `Reload`.
#[test]
fn a_bad_project_file_is_a_row_with_open_in_editor_and_reload() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let file = m.root.clone().unwrap().join("sdc-project.toml");
    std::fs::write(&file, "[[sets]\nid = ").unwrap();
    m.reload_documents();
    idle(&mut m);
    let r = row(&m, |f| matches!(f, Fix::OpenInEditor(_))).expect("a P8 row");
    assert_eq!((r.level, &r.at), (Level::Blocks, &At::Project));
    assert!(
        r.text
            .starts_with("sdc-project.toml cannot be read: TOML parse error at line 1"),
        "{}",
        r.text
    );
    let mut h = harness(m);
    h.run();
    assert!(h.state().shell.is_empty());
    click(&mut h, "Open in editor");
    assert_eq!(h.state().shell, vec![ShellRequest::Open(file.clone())]);
    std::fs::write(&file, "").unwrap();
    // The banner keeps its own Reload; the row's is named for its file (rule 8.4).
    click(&mut h, "Reload sdc-project.toml");
    idle(h.state_mut());
    assert!(row(h.state(), |f| matches!(f, Fix::OpenInEditor(_))).is_none());
    assert!(h.state().sets_read_only_reason().is_none());
}

fn png(path: &Path, px: [u8; 4]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ImageBuffer::from_pixel(16, 16, Rgba(px))
        .save(path)
        .unwrap();
}

/// `also_export.rs`'s project: a PBR set and a plain set under `<tmp>/mods/Mod`, exports to `<tmp>/out`.
fn also_project(d: &Path) -> AppModel {
    let t = d.join("mods/Mod/textures");
    png(&t.join("pbr/arm/steel.png"), [200, 200, 200, 255]);
    png(&t.join("pbr/arm/steel_n.png"), [128, 128, 255, 255]);
    png(&t.join("pbr/arm/steel_rmaos.png"), [128, 255, 200, 255]);
    let c = t.join("clutter");
    std::fs::create_dir_all(&c).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| {
        Rgb([(x * 16) as u8, (y * 16) as u8, ((x + y) * 8) as u8])
    })
    .save(c.join("cup.png"))
    .unwrap();
    png(&c.join("cup_n.png"), [128, 128, 255, 255]);
    opened_as_the_app(d.join("mods/Mod"), d.join("out"))
}

/// CN43: an unreadable `also.json` is a row; `Reset also.json` deletes only the record under the PBR folder's `_sdc`,
/// and the next export writes a new record.
#[test]
fn reset_also_json_only_under_sdc_and_writes_a_new_record() {
    let d = tempfile::tempdir().unwrap();
    let mut m = also_project(d.path());
    m.set_also_tick(engine::presets::AlsoTarget::Vanilla, true);
    idle(&mut m);
    export_through(&mut m);
    idle(&mut m);
    let rec = m.also_folders()[0].join("_sdc").join("also.json");
    assert!(rec.is_file(), "the first export wrote the record");
    assert!(row(&m, |f| matches!(f, Fix::ResetAlsoJson(_))).is_none());
    std::fs::write(&rec, "{").unwrap();
    export_through(&mut m);
    idle(&mut m);
    let r = row(&m, |f| matches!(f, Fix::ResetAlsoJson(_))).expect("an also.json row");
    assert_eq!(r.fix, Some(Fix::ResetAlsoJson(rec.clone())));
    assert!(
        r.shown().starts_with("also.json cannot be read"),
        "{}",
        r.shown()
    );
    // A path that is not this project's record is refused and left alone.
    let other = d.path().join("also.json");
    std::fs::write(&other, "{").unwrap();
    assert!(m.reset_also_json(&other).is_err());
    assert!(other.is_file());
    let mut h = harness(m);
    h.run();
    h.run();
    assert!(rec.is_file(), "drawing the row deletes nothing");
    click(&mut h, "Reset also.json");
    assert!(!rec.exists(), "the click deleted the record");
    assert!(row(h.state(), |f| matches!(f, Fix::ResetAlsoJson(_))).is_none());
    let m = h.state_mut();
    export_through(m);
    idle(m);
    let text = std::fs::read_to_string(&rec).expect("the next export wrote a record");
    serde_json::from_str::<serde_json::Value>(&text).expect("a readable record");
    assert!(row(m, |f| matches!(f, Fix::ResetAlsoJson(_))).is_none());
}

/// A 1:2 complex grass image (design §5): top half a cut-out diffuse, bottom half flat unit normals.
fn grass_png(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(32, 64, |x, y| {
        if y < 32 {
            Rgba([60u8, 140, 40, if x % 4 == 0 { 0 } else { 255 }])
        } else {
            Rgba([128u8, 128, 255, 200])
        }
    })
    .save(path)
    .unwrap();
}

/// X10/X3: a grass image in a folder with no `textures` ancestor is never auto-tagged; its row's `Tag as grass`
/// writes `subtype = "complex_grass"`, and the sub-type picker offers it there.
#[test]
fn tag_as_grass_writes_the_subtype() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("grassmod");
    grass_png(&root.join("plants").join("tundragrass.png"));
    let m = opened_as_the_app(root, d.path().join("out"));
    let key = "plants/tundragrass.png";
    let job = m.item(key).and_then(|i| i.job.clone()).expect("planned");
    assert!(engine::policy::grass_context(&job), "no textures ancestor");
    let r = row(&m, |f| matches!(f, Fix::TagAsGrass(_))).expect("a grass row");
    assert_eq!(r.fix, Some(Fix::TagAsGrass(key.into())));
    assert_eq!(r.level, Level::Info);
    let mut h = harness(m);
    h.run();
    let read = |m: &AppModel| {
        m.project
            .as_ref()
            .and_then(|p| p.file_value(key, "subtype"))
    };
    // Note FC1 recorded the plan's own pick (`diffuse`); drawing the row changes nothing.
    let before = read(h.state());
    assert_ne!(before, Some(json!("complex_grass")));
    h.run();
    assert_eq!(read(h.state()), before, "drawing the row writes nothing");
    click(&mut h, "Tag as grass");
    assert_eq!(read(h.state()), Some(json!("complex_grass")));
    let m = h.state_mut();
    idle(m);
    assert!(row(m, |f| matches!(f, Fix::TagAsGrass(_))).is_none());
    assert_eq!(
        m.item(key)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype),
        Some(engine::config::Subtype::ComplexGrass)
    );
}

/// X3 everywhere (Note FC10): the sub-type combos (the Undecided box's, Review's, the confirm box's) offer
/// `complex_grass` exactly where the type picker does: a grass-shader set, or a file with no `textures` ancestor;
/// never a plain diffuse under `textures/`.
#[test]
fn every_subtype_combo_offers_complex_grass_only_in_a_grass_context() {
    use engine::config::{Subtype, Target};
    use engine::shader::{Shader, ShaderOrigin, ShaderPick};
    use sdc_gui::ui::settings_panel::offered_groups;
    let lists = |job: &engine::policy::ResolvedJob| {
        offered_groups(Target::Cs, engine::policy::grass_context(job))
            .iter()
            .any(|(_, m)| m.contains(&Subtype::ComplexGrass))
    };
    let d = tempfile::tempdir().unwrap();
    let m = opened(d.path());
    let plain = m
        .items
        .iter()
        .find_map(|i| {
            i.job
                .clone()
                .filter(|j| j.game_path.starts_with("textures/"))
        })
        .expect("a texture under textures/");
    assert!(!lists(&plain), "a plain texture under textures/");
    let mut grass_set = plain.clone();
    grass_set.shader = Some(ShaderPick {
        shader: Shader::ComplexGrass,
        provenance: ShaderOrigin::Set,
        wanted: None,
    });
    assert!(lists(&grass_set), "a complex grass shader set");
    let e = tempfile::tempdir().unwrap();
    let root = e.path().join("grassmod");
    grass_png(&root.join("plants").join("tundragrass.png"));
    let m = opened_as_the_app(root, e.path().join("out"));
    let job = m
        .item("plants/tundragrass.png")
        .and_then(|i| i.job.clone())
        .expect("planned");
    assert!(lists(&job), "no textures ancestor");
}

/// An OpenEXR with partial alpha: the colour is divided by alpha on load (BQ15).
fn exr_with_alpha(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::Rgba32FImage::from_fn(16, 16, |x, _| {
        image::Rgba([0.4, 0.3, 0.2, if x < 8 { 0.5 } else { 1.0 }])
    })
    .save(path)
    .unwrap();
}

/// BQ15: a divided EXR alpha is an Info row; `Straight alpha` writes `alpha.exr_straight = true`, and the settings
/// panel's Alpha group has the same row.
#[test]
fn straight_alpha_writes_the_key() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("exrmod");
    exr_with_alpha(&root.join("textures").join("sky").join("glow.exr"));
    let m = opened_as_the_app(root, d.path().join("out"));
    let key = "textures/sky/glow.exr";
    let r = row(&m, |f| matches!(f, Fix::StraightAlpha(_))).expect("an EXR alpha row");
    assert_eq!(r.fix, Some(Fix::StraightAlpha(key.into())));
    assert_eq!(r.level, Level::Info);
    let mut h = harness(m);
    h.run();
    let read = |m: &AppModel| {
        m.project
            .as_ref()
            .and_then(|p| p.file_value(key, "alpha.exr_straight"))
    };
    assert_eq!(read(h.state()), None, "drawing the row writes nothing");
    click(&mut h, "Straight alpha");
    assert_eq!(read(h.state()), Some(json!(true)));
    idle(h.state_mut());
    assert!(row(h.state(), |f| matches!(f, Fix::StraightAlpha(_))).is_none());
    // The texture's settings show the field in its Alpha group (only for an EXR source).
    h.state_mut().select_item(key);
    h.run();
    h.run();
    assert!(h.query_all_by_label("Straight alpha").count() >= 1);
}

/// CV8: a stored `matchN = ""` (N >= 2) matches nothing; its row's `Remove match` removes the key.
#[test]
fn remove_match_clears_the_key() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let set = "landscape/rock01";
    m.set_json_value(set, "match3", Some(json!("")));
    idle(&mut m);
    let own = |m: &AppModel| {
        sdc_gui::model::sets::views(m)
            .iter()
            .find(|v| v.id == set)
            .and_then(|v| v.settings.get("match3").cloned())
    };
    assert_eq!(own(&m), Some(json!("")));
    let r = row(&m, |f| matches!(f, Fix::RemoveMatch(..))).expect("an empty match row");
    assert_eq!(r.fix, Some(Fix::RemoveMatch(set.into(), 3)));
    let mut h = harness(m);
    h.run();
    assert_eq!(
        own(h.state()),
        Some(json!("")),
        "drawing the row writes nothing"
    );
    click(&mut h, "Remove match");
    assert_eq!(own(h.state()), None);
    assert!(row(h.state(), |f| matches!(f, Fix::RemoveMatch(..))).is_none());
}

/// T8: a copy of a preset that keeps the original's suffix aliases is skipped (CFG-PRESET-SKIPPED); the row's
/// `Remove aliases from {id}` drops the copy's `suffix_aliases` table, keeping its comments, and the copy loads.
#[test]
fn an_alias_clash_row_removes_the_copys_aliases() {
    let d = tempfile::tempdir().unwrap();
    let presets: PathBuf = d.path().join("user-presets");
    std::fs::create_dir_all(&presets).unwrap();
    let text = |id: &str| {
        format!(
            "# my preset\nschema_version = 1\nid = \"{id}\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\n\
             [suffix_aliases]\nzz9 = \"n\"\n"
        )
    };
    std::fs::write(presets.join("a.toml"), text("mine")).unwrap();
    let copy = presets.join("b.toml");
    std::fs::write(&copy, text("mine-export")).unwrap();
    let mut m = opened(d.path());
    m.user_presets = Some(presets.clone());
    m.replan();
    idle(&mut m);
    let r = row(&m, |f| matches!(f, Fix::RemoveAliases(..))).expect("an alias clash row");
    assert_eq!(r.text, "Preset mine-export clashes with mine on aliases");
    let mut h = harness(m);
    h.run();
    assert!(std::fs::read_to_string(&copy).unwrap().contains("zz9"));
    press_keys_on(&mut h, "Remove aliases from mine-export");
    assert!(
        std::fs::read_to_string(&copy).unwrap().contains("zz9"),
        "Enter and Space on the focused button write nothing"
    );
    // Item 6: refused while an export runs.
    h.state_mut().export.running = Some(Default::default());
    assert!(h.state_mut().remove_preset_aliases(&copy).is_err());
    assert!(std::fs::read_to_string(&copy).unwrap().contains("zz9"));
    h.state_mut().export.running = None;
    click(&mut h, "Remove aliases from mine-export");
    let after = std::fs::read_to_string(&copy).unwrap();
    assert!(
        !after.contains("suffix_aliases") && !after.contains("zz9"),
        "{after}"
    );
    assert!(after.starts_with("# my preset\n"), "{after}");
    let m = h.state_mut();
    idle(m);
    assert!(m
        .loaded_user_presets
        .iter()
        .any(|(id, _)| id == "mine-export"));
    assert!(row(m, |f| matches!(f, Fix::RemoveAliases(..))).is_none());
}
