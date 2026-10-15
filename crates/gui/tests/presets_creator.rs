//! Note AM: the preset creator. Every test uses a temporary presets folder (or none); the real %APPDATA% is never
//! read or written.
use engine::config::{ResizeMode, Subtype, Target};
use engine::presets::PresetRegistry;
use sdc_gui::model::presets::PresetsModel;
use serde_json::json;
use std::path::Path;

fn files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn copy_of_quality(dir: &Path) -> PresetsModel {
    let mut pm = PresetsModel::new(Some(dir.to_path_buf()), Target::Cs);
    pm.select("cs-quality");
    assert!(
        pm.draft.as_ref().unwrap().read_only,
        "a built-in is read-only"
    );
    pm.duplicate();
    assert_eq!(pm.id(), "cs-quality-copy");
    assert!(!pm.draft.as_ref().unwrap().read_only);
    pm
}

#[test]
fn a_duplicated_builtin_edits_saves_and_reloads_equal() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.set_id("my-cs");
    pm.set_field(Subtype::Normal, "resize", json!("half"));
    pm.set_field(Subtype::Diffuse, "mips.levels", json!(5));
    assert_eq!(pm.status(), &Ok(()));
    let row = pm.row(Subtype::Normal).unwrap();
    assert_eq!(row.profile.as_ref().unwrap().resize, ResizeMode::Half);
    assert_eq!(row.base.as_ref().unwrap().resize, ResizeMode::None);
    let written = pm.draft.as_ref().unwrap().table.clone();
    let path = pm.save().expect("saved");
    assert_eq!(path, d.path().join("my-cs.toml"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        toml::to_string(&written).unwrap()
    );
    // Through the loader the presets folder takes, as a plan loads it.
    let mut r = PresetRegistry::builtin().unwrap();
    assert!(r.add_dir(d.path()).is_empty());
    assert_eq!(r.table("my-cs"), Some(&written));
    // The model rescanned: the preset is listed and shown, saved.
    assert!(pm.list().contains(&("my-cs".to_string(), false)));
    assert_eq!(pm.id(), "my-cs");
    assert!(!pm.dirty() && pm.deletable());
    // A second save of the same preset rewrites its own file.
    pm.set_field(Subtype::Normal, "resize", json!("pot-down"));
    assert_eq!(pm.save().unwrap(), path);
    assert_eq!(files(d.path()), ["my-cs.toml"]);
}

#[test]
fn an_invalid_alias_is_refused_with_its_message() {
    let d = tempfile::tempdir().unwrap();
    let pm = copy_of_quality(d.path());
    let err = |a: &str, c: &str| pm.check_alias(a, c).unwrap_err();
    assert!(
        err("n", "n").contains("is a canonical suffix"),
        "{}",
        err("n", "n")
    );
    assert!(err("my_n", "n").contains("lower-case ASCII letters and digits"));
    assert!(err("Bump", "n").contains("lower-case ASCII letters and digits"));
    assert!(err("orm", "rmaos").contains("Note AL"));
    assert!(pm.check_alias("bumpy", "n").is_ok());
}

#[test]
fn an_alias_declared_by_another_preset_conflicts() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.add_alias("bumpy", "n").unwrap();
    pm.save().unwrap();
    pm.new_preset(Target::Cs);
    pm.set_id("other");
    let e = pm.check_alias("bumpy", "n").unwrap_err();
    assert!(
        e.contains("already declared by preset 'cs-quality-copy'"),
        "{e}"
    );
    let e = pm.add_alias("bumpy", "n").unwrap_err();
    assert!(e.contains("already declared"), "{e}");
    assert!(!pm.toml_text().contains("bumpy"));
}

#[test]
fn the_format_list_follows_allowed_encodings() {
    let d = tempfile::tempdir().unwrap();
    let pm = copy_of_quality(d.path());
    for row in pm.rows() {
        let p = row.profile.as_ref().unwrap();
        let offered: Vec<_> = sdc_gui::ui::presets::field_options("format", Target::Cs, p)
            .into_iter()
            .map(|(v, _)| v)
            .collect();
        let allowed: Vec<_> = engine::policy::allowed_encodings(row.subtype, Target::Cs)
            .into_iter()
            .map(|(f, t)| json!([f, t]))
            .collect();
        assert_eq!(offered, allowed, "{:?}", row.subtype);
    }
}

