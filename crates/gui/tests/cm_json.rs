//! PGPatcher complex material JSON (complex material design 2026-10-14): the `[sets.cm]` block, the writer, the card
//! predicate, the export and the card's warnings.
mod support;
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;
use std::collections::BTreeMap;

fn doc(text: &str) -> (tempfile::TempDir, SetsDoc) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc-project.toml"), text).unwrap();
    let s = SetsDoc::open(d.path());
    (d, s)
}

const DOC: &str = "[[sets]]\nid = \"a/one\"\nhair = true\n\n[sets.cm]\n# mine\nexport = true\nglossiness = 80\n\
specular_color = [1, 0.85, 0.6]\nspecular_strength = \"high\"\nmyNote = \"keep\"\n\n[[sets]]\nid = \"b/two\"\nhair = true\n";

#[test]
fn a_cm_block_reads_back_with_unknown_and_wrong_typed_keys_and_never_reaches_the_settings() {
    let (_d, s) = doc(DOC);
    let e = s.entries();
    let b = e[0].cm.as_ref().expect("the block");
    assert!(b.export);
    assert_eq!(b.values["glossiness"], json!(80));
    assert_eq!(b.values["specular_color"], json!([1, 0.85, 0.6]));
    assert_eq!(b.bad, ["specular_strength"]);
    assert!(!b.values.contains_key("myNote"));
    // RESERVED: nothing of the block is a PGPatcher setting.
    assert_eq!(e[0].settings.keys().collect::<Vec<_>>(), ["hair"]);
    assert!(e[1].cm.is_none());
}

/// Review focus 1.
#[test]
fn a_wrong_typed_export_reads_as_off_and_is_listed() {
    let (_d, s) =
        doc("[[sets]]\nid = \"a/one\"\n\n[sets.cm]\nexport = \"yes\"\nspecular_color = [1, 1]\n");
    let b = s.entries()[0].cm.clone().unwrap();
    assert!(!b.export);
    assert_eq!(b.bad, ["export", "specular_color"]);
}

/// Edits keep comments and unknown keys, and a new `[sets.cm]` is printed under its own set.
#[test]
fn cm_edits_keep_the_file_and_land_in_their_own_set() {
    let (d, mut s) = doc(DOC);
    s.set_cm_value("b/two", "dynamic_cubemap", Some(json!(false)));
    s.set_cm_value("a/one", "glossiness", None);
    s.set_cm_export("a/one", false);
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    for keep in [
        "# mine",
        "myNote = \"keep\"",
        "specular_strength = \"high\"",
    ] {
        assert!(text.contains(keep), "{keep} in {text}");
    }
    let s = SetsDoc::open(d.path());
    let e = s.entries();
    let a = e[0].cm.clone().unwrap();
    assert!(!a.export, "unticked");
    assert!(a.values.contains_key("specular_color"), "values are kept");
    assert!(!a.values.contains_key("glossiness"));
    assert_eq!(
        e[1].cm.clone().unwrap().values["dynamic_cubemap"],
        json!(false)
    );
}

#[test]
fn removing_a_key_from_a_set_without_a_block_changes_nothing() {
    let (_d, mut s) = doc("[[sets]]\nid = \"b/two\"\nhair = true\n");
    s.set_cm_value("b/two", "glossiness", None);
    assert!(!s.is_dirty());
    assert!(s.entries()[0].cm.is_none());
}

/// Review focus 4.
#[test]
fn a_read_only_document_takes_no_cm_edit() {
    let (_d, mut s) = doc("[[sets]\nbroken");
    assert!(s.read_only());
    s.set_cm_value("a/one", "glossiness", Some(json!(1.0)));
    s.set_cm_export("a/one", true);
    s.set_cm_json_shas(&BTreeMap::from([("x.json".to_string(), "00".to_string())]));
    assert!(!s.is_dirty());
    assert!(s.cm_json_shas().is_empty());
}

