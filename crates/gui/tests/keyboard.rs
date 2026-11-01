//! Task 4 (items 5 and 19): one action list for the keys, the F1 shortcut sheet and the Ctrl+K command palette.
mod support;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::nav::{rank, Pick, PALETTE_CAP};
use sdc_gui::model::view::Channel;
use sdc_gui::model::{AppModel, Selection};
use sdc_gui::settings::GuiSettings;
use sdc_gui::ui::actions::{self, actions, Area};
use std::time::Duration;
use support::*;

/// A UI that only reads the keys, as the app does first thing in a frame; the last action that ran is kept.
fn keys_only(m: AppModel) -> Harness<'static, (AppModel, Option<&'static str>)> {
    Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, (m, fired): &mut (AppModel, Option<&'static str>)| {
                if let Some(id) = actions::keys(ui.ctx(), m, false) {
                    *fired = Some(id);
                }
            },
            (m, None),
        )
}

fn press(h: &mut Harness<'static, (AppModel, Option<&'static str>)>, mods: Modifiers, key: Key) {
    h.state_mut().1 = None;
    h.key_press_modifiers(mods, key);
    h.run();
}

#[test]
fn the_action_table_has_unique_ids_and_shortcuts() {
    let all = actions();
    for (i, a) in all.iter().enumerate() {
        assert!(
            all[i + 1..].iter().all(|b| b.id != a.id),
            "id {} twice",
            a.id
        );
        for b in &all[i + 1..] {
            for s in a.keys {
                if b.keys.contains(s) {
                    assert!(
                        a.area.exclusive_with(b.area),
                        "{} and {} share {} in areas that are live together",
                        a.id,
                        b.id,
                        s.text()
                    );
                }
            }
        }
    }
    // The shortcuts that existed before the table are in it.
    for (id, text) in [
        ("save", "Ctrl+S"),
        ("undo", "Ctrl+Z"),
        ("redo", "Ctrl+Y or Ctrl+Shift+Z"),
        ("select-all", "Ctrl+A"),
        ("clear", ""),
        ("rename", "F2"),
        ("close-preview", "Esc"),
        ("problems-accept", "Enter or Space"),
        ("dialog-enter", "Enter"),
        ("dialog-esc", "Esc"),
        ("palette", "Ctrl+K"),
        ("sheet", "F1 or ?"),
    ] {
        assert_eq!(actions::keys_text(id), text, "{id}");
    }
}

#[test]
fn every_shortcut_runs_its_action() {
    let mut h = keys_only(AppModel::placeholder(GuiSettings::default()));
    let mut checked = 0;
    for a in actions() {
        if a.run.is_none() {
            // A dialog's keys, the 3D panel's, the JSON editor's and a Problems row's are handled where they belong (the 3D viewports are not in AppModel).
            assert!(
                matches!(
                    a.area,
                    Area::Dialogs | Area::View3d | Area::JsonEditor | Area::Problems
                ),
                "{} is only listed",
                a.id
            );
            continue;
        }
        for s in a.keys {
            {
                let m = &mut h.state_mut().0;
                m.dialog = None;
                m.nav.palette = None;
                m.nav.sheet = false;
                m.expanded = (a.area == Area::Preview).then(|| "x".to_string());
            }
            // A key of a disabled action does nothing (the palette hides it as well).
            let want = (a.enabled)(&h.state().0).then_some(a.id);
            press(&mut h, s.mods, s.key);
            assert_eq!(h.state().1, want, "{}", s.text());
            checked += 1;
        }
    }
    assert!(checked >= 30, "{checked}");
}

