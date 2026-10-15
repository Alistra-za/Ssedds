//! Note AP: per-suffix branches in the preset creator. Every test uses a temporary presets folder; the real %APPDATA%
//! is never read or written.
use engine::config::{Format, ResizeMode, Subtype, Target};
use sdc_gui::model::presets::PresetsModel;
use serde_json::json;
use std::path::Path;

fn copy_of_quality(dir: &Path) -> PresetsModel {
    let mut pm = PresetsModel::new(Some(dir.to_path_buf()), Target::Cs);
    pm.select("cs-quality");
    pm.duplicate();
    pm.set_id("branchy");
    pm
}

/// The `[format, tag]` pair of the first BC1 encoding `st` allows under CS.
fn bc1_of(st: Subtype) -> serde_json::Value {
    let (f, t) = engine::policy::allowed_encodings(st, Target::Cs)
        .into_iter()
        .find(|(f, _)| *f == Format::Bc1)
        .expect("a BC1 encoding");
    json!([f, t])
}

fn rules(pm: &PresetsModel) -> Vec<toml::Value> {
    let t: toml::Table = toml::from_str(&pm.toml_text()).unwrap();
    t.get("rules")
        .and_then(|r| r.as_array().cloned())
        .unwrap_or_default()
}

#[test]
fn a_built_in_suffix_branch_saves_one_suffix_rule() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.set_branch_field(Subtype::Diffuse, "d", "format", bc1_of(Subtype::Diffuse));
    pm.set_branch_field(Subtype::Diffuse, "d", "resize", json!("half"));
    assert_eq!(pm.status(), &Ok(()));
    let r = rules(&pm);
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0]["match"]["suffix"].as_str(), Some("d"));
    assert_eq!(r[0]["match"]["subtypes"][0].as_str(), Some("diffuse"));
    assert_eq!(r[0]["overrides"]["format"].as_str(), Some("bc1"));
    assert_eq!(r[0]["overrides"]["resize"].as_str(), Some("half"));
    // The sub-type's own table is untouched: the no-suffix default.
    assert_eq!(pm.field(Subtype::Diffuse, "format"), None);
    let row = pm.row(Subtype::Diffuse).unwrap();
    let b = row.branches.iter().find(|b| b.suffix == "d").unwrap();
    assert!(!b.alias);
    assert_eq!(b.overrides, 2, "format (with its tag) and resize");
    assert_eq!(b.profile.as_ref().unwrap().format, Format::Bc1);
    assert_eq!(row.profile.as_ref().unwrap().format, Format::Bc7);
}

#[test]
fn reset_on_a_branch_field_removes_it_and_an_emptied_branch_writes_no_rule() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    let before = pm.toml_text();
    pm.set_branch_field(Subtype::Diffuse, "d", "resize", json!("half"));
    pm.set_branch_field(Subtype::Diffuse, "d", "mips.levels", json!(5));
    pm.clear_branch_field(Subtype::Diffuse, "d", "resize");
    assert_eq!(pm.branch_field(Subtype::Diffuse, "d", "resize"), None);
    assert_eq!(
        pm.branch_field(Subtype::Diffuse, "d", "mips.levels"),
        Some(json!(5))
    );
    pm.clear_branch_field(Subtype::Diffuse, "d", "mips.levels");
    assert!(rules(&pm).is_empty());
    assert_eq!(pm.toml_text(), before);
}

#[test]
fn an_alias_branch_saves_a_glob_rule_after_the_suffix_rules_and_removing_the_alias_drops_it() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.set_branch_field(Subtype::Diffuse, "d", "resize", json!("half"));
    pm.add_alias("foo", "d").unwrap();
    let row = pm.row(Subtype::Diffuse).unwrap();
    let b = row.branches.iter().find(|b| b.suffix == "foo").unwrap();
    assert!(
        b.alias && b.overrides == 0,
        "a new alias opens an empty branch"
    );
    pm.set_branch_field(Subtype::Diffuse, "foo", "mips.levels", json!(4));
    assert_eq!(pm.status(), &Ok(()));
    let r = rules(&pm);
    // Rules layer in file order (AP10): the alias rule comes after the `_d` rule, so it adds to it.
    assert_eq!(r[0]["match"]["suffix"].as_str(), Some("d"));
    assert_eq!(r[1]["match"]["glob"].as_str(), Some("*_foo.*"));
    assert_eq!(r[1]["match"]["subtypes"][0].as_str(), Some("diffuse"));
    pm.remove_alias("foo");
    let r = rules(&pm);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0]["match"]["suffix"].as_str(), Some("d"));
}