/// Design §3: the hash record reads back, and an undo (restore) keeps the live record.
#[test]
fn the_hash_record_round_trips_and_survives_undo() {
    let (_d, mut s) = doc(DOC);
    let before = s.text();
    s.set_cm_value("a/one", "glossiness", Some(json!(50.0)));
    let shas = BTreeMap::from([("m/iron_m.json".to_string(), "ab12".to_string())]);
    s.set_cm_json_shas(&shas);
    assert_eq!(s.cm_json_shas(), shas);
    assert!(s.text().contains("cm_json_sha256"));
    s.restore_keeping_json_sha(&before);
    assert_eq!(s.cm_json_shas(), shas, "undo keeps the record");
    assert_eq!(
        s.entries()[0].cm.clone().unwrap().values["glossiness"],
        json!(80),
        "undo took the value back"
    );
    s.set_cm_json_shas(&BTreeMap::new());
    assert!(!s.text().contains("cm_json_sha256"));
}

/// Design §3 RESERVED leak: through the model, `cm` keys never reach the TruePBR JSON or `shows_json_panel`.
#[test]
fn model_edits_are_undoable_and_never_reach_the_pgpatcher_json() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::model::sets::{set_views, shows_json_panel};
    use sdc_gui::pgp::writer::{document, to_text};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    for id in ["landscape/rock01", "landscape/leaf"] {
        m.set_cm_export(id, true);
        m.set_cm_value(id, "glossiness", Some(json!(80.0)));
    }
    let entries: Vec<_> = json_entries(&m).into_iter().map(|(_, e)| e).collect();
    let text = to_text(&document(&entries));
    for leak in ["glossiness", "\"cm", "export"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    let leaf = set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/leaf")
        .unwrap();
    assert!(leaf.settings.is_empty());
    assert!(!shows_json_panel(&leaf, m.target));
    assert_eq!(
        m.cm_block("landscape/leaf").unwrap().values["glossiness"],
        json!(80.0)
    );
    m.undo();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        !m.cm_block("landscape/leaf")
            .unwrap()
            .values
            .contains_key("glossiness"),
        "one step undone"
    );
}

use engine::config::{Subtype, Target};
use engine::shader::Shader;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use sdc_gui::model::cm_json::*;
use sdc_gui::model::sets::{set_views, SetView};
use sdc_gui::model::AppModel;
use sdc_gui::pgp::validate::validate_cm;
use sdc_gui::settings::GuiSettings;
use serde_json::{Map, Value as Json};
use std::path::Path;
use std::time::Duration;

const IRON_M: &str = "m/iron_m.png";
const BOLT_M: &str = "m/bolt_m.png";

fn obj(v: Json) -> Map<String, Json> {
    v.as_object().unwrap().clone()
}

fn lines(l: &[&str]) -> String {
    l.join("\n")
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

/// Opens `<dir>/src` (exports under `<dir>/export`), High suggestions accepted, under `target`.
fn open(dir: &Path, target: Target) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.join("src"));
    idle(&mut m);
    if target != Target::Cs {
        m.set_target(target);
        idle(&mut m);
    }
    m
}

/// `iron`: diffuse + `_m` with a varying (height) alpha. `bolt`: diffuse + RGB-only `_m`. `axe`: diffuse + `_em` with
/// a varying alpha. Colourful RGB, so no VAL-M-GRAYSCALE.
fn fixture(dir: &Path, target: Target) -> AppModel {
    let r = dir.join("src/m");
    std::fs::create_dir_all(&r).unwrap();
    let rgb = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    for base in ["iron", "bolt", "axe"] {
        ImageBuffer::from_fn(64, 64, |x, y| Rgb(rgb(x, y)))
            .save(r.join(format!("{base}.png")))
            .unwrap();
    }
    for name in ["iron_m.png", "axe_em.png"] {
        ImageBuffer::from_fn(64, 64, |x, y| {
            let [a, b, c] = rgb(x, y);
            Rgba([a, b, c, (40 + x * 2) as u8])
        })
        .save(r.join(name))
        .unwrap();
    }
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(rgb(x, y)))
        .save(r.join("bolt_m.png"))
        .unwrap();
    open(dir, target)
}

fn view(m: &AppModel, key_end: &str) -> SetView {
    set_views(m)
        .into_iter()
        .find(|v| v.key.ends_with(key_end))
        .unwrap_or_else(|| panic!("no set {key_end}"))
}

