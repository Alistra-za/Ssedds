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
fn a_swizzle_remap_is_its_alias_branchs_channels() {
    // Note AP6: a remap is `channels.swizzle` in the alias's branch rule.
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.add_alias("dmask", "m").unwrap();
    pm.set_branch_field(Subtype::Envmask, "dmask", "channels.swizzle", json!("bgr1"));
    let t: toml::Table = toml::from_str(&pm.toml_text()).unwrap();
    assert_eq!(t["suffix_aliases"]["dmask"].as_str(), Some("m"));
    let rule = &t["rules"].as_array().unwrap()[0];
    assert_eq!(rule["match"]["glob"].as_str(), Some("*_dmask.*"));
    assert_eq!(rule["match"]["subtypes"][0].as_str(), Some("envmask"));
    assert_eq!(
        rule["overrides"]["channels"]["swizzle"].as_str(),
        Some("bgr1")
    );
    assert_eq!(pm.status(), &Ok(()));
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
    pm.add_alias("dmask", "m").unwrap();
    pm.set_branch_field(Subtype::Envmask, "dmask", "channels.swizzle", json!("bgr1"));
    pm.save().unwrap();
    // What the window does after a save.
    m.replan();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(swizzle(&m).as_deref(), Some("bgr1"));
}

#[test]
fn a_saved_preset_is_offered_in_the_header_and_picking_it_plans_with_it() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    std::fs::create_dir_all(root.join("a")).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgb([x as u8, 10, 200]))
        .save(root.join("a/box.png"))
        .unwrap();
    let mut m = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings {
            export_folder: Some(d.path().join("export")),
            ..Default::default()
        },
        None,
        std::sync::Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap()),
    );
    m.user_presets = Some(d.path().join("presets"));
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(m.batch_preset, "cs-quality");
    let pm = m.presets_model();
    pm.select("cs-compact");
    pm.duplicate();
    pm.set_id("mine");
    pm.save().unwrap();
    let choices = m.preset_choices();
    assert!(choices.contains(&"mine".to_string()), "{choices:?}");
    // Only presets of this target.
    assert!(!choices.contains(&"vanilla-quality".to_string()));
    m.set_preset(Some("mine".into()));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(m.project_preset().as_deref(), Some("mine"));
    assert_eq!(m.batch_preset, "mine");
    m.set_preset(None);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(m.project_preset(), None);
}

#[test]
fn duplicate_drops_the_remap_rules_of_the_aliases_it_drops() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.add_alias("dmask", "m").unwrap();
    pm.set_branch_field(Subtype::Envmask, "dmask", "channels.swizzle", json!("bgr1"));
    pm.save().unwrap();
    pm.duplicate();
    let text = pm.toml_text();
    assert!(!text.contains("dmask"), "{text}");
    assert!(!text.contains("rules"), "{text}");
    assert_eq!(pm.status(), &Ok(()));
}

