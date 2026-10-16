mod support;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::{AppModel, Selection};
use sdc_gui::settings::GuiSettings;
use support::*;

/// A harness over `model`. The crate bundles no fonts, so the harness needs the system fonts, as the app does.
fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_empty_window_shows_its_regions() {
    let mut h = harness(AppModel::placeholder(GuiSettings::default()));
    h.run();
    for label in [
        "Open folder…",
        "Mode",
        "Export",
        "Save",
        "Textures",
        "Sets",
        "Advanced",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label_contains("Open a folder");
}

#[test]
fn an_opened_folder_shows_items_filters_and_the_undecided_count() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    for label in [
        "leaf.png",
        "rock01_normal.png",
        "Needs decision",
        "Not in a set",
    ] {
        h.get_by_label_contains(label);
    }
    // The packed output's tile, and the two raw maps that say they are packed into it.
    assert_eq!(h.query_all_by_label_contains("rock01_rmaos").count(), 3);
    h.get_by_label_contains("need a decision");
    // A set is named by its path under the export folder.
    h.get_by_label("landscape/rock01");
}

#[test]
fn collapsing_the_columns_hides_the_thumbnails() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.settings.textures_collapsed = true;
    model.settings.sets_collapsed = true;
    let mut h = harness(model);
    h.run();
    assert!(h.query_by_label_contains("leaf.png").is_none());
}

#[test]
fn clicking_a_tile_selects_it_and_the_needs_decision_filter_narrows_the_grid() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    h.get_by_label_contains("rock01_normal.png").click();
    h.run();
    assert!(
        matches!(&h.state().selection, Selection::Item(k) if k.ends_with("rock01_normal.png")),
        "selection was {:?}",
        h.state().selection
    );
    h.get_by_label("Needs decision").click();
    h.run();
    assert!(h.state().filter.needs_decision);
    h.get_by_label_contains("leaf.png");
    assert!(h.query_by_label_contains("rock01_roughness.png").is_none());
}

#[test]
fn closing_with_unsaved_edits_asks_and_discard_drops_them() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.set_target(engine::config::Target::Vanilla);
    assert!(model.is_dirty());
    assert!(!model.request_close());
    let mut h = harness(model);
    h.run();
    h.get_by_label("Cancel");
    // egui hit-tests against the previous frame: let the pointer settle on the button before the click.
    h.get_by_label("Discard").hover();
    h.run();
    h.get_by_label("Discard").click();
    h.run();
    assert!(!h.state().is_dirty());
    assert!(h.state().dialog.is_none());
}

#[test]
fn the_columns_keep_their_width_through_a_collapse_and_expand() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    // The Sets column starts where the Textures column ends.
    let before = h.get_by_label("Sets").rect().min.x;
    assert!(before > 300.0, "Sets starts at {before}");
    h.state_mut().settings.textures_collapsed = true;
    h.state_mut().settings.sets_collapsed = true;
    h.run();
    assert!(h
        .query_by_label("Sets")
        .is_some_and(|n| n.rect().width() < 30.0));
    h.state_mut().settings.textures_collapsed = false;
    h.state_mut().settings.sets_collapsed = false;
    h.run();
    let after = h.get_by_label("Sets").rect().min.x;
    assert!(after > 300.0, "Sets starts at {after} after expanding");
}

#[test]
fn selecting_a_texture_shows_its_decisions_and_groups() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.select_item("landscape/rock01_normal.png");
    let mut h = harness(model);
    h.run();
    for label in [
        "Needs a decision",
        "IN-GREEN-UNSTATED",
        "Colour",
        "Content",
        "DDS tag",
        "Compression",
    ] {
        h.get_by_label_contains(label);
    }
    // One frame with a Kaiser profile selected: the live Filter combo and the Edge row draw.
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.decide(
        "landscape/rock01.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    model.set_field(
        "landscape/rock01.png",
        "mips.filter",
        serde_json::json!("kaiser"),
    );
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/rock01.png");
    let mut h = harness(model);
    h.run();
    h.get_by_value("Kaiser");
    h.get_by_label("Edge");
}

