//! Note AO: the set header's input name text box: gray Tab completion, path segments (Note FI17) and the commit keys.
mod support;
use eframe::egui::{self, accesskit::Role, Key, Modifiers};
use egui_kittest::kittest::{by, NodeT as _, Queryable};
use egui_kittest::Harness;
use sdc_gui::model::input_name::{completion, in_form_of, tab_part};
use sdc_gui::model::known_paths::KnownPaths;
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::AppModel;
use sdc_gui::ui::State;
use std::time::Duration;
use support::*;

const ROCK_SET: &str = "landscape/rock01";

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_ghost_completes_a_prefix_and_tab_takes_it_to_the_next_separator() {
    let list = paths(&[
        "textures/architecture/whiterun/wall",
        "textures/landscape/rock02",
        "textures/landscape/rocks/big",
    ]);
    // Case-insensitive, `\` read as `/`; the first match in list order.
    assert_eq!(
        completion(&list, "Textures\\Land").as_deref(),
        Some("scape/rock02")
    );
    assert_eq!(tab_part(&list, "textures/land").as_deref(), Some("scape/"));
    // Tab again continues; one candidate left completes the whole path.
    assert_eq!(
        tab_part(&list, "textures/landscape/").as_deref(),
        Some("rock02")
    );
    assert_eq!(
        tab_part(&list, "textures/ar").as_deref(),
        Some("chitecture/whiterun/wall")
    );
    // No candidate, or the text is already a whole path: no ghost.
    assert_eq!(completion(&list, "meshes/"), None);
    assert_eq!(completion(&list, "textures/landscape/rock02"), None);
    assert_eq!(tab_part(&list, "meshes/"), None);
}

/// Note DO: a project with no `textures` folder in its path names its sets `clutter/...` (`discover::game_path`);
/// the scan's `textures/...` paths are offered in that form, so they fall under the lock.
#[test]
fn a_set_named_without_textures_is_offered_paths_without_it() {
    let list = paths(&["textures/clutter/lumbermill/saw01", "textures/other/thing"]);
    assert_eq!(
        in_form_of(list.clone(), "clutter/lumbermill/saw02"),
        paths(&["clutter/lumbermill/saw01", "other/thing"])
    );
    assert_eq!(in_form_of(list.clone(), "Textures\\clutter/saw02"), list);
}

type H = Harness<'static, (AppModel, State)>;

/// The folder of the rock set's name.
const DIR: &str = "textures/landscape/";
const ROCK01: &str = "textures/landscape/rock01";

fn harness() -> (tempfile::TempDir, H) {
    harness_dt(0.25)
}

fn harness_dt(dt: f32) -> (tempfile::TempDir, H) {
    harness_with(
        dt,
        &[
            "textures/landscape/rock02",
            "textures/landscape/rocks/big",
            "textures/landscape/rocks/small",
            "textures/other/thing",
        ],
    )
}

fn harness_with(dt: f32, known: &[&str]) -> (tempfile::TempDir, H) {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_known(KnownPaths {
        textures: known.iter().map(|p| (format!("{p}.dds"), 1u16)).collect(),
        mesh_dirs: vec![],
    });
    (d, harness_on(m, ROCK_SET, dt))
}

/// The app on `m` with set `set` selected.
fn harness_on(mut m: AppModel, set: &str, dt: f32) -> H {
    let key = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == set)
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    let mut h = Harness::builder()
        .with_size(egui::vec2(1600.0, 900.0))
        .with_step_dt(dt)
        // Note FB2: the name box sits in the set editor's scroll area, whose floating bar fades in while the pointer
        // is over it; with a short step and a 0.2 s animation time that fade must fit in one `run()`.
        .with_max_steps(((0.5 / dt) as u64).max(4))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, State::default()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h
}

/// The box's text.
fn free(h: &H) -> String {
    h.get(
        by().role(Role::TextInput)
            .label(sdc_gui::ui::input_name::HINT),
    )
    .accesskit_node()
    .value()
    .unwrap_or_default()
}

fn focus(h: &mut H) {
    h.get(
        by().role(Role::TextInput)
            .label(sdc_gui::ui::input_name::HINT),
    )
    .click();
    h.run();
    // The click puts the cursor where it lands; start from the end.
    key(h, Key::End, Modifiers::NONE);
}

/// A key press and its release (egui marks a press of a key still down as a repeat).
fn key(h: &mut H, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
    }
    h.run();
}

/// Replaces the whole text (Ctrl+A, then typing) with the rock set's folder followed by `s`.
fn clear_and_type(h: &mut H, s: &str) {
    type_all(h, &format!("{DIR}{s}"));
}

/// Replaces the whole text with `s`.
fn type_all(h: &mut H, s: &str) {
    key(h, Key::A, ctrl());
    h.input_mut().events.push(egui::Event::Text(s.into()));
    h.run();
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    }
}

fn ctrl_shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..ctrl()
    }
}

/// The box's selection as (secondary, primary) char indices: (anchor, head).
fn sel(h: &H) -> (usize, usize) {
    let r = egui::TextEdit::load_state(&h.ctx, sdc_gui::ui::input_name::edit_id(ROCK_SET))
        .and_then(|s| s.cursor.char_range())
        .expect("a cursor");
    (r.secondary.index.0, r.primary.index.0)
}

/// `sel` sorted.
fn sel_sorted(h: &H) -> (usize, usize) {
    let (a, b) = sel(h);
    (a.min(b), a.max(b))
}

fn box_rect(h: &H) -> egui::Rect {
    h.get(
        by().role(Role::TextInput)
            .label(sdc_gui::ui::input_name::HINT),
    )
    .rect()
}

