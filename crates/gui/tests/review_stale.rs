//! Note FC11 (theme 11): Review lists the stale outputs Tessalith wrote, each with a Delete that runs on its own click
//! only; Generate's provenance and the JSON hashes are recorded at write time in `<project root>/_sdc/generated.json`.
mod support;
use eframe::egui::{self, accesskit::Role};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::shader::Shader;
use sdc_gui::model::from_normal::GenMap;
use sdc_gui::model::review::{DiskState, JsonKind, STALE_NOT_OURS};
use sdc_gui::model::AppModel;
use std::path::Path;
use std::time::Duration;
use support::*;

const WAIT: Duration = Duration::from_secs(120);
const ROCK_SET: &str = "landscape/rock01";

fn idle(m: &mut AppModel) {
    assert!(m.wait_idle(WAIT), "the model did not settle");
}

fn harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn stale_names(m: &AppModel) -> Vec<(String, bool)> {
    let mut v: Vec<_> = m
        .stale
        .iter()
        .map(|s| {
            let n = s.path.file_name().unwrap().to_string_lossy().into_owned();
            (n, s.ours)
        })
        .collect();
    v.sort();
    v
}

/// Review Focus 3 and 4: Delete is on a stamp-matched row only, never on Enter or Space, never on Export; a file
/// changed since the plan is refused at click time and its row reads "edited since…".
#[test]
fn review_delete_removes_only_on_click() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    export_through(&mut m);
    idle(&mut m);
    assert!(m.stale.is_empty(), "{:?}", m.stale);
    let land = d.path().join("textures/landscape");
    for n in ["rock01.png", "leaf.png"] {
        std::fs::remove_file(land.join(n)).unwrap();
    }
    m.replan();
    idle(&mut m);
    assert_eq!(
        stale_names(&m),
        [
            ("leaf.dds".to_string(), true),
            ("rock01.dds".to_string(), true)
        ]
    );
    let path = |n: &str| {
        m.stale
            .iter()
            .find(|s| s.path.ends_with(n))
            .unwrap()
            .path
            .clone()
    };
    let (rock, leaf) = (path("rock01.dds"), path("leaf.dds"));
    // Only a listed path: a planned output is refused.
    let planned = rock.with_file_name("rock01_n.dds");
    assert!(planned.is_file());
    assert!(m.delete_stale_output(&planned).is_err());
    assert!(planned.is_file());
    // leaf.dds is edited after the plan listed it: the click-time check refuses it and the row loses its button.
    let mut bytes = std::fs::read(&leaf).unwrap();
    bytes.extend_from_slice(b"edited");
    std::fs::write(&leaf, bytes).unwrap();
    assert!(m.delete_stale_output(&leaf).is_err());
    assert!(leaf.is_file());
    assert_eq!(
        stale_names(&m),
        [
            ("leaf.dds".to_string(), false),
            ("rock01.dds".to_string(), true)
        ]
    );
    m.open_review();
    let mut h = harness(m);
    h.run();
    h.run();
    assert!(h.query_all_by_label_contains("Stale outputs (2)").count() >= 1);
    assert!(h.query_all_by_label_contains(STALE_NOT_OURS).count() >= 1);
    // One button, named for its file (rule 8.4); the edited leaf.dds has none.
    let deletes = h
        .query_all_by_role(Role::Button)
        .filter(|n| {
            use egui_kittest::kittest::NodeT;
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with("Delete "))
        })
        .count();
    assert_eq!(deletes, 1);
    let name = format!("Delete {}", rock.display());
    assert!(rock.is_file(), "drawing the row deletes nothing");
    for key in [egui::Key::Enter, egui::Key::Space] {
        h.get_by_role_and_label(Role::Button, &name).focus();
        h.run();
        h.key_press(key);
        h.run();
        assert!(
            rock.is_file(),
            "{key:?} on the focused Delete deletes nothing"
        );
    }
    h.get_by_role_and_label(Role::Button, &name).click();
    h.run();
    assert!(!rock.exists(), "the click deleted it");
    assert!(leaf.is_file(), "the edited file is left alone");
    assert_eq!(stale_names(h.state()), [("leaf.dds".to_string(), false)]);
}

/// `pbr_opened_plain` with a relief normal, so Generate makes a height map; the shader pick is unsaved.
fn relief_rock(d: &Path) -> AppModel {
    let mut m = pbr_opened_plain(d);
    relief_normal(&d.join("textures/landscape"), "rock01_normal.png", 64);
    m.replan();
    idle(&mut m);
    m
}

/// CO39/CO51, superseded: Generate records its provenance at write time, so closing without Save keeps the
/// generated file "generated here".
#[test]
fn generate_then_close_without_save_is_still_ours() {
    let d = tempfile::tempdir().unwrap();
    let mut m = relief_rock(d.path());
    assert!(
        m.sets_doc.as_ref().unwrap().is_dirty(),
        "the unsaved shader pick"
    );
    m.open_gen(ROCK_SET, Some(GenMap::Height));
    m.gen_generate();
    idle(&mut m);
    assert!(d
        .path()
        .join("textures/landscape/rock01_p_generated.png")
        .is_file());
    let root = d.path().join("textures");
    // Close without Save: a new session opens the folder.
    drop(m);
    let mut m = opened_as_the_app(root, d.path().join("export"));
    m.set_shader(ROCK_SET, Some(Shader::Pbr));
    idle(&mut m);
    assert!(
        m.provenance("landscape/rock01_p_generated.png").is_some(),
        "the provenance came back from the sidecar"
    );
    assert_eq!(
        m.gen_offer(ROCK_SET, GenMap::Height),
        Ok(()),
        "generated here"
    );
}

/// BD2/P21, superseded: the JSON hash is recorded at write time, so discarding the unsaved edits after an export
/// does not make the next export ask about Tessalith's own JSON.
#[test]
fn json_hash_survives_a_discard() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    export_through(&mut m);
    // BD2: an edit made while the export runs leaves the document dirty, so the hash record waits for a Save.
    assert!(m.export.running.is_some());
    let order: Vec<String> = vec![ROCK_SET.into()];
    m.sets_doc.as_mut().unwrap().set_order(&order);
    assert!(m.sets_doc.as_ref().unwrap().is_dirty());
    idle(&mut m);
    let json = pgp_json_path(&m);
    assert!(json.is_file());
    // Discard: the documents are read again from disk; the unsaved hash record is gone from sdc-project.toml.
    m.reload_documents();
    idle(&mut m);
    let line = m
        .review_json()
        .into_iter()
        .find(|l| l.kind == JsonKind::PgPatcher)
        .expect("the PGPatcher JSON line");
    assert_eq!(line.state, DiskState::Ours, "the next export does not ask");
}