#[test]
fn no_shortcut_fires_from_a_text_field_except_ctrl_k_and_ctrl_s() {
    let mut m = AppModel::placeholder(GuiSettings::default());
    m.select_item("a");
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, (m, fired, text): &mut (AppModel, Option<&'static str>, String)| {
                if let Some(id) = actions::keys(ui.ctx(), m, false) {
                    *fired = Some(id);
                }
                let r = ui.text_edit_singleline(text);
                if !r.has_focus() {
                    r.request_focus();
                }
            },
            (m, None, "hello".to_string()),
        );
    h.run();
    h.run();
    let (mut tried, mut palette) = (0, 0);
    for a in actions() {
        for s in a.keys {
            h.state_mut().1 = None;
            h.state_mut().0.nav.palette = None;
            h.run();
            if a.id == actions::PALETTE {
                // The cursor at the start: Ctrl+K reaching the field would delete the whole text.
                h.key_press(Key::Home);
                h.run();
            }
            h.key_press_modifiers(s.mods, s.key);
            h.run();
            // Note FC8: Ctrl+S commits the field and saves (`ctrl_s_in_a_field_saves`).
            let want = match a.id {
                actions::PALETTE => Some(actions::PALETTE),
                "save" => Some("save"),
                _ => None,
            };
            assert_eq!(h.state().1, want, "{} from a text field", s.text());
            tried += 1;
            palette += usize::from(h.state().1 == Some(actions::PALETTE));
        }
    }
    assert!(
        tried >= 20,
        "letter, Ctrl and Shift shortcuts were tried: {tried}"
    );
    assert_eq!(palette, 1, "only Ctrl+K fired");
    assert_eq!(h.state().2, "hello", "Ctrl+K never reached the field");
    assert_eq!(
        h.state().0.selection,
        Selection::Item("a".into()),
        "Esc left the field and did not clear the selection"
    );
}

/// Ten textures; the search shows the even ones: t00, t02, t04, t06, t08.
fn ten(dir: &std::path::Path) -> AppModel {
    let mut m = opened(dir);
    let base = m.items[0].clone();
    m.items = (0..10)
        .map(|n| {
            let tag = if n % 2 == 0 { "keep" } else { "drop" };
            let mut i = base.clone();
            i.key = format!("t/t{n:02}_{tag}");
            i.name = format!("t{n:02}_{tag}.png");
            i
        })
        .collect();
    m.invalidate_views();
    m.search = "keep".into();
    m
}

fn k(n: u32) -> String {
    format!("t/t{n:02}_keep")
}