#[test]
fn selecting_a_set_shows_its_recipe() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let key = model
        .sets
        .iter()
        .find(|s| s.key.ends_with("rock01"))
        .unwrap()
        .key
        .clone();
    model.select_set(&key);
    let mut h = harness(model);
    h.run();
    // The recipe line names the packed output and its status; the Sets column's pill says only "rmaos: run".
    h.get_by_label_contains("rock01_rmaos.dds: run");
}

fn settle(h: &mut Harness<'static, AppModel>) {
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
    h.run();
}

/// Click `label` the way a pointer does: settle on it first, because egui hit-tests against the previous frame.
fn press(h: &mut Harness<'static, AppModel>, label: &str) {
    h.get_by_label(label).hover();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

#[test]
fn the_opengl_button_on_the_unstated_green_line_writes_an_explicit_value() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.select_item("landscape/rock01_normal.png");
    let mut h = harness(model);
    h.run();
    press(&mut h, "Treat as OpenGL (flip green)");
    settle(&mut h);
    let item = h.state().item("landscape/rock01_normal.png").unwrap();
    let job = item.job.as_ref().unwrap();
    assert_eq!(
        job.profile.channels.green_convention,
        Some(engine::config::GreenConvention::Opengl)
    );
    assert!(h.state().is_dirty());
    // The assumption line and its button are gone; the sub-type question is still open, so the box stays.
    assert!(h.query_by_label("Treat as OpenGL (flip green)").is_none());
    h.get_by_label_contains("Needs a decision");
}

#[test]
fn an_alpha_button_writes_the_policy_and_the_reset_button_takes_it_back() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    // A plain diffuse takes its alpha meaning from the mesh, so once the sub-type is decided the alpha is asked.
    model.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/leaf.png");
    let mut h = harness(model);
    h.run();
    h.get_by_label_contains("Needs a decision");
    // The decision box offers the five meanings as buttons.
    for label in ["Remove", "Blend", "Cutout + Blend", "Data"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label}");
    }
    let cutout = h
        .query_all_by_label("Cutout")
        .next()
        .expect("a Cutout button");
    cutout.hover();
    h.run();
    h.query_all_by_label("Cutout").next().unwrap().click();
    h.run();
    settle(&mut h);
    let job = h
        .state()
        .item("landscape/leaf.png")
        .unwrap()
        .job
        .clone()
        .unwrap();
    assert_eq!(
        job.profile.alpha.policy,
        engine::config::AlphaPolicy::Cutout
    );
    assert_eq!(job.provenance["alpha.policy"].to_string(), "project_file");
    assert!(h.query_by_label_contains("Needs a decision").is_none());
    // The edited field offers a reset.
    // Two fields are edited now (sub-type, then alpha): the second reset is the alpha one.
    assert_eq!(h.query_all_by_label("↺").count(), 2);
    h.query_all_by_label("↺").nth(1).unwrap().hover();
    h.run();
    h.query_all_by_label("↺").nth(1).unwrap().click();
    h.run();
    settle(&mut h);
    let item = h.state().item("landscape/leaf.png").unwrap();
    assert_ne!(
        item.job.as_ref().unwrap().provenance["alpha.policy"].to_string(),
        "project_file"
    );
    assert!(!item.decisions.is_empty(), "the decision is open again");
}