#[test]
fn reset_drops_the_override_so_the_base_shows_through() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    let before = pm.toml_text();
    pm.set_field(Subtype::Envmask, "format", json!(["bc7", "unorm"]));
    assert_eq!(pm.field(Subtype::Envmask, "format"), Some(json!("bc7")));
    // The base's rgbcx cannot write BC7: the row shows the engine's follow-up, bc7e.
    let p = pm.row(Subtype::Envmask).unwrap().profile.clone().unwrap();
    assert_eq!(p.encoder.backend, engine::config::Backend::Bc7e);
    pm.clear_field(Subtype::Envmask, "format");
    assert_eq!(pm.field(Subtype::Envmask, "format"), None);
    assert_eq!(pm.field(Subtype::Envmask, "color.tag"), None);
    assert_eq!(pm.toml_text(), before);
    let row = pm.row(Subtype::Envmask).unwrap();
    assert_eq!(row.profile.as_ref().ok(), row.base.as_ref());
    pm.set_field(Subtype::Diffuse, "resize", json!("half"));
    pm.reset_all();
    assert_eq!(pm.toml_text(), before);
}

#[test]
fn a_swizzle_remap_writes_the_alias_and_the_rule() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.add_remap("dmask", "m", "bgr1").unwrap();
    let t: toml::Table = toml::from_str(&pm.toml_text()).unwrap();
    assert_eq!(t["suffix_aliases"]["dmask"].as_str(), Some("m"));
    let rule = &t["rules"].as_array().unwrap()[0];
    assert_eq!(rule["match"]["glob"].as_str(), Some("*_dmask.*"));
    assert_eq!(
        rule["overrides"]["channels"]["swizzle"].as_str(),
        Some("bgr1")
    );
    assert_eq!(pm.remaps(), [("dmask".into(), "m".into(), "bgr1".into())]);
    assert_eq!(pm.status(), &Ok(()));
    assert!(pm.add_remap("dmask2", "m", "rgbx").is_err());
    // Removing the alias removes its rule.
    pm.remove_alias("dmask");
    assert!(!pm.toml_text().contains("dmask"));
    assert!(!pm.toml_text().contains("rules"));
}

#[test]
fn an_invalid_preset_is_never_written() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    for (id, why) in [
        ("cs-quality", "already loaded"),
        ("sdc-base", "reserved"),
        ("My Preset", "lower-case letters"),
        ("", "Give the preset a name"),
    ] {
        pm.set_id(id);
        let e = pm.status().clone().unwrap_err();
        assert!(e.contains(why), "{id}: {e}");
        assert!(pm.save().unwrap_err().contains(why));
    }
    pm.set_id("fine");
    pm.set_base("no-such-preset");
    assert!(pm.status().is_err());
    assert!(pm.save().is_err());
    assert!(files(d.path()).is_empty(), "nothing was written");
}

#[test]
fn delete_removes_the_file_and_the_listing() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.save().unwrap();
    pm.delete().unwrap();
    assert!(files(d.path()).is_empty());
    assert!(!pm.list().iter().any(|(id, _)| id == "cs-quality-copy"));
    pm.select("cs-quality");
    assert!(!pm.deletable());
}

#[test]
fn without_a_presets_folder_nothing_is_written() {
    // AppModel::user_presets is None in tests: the creator then has no folder and never falls back on APPDATA.
    let mut pm = PresetsModel::new(None, Target::Cs);
    pm.new_preset(Target::Cs);
    pm.set_id("x");
    assert_eq!(pm.status(), &Ok(()));
    assert!(pm.save().unwrap_err().contains("No presets folder"));
}

#[test]
fn the_built_in_suffixes_come_from_the_engine() {
    use sdc_gui::model::presets::suffixes_of;
    assert_eq!(suffixes_of(Subtype::Rmaos, Target::Cs), ["rmaos"]);
    assert!(suffixes_of(Subtype::PbrNormal, Target::Cs).contains(&"n"));
    assert!(suffixes_of(Subtype::Envmask, Target::Cs).contains(&"m"));
    assert!(suffixes_of(Subtype::MsnSpecular, Target::Cs).contains(&"s"));
}

#[test]
fn a_saved_remap_reaches_the_plan_without_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let presets = d.path().join("presets");
    let root = d.path().join("textures");
    std::fs::create_dir_all(root.join("a")).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgb([x as u8, 10, 200]))
        .save(root.join("a/box_dmask.png"))
        .unwrap();
    let settings = sdc_gui::settings::GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..Default::default()
    };
    let mut m = sdc_gui::model::AppModel::new(
        settings,
        None,
        std::sync::Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap()),
    );
    m.user_presets = Some(presets.clone());
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let swizzle = |m: &sdc_gui::model::AppModel| {
        m.items
            .iter()
            .find(|i| i.name.contains("box_dmask"))
            .and_then(|i| i.job.as_ref())
            .and_then(|j| j.profile.channels.swizzle.clone())
    };
    assert_eq!(swizzle(&m), None);
    m.open_presets();
    let pm = m.presets_model();
    pm.select("cs-quality");
    pm.duplicate();
    pm.add_remap("dmask", "m", "bgr1").unwrap();
    pm.save().unwrap();
    // What the window does after a save.
    m.replan();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(swizzle(&m).as_deref(), Some("bgr1"));
}
