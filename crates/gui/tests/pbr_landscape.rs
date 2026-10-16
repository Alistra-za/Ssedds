//! Note BM: the PBR Landscape shader in the GUI: the card, the PGPatcher box, the badge, the Advanced records panel
//! and an old project's export.
mod support;
use egui_kittest::kittest::{NodeT as _, Queryable};
use egui_kittest::Harness;
use engine::shader::{AutoRule, Shader, ShaderOrigin};
use sdc_gui::model::cs_pbr::{Kind, SetFacts, Statics, PGP_BOX, PGP_BOX_UNAVAILABLE};
use sdc_gui::model::plugin_refs::set_key;
use sdc_gui::model::sets::{
    badge_text, set_views, SetView, BADGE_PBR_LANDSCAPE, BADGE_PBR_LANDSCAPE_NONE,
    BADGE_PBR_LANDSCAPE_PGP, BADGE_PBR_LANDSCAPE_PGP_ONLY,
};
use sdc_gui::model::AppModel;
use sdc_gui::ui::cs_pbr::{CARD, HIDDEN_ROWS, SECTION};
use std::time::Duration;

const ID: &str = "landscape/rock01";

fn view(m: &AppModel) -> SetView {
    set_views(m).into_iter().find(|v| v.id == ID).unwrap()
}

fn settle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

fn tall(mut model: AppModel) -> Harness<'static, AppModel> {
    let key = view(&model).key;
    model.select_set(&key);
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 2400.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

fn has(h: &Harness<'static, AppModel>, s: &str) -> bool {
    h.query_all_by_label_contains(s).next().is_some()
}

/// The fixture's rock01 is a PBR set under `textures/landscape/`: auto PBR Landscape, card shown, records panel not.
/// The first card edit creates the block and, with no scan, changes neither the box nor the badge (Note BM6).
#[test]
fn a_pbr_landscape_set_gets_the_card_and_edits_write_the_blocks_with_undo() {
    let d = tempfile::tempdir().unwrap();
    let m = support::pbr_opened(d.path());
    let p = view(&m).shader.unwrap();
    assert_eq!(
        (p.shader, p.provenance),
        (
            Shader::PbrLandscape,
            ShaderOrigin::Auto(AutoRule::PbrLandscape)
        )
    );
    assert_eq!(badge_text(&m, &view(&m)), BADGE_PBR_LANDSCAPE_PGP_ONLY);
    let mut h = tall(m);
    h.run();
    assert!(has(&h, CARD));
    assert!(!has(&h, SECTION), "no second editor of the same blocks");
    assert!(has(&h, "PBR texture set"));
    assert!(has(&h, "Add PBR material object"));
    assert!(box_on(&h));
    // The texture set shows before it exists; the first field edit creates it.
    assert!(h.state().cs_block(ID, Kind::Txst).is_none());
    // The PGPatcher panel below has its own "Roughness scale"; the card's row comes first.
    h.query_all_by_label("Roughness scale")
        .next()
        .unwrap()
        .click();
    h.run();
    let b = h.state().cs_block(ID, Kind::Txst).expect("block made");
    assert_eq!(b.values["roughnessScale"], serde_json::json!(1.0));
    assert!(box_on(&h), "no scan: the entry stays");
    assert_eq!(
        badge_text(h.state(), &view(h.state())),
        BADGE_PBR_LANDSCAPE_PGP_ONLY
    );
    h.get_by_label("Add PBR material object").click();
    h.run();
    assert!(h.state().cs_block(ID, Kind::Mato).is_some());
    h.state_mut().undo();
    settle(h.state_mut());
    assert!(h.state().cs_block(ID, Kind::Mato).is_none(), "one step");
    h.state_mut().undo();
    settle(h.state_mut());
    assert!(h.state().cs_block(ID, Kind::Txst).is_none());
}