#[test]
fn the_threshold_commits_when_the_drag_stops_not_while_dragging() {
    use eframe::egui::{Event, Modifiers, PointerButton};
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.decide(
        "landscape/leaf.png",
        sdc_gui::model::Answer::Alpha(engine::config::AlphaPolicy::Cutout),
    );
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/leaf.png");
    let mut h = harness(model);
    h.run();
    let before = h
        .state()
        .item("landscape/leaf.png")
        .unwrap()
        .job
        .as_ref()
        .unwrap()
        .profile
        .alpha
        .threshold;
    let rect = h
        .get_by_role_and_label(eframe::egui::accesskit::Role::Slider, "Alpha threshold")
        .rect();
    let at = |f: f32| {
        eframe::egui::pos2(
            rect.left() + 10.0 + (rect.width() - 20.0) * f,
            rect.center().y,
        )
    };
    let ev = |h: &mut Harness<'static, AppModel>, e: Event| {
        h.input_mut().events.push(e);
        h.step();
    };
    ev(&mut h, Event::PointerMoved(at(0.2)));
    ev(
        &mut h,
        Event::PointerButton {
            pos: at(0.2),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    );
    ev(&mut h, Event::PointerMoved(at(0.5)));
    ev(&mut h, Event::PointerMoved(at(0.8)));
    // Still dragging: nothing was sent to the model, so no re-plan is running.
    assert!(!h.state().busy(), "a re-plan started mid-drag");
    ev(
        &mut h,
        Event::PointerButton {
            pos: at(0.8),
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    );
    h.run();
    settle(&mut h);
    let job = h
        .state()
        .item("landscape/leaf.png")
        .unwrap()
        .job
        .clone()
        .unwrap();
    assert_ne!(job.profile.alpha.threshold, before);
    assert_eq!(
        job.provenance["alpha.threshold"].to_string(),
        "project_file"
    );
}

#[test]
fn a_packed_item_lists_its_channels_and_a_raw_map_points_to_the_output() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let packed = model
        .items
        .iter()
        .find(|i| i.kind == sdc_gui::model::items::ItemKind::Packed)
        .unwrap()
        .key
        .clone();
    model.select_item(&packed);
    let mut h = harness(model);
    h.run();
    h.get_by_label("Channels");
    h.get_by_label_contains("R: from ");
    h.get_by_label_contains("B: from rock01_ao.png");
    h.get_by_label("A: default value 1");
    // A raw map says it is packed and offers the way to its output.
    h.state_mut().select_item("landscape/rock01_roughness.png");
    h.run();
    press(&mut h, "Show packed output");
    assert_eq!(h.state().selection, Selection::Item(packed));
}

#[test]
fn a_locked_field_is_disabled_and_says_why() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.set_target(engine::config::Target::Vanilla);
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/rock01.png");
    let mut h = harness(model);
    h.run();
    h.get_by_label("_SRGB is only written under cs");
}

#[test]
fn export_asks_about_undecided_textures_then_the_drawer_reports() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d.path()));
    h.run();
    press(&mut h, "Export");
    h.get_by_label_contains("still need a decision");
    h.get_by_label("Accept all suggestions");
    h.get_by_label("Proceed anyway");
    assert!(h.query_all_by_label("\u{2713}").count() >= 1);
    // Cancel closes the question and starts nothing.
    press(&mut h, "Cancel");
    assert!(h.state().dialog.is_none());
    assert!(!h.state().export.open);
    press(&mut h, "Export");
    press(&mut h, "Proceed anyway");
    assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(120)));
    h.run();
    for label in [
        "Files",
        "Warnings",
        "Save report…",
        "Copy CLI command",
        "Close",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label_contains("converted, ");
    assert!(
        h.query_all_by_label_contains("skipped: needs decision")
            .count()
            >= 1
    );
    press(&mut h, "Warnings");
    press(&mut h, "Close");
    assert!(!h.state().export.open);
    assert!(h.query_by_label("Copy CLI command").is_none());
}

#[test]
fn the_expanded_preview_offers_encode_now_and_collapses_on_escape() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    h.get_by_label_contains("Not encoded");
    // The toolbar's button and the one on the empty encoded side.
    assert_eq!(h.query_all_by_label("Encode now").count(), 2);
    h.get_by_label("Live encode");
    h.get_by_label_contains("mip 0");
    h.key_press(eframe::egui::Key::Escape);
    h.run();
    assert!(h.state().expanded.is_none());
}

#[test]
fn encode_now_in_the_preview_encodes_and_shows_the_readouts() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    h.query_all_by_label("Encode now").next().unwrap().hover();
    h.run();
    h.query_all_by_label("Encode now").next().unwrap().click();
    h.run();
    settle(&mut h);
    assert!(h.query_by_label_contains("Not encoded").is_none());
    h.get_by_label("Minimum mip PSNR");
    // Step to another mip with the slider's keyboard control: the label follows.
    h.state_mut().view.mip = 2;
    h.run();
    h.get_by_label_contains("mip 2 (16×16)");
}

#[test]
fn the_preview_toolbar_changes_the_view() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    press(&mut h, "G");
    assert_eq!(h.state().view.channel, sdc_gui::model::view::Channel::G);
    press(&mut h, "Split");
    assert_eq!(h.state().view.layout, sdc_gui::model::view::Layout::Split);
    press(&mut h, "UNORM");
    assert!(h.state().view.view_linear);
    press(&mut h, "Live encode");
    assert!(h.state().preview.live);
}