/// The screen x of char boundary `prefix.len()` in the box (its text starts after a 4 pt margin).
fn x_after(h: &H, prefix: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(&h.ctx.global_style());
    let w = h.ctx.fonts_mut(|f| {
        f.layout_no_wrap(prefix.to_string(), font, egui::Color32::WHITE)
            .size()
            .x
    });
    box_rect(h).left() + 4.0 + w
}

fn pointer(h: &mut H, p: egui::Pos2, pressed: bool, modifiers: Modifiers) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    });
}

/// Note FI17: Ctrl+Left / Ctrl+Right jump to the previous segment start / the next segment end; with Shift they
/// extend the selection. `.` and `-` are not boundaries (egui's word jump stops at them).
#[test]
fn ctrl_arrows_jump_by_segment() {
    let (_d, mut h) = harness();
    focus(&mut h);
    type_all(&mut h, "textures/landscape/rock-02.v2");
    key(&mut h, Key::End, Modifiers::NONE);
    key(&mut h, Key::ArrowLeft, ctrl());
    assert_eq!(sel(&h), (19, 19));
    key(&mut h, Key::ArrowLeft, ctrl());
    assert_eq!(sel(&h), (9, 9));
    key(&mut h, Key::ArrowLeft, ctrl());
    assert_eq!(sel(&h), (0, 0));
    key(&mut h, Key::ArrowLeft, ctrl());
    assert_eq!(sel(&h), (0, 0), "nothing left of 0");
    key(&mut h, Key::ArrowRight, ctrl());
    assert_eq!(sel(&h), (8, 8));
    key(&mut h, Key::ArrowRight, ctrl());
    assert_eq!(sel(&h), (18, 18));
    key(&mut h, Key::ArrowRight, ctrl());
    assert_eq!(sel(&h), (29, 29));
    key(&mut h, Key::ArrowLeft, ctrl_shift());
    assert_eq!(sel(&h), (29, 19), "Shift extends");
    key(&mut h, Key::ArrowLeft, ctrl_shift());
    assert_eq!(sel(&h), (29, 9));
    assert_eq!(free(&h), "textures/landscape/rock-02.v2", "no text change");
}

/// Note FI17: a double-click selects the path segment under the pointer, not egui's word (`.` and `-` split words).
#[test]
fn double_click_selects_the_segment() {
    let (_d, mut h) = harness();
    focus(&mut h);
    type_all(&mut h, "textures/landscape/rock-02.v2");
    let p = egui::pos2(
        x_after(&h, "textures/landscape/roc"),
        box_rect(&h).center().y,
    );
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for _ in 0..2 {
        pointer(&mut h, p, true, Modifiers::NONE);
        pointer(&mut h, p, false, Modifiers::NONE);
        h.step();
    }
    h.run();
    assert_eq!(sel_sorted(&h), (19, 29));
    // On a folder: the folder only, not its `/`s.
    let p = egui::pos2(x_after(&h, "textures/lan"), box_rect(&h).center().y);
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for _ in 0..2 {
        pointer(&mut h, p, true, Modifiers::NONE);
        pointer(&mut h, p, false, Modifiers::NONE);
        h.step();
    }
    h.run();
    assert_eq!(sel_sorted(&h), (9, 18));
}

/// Press at `from`, move to `to` in four steps, release.
fn drag(h: &mut H, from: egui::Pos2, to: egui::Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(egui::Event::PointerMoved(from));
    h.run();
    pointer(h, from, true, modifiers);
    h.step();
    for i in 1..=4 {
        h.input_mut()
            .events
            .push(egui::Event::PointerMoved(from.lerp(to, i as f32 / 4.0)));
        h.step();
    }
    pointer(h, to, false, modifiers);
    h.step();
    h.run();
}

/// Note FI17: a drag whose head crosses a `/` selects whole segments, in either direction.
#[test]
fn a_drag_across_a_slash_snaps_to_segments() {
    let (_d, mut h) = harness();
    focus(&mut h);
    let y = box_rect(&h).center().y;
    let land = egui::pos2(x_after(&h, "textures/lands"), y);
    let rock = egui::pos2(x_after(&h, "textures/landscape/ro"), y);
    drag(&mut h, land, rock, Modifiers::NONE);
    assert_eq!(sel(&h), (9, 25), "right: landscape's start to rock01's end");
    drag(&mut h, rock, land, Modifiers::NONE);
    assert_eq!(sel(&h), (25, 9), "left: rock01's end to landscape's start");
    assert_eq!(free(&h), ROCK01);
}

/// Note FI17: a drag that stays inside one segment selects characters (egui's default).
#[test]
fn a_drag_inside_one_segment_selects_characters() {
    let (_d, mut h) = harness();
    focus(&mut h);
    let y = box_rect(&h).center().y;
    let a = egui::pos2(x_after(&h, "textures/la"), y);
    let b = egui::pos2(x_after(&h, "textures/landsc"), y);
    drag(&mut h, a, b, Modifiers::NONE);
    let (lo, hi) = sel_sorted(&h);
    assert!(lo > 9 && hi < 18 && lo < hi, "{lo}..{hi}");
}

/// Note FI17: Ctrl+A selects the whole path.
#[test]
fn ctrl_a_selects_the_whole_path() {
    let (_d, mut h) = harness();
    focus(&mut h);
    key(&mut h, Key::A, ctrl());
    assert_eq!(sel_sorted(&h), (0, ROCK01.len()));
}

/// Note FI17: Backspace on a `/` highlights it first (with a hint), a second press deletes it.
#[test]
fn backspace_on_a_slash_highlights_first_then_deletes() {
    let (_d, mut h) = harness();
    focus(&mut h);
    for _ in 0.."rock01".len() {
        key(&mut h, Key::ArrowLeft, Modifiers::NONE);
    }
    assert_eq!(sel(&h), (19, 19));
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(free(&h), ROCK01, "the first press keeps the /");
    assert_eq!(sel_sorted(&h), (18, 19), "and highlights it");
    h.get_by_label("Backspace again to remove the folder separator");
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(free(&h), "textures/landscaperock01");
    assert!(h
        .query_by_label("Backspace again to remove the folder separator")
        .is_none());
}