fn set_id(m: &AppModel, key_end: &str) -> String {
    view(m, key_end).id
}

fn ok(name: &str, rel: &str) -> (String, Result<String, String>) {
    (name.to_string(), Ok(rel.to_string()))
}

fn rels(m: &AppModel) -> Vec<(String, Result<String, String>)> {
    targets(m).into_iter().map(|t| (t.name, t.rel)).collect()
}

/// Design §9: the golden text of both schema examples: `$schema` first, floats, 2-space indent, final newline.
#[test]
fn the_schema_examples_are_written_with_schema_first_floats_and_a_newline() {
    let schema = format!("  \"$schema\": \"{SCHEMA_URL}\",");
    let ex2 = obj(
        json!({"specular_enabled": true, "specular_color": [1, 0.85, 0.6],
                         "specular_strength": 1.5, "glossiness": 80}),
    );
    assert_eq!(
        to_text(&ex2),
        lines(&[
            "{",
            &schema,
            "  \"glossiness\": 80.0,",
            "  \"specular_color\": [",
            "    1.0,",
            "    0.85,",
            "    0.6",
            "  ],",
            "  \"specular_enabled\": true,",
            "  \"specular_strength\": 1.5",
            "}",
            "",
        ])
    );
    let ex1 = obj(json!({"environment_map_scale": 0.5, "dynamic_cubemap": false}));
    assert_eq!(
        to_text(&ex1),
        lines(&[
            "{",
            &schema,
            "  \"dynamic_cubemap\": false,",
            "  \"environment_map_scale\": 0.5",
            "}",
            "",
        ])
    );
    // Zero keys: the file still exists, `$schema` only (it restates PGPatcher's defaults).
    assert_eq!(
        to_text(&Map::new()),
        format!("{{\n  \"$schema\": \"{SCHEMA_URL}\"\n}}\n")
    );
}

/// Review focus 2: only set, well-typed, known keys; ints become floats; colours above 1 stay.
#[test]
fn to_text_keeps_only_well_typed_keys_as_floats() {
    let v = obj(
        json!({"glossiness": "high", "specular_color": [2, 3.5, 1], "foo": 1, "specular_strength": 1}),
    );
    assert_eq!(
        to_text(&v),
        lines(&[
            "{",
            &format!("  \"$schema\": \"{SCHEMA_URL}\","),
            "  \"specular_color\": [",
            "    2.0,",
            "    3.5,",
            "    1.0",
            "  ],",
            "  \"specular_strength\": 1.0",
            "}",
            "",
        ])
    );
}

#[test]
fn every_written_text_passes_the_schema() {
    let mut all = Map::new();
    for k in &KEYS {
        let one = Map::from_iter([(k.name.to_string(), first_value(k))]);
        let t = to_text(&one);
        assert_eq!(
            validate_cm(&serde_json::from_str(&t).unwrap()),
            Ok(()),
            "{t}"
        );
        all.insert(k.name.to_string(), first_value(k));
    }
    let t = to_text(&all);
    assert_eq!(
        validate_cm(&serde_json::from_str(&t).unwrap()),
        Ok(()),
        "{t}"
    );
}

#[test]
fn two_members_writing_one_file_clash_whatever_the_case() {
    let c = clashes(["m/a_m.json", "M/A_m.JSON", "m/b_m.json"].into_iter());
    assert_eq!(c.into_iter().collect::<Vec<_>>(), ["m/a_m.json"]);
}

/// Design §4 predicate, both targets (D16).
#[test]
fn the_card_shows_for_complex_m_sets_on_both_targets_and_hides_otherwise() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = view(&m, "m/iron");
    assert_eq!(cm_members(&m, &iron), [IRON_M]);
    assert!(shows_card(&m, &iron));
    assert!(
        !shows_card(&m, &view(&m, "m/bolt")),
        "a plain env mask under the Envmap shader"
    );
    let bolt = set_id(&m, "m/bolt");
    m.set_shader(&bolt, Some(Shader::ComplexMaterial));
    idle(&mut m);
    assert_eq!(cm_members(&m, &view(&m, "m/bolt")), [BOLT_M]);
    m.set_shader(&bolt, None);
    idle(&mut m);
    m.set_target(Target::Vanilla);
    idle(&mut m);
    assert_eq!(
        m.item(IRON_M)
            .unwrap()
            .job
            .as_ref()
            .unwrap()
            .profile
            .subtype,
        Subtype::ComplexMaterial
    );
    assert!(shows_card(&m, &view(&m, "m/iron")));
    assert!(!shows_card(&m, &view(&m, "m/bolt")));
}