#[test]
fn a_save_keeps_the_files_comments_and_order_and_writes_atomically() {
    let d = tempfile::tempdir().unwrap();
    let text = "# My preset, hand-written.\nschema_version = 1\nid = \"hand\"\nversion = \"1.0.0\"\nbase = \"cs-quality\" # the default\n\n# Normals at half size.\n[subtypes.normal]\nresize = \"half\" # small\n\n[subtypes.diffuse]\nresize = \"half\"\n";
    let path = d.path().join("handmade.toml");
    std::fs::write(&path, text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    pm.select("hand");
    assert!(!pm.dirty());
    assert_eq!(
        pm.toml_text(),
        text,
        "an unchanged preset previews as its file"
    );
    pm.set_field(Subtype::Normal, "mips.levels", json!(5));
    pm.set_field(Subtype::Envmask, "resize", json!("half"));
    pm.clear_field(Subtype::Diffuse, "resize");
    assert_eq!(pm.save().unwrap(), path);
    let out = std::fs::read_to_string(&path).unwrap();
    for keep in [
        "# My preset, hand-written.",
        "base = \"cs-quality\" # the default",
        "# Normals at half size.",
        "resize = \"half\" # small",
    ] {
        assert!(out.contains(keep), "{keep} lost:\n{out}");
    }
    assert!(out.find("schema_version").unwrap() < out.find("id =").unwrap());
    assert!(!out.contains("subtypes.diffuse"), "{out}");
    let t: toml::Table = toml::from_str(&out).unwrap();
    assert_eq!(t["subtypes"]["envmask"]["resize"].as_str(), Some("half"));
    assert_eq!(
        t["subtypes"]["normal"]["mips"]["levels"].as_integer(),
        Some(5)
    );
    assert_eq!(
        files(d.path()),
        ["handmade.toml"],
        "no temporary file is left"
    );
}

#[test]
fn a_deleted_preset_named_in_sdc_toml_falls_back_with_a_banner() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    std::fs::create_dir_all(root.join("a")).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgb([x as u8, 10, 200]))
        .save(root.join("a/box.png"))
        .unwrap();
    let mut m = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings {
            export_folder: Some(d.path().join("export")),
            ..Default::default()
        },
        None,
        std::sync::Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap()),
    );
    m.user_presets = Some(d.path().join("presets"));
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let pm = m.presets_model();
    pm.select("cs-quality");
    pm.duplicate();
    pm.set_id("gone");
    pm.save().unwrap();
    m.set_preset(Some("gone".into()));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(m.batch_preset, "gone");
    assert_eq!(m.banner, None);
    m.presets_model().delete().unwrap();
    m.replan();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    // The plan runs on the fallback instead of failing, and says so.
    assert!(m.dialog.is_none(), "no error dialog");
    assert_eq!(m.batch_preset, "cs-quality");
    assert_eq!(
        m.banner.as_deref(),
        Some("preset gone not found: using cs-quality")
    );
    // sdc.toml keeps the name (the file may come back); picking another clears the banner.
    assert_eq!(m.project_preset().as_deref(), Some("gone"));
    m.set_preset(None);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(m.banner, None);
}

#[test]
fn a_startup_banner_survives_a_replan_until_dismissed() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("textures");
    std::fs::create_dir_all(root.join("a")).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgb([x as u8, 10, 200]))
        .save(root.join("a/box.png"))
        .unwrap();
    let mut m = sdc_gui::model::AppModel::new(
        sdc_gui::settings::GuiSettings {
            export_folder: Some(d.path().join("export")),
            ..Default::default()
        },
        None,
        std::sync::Arc::new(engine::session::Session::new(encoders_sys::Bc1Model::Ideal).unwrap()),
    );
    m.keep_banner("Settings were not read, defaults are used: bad".into());
    m.open_folder(root);
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.replan();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert_eq!(
        m.banner.as_deref(),
        Some("Settings were not read, defaults are used: bad")
    );
    // It sits beside the banners refresh_banner builds.
    m.set_preset(Some("ghost".into()));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let b = m.banner.clone().unwrap();
    assert!(
        b.starts_with("Settings were not read") && b.contains("preset ghost not found"),
        "{b}"
    );
    m.dismiss_kept_banner();
    assert_eq!(
        m.banner.as_deref(),
        Some("preset ghost not found: using cs-quality")
    );
}

#[test]
fn rows_are_grouped_pbr_cs_then_vanilla_each_sub_type_once() {
    // Note AP9: the family comes from the engine's availability per target; the CPM family folded into Vanilla
    // (Note AZ).
    use sdc_gui::model::presets::{family_of, layout, FAMILIES};
    let names: Vec<&str> = FAMILIES.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["PBR / Community Shaders", "Vanilla"]);
    let l = layout();
    let order: Vec<&str> = l.iter().map(|(f, _, _)| *f).collect();
    let mut dedup = order.clone();
    dedup.dedup();
    assert_eq!(dedup, names, "families in order, each once");
    let all: Vec<Subtype> = l.iter().flat_map(|(_, _, s)| s.clone()).collect();
    assert_eq!(all.len(), Subtype::ALL.len());
    for st in Subtype::ALL {
        assert_eq!(all.iter().filter(|s| **s == st).count(), 1, "{st:?}");
    }
    assert!(
        l.iter().all(|(_, _, s)| !s.is_empty()),
        "no empty sub-group"
    );
    assert_eq!(family_of(Subtype::PbrEmissive), "PBR / Community Shaders");
    assert_eq!(family_of(Subtype::Glow), "Vanilla");
    assert_eq!(family_of(Subtype::ComplexMaterial), "Vanilla");
    assert_eq!(family_of(Subtype::Diffuse), "Vanilla");
    // By hand (AP9): a Community Shaders feature, though every target has it.
    assert_eq!(family_of(Subtype::ComplexGrass), "PBR / Community Shaders");
}