#[test]
fn grid_arrows_move_in_the_displayed_order_and_shift_extends() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ten(d.path());
    m.nav.cols = 2;
    let mut h = keys_only(m);
    let sel = |h: &Harness<'static, (AppModel, Option<&'static str>)>| h.state().0.selected_items();
    press(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(sel(&h), vec![k(0)], "nothing selected: the first texture");
    press(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(sel(&h), vec![k(2)], "t01 is hidden by the search");
    press(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(sel(&h), vec![k(6)], "down a row of two");
    assert_eq!(h.state().0.nav.reveal_item, Some(k(6)), "scrolled to");
    press(&mut h, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(sel(&h), vec![k(2)]);
    press(&mut h, Modifiers::NONE, Key::ArrowLeft);
    press(&mut h, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(sel(&h), vec![k(0)], "off the grid: no move");
    press(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(h.state().1, Some("extend-right"));
    assert_eq!(sel(&h), vec![k(0), k(2)]);
    press(&mut h, Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(sel(&h), vec![k(0), k(2), k(4), k(6)], "from the anchor");
    assert_eq!(
        h.state().0.selection,
        Selection::Item(k(0)),
        "the anchor stays active"
    );
    press(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(
        sel(&h),
        vec![k(8)],
        "a plain arrow moves on from the range's end"
    );
    press(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(sel(&h), vec![k(8)], "no row below");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(h.state().0.expanded, Some(k(8)), "Enter opens the preview");
    press(&mut h, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(
        sel(&h),
        vec![k(8)],
        "the arrows are the grid's, not the preview's"
    );
}

#[test]
fn preview_keys_step_textures_channels_and_mips() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ten(d.path());
    m.expand(&k(2));
    m.expand_step(1);
    assert_eq!(
        m.expanded,
        Some(k(4)),
        "] skips the texture the search hides"
    );
    m.expand_step(-1);
    m.expand_step(-1);
    assert_eq!(m.expanded, Some(k(0)));
    m.expand_step(-1);
    assert_eq!(m.expanded, Some(k(0)), "[ on the first does nothing");
    let order = [
        Channel::Rgba,
        Channel::R,
        Channel::G,
        Channel::B,
        Channel::A,
        Channel::AlphaMask,
    ];
    for (n, c) in order.into_iter().enumerate() {
        m.pick_channel(n as u32);
        assert_eq!(m.view.channel, c, "key {}", n + 1);
    }

    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.expand("landscape/rock01_rmaos");
    assert!(m.wait_idle(Duration::from_secs(120)));
    let (dims, first, end) = m.preview_mips();
    assert_eq!((first, end, dims[0]), (0, 7, (64, 64)));
    m.view.zoom = 1.0;
    m.step_mip(1);
    assert_eq!(m.view.mip, 1, "PageDown: the next, smaller mip");
    assert_eq!(
        m.view.zoom, 2.0,
        "the same part of the picture stays in view"
    );
    m.step_mip(-1);
    m.step_mip(-1);
    assert_eq!(m.view.mip, 0, "PageUp stops at the first mip");
    for _ in 0..10 {
        m.step_mip(1);
    }
    assert_eq!(m.view.mip, 6, "PageDown stops at the last");
}

#[test]
fn the_palette_ranks_prefix_then_substring_then_subsequence() {
    assert_eq!(rank("ro", "Rock.png"), Some(0));
    assert_eq!(rank("ro", "bedrock.png"), Some(1));
    assert_eq!(rank("ro", "r_o_x.png"), Some(2));
    assert_eq!(rank("ro", "zzz.png"), None);
    assert_eq!(rank("", "anything"), Some(0));

    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let base = m.items[0].clone();
    m.items = ["zzz.png", "r_o_x.png", "bedrock.png", "rock.png"]
        .iter()
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/{n}");
            i.name = n.to_string();
            i
        })
        .collect();
    m.invalidate_views();
    let acts = [
        ("a", "Rotate", String::new()),
        ("b", "Errors", String::new()),
    ];
    let hits = m.palette_hits("ro", &acts);
    assert_eq!(hits[0].pick, Pick::Action("a"), "an action prefix first");
    let textures: Vec<&str> = hits
        .iter()
        .filter(|h| matches!(h.pick, Pick::Texture(_)))
        .map(|h| h.label.as_str())
        .collect();
    assert_eq!(textures, ["rock.png", "bedrock.png", "r_o_x.png"]);
    assert!(
        hits.windows(2).all(|w| w[0].rank <= w[1].rank),
        "prefix, then substring, then subsequence: {hits:?}"
    );
    let errors = hits.iter().position(|h| h.label == "Errors").unwrap();
    let bedrock = hits.iter().position(|h| h.label == "bedrock.png").unwrap();
    assert!(
        errors < bedrock,
        "within a rank: actions, then textures, then sets"
    );
}

#[test]
fn the_palette_lists_at_most_200() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let base = m.items[0].clone();
    m.items = (0..10_000)
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/tex_{n}");
            i.name = format!("tex_{n}.png");
            i
        })
        .collect();
    m.invalidate_views();
    let t = std::time::Instant::now();
    assert_eq!(m.palette_hits("tex", &[]).len(), PALETTE_CAP);
    assert_eq!(PALETTE_CAP, 200);
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    // The next frame with the same query and plan reuses the search (a rename in place is not seen)...
    let first = |m: &mut AppModel, q: &str| m.palette_hits(q, &[])[0].label.clone();
    assert_eq!(first(&mut m, "tex_0"), "tex_0.png");
    m.items[0].name = "tex_0 renamed".into();
    assert_eq!(first(&mut m, "tex_0"), "tex_0.png");
    // ...until the plan (here its revision) changes.
    m.invalidate_views();
    assert_eq!(first(&mut m, "tex_0"), "tex_0 renamed");
}

fn app_harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn enter_in_the_palette_selects_the_texture() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app_harness(opened(d.path()));
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    assert!(h.state().nav.palette.is_some(), "Ctrl+K opens the palette");
    h.state_mut().nav.palette.as_mut().unwrap().query = "leaf".into();
    h.run();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().nav.palette.is_none(), "Enter closes it");
    assert_eq!(
        h.state().selection,
        Selection::Item("landscape/leaf.png".into()),
        "and selects the texture"
    );
    assert_eq!(h.state().nav.reveal_item, None, "the grid scrolled to it");
    // Down then Enter picks the second result at once (the arrows do not take the focus out of the field).
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.state_mut().nav.palette.as_mut().unwrap().query = "rock01".into();
    h.run();
    h.run();
    let second = h
        .state_mut()
        .palette_hits("rock01", &[])
        .get(1)
        .map(|h| h.pick.clone())
        .expect("two textures match");
    h.key_press(Key::ArrowDown);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().nav.palette.is_none(), "the first Enter picks");
    let Pick::Texture(k) = second else {
        panic!("{second:?}")
    };
    assert_eq!(h.state().selection, Selection::Item(k));
    // Esc closes the palette without picking.
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().nav.palette.is_none());
}