#[test]
fn a_hand_written_rule_survives_a_save_and_is_listed_read_only() {
    let d = tempfile::tempdir().unwrap();
    let text = "schema_version = 1\nid = \"hand\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\n# Keep me.\n[[rules]]\nmatch = { glob = \"**/armor/**\" }\noverrides = { resize = \"half\" }\n";
    std::fs::write(d.path().join("hand.toml"), text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    pm.select("hand");
    assert_eq!(pm.hand_rules().len(), 1);
    assert!(pm.hand_rules()[0].contains("**/armor/**"));
    pm.set_branch_field(Subtype::Normal, "n", "resize", json!("half"));
    pm.save().unwrap();
    let r = rules(&pm);
    assert_eq!(r.len(), 2);
    // A branch goes after the hand-written rules, so it layers on top of them.
    assert_eq!(r[0]["match"]["glob"].as_str(), Some("**/armor/**"));
    assert_eq!(r[1]["match"]["suffix"].as_str(), Some("n"));
    assert_eq!(pm.hand_rules().len(), 1);
    // A rule that names several sub-types is never taken for a branch.
    pm.select("cs-compact");
    pm.duplicate();
    assert_eq!(pm.hand_rules().len(), rules(&pm).len());
    assert!(pm
        .row(Subtype::Diffuse)
        .unwrap()
        .branches
        .iter()
        .all(|b| b.overrides == 0));
}

#[test]
fn branches_round_trip_through_save_and_reload() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.add_alias("foo", "d").unwrap();
    pm.set_branch_field(Subtype::Diffuse, "foo", "resize", json!("half"));
    pm.set_branch_field(Subtype::Diffuse, "d", "format", bc1_of(Subtype::Diffuse));
    pm.add_alias("dmask", "m").unwrap();
    pm.set_branch_field(Subtype::Envmask, "dmask", "channels.swizzle", json!("bgr1"));
    let shown = |pm: &PresetsModel| -> Vec<(Subtype, String, usize, Option<Format>)> {
        pm.rows()
            .iter()
            .flat_map(|r| {
                r.branches.iter().map(move |b| {
                    (
                        r.subtype,
                        b.suffix.clone(),
                        b.overrides,
                        b.profile.as_ref().ok().map(|p| p.format),
                    )
                })
            })
            .collect()
    };
    let before = shown(&pm);
    pm.save().unwrap();
    let mut again = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    again.select("branchy");
    assert_eq!(shown(&again), before);
    assert_eq!(
        again.branch_field(Subtype::Envmask, "dmask", "channels.swizzle"),
        Some(json!("bgr1"))
    );
}

fn model_on(dir: &Path, files: &[&str]) -> sdc_gui::model::AppModel {
    let root = dir.join("textures");
    std::fs::create_dir_all(root.join("a")).unwrap();
    for f in files {
        image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgb([x as u8, 10, 200]))
            .save(root.join("a").join(f))
            .unwrap();
    }
    let mut m = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings {
            export_folder: Some(dir.join("export")),
            ..Default::default()
        },
        None,
        std::sync::Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap()),
    );
    m.user_presets = Some(dir.join("presets"));
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m
}

fn profile(m: &sdc_gui::model::AppModel, key: &str) -> engine::config::Profile {
    m.item(key)
        .and_then(|i| i.job.as_ref())
        .unwrap_or_else(|| panic!("{key} planned"))
        .profile
        .clone()
}