/// Review focus 3: a stored block keeps the card visible; a PBR set without one is hidden.
#[test]
fn a_stored_block_keeps_the_card() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let bolt = set_id(&m, "m/bolt");
    m.set_cm_value(&bolt, "glossiness", Some(json!(80.0)));
    assert!(shows_card(&m, &view(&m, "m/bolt")));
    let d = tempfile::tempdir().unwrap();
    let m = support::pbr_opened(d.path());
    assert!(!shows_card(&m, &view(&m, "landscape/rock01")));
}

/// Design §6 step 2: the JSON follows the output as resolved (`_em`, an `output_name` rename).
#[test]
fn targets_follow_the_resolved_output_name() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let r = rels(&m);
    assert!(r.contains(&ok("iron_m", "m/iron_m.json")), "{r:?}");
    assert!(r.contains(&ok("axe_em", "m/axe_em.json")), "{r:?}");
    let iron = set_id(&m, "m/iron");
    m.set_output_name(&iron, Some("steel")).unwrap();
    idle(&mut m);
    let r = rels(&m);
    assert!(r.contains(&ok("steel_m", "m/steel_m.json")), "{r:?}");
}

/// Design §6 step 2: a packed complex `_m` (vanilla, complex-material set, Note AZ) gets its JSON next to the packed
/// output.
#[test]
fn a_packed_complex_m_gets_its_json_next_to_the_packed_output() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("src/p");
    std::fs::create_dir_all(&p).unwrap();
    for n in ["steel_metallic.png", "steel_glossiness.png"] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 4) as u8]))
            .save(p.join(n))
            .unwrap();
    }
    let mut m = open(d.path(), Target::Vanilla);
    let steel = set_id(&m, "p/steel");
    m.set_shader(&steel, Some(Shader::ComplexMaterial));
    idle(&mut m);
    let r = rels(&m);
    assert!(r.contains(&ok("steel_m", "p/steel_m.json")), "{r:?}");
}

fn target(set: &str, rel: &str) -> CmTarget {
    let name = rel.rsplit('/').next().unwrap().trim_end_matches(".json");
    CmTarget {
        set: set.into(),
        key: format!("{set}_m.png"),
        name: name.into(),
        rel: Ok(rel.into()),
    }
}

/// Review fix round 1. The planner never gives two outputs one path (a case-only rename is dropped, a packed and a
/// plain `steel_m` make one target), so the refusal is driven with hand-made targets and a real document.
#[test]
fn exporting_targets_on_one_path_are_refused_and_the_first_duplicate_entry_decides() {
    let run = |toml: &str| {
        let (_d, s) = doc(toml);
        let entries = s.entries();
        let mut t = vec![
            target("m/iron", "m/iron_m.json"),
            target("m/bolt", "m/IRON_m.json"),
            target("m/axe", "m/axe_em.json"),
        ];
        refuse_clashes(&mut t, |set| exports(&entries, set));
        t.into_iter().map(|t| t.rel.is_ok()).collect::<Vec<_>>()
    };
    let on = |id: &str| format!("[[sets]]\nid = \"{id}\"\n\n[sets.cm]\nexport = true\n\n");
    let off = |id: &str| format!("[[sets]]\nid = \"{id}\"\n\n[sets.cm]\nexport = false\n\n");
    let all = format!("{}{}{}", on("m/iron"), on("m/bolt"), on("m/axe"));
    assert_eq!(
        run(&all),
        [false, false, true],
        "both colliding exports are refused"
    );
    assert_eq!(run(&on("m/iron")), [true, true, true], "only one exports");
    assert_eq!(
        run(&on("m/bolt")),
        [true, true, true],
        "only the other exports"
    );
    let dup = format!("{}{}{}", off("m/iron"), on("m/bolt"), on("m/iron"));
    assert_eq!(
        run(&dup),
        [true, true, true],
        "the first m/iron entry is off"
    );
    let dup = format!("{}{}{}", on("m/iron"), on("m/bolt"), off("m/iron"));
    assert_eq!(
        run(&dup),
        [false, false, true],
        "the first m/iron entry is on"
    );
    let r = {
        let (_d, s) = doc(&all);
        let entries = s.entries();
        let mut t = vec![
            target("m/iron", "m/iron_m.json"),
            target("m/bolt", "m/IRON_m.json"),
        ];
        refuse_clashes(&mut t, |set| exports(&entries, set));
        t[0].rel.clone()
    };
    assert_eq!(
        r,
        Err("iron_m.json is written by two complex _m (names ignore case)".into())
    );
}