/// Note FI17: Delete mirrors Backspace for a `/` after the cursor.
#[test]
fn delete_on_a_slash_highlights_first_then_deletes() {
    let (_d, mut h) = harness();
    focus(&mut h);
    for _ in 0.."/rock01".len() {
        key(&mut h, Key::ArrowLeft, Modifiers::NONE);
    }
    assert_eq!(sel(&h), (18, 18));
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(free(&h), ROCK01);
    assert_eq!(sel_sorted(&h), (18, 19));
    h.get_by_label("Delete again to remove the folder separator");
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(free(&h), "textures/landscaperock01");
}

/// A key press with no release: while the key stays down, egui marks each further press a repeat (as a held key's
/// auto-repeat arrives).
fn key_down(h: &mut H, key: Key) {
    h.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

fn key_up(h: &mut H, key: Key) {
    h.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// Note FI17: a held Backspace (repeat key events) deletes characters but stops at a `/`, without highlighting it.
#[test]
fn held_backspace_stops_at_a_slash() {
    let (_d, mut h) = harness();
    focus(&mut h);
    // The first press, then 12 repeats.
    for _ in 0..13 {
        key_down(&mut h, Key::Backspace);
    }
    assert_eq!(free(&h), DIR, "stopped at the /");
    assert_eq!(sel(&h), (19, 19), "nothing highlighted");
    // Released and pressed again: the two-press rule.
    key_up(&mut h, Key::Backspace);
    key_down(&mut h, Key::Backspace);
    assert_eq!(sel_sorted(&h), (18, 19));
    // Still held: the repeat never deletes the highlighted /.
    key_down(&mut h, Key::Backspace);
    assert_eq!(free(&h), DIR);
    key_up(&mut h, Key::Backspace);
}

/// Note FI17 (supersedes BH2): no padlock, no lock slider; the box is still there.
#[test]
fn the_box_has_no_path_lock() {
    let (_d, h) = harness();
    assert_eq!(free(&h), ROCK01, "the box holds the whole path");
    assert!(h.query_by_label("Path lock").is_none());
}

/// Note FC12: the selector starts at the set's lowest unused match (2 here); the rename list is Match 1's.
fn match1(h: &mut H) {
    h.get_by_value("Match 2").click();
    h.run();
    h.get_by_label("Match 1 · Diffuse").click();
    h.run();
}

/// Note FI11 (A15): the Match menu and Slot commands name a slot as the set editor's card does; the key is on hover.
#[test]
fn the_match_menu_uses_the_sets_slot_names() {
    let (_d, mut h) = harness();
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.get_by_value("Match 2").click();
    h.run();
    h.get_by_label("Match 6 · RMAOS").hover();
    h.run();
    h.run();
    h.get_by_label("match6");
    h.get_by_label("Match 3 · Emissive / Skin");
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("Slot commands…").click();
    h.run();
    // The set editor's empty slot-3 card reads "Emissive / Skin (slot 3)"; the number is already in the text.
    h.get_by_label("Slot 3 (Emissive / Skin)");
    h.get_by_label("Slot 6 (RMAOS)");
    h.get_by_label("Slot 7 (Coat normal / Fuzz)");
}

/// Note FI17 (was the path lock's filter): the ▾ list and the live list show the paths under the folder before the
/// segment being edited.
#[test]
fn the_list_filters_by_the_folder_before_the_cursor_segment() {
    let (_d, mut h) = harness();
    match1(&mut h);
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label("Read this set's textures as");
    h.get_by_label("textures/landscape/rock02");
    assert!(h.query_by_label("textures/other/thing").is_none());
    h.key_press(Key::Escape);
    h.run();
    // The live list: no path starts with the text, so the paths under its folder that contain the last segment's words.
    focus(&mut h);
    clear_and_type(&mut h, "big");
    h.get_by_label("textures/landscape/rocks/big");
    assert!(h.query_by_label("textures/other/thing").is_none());
    type_all(&mut h, "textures/thing");
    h.get_by_label("textures/other/thing");
}

/// Note FC12 (supersedes DO5's auto-switch and DO6's unstored selector): the Match selector starts from the set's lowest
/// unused match, a pick from "All slots" goes to the selector's match and never moves it, and the selector a user
/// chooses is kept in `gui.toml` per project and set.
#[test]
fn match_selector_starts_from_the_set_and_stays() {
    let (_d, mut h) = harness();
    // match1 is the derived diffuse, so the lowest unused match is 2. A combo's accessible text is its value (10.8).
    h.get_by_value("Match 2");
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label(sdc_gui::ui::input_name::ALL_SLOTS).click();
    h.run();
    // A diffuse (seen in slot 1 only) picked under Match 2: it is match2, the set is not renamed. (The list grew inside
    // the open menu: its rows must still be inside the clip to take the click.)
    h.get_by_label("textures/landscape/rock02").click();
    h.run();
    assert!(h.state_mut().0.wait_idle(Duration::from_secs(60)));
    h.run();
    let rock = |m: &AppModel| {
        json_entries(m)
            .into_iter()
            .find(|(v, _)| v.id == ROCK_SET)
            .expect("the rock set")
            .0
    };
    let m = &h.state().0;
    assert_eq!(
        rock(m).settings.get("match2").and_then(|x| x.as_str()),
        Some("textures\\landscape\\rock02")
    );
    assert_eq!(
        m.input_name_of(ROCK_SET).as_deref(),
        Some("textures/landscape/rock01"),
        "no rename"
    );
    h.get_by_value("Match 2");
    assert!(m.settings.match_slots.is_empty(), "nothing chosen yet");
    // The user's choice is stored, and a new window starts from it.
    h.get_by_value("Match 2").click();
    h.run();
    h.get_by_label("Match 4 · Height _p").click();
    h.run();
    h.get_by_value("Match 4");
    assert_eq!(
        h.state()
            .0
            .settings
            .match_slots
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![4]
    );
    // A new session (egui's memory gone) starts from the stored choice ...
    h.ctx.data_mut(|d| d.clear());
    h.run();
    h.get_by_value("Match 4");
    // ... and with none stored, from the lowest unused match, now 3 (match2 is set).
    h.state_mut().0.settings.match_slots.clear();
    h.ctx.data_mut(|d| d.clear());
    h.run();
    h.get_by_value("Match 3");
}

/// Fix round 1 (6b): the selector is per project. Another folder's set with the same id starts from its own value
/// (none stored here, so its lowest unused match), not from the first folder's choice in egui's memory.
#[test]
fn the_match_selector_is_per_project() {
    let (_d, mut h) = harness();
    h.get_by_value("Match 2").click();
    h.run();
    h.get_by_label("Match 4 · Height _p").click();
    h.run();
    h.get_by_value("Match 4");
    let d2 = tempfile::tempdir().unwrap();
    let mut m2 = pbr_opened(d2.path());
    m2.settings = h.state().0.settings.clone();
    let key = json_entries(&m2)
        .into_iter()
        .find(|(v, _)| v.id == ROCK_SET)
        .unwrap()
        .0
        .key;
    m2.select_set(&key);
    h.state_mut().0 = m2;
    h.run();
    h.get_by_value("Match 2");
    assert_eq!(
        h.state()
            .0
            .settings
            .match_slots
            .values()
            .copied()
            .collect::<Vec<_>>(),
        vec![4],
        "the first folder's choice is kept, for that folder"
    );
}

/// Note FC13 (FC12 extra): the "All slots" tick is per project too, so it does not carry to another folder's set with
/// the same id.
#[test]
fn the_all_slots_tick_is_per_project() {
    use egui_kittest::kittest::NodeT;
    let (_d, mut h) = harness();
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label(sdc_gui::ui::input_name::ALL_SLOTS).click();
    h.run();
    assert_eq!(
        h.get_by_label(sdc_gui::ui::input_name::ALL_SLOTS)
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    let d2 = tempfile::tempdir().unwrap();
    let mut m2 = pbr_opened(d2.path());
    let key = json_entries(&m2)
        .into_iter()
        .find(|(v, _)| v.id == ROCK_SET)
        .unwrap()
        .0
        .key;
    m2.select_set(&key);
    h.state_mut().0 = m2;
    h.run();
    // The list may still be open from before the swap.
    if h.query_by_label(sdc_gui::ui::input_name::ALL_SLOTS)
        .is_none()
    {
        h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
        h.run();
    }
    assert_eq!(
        h.get_by_label(sdc_gui::ui::input_name::ALL_SLOTS)
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::False),
        "the other folder starts unticked"
    );
}

#[test]
fn tab_accepts_the_ghost_and_with_no_ghost_moves_focus() {
    let (_d, mut h) = harness();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(free(&h), format!("{DIR}rocks/"));
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(free(&h), format!("{DIR}rocks/big"));
    // Nothing to complete: Tab leaves the box; a|b is refused, so the box reverts. The ghost never commits.
    let steps = h.state().0.history.undo.len();
    clear_and_type(&mut h, "a|b");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(free(&h), ROCK01);
    assert_eq!(h.state().0.history.undo.len(), steps);
    // Right arrow at the end takes the whole ghost.
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    key(&mut h, Key::End, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(free(&h), format!("{DIR}rocks/big"));
}

/// Trace #8 (AO2): Shift+Tab leaves the box; it never takes the ghost.
#[test]
fn shift_tab_does_not_accept_the_ghost() {
    let (_d, mut h) = harness();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    key(&mut h, Key::Tab, Modifiers::SHIFT);
    assert_ne!(free(&h), format!("{DIR}rocks/"), "Shift+Tab took the ghost");
}

#[test]
fn enter_commits_one_step_esc_reverts_and_invalid_text_does_not_commit() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    // The first Esc closes the live list, the second reverts (AO4).
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(free(&h), ROCK01, "Esc reverts");
    assert_eq!(h.state().0.history.undo.len(), steps);
    // leaf.png already has that name: invalid, shown red with its reason, and never committed.
    focus(&mut h);
    clear_and_type(&mut h, "leaf");
    let why = h
        .get(
            by().role(Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .accesskit_node()
        .description()
        .unwrap_or_default();
    assert!(why.contains("already has this name"), "{why}");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().0.history.undo.len(), steps);
    assert_eq!(free(&h), ROCK01);
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    assert!(h.state_mut().0.wait_idle(Duration::from_secs(120)));
    assert_eq!(
        h.state().0.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
}

/// Note AO8: paths under `textures\pbr\` are PGPatcher's outputs, not input names: no row, no ghost.
#[test]
fn pbr_output_paths_are_never_offered() {
    let (_d, mut h) = harness();
    h.state_mut().0.set_known(KnownPaths {
        textures: [
            ("textures/pbr/landscape/rock02.dds".to_string(), 1u16),
            ("textures/landscape/rock02.dds".to_string(), 1u16),
        ]
        .into(),
        mesh_dirs: vec![],
    });
    let list = h.state().0.input_name_candidates();
    assert!(list.iter().any(|p| p == "textures/landscape/rock02"));
    assert!(
        !list
            .iter()
            .any(|p| p.to_lowercase().starts_with("textures/pbr/")),
        "{list:?}"
    );
    assert_eq!(completion(&list, "Textures\\PBR"), None);
    // The ▾ list has no pbr row. (With no path lock it lists the name's own folder, so this no longer reaches `textures/`
    // as a whole: the `input_name_candidates` check above carries the rule.)
    h.run();
    match1(&mut h);
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label("textures/landscape/rock02");
    assert!(h.query_by_label("textures/pbr/landscape/rock02").is_none());
}

const BIG: &str = "textures/landscape/rocks/big";
const SMALL: &str = "textures/landscape/rocks/small";

fn click_at(h: &mut H, p: egui::Pos2) {
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    h.run();
}

fn settle(h: &mut H) {
    assert!(h.state_mut().0.wait_idle(Duration::from_secs(120)));
    h.run();
}

/// Note AO4: typing shows a live list under the box; Down then Enter commits the highlighted row.
#[test]
fn typing_filters_the_live_list_and_down_enter_commits_its_row() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    h.get_by_label(BIG);
    h.get_by_label(SMALL);
    assert!(h.query_by_label("textures/landscape/rock02").is_none());
    assert!(
        h.query_by_label("Read this set's textures as").is_none(),
        "not the ▾ list"
    );
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    settle(&mut h);
    assert_eq!(
        h.state().0.input_name_of("landscape/rocks/big").as_deref(),
        Some(BIG)
    );
    assert!(h.query_by_label(SMALL).is_none(), "the list closed");
}

/// Note AO4: a click on a live row commits that row, not the half-typed text the click's focus loss would.
#[test]
fn a_click_on_a_live_row_commits_that_row() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    let p = h.get_by_label(SMALL).rect().center();
    click_at(&mut h, p);
    h.run();
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    settle(&mut h);
    assert_eq!(
        h.state()
            .0
            .input_name_of("landscape/rocks/small")
            .as_deref(),
        Some(SMALL)
    );
}

/// Note AO4: the first Esc closes the live list and keeps the text; the second reverts.
#[test]
fn esc_closes_the_live_list_then_reverts() {
    let (_d, mut h) = harness();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    h.get_by_label(BIG);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.query_by_label(BIG).is_none(), "closed");
    assert_eq!(free(&h), format!("{DIR}rocks"));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(free(&h), ROCK01);
}

fn push_key(h: &mut H, key: Key) {
    h.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
}

/// Review fix: the ghost and Tab's hold follow the text in the frame it changes, with no settling run.
#[test]
fn the_ghost_follows_a_keystroke_by_the_next_frame() {
    let (_d, mut h) = harness();
    focus(&mut h);
    // "rocks/biz" has no candidate: no ghost, so Tab would leave the box.
    clear_and_type(&mut h, "rocks/biz");
    push_key(&mut h, Key::Backspace);
    h.step();
    // "rocks/bi" completes to "rocks/big": the very next frame's Tab is held and takes it.
    push_key(&mut h, Key::Tab);
    h.step();
    h.step();
    assert_eq!(free(&h), format!("{DIR}rocks/big"));
}

/// Trace-debug #11: a click on another set in the Sets column commits the typed name first (AO1). Move, press and
/// release land in separate frames, as a real click does.
#[test]
fn a_click_on_another_set_commits_the_typed_name_first() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    let rock = h.state().0.selection.clone();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    let p = h.get_by_label("landscape/leaf").rect().center();
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
    settle(&mut h);
    let now = &h.state().0.selection;
    assert!(
        matches!(now, sdc_gui::model::Selection::Set(_)) && *now != rock,
        "the click selected leaf: {now:?}"
    );
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    assert_eq!(
        h.state().0.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
}

/// Review fix: a row commits only when the press and the release are both on it.
#[test]
fn a_press_dragged_off_a_live_row_commits_nothing() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    let p = h.get_by_label(SMALL).rect().center();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let away = egui::pos2(1500.0, 300.0);
    h.input_mut().events.push(egui::Event::PointerMoved(away));
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: away,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    h.run();
    assert_eq!(h.state().0.history.undo.len(), steps, "nothing committed");
    // A drag is no click: the box keeps its text and focus. Enter then takes the typed text, not the pressed row.
    assert_eq!(free(&h), format!("{DIR}rocks"));
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert!(h.state().0.input_name_of("landscape/rocks/small").is_none());
}

