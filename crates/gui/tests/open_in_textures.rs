//! Item 9 (Task 12): "Open in Textures" on a Meshes-view texture path that is in the open folder.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use modscan::store::{NifRow, ShapeView};
use sdc_gui::model::meshes::MeshesModel;
use sdc_gui::model::Selection;
use support::*;

const PATH: &str = "textures/landscape/leaf.dds";
const OUTSIDE: &str = "textures/other/rock.dds";

fn shape() -> ShapeView {
    ShapeView {
        block: 4,
        name: Some("Body".into()),
        kind: "BSTriShape".into(),
        shader_kind: 1,
        shader_type: Some(1),
        slsf1: None,
        slsf2: None,
        textures: vec![Some(PATH.into()), Some(OUTSIDE.into())],
        raw: vec![None; 2],
        alpha_flags: None,
        alpha_threshold: None,
        params: None,
        skinned: false,
        vertex_desc: None,
        num_vertices: None,
        num_triangles: None,
    }
}

fn meshes() -> MeshesModel {
    let mut m = MeshesModel::empty();
    m.nifs = vec![NifRow {
        copy_id: 7,
        path: "meshes/x.nif".into(),
        wins: true,
        overridden_by: None,
        n_copies: 1,
        derived: false,
        derive_check: None,
        le_mesh: false,
        problem: None,
        patched_in_game: false,
    }];
    m.selected_nif = Some(7);
    m.shapes = vec![shape()];
    m.shapes_col = true;
    m
}

#[test]
fn only_a_path_in_the_open_folder_has_the_button_and_a_click_asks_for_it() {
    let mut m = meshes();
    m.folder_items
        .insert(PATH.to_string(), "landscape/leaf.png".to_string());
    // The users list of a texture names it too.
    m.tex_users = Some((PATH.to_string(), vec![]));
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 800.0))
        .build_ui_state(
            |ui, m: &mut MeshesModel| sdc_gui::ui::meshes::show(ui, m),
            m,
        );
    h.run();
    assert_eq!(h.query_all_by_label("Open in Textures").count(), 2);
    h.query_all_by_label("Open in Textures")
        .next()
        .unwrap()
        .click();
    h.run();
    assert_eq!(
        h.state().open_in_textures.as_deref(),
        Some("landscape/leaf.png")
    );
}

#[test]
fn open_in_textures_switches_the_view_selects_and_reveals_the_texture() {
    use sdc_gui::app::SdcApp;
    let d = tempfile::tempdir().unwrap();
    let mut app = SdcApp::new(opened(d.path()));
    app.meshes.shapes = vec![shape()];
    let ctx = eframe::egui::Context::default();
    sdc_gui::ui::header::set_meshes_view(&ctx, true);
    app.route_3d(&ctx);
    assert_eq!(
        app.meshes.folder_items.get(PATH).map(String::as_str),
        Some("landscape/leaf.png")
    );
    assert!(!app.meshes.folder_items.contains_key(OUTSIDE));
    app.meshes.open_in_textures = Some("landscape/leaf.png".into());
    app.route_3d(&ctx);
    assert!(!sdc_gui::ui::header::meshes_view_active(&ctx));
    assert_eq!(
        app.model.selection,
        Selection::Item("landscape/leaf.png".into())
    );
    assert_eq!(
        app.model.nav.reveal_item.as_deref(),
        Some("landscape/leaf.png")
    );
    assert!(app.meshes.open_in_textures.is_none());
}