#[test]
fn a_double_click_in_a_viewport_collapses_the_preview() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    // Two clicks must land within egui's double-click delay: shorten the simulated frame.
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    // A point inside the encoded viewport, below its message.
    let label = h.get_by_label_contains("Not encoded").rect();
    let at = label.center() + eframe::egui::vec2(0.0, 100.0);
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        h.event(eframe::egui::Event::PointerButton {
            pos: at,
            button: eframe::egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.event(eframe::egui::Event::PointerButton {
            pos: at,
            button: eframe::egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
    }
    h.run();
    assert!(h.state().expanded.is_none());
}

#[test]
fn the_settings_strip_is_compact() {
    let d = tempfile::tempdir().unwrap();
    // The full panel names where each value comes from.
    let mut model = opened(d.path());
    model.select_item("landscape/rock01_rmaos");
    let mut h = harness(model);
    h.run();
    assert!(h.query_all_by_label_contains("built-in default").count() > 0);
    // The strip keeps Alpha, Mipmaps and Compression and drops that text.
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    for group in ["Alpha", "Mipmaps", "Compression"] {
        assert!(h.query_all_by_label_contains(group).count() > 0, "{group}");
    }
    assert!(h.query_by_label_contains("Colour").is_none());
    assert_eq!(h.query_all_by_label_contains("built-in default").count(), 0);
}

#[test]
fn an_edit_after_encoding_shows_the_stale_badge() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    model.encode_now();
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.set_field(
        "landscape/rock01_rmaos",
        "encoder.effort",
        serde_json::json!("fast"),
    );
    let mut h = harness(model);
    settle(&mut h);
    h.get_by_label_contains("stale");
    assert!(h.query_by_label_contains("Not encoded").is_none());
}

#[test]
fn a_changed_source_offers_reload_which_plans_again() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let p = model.root.clone().unwrap().join("landscape/rock01.png");
    image::ImageBuffer::from_fn(32, 32, |_, _| image::Rgb([9u8, 9, 9]))
        .save(&p)
        .unwrap();
    assert_eq!(model.check_sources(), 1);
    let mut h = harness(model);
    h.run();
    h.get_by_label_contains("1 changed on disk");
    press(&mut h, "Reload");
    settle(&mut h);
    assert!(h.query_by_label_contains("changed on disk").is_none());
}

#[test]
fn without_a_gpu_the_preview_says_so_and_cannot_encode() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.gpu_available = false;
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    h.get_all_by_label_contains("No usable GPU: the preview is off; everything else works")
        .next()
        .unwrap();
    // The toolbar's button stays, disabled; the pane offers none.
    assert_eq!(h.query_all_by_label("Encode now").count(), 1);
    assert!(h.get_by_label("Encode now").accesskit_node().is_disabled());
}

#[test]
fn closing_during_an_export_asks_and_keep_running_leaves_it_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.export_clicked();
    model.proceed_anyway();
    assert!(!model.request_close());
    let mut h = harness(model);
    h.run();
    h.get_by_label_contains("An export is running");
    h.get_by_label("Cancel export and continue");
    press(&mut h, "Keep running");
    assert!(h.state().dialog.is_none());
    assert!(!h.state().export.running.as_ref().unwrap().is_cancelled());
    // Asking again and cancelling sets the token.
    assert!(!h.state_mut().request_close());
    h.run();
    press(&mut h, "Cancel export and continue");
    assert!(h.state().export.running.as_ref().unwrap().is_cancelled());
    settle(&mut h);
    assert!(h.state_mut().take_close_ready());
}

#[test]
fn the_header_shows_the_effective_export_folder() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let mut model = AppModel::new(GuiSettings::default(), None, session());
    model.accept = engine::policy::AcceptSuggestions::None;
    model.open_folder(root.clone());
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    let want = format!("Export folder: {}", root.join("sdc-out").display());
    h.get_by_label(&want);
    assert!(h.query_by_label_contains("not set").is_none());
}