/// Note BH1: the set's name shows once, as the input name box; no heading repeats it beside the box.
#[test]
fn the_set_header_shows_the_name_only_in_the_box() {
    let (_d, h) = harness();
    let edit = h
        .get(
            by().role(Role::TextInput)
                .label(sdc_gui::ui::input_name::HINT),
        )
        .rect();
    // Only the Sets column's row is labelled with the set's path; nothing on the box's row is.
    assert!(h.query_all_by_label(ROCK_SET).count() >= 1);
    let on_row = h
        .query_all_by_label(ROCK_SET)
        .filter(|n| (n.rect().center().y - edit.center().y).abs() < 12.0)
        .count();
    assert_eq!(on_row, 0);
}

/// Presses and releases the primary button at `p`: in separate frames, as a mouse click is, or in one frame, as a
/// touchpad tap or a slow frame gives. One `step` between them: the plan the press's commit starts keeps `run` stepping
/// past egui's 0.8 s click limit.
fn press_release(h: &mut H, p: egui::Pos2, one_frame: bool) {
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        if !pressed {
            h.run();
        } else if !one_frame {
            h.step();
        }
    }
}

/// Note FC8 (supersedes BS7's refusal): Save clicked while the input name box has focus commits the name and saves,
/// in one click, whether the press and the release land in separate frames or in one.
#[test]
fn save_click_commits_the_field_and_saves() {
    for one_frame in [false, true] {
        let (_d, mut h) = harness();
        // Another unsaved edit, so Save is enabled.
        h.state_mut()
            .0
            .set_json_value(ROCK_SET, "specular_level", Some(serde_json::json!(0.3)));
        h.run();
        let file = h.state().0.sets_doc.as_ref().unwrap().path().to_path_buf();
        focus(&mut h);
        clear_and_type(&mut h, "rock02");
        let p = h.query_all_by_label("Save").next().unwrap().rect().center();
        press_release(&mut h, p, one_frame);
        settle(&mut h);
        let on_disk = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(on_disk.contains("specular_level"), "{one_frame}: saved");
        assert!(
            on_disk.contains("landscape/rock02"),
            "{one_frame}: {on_disk}"
        );
        assert!(!h.state().0.is_dirty(), "{one_frame}: nothing left unsaved");
        assert!(!h
            .state()
            .0
            .messages
            .entries()
            .any(|e| e.text.contains("leave the text field first")));
    }
}