use sdc_gui::model::Dialog;
use std::path::PathBuf;

fn export_and_wait(m: &mut AppModel) {
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "export did not finish"
    );
}

fn out(m: &AppModel, rel: &str) -> PathBuf {
    m.export_root().join(rel)
}

fn notes(m: &AppModel) -> String {
    m.export.notes.join("\n")
}

#[test]
fn nothing_is_written_without_a_ticked_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_value(&iron, "glossiness", Some(json!(80.0)));
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.dds").is_file(), "{}", notes(&m));
    assert!(!out(&m, "m/iron_m.json").exists());
    assert!(!out(&m, "m/axe_em.json").exists());
    assert!(m.sets_doc.as_ref().unwrap().cm_json_shas().is_empty());
}

/// Design §6/§9: the file sits next to the `_m`, holds only the set keys, and is recorded. A `--sidecar` record
/// (`<name>.dds.json`) is another file and stays. The GUI exports with `sidecar = false`, and the engine reads or
/// removes `<name>.dds.json` only under `--sidecar` (`api.rs:1798`, `:1934`), so the stub is never read.
#[test]
fn export_writes_the_json_next_to_the_m_and_records_its_hash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let side = out(&m, "m/iron_m.dds.json");
    std::fs::create_dir_all(side.parent().unwrap()).unwrap();
    std::fs::write(&side, "{\"sidecar\": true}").unwrap();
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    m.set_cm_value(&iron, "glossiness", Some(json!(80)));
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.dds").is_file(), "{}", notes(&m));
    assert_eq!(
        std::fs::read_to_string(out(&m, "m/iron_m.json")).unwrap(),
        format!("{{\n  \"$schema\": \"{SCHEMA_URL}\",\n  \"glossiness\": 80.0\n}}\n")
    );
    assert_eq!(
        std::fs::read_to_string(&side).unwrap(),
        "{\"sidecar\": true}"
    );
    assert!(!out(&m, "m/axe_em.json").exists(), "axe's set exports none");
    let shas = m.sets_doc.as_ref().unwrap().cm_json_shas();
    assert_eq!(shas.keys().collect::<Vec<_>>(), ["m/iron_m.json"]);
    assert!(
        notes(&m).contains("Complex material JSON written: 1 file"),
        "{}",
        notes(&m)
    );
}

#[test]
fn a_hand_edited_file_is_kept_and_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    export_and_wait(&mut m);
    let f = out(&m, "m/iron_m.json");
    std::fs::write(&f, "{}\n").unwrap();
    m.set_cm_value(&iron, "glossiness", Some(json!(50.0)));
    export_and_wait(&mut m);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}\n");
    assert!(
        notes(&m).contains("kept as edited outside Tessalith"),
        "{}",
        notes(&m)
    );
}

/// D7: unticking leaves the file and its record, with a note; sdc never deletes it.
#[test]
fn unticking_leaves_the_file_in_place_with_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    export_and_wait(&mut m);
    m.set_cm_export(&iron, false);
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.json").is_file());
    assert!(
        notes(&m).contains("complex material export is off for its set"),
        "{}",
        notes(&m)
    );
    assert!(m
        .sets_doc
        .as_ref()
        .unwrap()
        .cm_json_shas()
        .contains_key("m/iron_m.json"));
}