#[test]
fn export_is_disabled_while_sdc_toml_is_ignored() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    std::fs::write(root.join("sdc.toml"), "target = [\n").unwrap();
    let settings = GuiSettings {
        export_folder: Some(d.path().join("export")),
        ..GuiSettings::default()
    };
    let mut model = AppModel::new(settings, None, session());
    model.accept = engine::policy::AcceptSuggestions::None;
    model.open_folder(root);
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    assert!(h.get_by_label("Export").accesskit_node().is_disabled());
    // A healthy folder has it enabled.
    let d2 = tempfile::tempdir().unwrap();
    let mut h = harness(opened(d2.path()));
    h.run();
    assert!(!h.get_by_label("Export").accesskit_node().is_disabled());
}

#[test]
fn a_double_click_on_a_set_member_expands_it() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let key = model
        .sets
        .iter()
        .find(|s| s.key.ends_with("rock01"))
        .unwrap()
        .key
        .clone();
    model.select_set(&key);
    // Two clicks must land within egui's double-click delay: shorten the simulated frame.
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    // The member name sits under its thumbnail; the same name is also a tile in the Textures column to the left.
    let label = h
        .query_all_by_label_contains("rock01_normal.png")
        .map(|n| n.rect())
        .max_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the member in the set view");
    let at = label.center() - eframe::egui::vec2(0.0, 30.0);
    h.hover_at(at);
    h.run();
    assert!(h.state().expanded.is_none());
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(eframe::egui::Event::PointerButton {
                pos: at,
                button: eframe::egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
    }
    h.run();
    assert_eq!(
        h.state().expanded.as_deref(),
        Some("landscape/rock01_normal.png")
    );
}

#[test]
fn a_single_click_on_a_set_member_selects_it_after_the_double_click_delay() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let key = model
        .sets
        .iter()
        .find(|s| s.key.ends_with("rock01"))
        .unwrap()
        .key
        .clone();
    model.select_set(&key);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let label = h
        .query_all_by_label_contains("rock01_normal.png")
        .map(|n| n.rect())
        .max_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the member in the set view");
    let at = label.center() - eframe::egui::vec2(0.0, 30.0);
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(eframe::egui::Event::PointerButton {
            pos: at,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    h.step();
    assert_eq!(h.state().selection, Selection::Set(key), "not yet");
    for _ in 0..40 {
        h.step();
    }
    assert_eq!(
        h.state().selection,
        Selection::Item("landscape/rock01_normal.png".into())
    );
    assert!(h.state().expanded.is_none());
}

#[test]
fn a_stale_pending_member_click_does_not_replace_a_set_opened_later() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    let key = model
        .sets
        .iter()
        .find(|s| s.key.ends_with("rock01"))
        .unwrap()
        .key
        .clone();
    model.select_set(&key);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1280.0, 800.0))
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model),
            model,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    let label = h
        .query_all_by_label_contains("rock01_normal.png")
        .map(|n| n.rect())
        .max_by(|a, b| a.min.x.total_cmp(&b.min.x))
        .expect("the member in the set view");
    let at = label.center() - eframe::egui::vec2(0.0, 30.0);
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(eframe::egui::Event::PointerButton {
            pos: at,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    h.step();
    // Something else is selected before the wait is over: the set view is no longer drawn.
    h.state_mut().select_item("landscape/leaf.png");
    for _ in 0..120 {
        h.step();
    }
    h.state_mut().select_set(&key);
    for _ in 0..10 {
        h.step();
    }
    assert_eq!(h.state().selection, Selection::Set(key));
}

#[test]
fn the_preview_has_a_close_button_that_collapses_it() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/rock01_rmaos");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    h.get_by_label("✕ Close");
    press(&mut h, "✕ Close");
    assert!(h.state().expanded.is_none());
}

#[test]
fn the_export_button_is_big_enough_to_find() {
    let mut h = harness(AppModel::placeholder(GuiSettings::default()));
    h.run();
    let r = h.get_by_label("Export").rect();
    assert!(r.width() >= 110.0 && r.height() >= 28.0, "{r:?}");
}

fn leaf_with_policy(policy: engine::config::AlphaPolicy) -> Harness<'static, AppModel> {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.decide("landscape/leaf.png", sdc_gui::model::Answer::Alpha(policy));
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/leaf.png");
    // The temp folder is only read while the model opens; the harness keeps the model.
    std::mem::forget(d);
    let mut h = harness(model);
    h.run();
    h
}