/// Note FC8 (supersedes BS14's refusal): the header Export clicked while the input name box has focus commits the
/// name and runs, in one click: here it opens Review, whose rows already see the committed name.
#[test]
fn export_click_commits_the_field_and_runs() {
    let (_d, mut h) = harness();
    h.state_mut()
        .0
        .set_json_value(ROCK_SET, "specular_level", Some(serde_json::json!(0.3)));
    h.run();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    let p = h
        .query_all_by_label(&header_export_label(&h))
        .next()
        .unwrap()
        .rect()
        .center();
    press_release(&mut h, p, false);
    settle(&mut h);
    let m = &h.state().0;
    // The fixture always opens Review (Note FC14: an OR with "it exported" would pass a click that skipped Review).
    assert!(m.review.is_some(), "the click opened Review");
    assert_eq!(
        m.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
}

/// Note FC8 (supersedes CA14's refusal): Review's Export clicked while the input name box has focus commits the
/// name, saves it and exports, in one click.
#[test]
fn review_export_click_commits_the_field_and_runs() {
    for one_frame in [false, true] {
        let (_d, mut h) = harness();
        h.state_mut()
            .0
            .set_json_value(ROCK_SET, "specular_level", Some(serde_json::json!(0.3)));
        h.state_mut().0.open_review();
        h.run();
        let file = h.state().0.sets_doc.as_ref().unwrap().path().to_path_buf();
        let text = h.state().0.review_button_text();
        assert!(h.state().0.review_gate().is_ok());
        focus(&mut h);
        clear_and_type(&mut h, "rock02");
        let p = h.get_by_label(&text).rect().center();
        press_release(&mut h, p, one_frame);
        settle(&mut h);
        settle(&mut h);
        let on_disk = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            on_disk.contains("landscape/rock02"),
            "{one_frame}: {on_disk}"
        );
        let m = &h.state().0;
        assert!(
            m.export.running.is_some() || m.export.summary.is_some(),
            "{one_frame}: the export ran"
        );
    }
}