fn box_on(h: &Harness<'static, AppModel>) -> bool {
    h.get_by_label(PGP_BOX).accesskit_node().toggled()
        == Some(eframe::egui::accesskit::Toggled::True)
}

/// The box stores statics; with a scan, auto follows it; the badge says what is written (review I2).
#[test]
fn the_pgpatcher_box_stores_statics_and_drives_the_badge() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.set_cs_edids(ID, Kind::Txst, &["LandscapeRock01".into()]);
    settle(&mut m);
    // CS records and, with no scan, the PGPatcher entry.
    assert_eq!(badge_text(&m, &view(&m)), BADGE_PBR_LANDSCAPE_PGP);
    let mut h = tall(m);
    h.run();
    assert!(has(&h, "No scan yet"));
    assert!(box_on(&h));
    let st = |h: &Harness<'static, AppModel>| h.state().cs_block(ID, Kind::Txst).unwrap().statics;
    h.get_by_label(PGP_BOX).click();
    h.run();
    assert_eq!(st(&h), Statics::Never);
    assert_eq!(badge_text(h.state(), &view(h.state())), BADGE_PBR_LANDSCAPE);
    assert!(has(&h, BADGE_PBR_LANDSCAPE));
    h.get_by_label(PGP_BOX).click();
    h.run();
    assert_eq!(st(&h), Statics::Always);
    assert_eq!(
        badge_text(h.state(), &view(h.state())),
        BADGE_PBR_LANDSCAPE_PGP
    );
    h.get_by_label("Back to auto").click();
    h.run();
    assert_eq!(st(&h), Statics::Auto);
    // A scan that finds no mesh using the diffuse: auto writes no entry.
    let key = set_key(&view(h.state())).unwrap();
    h.state().cs_pbr.facts.borrow_mut().insert(
        key,
        Ok(SetFacts {
            txst_edids: vec![],
            statics: vec![],
        }),
    );
    h.run();
    assert!(!box_on(&h));
    assert_eq!(badge_text(h.state(), &view(h.state())), BADGE_PBR_LANDSCAPE);
}

/// Review I2: a PBR Landscape set whose colour map is a landscape diffuse has no PGPatcher entry: the box is off and
/// says why, and the badge claims no JSON.
#[test]
fn a_landscape_diffuse_albedo_gets_no_pgpatcher_entry() {
    use engine::config::Subtype;
    use sdc_gui::model::Answer;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.decide(
        "landscape/rock01.png",
        Answer::Subtype(Subtype::LandscapeDiffuse),
    );
    settle(&mut m);
    let v = view(&m);
    assert_eq!(v.shader.as_ref().unwrap().shader, Shader::PbrLandscape);
    assert!(!v.pbr);
    assert_eq!(badge_text(&m, &v), BADGE_PBR_LANDSCAPE_NONE);
    let mut h = tall(m);
    h.run();
    assert!(!box_on(&h));
    assert!(h.get_by_label(PGP_BOX).accesskit_node().is_disabled());
    assert!(has(&h, PGP_BOX_UNAVAILABLE));
}

/// Owner: rows CS's landscape shader does not read are hidden while the box is off, shown while it is on, and a
/// hidden row with a value stays visible.
#[test]
fn rows_landscape_does_not_read_hide_while_the_box_is_off() {
    let d = tempfile::tempdir().unwrap();
    let m = support::pbr_opened(d.path());
    let mut h = tall(m);
    h.run();
    assert!(box_on(&h));
    assert!(has(&h, "Coat strength") && has(&h, "Fuzz weight"));
    assert!(!has(&h, HIDDEN_ROWS));
    h.get_by_label(PGP_BOX).click();
    h.run();
    assert!(!box_on(&h));
    assert!(!has(&h, "Coat strength") && !has(&h, "Subsurface opacity"));
    assert!(has(&h, "Roughness scale") && has(&h, "Glint parameters"));
    assert!(has(&h, &format!("9 {HIDDEN_ROWS}")));
    h.state_mut()
        .set_cs_value(ID, Kind::Txst, "coatStrength", Some(serde_json::json!(0.5)));
    h.run();
    assert!(has(&h, "Coat strength"), "a valued row is never hidden");
    assert!(has(&h, &format!("8 {HIDDEN_ROWS}")));
}