fn mips_levels(h: &Harness<'static, AppModel>) -> serde_json::Value {
    let job = h.state().item("landscape/leaf.png").unwrap().job.clone();
    serde_json::to_value(job.unwrap().profile.mips.levels).unwrap()
}

#[test]
fn the_mipmaps_tick_box_writes_none_and_full() {
    use eframe::egui::accesskit::Role;
    let mut h = leaf_with_policy(engine::config::AlphaPolicy::Cutout);
    assert_eq!(mips_levels(&h), serde_json::json!("full"));
    let tick = |h: &mut Harness<'static, AppModel>| {
        h.get_by_role_and_label(Role::CheckBox, "Mipmaps").hover();
        h.run();
        h.get_by_role_and_label(Role::CheckBox, "Mipmaps").click();
        h.run();
        settle(h);
    };
    tick(&mut h);
    assert_eq!(mips_levels(&h), serde_json::json!("none"));
    let item = h.state().item("landscape/leaf.png").unwrap();
    assert_eq!(
        item.job.as_ref().unwrap().provenance["mips.levels"].to_string(),
        "project_file"
    );
    tick(&mut h);
    assert_eq!(mips_levels(&h), serde_json::json!("full"));
}

#[test]
fn a_mip_count_shows_ticked_with_a_note() {
    use eframe::egui::accesskit::Role;
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.set_field("landscape/leaf.png", "mips.levels", serde_json::json!(3));
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/leaf.png");
    let mut h = harness(model);
    h.run();
    let tick = h.get_by_role_and_label(Role::CheckBox, "Mipmaps");
    assert_eq!(
        tick.accesskit_node().toggled(),
        Some(eframe::egui::accesskit::Toggled::True)
    );
    h.get_by_label("(3 levels — see Advanced)");
}

#[test]
fn levels_live_in_the_advanced_drawer_not_the_main_panel() {
    let mut h = leaf_with_policy(engine::config::AlphaPolicy::Cutout);
    assert!(h.query_by_label("Levels").is_none());
    h.state_mut().settings.advanced_open = true;
    h.run();
    h.get_by_label("Levels");
    // The drawer adds its own "Mipmaps" section heading next to the group heading and the tick box.
    assert!(h.query_all_by_label("Mipmaps").count() >= 3);
}

#[test]
fn coverage_sits_under_alpha_for_cutout_policies_only() {
    use eframe::egui::accesskit::Role;
    let h = leaf_with_policy(engine::config::AlphaPolicy::Cutout);
    let y = |label: &str| h.get_by_label(label).rect().top();
    let tick_y = h
        .get_by_role_and_label(Role::CheckBox, "Mipmaps")
        .rect()
        .top();
    assert!(y("Coverage") > y("Comparison"), "directly under Comparison");
    assert!(y("Coverage") < tick_y, "inside the Alpha group");
    let h = leaf_with_policy(engine::config::AlphaPolicy::Blend);
    assert!(h.query_by_label("Coverage").is_none());
    h.get_by_label("Threshold"); // locked, but still listed
}

#[test]
fn the_filter_row_is_a_live_combo_in_the_mipmaps_group() {
    let h = leaf_with_policy(engine::config::AlphaPolicy::Remove);
    h.get_by_label("Filter");
    assert!(!h.get_by_value("Box").accesskit_node().is_disabled());
}

#[test]
fn an_8_bit_emissive_says_why_it_is_bc7() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/pbr");
    std::fs::create_dir_all(&t).unwrap();
    image::ImageBuffer::from_fn(16, 16, |x, _| image::Rgb([(x * 16) as u8, 40u8, 20]))
        .save(t.join("lamp_g.png"))
        .unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(d.path().join("textures"));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.select_item("pbr/lamp_g.png");
    let mut h = harness(m);
    h.run();
    // Exact and unique: shown once, under Compression, without the code prefix of the Warnings list.
    h.get_by_label("8-bit source, no HDR → BC7 sRGB");
}