/// Review focus 3.
#[test]
fn a_set_without_a_complex_m_gets_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let bolt = set_id(&m, "m/bolt");
    m.set_cm_export(&bolt, true);
    export_and_wait(&mut m);
    assert!(!out(&m, "m/bolt_m.json").exists());
    assert!(
        notes(&m).contains(&format!(
            "set {bolt}: no complex _m; complex material JSON not written"
        )),
        "{}",
        notes(&m)
    );
}

#[test]
fn wrong_typed_keys_are_left_out_with_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    m.set_cm_value(&iron, "specular_color", Some(json!([1.0, 2.0])));
    let p = prepare(&m).unwrap();
    assert!(
        p.notes.contains(&format!(
            "{iron}: cm.specular_color has the wrong type and was left out"
        )),
        "{:?}",
        p.notes
    );
    assert!(!p.files[0].1.contains("specular_color"));
}

#[test]
fn an_unfinished_export_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    let p = prepare(&m).unwrap();
    assert_eq!(p.files.len(), 1);
    m.export.error = Some("boom".into());
    m.finish_cm_json(Some(p));
    assert!(!out(&m, "m/iron_m.json").exists());
    assert!(notes(&m).contains("Complex material JSON not written: the export did not finish"));
}

#[test]
fn a_member_whose_dds_is_missing_gets_a_note_and_no_json() {
    let d = tempfile::tempdir().unwrap();
    let p = PendingCm {
        root: d.path().to_path_buf(),
        files: vec![("textures/a/x_m.json".into(), to_text(&Map::new()))],
        notes: vec![],
    };
    let mut n = vec![];
    let shas = finish(&p, &BTreeMap::new(), &mut n);
    assert!(shas.is_empty());
    assert!(!d.path().join("textures/a/x_m.json").exists());
    assert!(
        n.iter()
            .any(|x| x.contains("x_m.dds was not exported; JSON not written")),
        "{n:?}"
    );
}

/// D16: a Vanilla complex `_m` gets its JSON too.
#[test]
fn a_vanilla_complex_m_writes_its_json() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Vanilla);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.json").is_file(), "{}", notes(&m));
}

/// Ruling C7: the files are written, but a read-only `sdc-project.toml` takes no hash record (a note says so).
#[test]
fn a_read_only_document_records_no_hash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    export_and_wait(&mut m);
    let p = prepare(&m).unwrap();
    std::fs::remove_file(out(&m, "m/iron_m.json")).unwrap();
    let (_b, broken) = doc("[[sets]\nbroken");
    m.sets_doc = Some(broken);
    m.finish_cm_json(Some(p));
    assert!(out(&m, "m/iron_m.json").is_file(), "{}", notes(&m));
    assert!(
        notes(&m).contains("hashes were not recorded: sdc-project.toml is read-only"),
        "{}",
        notes(&m)
    );
    assert!(m.sets_doc.as_ref().unwrap().cm_json_shas().is_empty());
}

/// Fix round 1 (1): a file sdc did not write is never adopted, even with the same bytes.
#[test]
fn an_identical_unrecorded_file_is_never_overwritten() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    let mine = prepare(&m).unwrap().files[0].1.clone();
    let f = out(&m, "m/iron_m.json");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, &mine).unwrap();
    export_and_wait(&mut m);
    assert!(
        notes(&m).contains("kept as edited outside Tessalith"),
        "{}",
        notes(&m)
    );
    m.set_cm_value(&iron, "glossiness", Some(json!(50.0)));
    export_and_wait(&mut m);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), mine);
    assert!(m.sets_doc.as_ref().unwrap().cm_json_shas().is_empty());
}

/// Fix round 1 (2): something at the path that cannot be read as a file is kept, never treated as absent.
#[test]
fn an_unreadable_file_is_kept() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("m/x_m.json");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(may_replace(&dir, None)
        .unwrap_err()
        .contains("could not be read"));
    assert!(may_replace(&dir, Some("00")).is_err());
    assert_eq!(may_replace(&d.path().join("absent.json"), None), Ok(()));
}