#[test]
fn f1_opens_the_sheet_and_it_lists_ctrl_z() {
    let d = tempfile::tempdir().unwrap();
    let mut h = app_harness(opened(d.path()));
    h.run();
    h.key_press(Key::F1);
    h.run();
    h.run();
    assert!(h.state().nav.sheet);
    h.get_by_label("Ctrl+Z");
    h.get_by_label("Textures grid");
    h.get_by_label("Dialogs");
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().nav.sheet, "Esc closes it");
    h.key_press(Key::Questionmark);
    h.run();
    assert!(h.state().nav.sheet, "? opens it too");
}

#[test]
fn the_user_guide_lists_every_shortcut() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/gui.md"),
    )
    .unwrap()
    .replace("\r\n", "\n");
    let table = actions::markdown_table();
    assert!(
        doc.contains(&table),
        "docs/gui.md's Keyboard table is out of date; it should read:\n{table}"
    );
}

#[test]
fn a_texture_moved_to_scrolls_into_view() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let base = m.items[0].clone();
    m.items = (0..300)
        .map(|n| {
            let mut i = base.clone();
            i.key = format!("t/tex_{n:03}");
            i.name = format!("tex_{n:03}.png");
            i
        })
        .collect();
    m.invalidate_views();
    let mut h = app_harness(m);
    h.run();
    let n = |h: &Harness<'static, AppModel>, l: &str| h.query_all_by_label(l).count();
    assert_eq!(n(&h, "tex_299.png"), 0, "off screen at first");
    h.state_mut()
        .palette_pick(&Pick::Texture("t/tex_299".into()));
    h.run();
    h.run();
    assert_eq!(
        n(&h, "tex_299.png"),
        2,
        "the grid scrolled to it (the tile and the settings panel)"
    );
    h.state_mut()
        .palette_pick(&Pick::Texture("t/tex_000".into()));
    h.run();
    h.run();
    assert_eq!(n(&h, "tex_000.png"), 2, "and back to the top");
    assert_eq!(n(&h, "tex_299.png"), 0);
}

#[test]
fn a_disabled_action_does_not_run_from_its_key() {
    let d = tempfile::tempdir().unwrap();
    let mut h = keys_only(opened(d.path()));
    assert!(!h.state().0.is_dirty() && !h.state().0.can_undo());
    press(&mut h, Modifiers::COMMAND, Key::S);
    assert_eq!(h.state().1, None, "Ctrl+S on a clean project");
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(h.state().1, None, "Ctrl+Z with no history");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(h.state().1, None, "Enter with no texture selected");
}

#[test]
fn up_and_down_wait_for_a_drawn_grid() {
    let d = tempfile::tempdir().unwrap();
    // Never drawn: nav.cols is 0, so Up and Down do nothing instead of stepping by one.
    let mut h = keys_only(ten(d.path()));
    press(&mut h, Modifiers::NONE, Key::ArrowRight);
    let at = h.state().0.selected_items();
    press(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(h.state().1, None);
    assert_eq!(h.state().0.selected_items(), at);
    // The Textures column drew: it knows its width.
    let mut h = app_harness(ten(d.path()));
    h.run();
    assert!(h.state().nav.cols >= 1, "the drawn grid sets the width");
    // Hidden: the next frame has no row width again.
    h.state_mut().settings.textures_hidden = true;
    h.run();
    h.run();
    assert_eq!(h.state().nav.cols, 0, "a hidden column has no rows");
}

/// Note FC8 (supersedes AG23/AG32's refusal): Ctrl+S in a text field ends the edit, so the field commits, and saves.
#[test]
fn ctrl_s_in_a_field_saves() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_field(
        "landscape/rock01.png",
        "alpha.policy",
        serde_json::json!("remove"),
    );
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(m.is_dirty());
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .build_ui_state(
            |ui, (m, text): &mut (AppModel, String)| {
                actions::keys(ui.ctx(), m, false);
                let r = ui.text_edit_singleline(text);
                if !r.has_focus() {
                    r.request_focus();
                }
            },
            (m, "x".to_string()),
        );
    h.run();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    h.run();
    assert!(!h.state().0.is_dirty(), "saved");
    assert!(!h
        .state()
        .0
        .messages
        .entries()
        .any(|e| e.text.contains("leave the text field first")));
}