/// Controller ruling F2 (M6 Task 11): an RGBA16F preview is tone-mapped, so its sRGB / UNORM toggle is disabled.
#[test]
fn an_rgba16f_preview_disables_the_srgb_unorm_toggle() {
    use engine::config::Format;
    use engine::dds::*;
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let mut levels = vec![];
    let mut s = 16u32;
    loop {
        levels.push(vec![0u8; level_size(Format::Rgba16f, s, s)]);
        if s == 1 {
            break;
        }
        s /= 2;
    }
    let desc = DdsDesc {
        width: 16,
        height: 16,
        format: Format::Rgba16f,
        srgb: false,
        header: ResolvedHeader::Dx10,
    };
    std::fs::write(
        root.join("landscape/sky.dds"),
        dds_bytes(&desc, &levels).unwrap(),
    )
    .unwrap();
    let mut model = opened(d.path());
    model.expand("landscape/sky.dds");
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = harness(model);
    h.run();
    assert!(matches!(
        &h.state().preview.encoded,
        sdc_gui::model::preview::EncodedState::Done(o) if o.result.format == Format::Rgba16f
    ));
    // The header panel's tag radios are also labelled sRGB / UNORM: take the toolbar's toggles.
    let radio = eframe::egui::accesskit::Role::RadioButton;
    for label in ["sRGB", "UNORM"] {
        let toggles: Vec<_> = h
            .query_all_by_label(label)
            .filter(|n| n.accesskit_node().role() != radio)
            .collect();
        assert_eq!(toggles.len(), 1, "{label}");
        assert!(toggles[0].accesskit_node().is_disabled(), "{label}");
    }
}

#[test]
fn a_dds_with_undeclared_mips_offers_a_header_patch_and_writes_a_copy() {
    let d = tempfile::tempdir().unwrap();
    let (src, b) = broken_dds(&mod_folder(d.path()));
    let mut m = opened(d.path());
    m.select_item("landscape/broken.dds");
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("mip count 1 → 7");
    press(&mut h, "Patch header…");
    press(&mut h, "Patch");
    let out = d.path().join("export/landscape/broken.dds");
    let patched = std::fs::read(&out).expect("a patched copy in the export folder");
    assert_eq!(&patched[148..], &b[148..]);
    assert_eq!(std::fs::read(&src).unwrap(), b, "the source is untouched");
    h.get_by_label_contains("DDS-HEADER-PATCHED");
}

/// A full-chain 64x64 BC7 DDS at `<root>/landscape/broken.dds` whose header declares 1 mip; returns (path, bytes).
fn broken_dds(root: &std::path::Path) -> (std::path::PathBuf, Vec<u8>) {
    use engine::config::Format;
    use engine::dds::*;
    let mut levels = vec![];
    let mut s = 64u32;
    loop {
        levels.push(vec![0u8; level_size(Format::Bc7, s, s)]);
        if s == 1 {
            break;
        }
        s /= 2;
    }
    let mut b = dds_bytes(
        &DdsDesc {
            width: 64,
            height: 64,
            format: Format::Bc7,
            srgb: false,
            header: ResolvedHeader::Dx10,
        },
        &levels,
    )
    .unwrap();
    b[28..32].copy_from_slice(&1u32.to_le_bytes());
    let src = root.join("landscape/broken.dds");
    std::fs::write(&src, &b).unwrap();
    (src, b)
}

/// Fix round 1: with no export folder set, Patch writes to `<root>/sdc-out` (what the header shows) and ends the
/// confirmation; the outcome is in `dds_patch.last`.
#[test]
fn patch_without_an_export_folder_writes_to_sdc_out() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let (src, b) = broken_dds(&root);
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root.clone());
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    m.dds_patch.confirming = Some("landscape/broken.dds".into());
    let out = m.patch_dds_header("landscape/broken.dds").unwrap();
    assert_eq!(out, root.join("sdc-out/landscape/broken.dds"));
    assert_eq!(&std::fs::read(&out).unwrap()[148..], &b[148..]);
    assert_eq!(std::fs::read(&src).unwrap(), b);
    assert!(m.dds_patch.confirming.is_none());
    assert!(m
        .dds_patch
        .last_for("landscape/broken.dds")
        .unwrap()
        .starts_with("DDS-HEADER-PATCHED"));
    // A key that is not a DDS item: the error is shown, not dropped.
    m.dds_patch.confirming = Some("landscape/rock01.png".into());
    assert!(m.patch_dds_header("landscape/rock01.png").is_err());
    assert_eq!(
        m.dds_patch.last_for("landscape/rock01.png"),
        Some("not a DDS item")
    );
    assert!(m.dds_patch.confirming.is_none());
}