/// Note FC8 (BN3's deferral): a press and release in one frame on another set (a touchpad tap) still commits the
/// typed name first; the new selection follows a frame later.
#[test]
fn tap_in_the_same_frame_keeps_the_typed_name() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    let rock = h.state().0.selection.clone();
    focus(&mut h);
    clear_and_type(&mut h, "rock02");
    let p = h.get_by_label("landscape/leaf").rect().center();
    press_release(&mut h, p, true);
    settle(&mut h);
    let now = &h.state().0.selection;
    assert!(
        matches!(now, sdc_gui::model::Selection::Set(_)) && *now != rock,
        "the tap selected leaf: {now:?}"
    );
    assert_eq!(h.state().0.history.undo.len(), steps + 1, "one undo step");
    assert_eq!(
        h.state().0.input_name_of("landscape/rock02").as_deref(),
        Some("textures/landscape/rock02")
    );
}

/// Trace-debug wave 2 #5: the project's own rows name outputs, so they carry no source extension (`leaf.png` is
/// offered as `textures/landscape/leaf`, never `textures/landscape/leaf.png`).
#[test]
fn own_output_rows_have_no_source_extension() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let known = m.known_textures(None);
    let names = m.input_name_candidates();
    for list in [&known, &names] {
        assert!(!list.iter().any(|p| p.ends_with(".png")), "{list:?}");
    }
    assert!(
        names.iter().any(|p| p == "textures/landscape/leaf"),
        "{names:?}"
    );
}

/// Alpha of the text shape that contains `needle`: the fade an Area applies reaches its fallback colour. The popup
/// is a Foreground Area, painted after everything else, so the last match is the list row.
fn row_alpha(h: &H, needle: &str) -> Option<u8> {
    fn find(s: &egui::Shape, needle: &str) -> Option<u8> {
        match s {
            egui::Shape::Text(t) if t.galley.text().contains(needle) => Some(t.fallback_color.a()),
            egui::Shape::Vec(v) => v.iter().find_map(|s| find(s, needle)),
            _ => None,
        }
    }
    h.output()
        .shapes
        .iter()
        .filter_map(|c| find(&c.shape, needle))
        .next_back()
}

/// Note CF3: the live path list is typed open, so it appears at its resting opacity on its first visible frame.
#[test]
fn the_path_list_shows_at_once_when_typing_reopens_it() {
    let (_d, mut h) = harness_dt(0.02);
    // kittest zeroes the stock animation time, which would turn every Area fade off.
    h.ctx.global_style_mut(|s| s.animation_time = 0.2);
    focus(&mut h);
    clear_and_type(&mut h, "rocks");
    h.run_steps(20);
    let rest = row_alpha(&h, BIG).expect("the list is open");
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.run_steps(20);
    assert!(h.query_by_label(BIG).is_none(), "closed");
    h.input_mut().events.push(egui::Event::Text("/".into()));
    for _ in 0..6 {
        h.step();
        if let Some(first) = row_alpha(&h, BIG) {
            assert_eq!(first, rest, "the list faded in");
            return;
        }
    }
    panic!("the list never reopened");
}