/// Alpha of a modal backdrop: the last full-width black rect (egui multiplies an Area fade into its shapes). None until something is painted; a new Area first pass is an invisible sizing pass.
fn frame_alpha<S>(h: &Harness<'_, S>) -> Option<u8> {
    fn rects(s: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| rects(s, out)),
            egui::Shape::Rect(r) => out.push(r.clone()),
            _ => {}
        }
    }
    let mut all = Vec::new();
    h.output()
        .shapes
        .iter()
        .for_each(|c| rects(&c.shape, &mut all));
    all.iter()
        .rev()
        .find(|r| {
            r.rect.width() >= 1400.0
                && r.fill.to_srgba_unmultiplied()[..3] == [0; 3]
                && r.fill.a() > 0
        })
        .map(|r| r.fill.a())
}

/// Steps (20 ms each, so a 0.2 s fade cannot finish in one) until an overlay frame is painted; returns its alpha on
/// that first visible frame and once settled.
fn first_and_rest(h: &mut Harness<'static, AppModel>) -> (u8, u8) {
    for _ in 0..6 {
        h.step();
        if let Some(first) = frame_alpha(h) {
            h.run_steps(20);
            return (first, frame_alpha(h).expect("still painted"));
        }
    }
    panic!("no overlay was painted");
}

fn fast_app(m: AppModel) -> Harness<'static, AppModel> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 900.0))
        .with_step_dt(0.02)
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    // kittest zeroes the stock animation time, which would turn every Area fade off.
    h.ctx.global_style_mut(|s| s.animation_time = 0.2);
    h.run();
    h
}

/// Note CF3: an overlay opened from the keyboard is at its resting opacity on its first visible frame; one opened
/// with the mouse (the error dialog is the control) still fades in, so the pin can see a fade.
#[test]
fn the_palette_and_the_sheet_open_without_a_fade() {
    let d = tempfile::tempdir().unwrap();
    let mut h = fast_app(opened(d.path()));
    h.key_press(Key::F1);
    let (first, rest) = first_and_rest(&mut h);
    assert_eq!(first, rest, "the sheet faded in");
    h.key_press(Key::Escape);
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    let (first, rest) = first_and_rest(&mut h);
    assert_eq!(first, rest, "the palette faded in");
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().dialog = Some(sdc_gui::model::Dialog::Error("Boom".into()));
    let (first, rest) = first_and_rest(&mut h);
    assert!(first < rest, "the control must fade: {first} vs {rest}");
}