/// Fix round 1 (3): an export with a failed job writes no JSON, even where an older `_m` .dds is on disk.
#[test]
fn a_failed_export_writes_nothing_next_to_a_stale_dds() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.dds").is_file(), "{}", notes(&m));
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    let p = prepare(&m).unwrap();
    m.export.summary.as_mut().unwrap().failed = 1;
    m.finish_cm_json(Some(p));
    assert!(!out(&m, "m/iron_m.json").exists());
    assert!(notes(&m).contains("Complex material JSON not written: the export did not finish"));
}

/// Fix round 1 (4): of two entries with one id, the first decides (as `exports` and `cm_block`).
#[test]
fn prepare_follows_the_first_entry_of_a_duplicated_id() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    let mut run = |a: &str, b: &str| {
        let toml = format!(
            "[[sets]]\nid = \"{iron}\"\n\n[sets.cm]\n{a}\n\n[[sets]]\nid = \"{iron}\"\n\n[sets.cm]\n{b}\n"
        );
        let (_t, s) = doc(&toml);
        m.sets_doc = Some(s);
        prepare(&m).unwrap().files
    };
    assert!(run("export = false", "export = true").is_empty());
    let f = run(
        "export = true\nglossiness = 10",
        "export = true\nglossiness = 20",
    );
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(
        f[0].1.contains("10.0") && !f[0].1.contains("20.0"),
        "{}",
        f[0].1
    );
}

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::codes::{Code, Warning};
use sdc_gui::model::export::{Row, RowState};

fn margin_row(m: &AppModel, key: &str, alpha: f64) -> Row {
    Row {
        job_key: m.item(key).unwrap().job_key.clone(),
        path: String::new(),
        state: RowState::Done { passed: true },
        warnings: vec![Warning::new(Code::ValMComplexMargin, "margin")
            .with_data(json!({"smallest_alpha": alpha, "complex": alpha < 251.0}))],
    }
}

/// `card_warnings` with the targets computed once, as the card does (ruling C5).
fn warns(m: &AppModel, v: &SetView) -> Vec<String> {
    card_warnings(m, v, &targets(m))
}

/// D12/D17 and Review focus 5: the 251–254 line and the 255 line come from the latest export row only; a fractional
/// alpha is rounded.
#[test]
fn the_alpha_lines_follow_the_latest_export_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = view(&m, "m/iron");
    assert!(
        warns(&m, &iron).is_empty(),
        "before an export nothing shows"
    );
    for (a, want) in [
        (
            252.0,
            Some("iron_m: smallest-mip alpha 252. PGPatcher treats it as complex material and applies iron_m.json"),
        ),
        (250.6, Some("iron_m: smallest-mip alpha 251.")),
        (250.0, None),
        (
            255.0,
            Some("PGPatcher likely reads iron_m as an env mask; the JSON will not be applied."),
        ),
    ] {
        m.export.rows = vec![margin_row(&m, IRON_M, a)];
        let w = warns(&m, &iron);
        match want {
            Some(s) => assert!(w.iter().any(|l| l.contains(s)), "{a}: {w:?}"),
            None => assert!(w.is_empty(), "{a}: {w:?}"),
        }
    }
}

/// Design §5: env-mask members warn; a Vanilla complex `_m` does not (D16); a wrong-typed key is listed.
#[test]
fn env_mask_members_and_bad_keys_warn_and_a_vanilla_complex_m_does_not() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let bolt = set_id(&m, "m/bolt");
    m.set_shader(&bolt, Some(Shader::ComplexMaterial));
    idle(&mut m);
    let w = warns(&m, &view(&m, "m/bolt"));
    assert!(
        w.iter()
            .any(|l| l.contains("bolt_m is written as an env mask without alpha")),
        "{w:?}"
    );
    m.set_field(IRON_M, "alpha.policy", json!("remove"));
    idle(&mut m);
    let w = warns(&m, &view(&m, "m/iron"));
    assert!(
        w.iter()
            .any(|l| l.contains("iron_m is written as an env mask")),
        "{w:?}"
    );
    m.clear_field(IRON_M, "alpha.policy");
    idle(&mut m);
    m.set_target(Target::Vanilla);
    idle(&mut m);
    assert_eq!(warns(&m, &view(&m, "m/iron")), Vec::<String>::new());
    let iron = set_id(&m, "m/iron");
    m.set_cm_value(&iron, "glossiness", Some(json!("high")));
    assert_eq!(
        warns(&m, &view(&m, "m/iron")),
        ["glossiness: wrong type in sdc-project.toml; not exported"]
    );
}

