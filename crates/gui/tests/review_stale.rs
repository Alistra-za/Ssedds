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
    // The refused delete's status line repeats STALE_NOT_OURS after the full path, so the row is matched by its own
    // `rel · why` text (that status line was the second match the earlier `>= 1` hid).
    let row = format!("leaf.dds · {STALE_NOT_OURS}");
    for t in ["Stale outputs (2)", row.as_str()] {
        let all: Vec<String> = h
            .query_all_by_label_contains(t)
            .map(|n| {
                use egui_kittest::kittest::NodeT;
                let a = n.accesskit_node();
                format!("{:?} {:?} {:?}", a.role(), a.label(), a.value())
            })
            .collect();
        assert_eq!(all.len(), 1, "{t}: {all:?}");
    }
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

/// Findings r1 item 1: an `ours` stale row puts one Info row in Problems whose `Review…` opens Review on the
/// Stale outputs group; the row gates nothing and does not open Review by itself.
#[test]
fn a_stale_output_shows_a_problems_row_that_opens_review() {
    use sdc_gui::model::bottom::{problems, stale_row_text, Fix, Level};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    export_through(&mut m);
    idle(&mut m);
    assert!(!problems(&m).iter().any(|p| p.fix == Some(Fix::ReviewStale)));
    std::fs::remove_file(d.path().join("textures/landscape/leaf.png")).unwrap();
    m.replan();
    idle(&mut m);
    let rows: Vec<_> = problems(&m)
        .iter()
        .filter(|p| p.fix == Some(Fix::ReviewStale))
        .cloned()
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].level, Level::Info);
    assert_eq!(rows[0].text, stale_row_text(1));
    assert!(m.review.is_none(), "the row does not open Review by itself");
    m.open_problems(None);
    let mut h = harness(m);
    h.run_steps(4);
    h.get_by_role_and_label(Role::Button, "Review…").click();
    h.run_steps(4);
    assert!(h.state().review.is_some());
    assert!(h.query_all_by_label_contains("Stale outputs (1)").count() >= 1);
}

/// Findings r1 item 2 (AG90): a PGPatcher JSON the plan no longer makes is a stale row; its recorded hash is the
/// baseline, so an edited copy is left alone and Tessalith's own bytes get a Delete.
#[test]
fn a_pgpatcher_json_no_longer_made_is_listed_by_its_hash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    export_through(&mut m);
    idle(&mut m);
    let json = pgp_json_path(&m);
    let bytes = std::fs::read(&json).unwrap();
    assert!(m.stale.iter().all(|s| s.path != json), "{:?}", m.stale);
    // A renamed JSON: the old name is no longer made.
    m.set_json_name("renamed");
    idle(&mut m);
    assert_ne!(pgp_json_path(&m), json);
    std::fs::write(&json, b"{}").unwrap();
    m.replan();
    idle(&mut m);
    let row = |m: &AppModel| m.stale.iter().find(|s| s.path == json).cloned();
    assert_eq!(row(&m).map(|s| s.ours), Some(false), "{:?}", m.stale);
    assert!(m.delete_stale_output(&json).is_err());
    assert!(json.is_file());
    std::fs::write(&json, &bytes).unwrap();
    m.replan();
    idle(&mut m);
    assert_eq!(row(&m).map(|s| s.ours), Some(true), "{:?}", m.stale);
    m.delete_stale_output(&json).unwrap();
    assert!(!json.exists());
    assert!(row(&m).is_none());
}

/// Note FC14 (final review, minor; rule 9.2): the footer's lists share one scroll area of at most a third of the tab,
/// and only the rows on screen are laid out: 2000 stale rows draw a screenful.
#[test]
fn the_review_footer_lays_out_only_the_rows_on_screen() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    decide_all(&mut m);
    for i in 0..2000 {
        m.stale.push(engine::manifest::StaleOutput {
            path: d.path().join(format!("out/textures/old{i:04}.dds")),
            reason: engine::manifest::StaleWhy::SourceGone,
            ours: false,
        });
    }
    m.open_review();
    let mut h = harness(m);
    h.run();
    h.run();
    let drawn = h.query_all_by_label_contains(STALE_NOT_OURS).count();
    assert!((1..60).contains(&drawn), "{drawn} rows laid out");
}

