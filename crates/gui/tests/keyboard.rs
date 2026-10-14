//! Task 4 (items 5 and 19): one action list for the keys, the F1 shortcut sheet and the Ctrl+K command palette.
mod support;
use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable;
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
        ("clear", "Esc"),
        ("rename", "F2"),
        ("close-preview", "Esc"),
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
            assert_eq!(a.area, Area::Dialogs, "{} is only listed", a.id);
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
            press(&mut h, s.mods, s.key);
            assert_eq!(h.state().1, Some(a.id), "{}", s.text());
            checked += 1;
        }
    }
    assert!(checked >= 30, "{checked}");
}

#[test]
fn no_shortcut_fires_from_a_text_field_except_ctrl_k() {
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
    for a in actions() {
        for s in a.keys {
            h.state_mut().1 = None;
            h.state_mut().0.nav.palette = None;
            h.run();
            h.key_press_modifiers(s.mods, s.key);
            h.run();
            let want = (a.id == actions::PALETTE).then_some(actions::PALETTE);
            assert_eq!(h.state().1, want, "{} from a text field", s.text());
        }
    }
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
