//! Note BM: the PBR Landscape shader in the GUI: the card, the PGPatcher box, the badge, the Advanced records panel
//! and an old project's export.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::shader::{AutoRule, Shader, ShaderOrigin};
use sdc_gui::model::cs_pbr::{Kind, Statics, PGP_BOX};
use sdc_gui::model::sets::{
    badge_text, set_views, SetView, BADGE_PBR_LANDSCAPE, BADGE_PBR_LANDSCAPE_PGP,
};
use sdc_gui::model::AppModel;
use sdc_gui::ui::cs_pbr::{CARD, SECTION};
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
    let mut h = tall(m);
    h.run();
    assert!(has(&h, CARD));
    assert!(!has(&h, SECTION), "no second editor of the same blocks");
    assert!(has(&h, "PBR texture set"));
    assert!(has(&h, "Add PBR material object"));
    // The texture set shows before it exists; the first field edit creates it.
    assert!(h.state().cs_block(ID, Kind::Txst).is_none());
    h.get_by_label("roughnessScale").click();
    h.run();
    let b = h.state().cs_block(ID, Kind::Txst).expect("block made");
    assert_eq!(b.values["roughnessScale"], serde_json::json!(1.0));
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

/// The box replaces statics: unticked follows the scan (none here), a click stores always / never, the badge follows.
#[test]
fn the_pgpatcher_box_stores_statics_and_drives_the_badge() {
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    // No block yet: written as for any PBR set.
    assert_eq!(badge_text(&m, &view(&m)), BADGE_PBR_LANDSCAPE_PGP);
    m.add_cs_block(ID, Kind::Txst);
    settle(&mut m);
    // A block and no scan: auto writes no entry.
    assert_eq!(badge_text(&m, &view(&m)), BADGE_PBR_LANDSCAPE);
    let mut h = tall(m);
    h.run();
    assert!(has(&h, "No scan yet"));
    h.get_by_label(PGP_BOX).click();
    h.run();
    let st = |h: &Harness<'static, AppModel>| h.state().cs_block(ID, Kind::Txst).unwrap().statics;
    assert_eq!(st(&h), Statics::Always);
    assert_eq!(
        badge_text(h.state(), &view(h.state())),
        BADGE_PBR_LANDSCAPE_PGP
    );
    assert!(has(&h, BADGE_PBR_LANDSCAPE_PGP));
    h.get_by_label(PGP_BOX).click();
    h.run();
    assert_eq!(st(&h), Statics::Never);
    assert_eq!(badge_text(h.state(), &view(h.state())), BADGE_PBR_LANDSCAPE);
    h.get_by_label("Back to auto").click();
    h.run();
    assert_eq!(st(&h), Statics::Auto);
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
    m.add_cs_block(ID, Kind::Mato);
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