/// The card in the set editor: the note, the target path, and one undo step per tick.
#[test]
fn the_card_ticks_export_and_a_key_in_one_undo_step_each() {
    use sdc_gui::ui::cm_card::{EXPORT, NOTE, SECTION};
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let v = view(&m, "m/iron");
    m.select_set(&v.key);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 2400.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label(SECTION).click();
    h.run();
    assert!(h.query_all_by_label_contains(NOTE).next().is_some());
    assert!(h
        .query_all_by_label_contains("m\\iron_m.json")
        .next()
        .is_some());
    h.get_by_label(EXPORT).click();
    h.run();
    h.get_by_label("glossiness").click();
    h.run();
    let m = h.state_mut();
    let b = m.cm_block(&v.id).unwrap();
    assert!(b.export);
    assert_eq!(b.values["glossiness"], json!(80.0));
    m.undo();
    idle(m);
    let b = m.cm_block(&v.id).unwrap();
    assert!(b.export && !b.values.contains_key("glossiness"), "{b:?}");
}

fn cm_pending(root: &Path, rel: &str, strength: f64) -> PendingCm {
    PendingCm {
        root: root.to_path_buf(),
        files: vec![(
            rel.into(),
            to_text(&obj(json!({ "specular_strength": strength }))),
        )],
        notes: vec![],
    }
}

/// Trace-debug 2026-10-16 #8: a case-only rename of the `_m` rewrites sdc's own JSON under the new name and keeps
/// the hash record, on this export and every later one.
#[test]
fn finish_case_only_rename_rewrites_own_json_and_keeps_record() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("m")).unwrap();
    std::fs::write(d.path().join("m/Iron_M.dds"), b"dds").unwrap();
    let mut n = vec![];
    let mut rec = finish(
        &cm_pending(d.path(), "m/Iron_M.json", 1.0),
        &BTreeMap::new(),
        &mut n,
    );
    std::fs::rename(d.path().join("m/Iron_M.dds"), d.path().join("m/iron_m.dds")).unwrap();
    for strength in [2.0, 3.0] {
        let p = cm_pending(d.path(), "m/iron_m.json", strength);
        let mut n = vec![];
        rec = finish(&p, &rec, &mut n);
        assert!(!n.iter().any(|x| x.contains("edited outside")), "{n:?}");
        assert_eq!(rec.keys().collect::<Vec<_>>(), ["m/iron_m.json"]);
        assert_eq!(
            std::fs::read_to_string(d.path().join("m/iron_m.json")).unwrap(),
            p.files[0].1
        );
        assert_eq!(files_in(&d.path().join("m")), ["iron_m.dds", "iron_m.json"]);
    }
}

/// The negative case: a real hand edit is kept, and its record survives the case-only rename.
#[test]
fn finish_case_only_rename_keeps_a_hand_edit_and_its_record() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("m")).unwrap();
    std::fs::write(d.path().join("m/iron_m.dds"), b"dds").unwrap();
    let mut n = vec![];
    let r1 = finish(
        &cm_pending(d.path(), "m/Iron_M.json", 1.0),
        &BTreeMap::new(),
        &mut n,
    );
    std::fs::write(d.path().join("m/Iron_M.json"), "{ \"hand\": 1 }\n").unwrap();
    let mut n = vec![];
    let r2 = finish(&cm_pending(d.path(), "m/iron_m.json", 2.0), &r1, &mut n);
    assert!(n.iter().any(|x| x.contains("edited outside")), "{n:?}");
    assert_eq!(r2, r1);
    let kept = d.path().join("m/Iron_M.json");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "{ \"hand\": 1 }\n");
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}