/// Note FC14 (final review I2): while the PBR set's diffuse is undecided again, `prepare` makes no entry, yet the
/// JSON at the current name is still this project's: it is not listed (no Delete for the file PGPatcher reads).
#[test]
fn an_undecided_pbr_set_leaves_its_recorded_json_unlisted() {
    use sdc_gui::model::items::ItemStatus;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    export_through(&mut m);
    idle(&mut m);
    let json = pgp_json_path(&m);
    assert!(json.is_file());
    m.set_shader(ROCK_SET, None);
    idle(&mut m);
    m.project
        .as_mut()
        .unwrap()
        .clear_file_value("landscape/rock01.png", "subtype");
    m.replan();
    idle(&mut m);
    assert_eq!(
        m.item("landscape/rock01.png").map(|i| i.status.clone()),
        Some(ItemStatus::NeedsDecision)
    );
    assert!(sdc_gui::model::pbr_export::prepare(&m).is_none_or(|p| p.entries.is_empty()));
    assert!(m.stale.iter().all(|s| s.path != json), "{:?}", m.stale);
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
    idle(&mut m);
    let json = pgp_json_path(&m);
    assert!(json.is_file());
    // BD2: the hash record lived only in the unsaved document; strip it from sdc-project.toml, which is what a
    // discarded edit leaves on disk (deterministic: no race with the running export).
    let toml = d.path().join("textures/sdc-project.toml");
    let stripped: String = std::fs::read_to_string(&toml)
        .unwrap()
        .lines()
        .filter(|l| !l.contains("json_sha256"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&toml, stripped).unwrap();
    let state = |m: &mut AppModel| {
        m.reload_documents();
        idle(m);
        assert!(
            !m.sets_doc.as_ref().unwrap().is_dirty(),
            "reconciling the sidecar on open does not dirty the document"
        );
        m.review_json()
            .into_iter()
            .find(|l| l.kind == JsonKind::PgPatcher)
            .expect("the PGPatcher JSON line")
            .state
    };
    assert_eq!(
        state(&mut m),
        DiskState::Ours,
        "the next export does not ask"
    );
    // Control: the quiet reload wrote nothing back to the toml, so without the sidecar the record is gone and the
    // same JSON is no longer Tessalith's.
    assert!(!std::fs::read_to_string(&toml)
        .unwrap()
        .contains("json_sha256"));
    std::fs::remove_file(d.path().join("textures/_sdc/generated.json")).unwrap();
    assert_ne!(state(&mut m), DiskState::Ours);
}

/// Note FC11 (owner): the sidecar wins over a hand edit of the same key in sdc-project.toml. Reconciling on open is
/// quiet (no unsaved changes, Discard leaves a clean document); the next real Save writes the readable copy.
#[test]
fn the_sidecar_wins_and_reconcile_is_quiet() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened_plain(d.path());
    idle(&mut m);
    m.save().unwrap();
    idle(&mut m);
    let root = d.path().join("textures");
    let toml = root.join("sdc-project.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(
        &toml,
        format!("{text}\n[project.cs_pbr_sha256]\n\"a.json\" = \"hand\"\n"),
    )
    .unwrap();
    std::fs::create_dir_all(root.join("_sdc")).unwrap();
    std::fs::write(
        root.join("_sdc/generated.json"),
        r#"{"cs_pbr_sha256":{"a.json":"side"}}"#,
    )
    .unwrap();
    m.reload_documents();
    idle(&mut m);
    let doc = m.sets_doc.as_ref().unwrap();
    assert_eq!(
        doc.cs_pbr_shas().get("a.json").map(String::as_str),
        Some("side")
    );
    assert!(!doc.is_dirty(), "open reconciles quietly");
    m.reload_documents();
    idle(&mut m);
    assert!(
        !m.sets_doc.as_ref().unwrap().is_dirty(),
        "Discard leaves a clean document"
    );
    assert!(std::fs::read_to_string(&toml).unwrap().contains("hand"));
    // A real edit, then Save: the readable copy follows the sidecar.
    let order: Vec<String> = vec![ROCK_SET.into()];
    m.sets_doc.as_mut().unwrap().set_order(&order);
    assert!(m.sets_doc.as_ref().unwrap().is_dirty());
    m.save().unwrap();
    idle(&mut m);
    let saved = std::fs::read_to_string(&toml).unwrap();
    assert!(
        saved.contains("\"side\"") && !saved.contains("\"hand\""),
        "{saved}"
    );
}