/// Owner amendment G (Note FB11): Esc closes things and never clears the selection.
#[test]
fn esc_keeps_the_selection() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.selection = Selection::Item("landscape/leaf.png".into());
    let mut h = app_harness(m);
    h.run();
    h.run();
    let panel = |h: &Harness<'static, AppModel>| {
        h.query_all_by_role_and_label(egui::accesskit::Role::ComboBox, "Sub-type")
            .count()
    };
    assert_eq!(panel(&h), 1, "the settings panel shows the texture");
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert_eq!(
        h.state().selection,
        Selection::Item("landscape/leaf.png".into())
    );
    assert_eq!(panel(&h), 1, "and still shows it after Esc");
}
#[test]
fn esc_still_closes() {
    let d = tempfile::tempdir().unwrap();
    // The expanded preview.
    let mut m = ten(d.path());
    m.selection = Selection::Item(k(2));
    m.expanded = Some(k(2));
    let mut h = keys_only(m);
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert_eq!(h.state().0.expanded, None, "Esc collapses the preview");
    assert_eq!(h.state().0.selection, Selection::Item(k(2)));
    // A dialog and a popup (the palette), in the whole app.
    let mut m = opened(d.path());
    m.selection = Selection::Item("landscape/leaf.png".into());
    let mut h = app_harness(m);
    h.run();
    h.state_mut().dialog = Some(sdc_gui::model::Dialog::Error("Boom".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().dialog.is_none(), "Esc closes the dialog");
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.run();
    assert!(h.state().nav.palette.is_some());
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().nav.palette.is_none(), "Esc closes the palette");
    h.key_press(Key::F1);
    h.run();
    h.run();
    assert!(h.state().nav.sheet, "F1 opens the sheet");
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().nav.sheet, "Esc closes the sheet");
    assert_eq!(
        h.state().selection,
        Selection::Item("landscape/leaf.png".into()),
        "the selection stayed through every Esc"
    );
}

/// Note FB7 x FB11: with a docked Presets tab in front, the grid keys do not move the selection from under it.
#[test]
fn grid_keys_wait_while_a_docked_view_is_in_front() {
    use sdc_gui::model::CentreTab;
    let d = tempfile::tempdir().unwrap();
    let mut m = ten(d.path());
    m.open_presets();
    assert_eq!(m.centre_front(), CentreTab::Presets);
    let mut h = keys_only(m);
    press(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(h.state().1, None, "no grid action ran");
    assert!(h.state().0.selected_items().is_empty());
    assert_eq!(h.state().0.centre_front(), CentreTab::Presets);
}

/// Note F?8 (D8): the palette's result rows are the buttons whose name is "label   detail".
fn palette_rows_selected(h: &Harness<'static, AppModel>) -> usize {
    h.query_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| {
            let a = n.accesskit_node();
            a.label().is_some_and(|l| l.contains("   "))
                && a.toggled() == Some(egui::accesskit::Toggled::True)
        })
        .count()
}

/// An opened folder with a texture selected, so that "Clear the selection" (the first empty-query row) is enabled
/// and would show as a change.
fn palette_app(d: &std::path::Path) -> Harness<'static, AppModel> {
    let mut m = opened(d);
    m.click_item("landscape/leaf.png", false, false);
    let mut h = app_harness(m);
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    h.run();
    assert!(h.state().nav.palette.is_some(), "Ctrl+K opens the palette");
    h
}

#[test]
fn enter_on_an_empty_palette_does_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut h = palette_app(d.path());
    let before = h.state().selected_items().len();
    assert_eq!(before, 1, "a texture is selected");
    assert_eq!(
        palette_rows_selected(&h),
        0,
        "an empty palette highlights no row"
    );
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().nav.palette.is_some(), "the palette stays open");
    assert_eq!(h.state().selected_items().len(), before, "no action ran");
    assert!(
        h.state().review.is_none() && h.state().dialog.is_none(),
        "Export did not start"
    );
}

#[test]
fn enter_on_a_typed_query_runs_the_top_hit() {
    let d = tempfile::tempdir().unwrap();
    let mut h = palette_app(d.path());
    h.state_mut().nav.palette.as_mut().unwrap().query = "keyb".into();
    h.run();
    h.run();
    assert_eq!(palette_rows_selected(&h), 1, "the top hit is highlighted");
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().nav.palette.is_none(), "Enter closes the palette");
    assert!(h.state().nav.sheet, "and opens the shortcut sheet");
}

#[test]
fn arrow_then_enter_runs_the_highlight() {
    let d = tempfile::tempdir().unwrap();
    let mut h = palette_app(d.path());
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(palette_rows_selected(&h), 1, "the arrow highlights a row");
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert!(h.state().nav.palette.is_none(), "Enter runs it and closes");
    // The second row is Export, not the first row (Clear the selection).
    assert_eq!(
        h.state().selected_items().len(),
        1,
        "the first row did not run"
    );
    assert!(
        h.state().review.is_some() || h.state().dialog.is_some(),
        "the second row, Export, ran"
    );
}