/// From the end of the text, Ctrl+Left `n` times: the cursor at the start of the `n`-th segment from the end.
fn segment_back(h: &mut H, n: usize) {
    key(h, Key::End, Modifiers::NONE);
    for _ in 0..n {
        key(h, Key::ArrowLeft, ctrl());
    }
}

fn selected(h: &H, label: &str) -> bool {
    // A selectable button's highlight is its AccessKit toggled state.
    h.get_by_label(label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

fn y(h: &H, label: &str) -> f32 {
    h.get_by_label(label).rect().center().y
}

/// Note FI17: Ctrl+Down lists the known names for the segment under the cursor; Up/Down then Enter replace only that
/// segment, and nothing is committed until the box itself commits.
#[test]
fn ctrl_down_opens_the_segment_list_and_enter_replaces_only_that_segment() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    segment_back(&mut h, 2);
    assert_eq!(sel(&h), (9, 9), "in `landscape`");
    key(&mut h, Key::ArrowDown, ctrl());
    // The folders beside `landscape` under `textures/`; landscape also holds the rest of the path (rock01).
    assert!(
        selected(&h, "landscape, has rock01"),
        "the current folder is highlighted"
    );
    assert!(y(&h, "landscape, has rock01") < y(&h, "other"));
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(free(&h), "textures/other/rock01");
    assert_eq!(
        h.state().0.history.undo.len(),
        steps,
        "a pick only edits the text"
    );
    // The next segment's list is open: the textures in `other`, none of them rock01, so none highlighted.
    assert!(!selected(&h, "textures/other/thing"));
    assert_eq!(
        segment_at_cursor(&h),
        (15, 21),
        "the cursor is in that segment"
    );
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.query_by_label("textures/other/thing").is_none());
    assert_eq!(free(&h), "textures/other/rock01", "Esc keeps the tail");
}

/// The bounds of the segment the cursor is in.
fn segment_at_cursor(h: &H) -> (usize, usize) {
    let r = sdc_gui::model::input_name::segment_at(&free(h), sel(h).1);
    (r.start, r.end)
}

/// Note FI17: Esc closes the segment list and changes nothing; the box keeps its focus.
#[test]
fn esc_closes_the_segment_list_unchanged() {
    let (_d, mut h) = harness();
    let steps = h.state().0.history.undo.len();
    focus(&mut h);
    segment_back(&mut h, 2);
    key(&mut h, Key::ArrowUp, ctrl());
    h.get_by_label("landscape, has rock01");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.query_by_label("landscape, has rock01").is_none());
    assert_eq!(free(&h), ROCK01);
    assert_eq!(h.state().0.history.undo.len(), steps);
    let edit = sdc_gui::ui::input_name::edit_id(ROCK_SET);
    assert!(
        h.ctx.memory(|m| m.has_focus(edit)),
        "the box keeps its focus"
    );
}

/// Note FI17 (rule 10, controller ruling R2): with the whole text selected, Ctrl+Down opens the ▾ list unfiltered
/// (another folder's rows too); a plain ▾ click afterwards keeps the folder filter.
#[test]
fn ctrl_down_with_all_selected_opens_the_full_list() {
    let (_d, mut h) = harness();
    match1(&mut h);
    focus(&mut h);
    key(&mut h, Key::A, ctrl());
    key(&mut h, Key::ArrowDown, ctrl());
    h.get_by_label("Read this set's textures as");
    h.get_by_label("textures/landscape/rock02");
    h.get_by_label("textures/other/thing");
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Read this set's textures as").is_none());
    h.get_by_label(sdc_gui::ui::input_name::DROPDOWN).click();
    h.run();
    h.get_by_label("textures/landscape/rock02");
    assert!(
        h.query_by_label("textures/other/thing").is_none(),
        "a ▾ click is filtered"
    );
}

/// Review fix (FI17 rule 9): a click on a segment row replaces only that segment and commits nothing, whether the
/// press and the release land in separate frames or in one.
#[test]
fn a_click_on_a_segment_row_replaces_only_that_segment() {
    for one_frame in [false, true] {
        let (_d, mut h) = harness();
        let steps = h.state().0.history.undo.len();
        focus(&mut h);
        segment_back(&mut h, 2);
        key(&mut h, Key::ArrowDown, ctrl());
        let p = h.get_by_label("other").rect().center();
        press_release(&mut h, p, one_frame);
        h.run();
        assert_eq!(free(&h), "textures/other/rock01", "{one_frame}");
        assert_eq!(
            h.state().0.history.undo.len(),
            steps,
            "{one_frame}: a pick only edits the text"
        );
        assert_eq!(
            h.state().0.input_name_of(ROCK_SET).as_deref(),
            Some(ROCK01),
            "{one_frame}: nothing committed"
        );
        let edit = sdc_gui::ui::input_name::edit_id(ROCK_SET);
        assert!(
            h.ctx.memory(|m| m.has_focus(edit)),
            "{one_frame}: the box keeps its focus"
        );
    }
}

/// Note FI17 (rule 1): a triple-click still selects the whole path (egui's line select).
#[test]
fn triple_click_selects_the_whole_path() {
    let (_d, mut h) = harness();
    focus(&mut h);
    let p = egui::pos2(x_after(&h, "textures/lan"), box_rect(&h).center().y);
    h.input_mut().events.push(egui::Event::PointerMoved(p));
    h.run();
    for _ in 0..3 {
        pointer(&mut h, p, true, Modifiers::NONE);
        pointer(&mut h, p, false, Modifiers::NONE);
        h.step();
    }
    h.run();
    assert_eq!(sel_sorted(&h), (0, ROCK01.len()));
}