/// Fix wave (final review I1, amended S7): Patch on `rock01.dds` next to `rock01.png` would write over rock01.png's
/// converted output (same output key). It is refused with a visible error and the exported bytes are unchanged.
/// The result line belongs to the item it was pressed on only (final review minor 8).
#[test]
fn patch_refuses_an_output_key_another_item_exports_to() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let (_, b) = broken_dds(&root);
    std::fs::write(root.join("landscape/rock01.dds"), &b).unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root.clone());
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let (dds, png) = ("landscape/rock01.dds", "landscape/rock01.png");
    let job_key = m.item(dds).unwrap().job_key.clone();
    assert_eq!(m.item(png).unwrap().job_key, job_key);
    let dest = m.export_root().join(&job_key);
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, b"rock01.png's output").unwrap();
    let e = m.patch_dds_header(dds).unwrap_err();
    assert!(e.contains("rock01.png"), "{e}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"rock01.png's output");
    assert_eq!(m.dds_patch.last_for(dds), Some(e.as_str()));
    assert_eq!(m.dds_patch.last_for("landscape/broken.dds"), None);
    // Re-patching a DDS whose own copy already exists is not refused.
    for _ in 0..2 {
        m.patch_dds_header("landscape/broken.dds").unwrap();
    }
    assert!(m
        .dds_patch
        .last_for("landscape/broken.dds")
        .unwrap()
        .starts_with("DDS-HEADER-PATCHED"));
    assert_eq!(m.dds_patch.last_for(dds), None);
}

/// Residual check (re-review minor 2): output keys that differ only in case name one file on Windows. `job_key` is
/// `manifest::output_key`, which case-folds (as `dds::same_file` does), so Patch on `Rock01.dds` next to
/// `rock01.png` is refused like the same-case pair. A regression guard: it passed before this change too.
#[test]
fn patch_refuses_an_output_key_that_differs_only_in_case() {
    let d = tempfile::tempdir().unwrap();
    let root = mod_folder(d.path());
    let (_, b) = broken_dds(&root);
    std::fs::write(root.join("landscape/Rock01.dds"), &b).unwrap();
    let mut m = AppModel::new(GuiSettings::default(), None, session());
    m.accept = engine::policy::AcceptSuggestions::None;
    m.open_folder(root.clone());
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let (dds, png) = ("landscape/Rock01.dds", "landscape/rock01.png");
    let (kd, kp) = (
        m.item(dds).unwrap().job_key.clone(),
        m.item(png).unwrap().job_key.clone(),
    );
    assert_eq!(kd, kp);
    let e = m.patch_dds_header(dds).unwrap_err();
    assert!(e.contains("rock01.png"), "{e}");
}

#[test]
fn the_dim_npot_box_offers_a_power_of_two_resize() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened(d.path());
    model.decide(
        "landscape/odd.png",
        sdc_gui::model::Answer::Subtype(engine::config::Subtype::Diffuse),
    );
    assert!(model.wait_idle(std::time::Duration::from_secs(60)));
    model.select_item("landscape/odd.png");
    let mut h = harness(model);
    h.run();
    h.get_by_label("Acknowledge for this texture");
    press(&mut h, "Resize to a power of two");
    settle(&mut h);
    let job = h.state().item("landscape/odd.png").unwrap().job.clone();
    assert_eq!(
        job.unwrap().profile.resize,
        engine::config::ResizeMode::PotDown
    );
    assert!(h.query_by_label("Resize to a power of two").is_none());
}

/// M6 Track E spec §2, §6: a sub-type decided by an accepted High suggestion reads "(auto)" next to it, with the
/// suggestion's reason as hover text.
#[test]
fn an_accepted_suggestion_reads_auto_with_its_reason_on_hover() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened_as_the_app(mod_folder(d.path()), d.path().join("export"));
    model.select_item("landscape/rock01.png");
    let mut h = harness(model);
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.run();
    assert!(
        h.query_by_label_contains("colour under textures/landscape")
            .is_none(),
        "the reason shows only on hover"
    );
    h.get_by_label("(auto)").hover();
    h.run();
    h.run();
    h.get_by_label_contains("colour under textures/landscape");
}