fn glint(m: &AppModel, kind: Kind) -> Option<serde_json::Value> {
    m.cs_block(ID, kind)
        .and_then(|b| b.values.get("glintParameters").cloned())
}

/// Note BM16: a Shared-section edit writes the texture set and the material object in one undo step.
#[test]
fn a_shared_edit_writes_every_block_with_one_undo() {
    let d = tempfile::tempdir().unwrap();
    let mut h = tall(support::pbr_opened(d.path()));
    h.run();
    h.get_by_label("Add PBR material object").click();
    h.run();
    assert!(has(&h, "Glint parameters: shared (above)"));
    // The Shared section comes first in the card (the PGPatcher panel below has its own glint rows).
    h.query_all_by_label("Glint parameters")
        .next()
        .unwrap()
        .click();
    h.run();
    let on = Some(serde_json::json!({"enabled": true}));
    assert_eq!(glint(h.state(), Kind::Txst), on);
    assert_eq!(glint(h.state(), Kind::Mato), on);
    h.state_mut().undo();
    settle(h.state_mut());
    assert_eq!(glint(h.state(), Kind::Txst), None);
    assert_eq!(glint(h.state(), Kind::Mato), None, "one step for both");
}

/// Note BM16: Unsync keeps the material object's own value through shared edits; Sync takes the shared one again;
/// the `unsynced` list never reaches the CS JSON.
#[test]
fn an_unsynced_material_object_keeps_its_own_value() {
    use serde_json::json;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.add_mato_synced(ID);
    m.set_shared_value(ID, "glintParameters", Some(json!({"enabled": true})));
    m.set_cs_edids(ID, Kind::Mato, &["MyMato".into()]);
    settle(&mut m);
    let mut h = tall(m);
    h.run();
    h.get_by_label("Unsync").click();
    h.run();
    assert_eq!(
        h.state().cs_block(ID, Kind::Mato).unwrap().unsynced,
        ["glintParameters"]
    );
    h.state_mut()
        .set_shared_value(ID, "glintParameters.screenSpaceScale", Some(json!(2.5)));
    h.run();
    assert_eq!(
        glint(h.state(), Kind::Txst),
        Some(json!({"enabled": true, "screenSpaceScale": 2.5}))
    );
    assert_eq!(glint(h.state(), Kind::Mato), Some(json!({"enabled": true})));
    let files = sdc_gui::model::cs_pbr_export::prepare(h.state())
        .unwrap()
        .files;
    assert!(
        files.iter().all(|(_, t)| !t.contains("unsynced")),
        "{files:?}"
    );
    h.get_by_label("Sync Glint parameters").click();
    h.run();
    assert_eq!(glint(h.state(), Kind::Mato), glint(h.state(), Kind::Txst));
    assert!(h
        .state()
        .cs_block(ID, Kind::Mato)
        .unwrap()
        .unsynced
        .is_empty());
    assert!(has(&h, "Glint parameters: shared (above)"));
    h.state_mut().undo();
    settle(h.state_mut());
    assert_eq!(glint(h.state(), Kind::Mato), Some(json!({"enabled": true})));
}

/// Note BM16: blocks that already differ (an older project) load as unsynced and keep their values.
#[test]
fn differing_blocks_load_as_unsynced() {
    use serde_json::json;
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.set_cs_value(
        ID,
        Kind::Txst,
        "glintParameters",
        Some(json!({"enabled": true})),
    );
    m.set_cs_value(
        ID,
        Kind::Mato,
        "glintParameters",
        Some(json!({"enabled": false})),
    );
    settle(&mut m);
    let mut h = tall(m);
    h.run();
    assert!(has(&h, "Sync Glint parameters"));
    assert!(!has(&h, "Glint parameters: shared (above)"));
    assert_eq!(
        glint(h.state(), Kind::Mato),
        Some(json!({"enabled": false}))
    );
    assert_eq!(glint(h.state(), Kind::Txst), Some(json!({"enabled": true})));
}