/// Note FI17 (rule 5): a held Delete deletes characters but stops at a `/`, without highlighting it.
#[test]
fn held_delete_stops_at_a_slash() {
    let (_d, mut h) = harness();
    focus(&mut h);
    segment_back(&mut h, 2);
    assert_eq!(sel(&h), (9, 9));
    // The first press, then 12 repeats: `landscape` is 9 characters.
    for _ in 0..13 {
        key_down(&mut h, Key::Delete);
    }
    key_up(&mut h, Key::Delete);
    assert_eq!(free(&h), "textures//rock01", "stopped at the /");
    assert_eq!(sel(&h), (9, 9), "nothing highlighted");
}

const ARMOR: &str = "textures/armor/iron/shield";

/// Note FI17: in a folder list, folders that also hold the rest of the path are marked and listed first; picking one
/// opens the next segment's list with the old tail highlighted, and Enter keeps it.
#[test]
fn picking_a_folder_opens_the_next_segment_with_the_tail_highlighted() {
    let (_d, mut h) = harness_with(
        0.25,
        &[
            "textures/armor/bronze/helmet",
            "textures/armor/iron/shield",
            "textures/armor/steel/shield",
        ],
    );
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    focus(&mut h);
    type_all(&mut h, ARMOR);
    segment_back(&mut h, 2);
    key(&mut h, Key::ArrowDown, ctrl());
    assert!(selected(&h, "iron, has shield"));
    assert!(y(&h, "iron, has shield") < y(&h, "steel, has shield"));
    assert!(y(&h, "steel, has shield") < y(&h, "bronze"));
    h.get_by_label("steel, has shield").hover();
    h.run();
    h.get_by_label("Has shield");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(free(&h), "textures/armor/steel/shield");
    assert!(
        selected(&h, "textures/armor/steel/shield"),
        "the old tail is highlighted"
    );
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(free(&h), "textures/armor/steel/shield", "Enter keeps it");
    assert!(
        h.query_by_label("textures/armor/steel/shield").is_none(),
        "closed"
    );
}

/// Note FI17: when the new folder lacks the old tail, its closest names come first, none is highlighted, and Esc keeps
/// the old tail's text.
#[test]
fn a_missing_tail_keeps_its_text_and_lists_close_names_first() {
    let (_d, mut h) = harness_with(
        0.25,
        &[
            "textures/armor/iron/shield",
            "textures/armor/steel/apron",
            "textures/armor/steel/shield_d",
        ],
    );
    focus(&mut h);
    type_all(&mut h, ARMOR);
    segment_back(&mut h, 2);
    key(&mut h, Key::ArrowDown, ctrl());
    // steel has no shield: no mark.
    h.get_by_label("steel");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(free(&h), "textures/armor/steel/shield");
    let (d, a) = (
        "textures/armor/steel/shield_d",
        "textures/armor/steel/apron",
    );
    assert!(y(&h, d) < y(&h, a), "shield_d first");
    assert!(!selected(&h, d) && !selected(&h, a), "nothing highlighted");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(free(&h), "textures/armor/steel/shield");
}

/// The amber sections of the box's own galley (other panels use amber too).
fn amber_text(h: &H) -> Vec<String> {
    fn walk(s: &egui::Shape, out: &mut Vec<String>) {
        match s {
            egui::Shape::Text(t) if t.galley.job.text.starts_with("textures/") => {
                for sec in &t.galley.job.sections {
                    if sec.format.color == sdc_gui::ui::status::AMBER {
                        let r = sec.byte_range.start.0..sec.byte_range.end.0;
                        out.push(t.galley.job.text[r].to_string());
                    }
                }
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    h.output()
        .shapes
        .iter()
        .for_each(|c| walk(&c.shape, &mut out));
    out
}

/// UI-D final review Important 2: the offered names leave out `textures/pbr/` (Note AO8), but a set whose own
/// diffuse sits there is a known texture: its name is never amber, while a name the folder lacks still is.
#[test]
fn a_set_under_textures_pbr_is_not_amber() {
    use engine::config::{GreenConvention, Subtype};
    use sdc_gui::model::Answer;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let tex = d.path().join("textures");
    std::fs::create_dir_all(tex.join("pbr")).unwrap();
    std::fs::rename(tex.join("landscape"), tex.join("pbr/landscape")).unwrap();
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.decide(
        "pbr/landscape/rock01.png",
        Answer::Subtype(Subtype::PbrAlbedo),
    );
    m.decide(
        "pbr/landscape/rock01_normal.png",
        Answer::Subtype(Subtype::PbrNormal),
    );
    m.decide(
        "pbr/landscape/rock01_normal.png",
        Answer::Green(GreenConvention::Directx),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_known(KnownPaths {
        textures: [("textures/landscape/rock02.dds".to_string(), 1u16)].into(),
        mesh_dirs: vec![],
    });
    let mut h = harness_on(m, "pbr/landscape/rock01", 0.25);
    assert_eq!(free(&h), "textures/pbr/landscape/rock01");
    assert!(amber_text(&h).is_empty(), "{:?}", amber_text(&h));
    focus(&mut h);
    clear_and_type(&mut h, "nothere");
    assert_eq!(amber_text(&h), vec!["nothere".to_string()]);
}

/// Note FI17: a name that is no known texture draws its last segment amber, with a hover; it stays editable.
#[test]
fn an_unknown_tail_is_amber_with_a_hover() {
    let (_d, mut h) = harness();
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    assert!(amber_text(&h).is_empty(), "rock01 is known");
    focus(&mut h);
    clear_and_type(&mut h, "nothere");
    assert_eq!(amber_text(&h), vec!["nothere".to_string()]);
    h.get(
        by().role(Role::TextInput)
            .label(sdc_gui::ui::input_name::HINT),
    )
    .hover();
    h.run();
    h.get_by_label("No known texture at textures/landscape/nothere");
    assert_eq!(free(&h), format!("{DIR}nothere"), "still editable");
}