#[test]
fn a_plan_uses_the_branch_for_its_suffix_and_files_beats_it() {
    let d = tempfile::tempdir().unwrap();
    let mut m = model_on(d.path(), &["x_d.png", "x.png", "y_foo.png"]);
    assert_eq!(profile(&m, "a/x.png").subtype, Subtype::Diffuse);
    let pm = m.presets_model();
    pm.select("cs-quality");
    pm.duplicate();
    pm.set_id("branchy");
    pm.set_branch_field(Subtype::Diffuse, "d", "format", bc1_of(Subtype::Diffuse));
    pm.add_alias("foo", "d").unwrap();
    pm.set_branch_field(Subtype::Diffuse, "foo", "resize", json!("half"));
    pm.save().unwrap();
    m.set_preset(Some("branchy".into()));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(
        profile(&m, "a/x_d.png").format,
        Format::Bc1,
        "the _d branch"
    );
    assert_eq!(
        profile(&m, "a/x.png").format,
        Format::Bc7,
        "no suffix: the default"
    );
    let foo = profile(&m, "a/y_foo.png");
    assert_eq!(foo.subtype, Subtype::Diffuse);
    assert_eq!(foo.resize, ResizeMode::Half, "the _foo branch");
    assert_eq!(foo.format, Format::Bc1, "layered on the _d branch (AP10)");
    // An explicit [files] value beats the branch.
    m.set_field("a/x_d.png", "format", json!("bc7"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(profile(&m, "a/x_d.png").format, Format::Bc7);
}

#[test]
fn a_branch_layers_on_a_duplicated_compacts_bc1_rule() {
    // AP10: cs-compact's opaque-colour rule (format bc1, resize none) still applies to `_d` files; the `_d` branch
    // after it changes only the field it sets.
    let d = tempfile::tempdir().unwrap();
    let mut m = model_on(d.path(), &["x_d.png", "x.png"]);
    let pm = m.presets_model();
    pm.select("cs-compact");
    pm.duplicate();
    pm.set_id("compact-branchy");
    pm.set_branch_field(Subtype::Diffuse, "d", "resize", json!("half"));
    let t: toml::Table = toml::from_str(&pm.toml_text()).unwrap();
    let r = t["rules"].as_array().unwrap();
    assert_eq!(r.last().unwrap()["match"]["suffix"].as_str(), Some("d"));
    pm.save().unwrap();
    m.set_preset(Some("compact-branchy".into()));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let xd = profile(&m, "a/x_d.png");
    assert_eq!((xd.format, xd.resize), (Format::Bc1, ResizeMode::Half));
    let x = profile(&m, "a/x.png");
    assert_eq!((x.format, x.resize), (Format::Bc1, ResizeMode::None));
}

#[test]
fn an_alias_branch_shows_what_its_files_get_on_top_of_the_canonical_branch() {
    // AP11: the `_foo` header and hints show the `_d` branch underneath.
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.set_branch_field(Subtype::Diffuse, "d", "format", bc1_of(Subtype::Diffuse));
    pm.add_alias("foo", "d").unwrap();
    pm.set_branch_field(Subtype::Diffuse, "foo", "resize", json!("half"));
    let row = pm.row(Subtype::Diffuse).unwrap();
    let foo = row.branches.iter().find(|b| b.suffix == "foo").unwrap();
    let p = foo.profile.as_ref().unwrap();
    assert_eq!((p.format, p.resize), (Format::Bc1, ResizeMode::Half));
    let i = foo.inherited.as_ref().unwrap();
    assert_eq!((i.format, i.resize), (Format::Bc1, ResizeMode::None));
    assert!(!foo.by_alpha);
    // A rule on alpha content is not guessed: the header says so.
    pm.select("cs-compact");
    pm.duplicate();
    let row = pm.row(Subtype::Diffuse).unwrap();
    let b = row.branches.iter().find(|b| b.suffix == "d").unwrap();
    assert!(b.by_alpha);
    assert_eq!(b.profile.as_ref().unwrap().format, Format::Bc7);
}

#[test]
fn a_new_built_in_branch_goes_after_every_hand_written_rule() {
    let d = tempfile::tempdir().unwrap();
    let text = "schema_version = 1\nid = \"mixed\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n\n[suffix_aliases]\nfoo = \"d\"\n\n[[rules]]\nmatch = { glob = \"*_foo.*\", subtypes = [\"diffuse\"] }\noverrides = { resize = \"half\" }\n\n[[rules]]\nmatch = { glob = \"**/armor/**\" }\noverrides = { mips = { levels = 3 } }\n";
    std::fs::write(d.path().join("mixed.toml"), text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    pm.select("mixed");
    pm.set_branch_field(Subtype::Diffuse, "d", "mips.levels", json!(5));
    let r = rules(&pm);
    assert_eq!(r[0]["match"]["glob"].as_str(), Some("*_foo.*"));
    assert_eq!(r[1]["match"]["glob"].as_str(), Some("**/armor/**"));
    assert_eq!(r[2]["match"]["suffix"].as_str(), Some("d"));
}