/// A plain PBR set keeps the collapsed "Advanced: CS PBR records" panel and no card.
#[test]
fn a_plain_pbr_set_still_reaches_the_advanced_records_panel() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.set_shader(ID, Some(Shader::Pbr));
    m.add_cs_block(ID, Kind::Txst); // explicit pbr beats the block
    settle(&mut m);
    assert_eq!(badge_text(&m, &view(&m)), "PBR → JSON");
    let mut h = tall(m);
    h.run();
    assert!(!has(&h, CARD));
    assert!(!has(&h, "Add PBR material object"), "collapsed");
    h.get_by_label(SECTION).click();
    h.run();
    assert!(has(&h, "Add PBR material object"));
    assert!(has(&h, "PGPatcher entry for statics:"));
}

/// An old project (a texture-set block, no shader key) loads as PBR Landscape and exports the same CS and PGPatcher
/// files as when the set is PBR.
#[test]
fn an_old_project_with_a_texture_set_block_exports_the_same_files() {
    use sdc_gui::model::{cs_pbr_export, pbr_export};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.apply_cs_template(ID, Kind::Txst, "LandscapeMountainSlab")
        .unwrap();
    m.set_cs_statics(ID, Statics::Always);
    settle(&mut m);
    assert_eq!(view(&m).shader.unwrap().shader, Shader::PbrLandscape);
    let files = |m: &AppModel| {
        (
            cs_pbr_export::prepare(m).unwrap().files,
            pbr_export::prepare(m).map(|p| format!("{p:?}")),
        )
    };
    let landscape = files(&m);
    assert!(!landscape.0.is_empty());
    assert!(landscape.1.is_some());
    m.set_shader(ID, Some(Shader::Pbr));
    settle(&mut m);
    assert_eq!(view(&m).shader.unwrap().shader, Shader::Pbr);
    assert_eq!(files(&m), landscape);
}

/// Note BM: renders the PBR Landscape card (a template, EDIDs and a material object filled in) to
/// `SDC_PBR_LANDSCAPE_PNG` for the owner. Needs a GPU: `cargo test -p sdc-gui --test pbr_landscape -- --ignored`.
#[test]
#[ignore]
fn render_the_card() {
    let out =
        std::env::var("SDC_PBR_LANDSCAPE_PNG").unwrap_or_else(|_| "pbr-landscape-card.png".into());
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    m.apply_cs_template(ID, Kind::Txst, "LandscapeMountainSlab")
        .unwrap();
    m.add_mato_synced(ID);
    m.set_cs_edids(ID, Kind::Mato, &["MyLandscapeMATO".into()]);
    settle(&mut m);
    let key = view(&m).key;
    m.select_set(&key);
    let scale = 1.5;
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1440.0, 2400.0))
        .with_pixels_per_point(scale)
        .wgpu()
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(6);
    let top = h.get_by_label(CARD).rect();
    let bottom = h
        .query_all_by_label("Add EDID")
        .map(|n| n.rect().bottom())
        .fold(top.bottom(), f32::max);
    let img = h.render().expect("render");
    let px = |v: f32| (v * scale).max(0.0) as u32;
    let x0 = px(top.left() - 12.0);
    let y0 = px(top.top() - 8.0);
    let crop = image::imageops::crop_imm(
        &img,
        x0,
        y0,
        img.width() - x0,
        (px(bottom + 60.0)).min(img.height()) - y0,
    )
    .to_image();
    crop.save(&out).expect("save png");
}