/// Audit 2026-10-15 item 4: Save does not write over a preset file edited by hand since it was picked.
#[test]
fn save_refuses_a_preset_file_changed_on_disk_until_overwrite() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("hand.toml");
    let text = "schema_version = 1\nid = \"hand\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n";
    std::fs::write(&path, text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    pm.select("hand");
    pm.set_field(Subtype::Normal, "resize", json!("half"));
    let edited = format!("{text}# a hand edit\n");
    std::fs::write(&path, &edited).unwrap();
    assert!(pm.save().is_err());
    assert!(pm.changed_on_disk);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    pm.save_overwrite().unwrap();
    assert!(!pm.changed_on_disk);
    assert!(std::fs::read_to_string(&path).unwrap().contains("half"));
}

#[test]
fn reload_after_a_disk_change_drops_the_draft() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("hand.toml");
    let text = "schema_version = 1\nid = \"hand\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n";
    std::fs::write(&path, text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    pm.select("hand");
    pm.set_field(Subtype::Normal, "resize", json!("half"));
    std::fs::write(&path, format!("{text}# a hand edit\n")).unwrap();
    assert!(pm.save().is_err());
    pm.reload_from_disk();
    assert!(!pm.dirty());
    assert!(!pm.changed_on_disk);
    assert!(pm.toml_text().contains("# a hand edit"));
}

/// Review minor 3: Reload of a preset whose file was deleted keeps the edits, says so, and Overwrite writes it back.
#[test]
fn reload_of_a_deleted_preset_file_keeps_the_edits() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("handmade.toml");
    let text = "schema_version = 1\nid = \"hand\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n";
    std::fs::write(&path, text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    pm.select("hand");
    pm.set_field(Subtype::Normal, "resize", json!("half"));
    std::fs::remove_file(&path).unwrap();
    assert!(pm.save().is_err());
    pm.reload_from_disk();
    assert!(pm.dirty(), "the edits are kept");
    assert!(pm.changed_on_disk);
    let e = pm.error.clone().unwrap();
    assert!(e.ends_with("was deleted: kept your edits"), "{e}");
    pm.save_overwrite().unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("half"));
}

#[test]
fn reload_of_a_preset_file_that_no_longer_loads_keeps_the_edits() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("hand.toml");
    let text = "schema_version = 1\nid = \"hand\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n";
    std::fs::write(&path, text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    pm.select("hand");
    pm.set_field(Subtype::Normal, "resize", json!("half"));
    std::fs::write(&path, "id = [broken").unwrap();
    pm.reload_from_disk();
    assert!(pm.dirty());
    let e = pm.error.clone().unwrap();
    assert!(e.ends_with("no longer loads: kept your edits"), "{e}");
}

/// Trace-debug 2026-10-16 #7: a preset edited on disk after the last rescan and then picked shows the edit, and a
/// save keeps it instead of writing the old value back.
#[test]
fn select_after_outside_edit_does_not_revert_it() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("hand.toml");
    let text = "schema_version = 1\nid = \"hand\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n";
    std::fs::write(&path, text).unwrap();
    let mut pm = PresetsModel::new(Some(d.path().to_path_buf()), Target::Cs);
    std::fs::write(&path, text.replace("1.0.0", "2.0.0")).unwrap();
    pm.select("hand");
    pm.set_field(Subtype::Normal, "resize", json!("half"));
    pm.save().unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("2.0.0"), "{saved}");
    assert!(saved.contains("half"), "{saved}");
}

/// Trace-debug 2026-10-16 #46: saving a preset under a new id is refused while another user preset builds on the
/// old id (that preset would be skipped on the next load).
#[test]
fn renaming_a_preset_others_build_on_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let mut pm = copy_of_quality(d.path());
    pm.set_id("a");
    pm.save().unwrap();
    pm.new_preset(Target::Cs);
    pm.set_id("b");
    pm.set_base("a");
    pm.save().unwrap();
    pm.select("a");
    pm.set_id("a2");
    let e = pm.status().clone().unwrap_err();
    assert!(e.contains("'b'"), "{e}");
    assert!(pm.save().is_err());
    let mut names = files(d.path());
    names.sort();
    assert_eq!(names, ["a.toml", "b.toml"]);
}
