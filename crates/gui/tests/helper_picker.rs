//! Helper textures Task 9 (Note DT18): each packed channel row on the set card picks its source: a greyscale helper
//! of the set or a value. The pick writes `helper.fills` of every helper it moves the channel from or to, as one undo
//! step. Temp dirs only.
mod support;
use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use engine::pack::ChannelSource;
use image::{ImageBuffer, Luma, Rgb};
use sdc_gui::model::pack_fills::channel_helpers;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use support::session;

type H = Harness<'static, AppModel>;

const RMAOS: &str = "x_rmaos";
const AO_SOURCE: &str = "AO channel source";

/// A folder with `files` (a diffuse for names without `_`, flat normals for `_n`, grey ramps for the rest) and `toml`
/// as its `sdc.toml`, opened with high-confidence suggestions accepted.
fn fixture(dir: &Path, files: &[&str], toml: &str) -> AppModel {
    for f in files {
        let p = dir.join(f);
        if f.ends_with("_n.png") {
            ImageBuffer::from_fn(64, 64, |_, _| Rgb([128u8, 128, 255]))
                .save(p)
                .unwrap();
        } else if !f.contains('_') {
            ImageBuffer::from_fn(64, 64, |x, y| Rgb([(x * 4) as u8, (y * 4) as u8, 90u8]))
                .save(p)
                .unwrap();
        } else {
            ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 3) as u8]))
                .save(p)
                .unwrap();
        }
    }
    if !toml.is_empty() {
        std::fs::write(dir.join("sdc.toml"), toml).unwrap();
    }
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.to_path_buf());
    idle(&mut m);
    m
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

/// `x_ao.png` feeds `rmaos.b` by its hand-set `helper.fills`; `x_cavity2.png` is an AO helper by its hand-set role.
/// (A hand-set role alone is assigned and would win the channel by Ruling 8, so `x_ao` holds it by hand here.)
const TWO_AO: [&str; 4] = ["x.png", "x_roughness.png", "x_ao.png", "x_cavity2.png"];
const TWO_AO_TOML: &str = "[files.\"x_ao.png\"]\nhelper.fills = [\"rmaos.b\"]\n\n[files.\"x_cavity2.png\"]\nhelper.role = \"ao\"\n";

/// The set editor of the `x` set at `size`.
fn editor(mut m: AppModel, size: egui::Vec2) -> H {
    let key = m
        .sets
        .iter()
        .find(|s| s.key.ends_with('x'))
        .expect("the x set")
        .key
        .clone();
    m.select_set(&key);
    let mut h = Harness::builder()
        .with_size(size)
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.run();
    h
}

fn value(h: &H, key: &str, field: &str) -> Option<Value> {
    h.state().project.as_ref().unwrap().file_value(key, field)
}

fn settle(h: &mut H) {
    h.run();
    idle(h.state_mut());
    h.run();
    h.run();
}

/// The file name of the source of channel `c` of `x_rmaos`, or `None` for a fill or default.
fn source_of(m: &AppModel, c: usize) -> Option<String> {
    let pk = m.item(RMAOS)?.job.as_ref()?.pack.as_ref()?;
    match &pk.channels[c] {
        ChannelSource::Source { path, .. } => {
            Some(path.file_name()?.to_string_lossy().into_owned())
        }
        _ => None,
    }
}

/// Opens the combo named `name` and picks `option` (the nearest node of that label below the combo).
fn pick(h: &mut H, name: &str, option: &str) {
    h.get_by_role_and_label(Role::ComboBox, name).scroll_to_me();
    h.run();
    let below = h.get_by_role_and_label(Role::ComboBox, name).rect().max.y;
    h.get_by_role_and_label(Role::ComboBox, name).click();
    h.run();
    h.query_all_by_label(option)
        .filter(|n| n.rect().min.y >= below - 1.0)
        .min_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .unwrap_or_else(|| panic!("no {option} under {name}"))
        .click();
    settle(h);
}

/// The options of the open combo `name`: the labels of the selectable buttons under it.
fn options(h: &mut H, name: &str) -> Vec<String> {
    h.get_by_role_and_label(Role::ComboBox, name).scroll_to_me();
    h.run();
    let below = h.get_by_role_and_label(Role::ComboBox, name).rect().max.y;
    h.get_by_role_and_label(Role::ComboBox, name).click();
    h.run();
    let opts: Vec<String> = h
        .query_all_by_role(Role::Button)
        .filter(|n| n.rect().min.y >= below - 1.0)
        .filter_map(|n| n.accesskit_node().label())
        .collect();
    h.key_press(egui::Key::Escape);
    h.run();
    opts
}

#[test]
fn picker_lists_greyscale_helpers_of_set_only() {
    let d = tempfile::tempdir().unwrap();
    let files = [
        "x.png",
        "x_ao.png",
        "x_n.png",
        "x_bump.png",
        "y.png",
        "y_ao.png",
    ];
    let toml = "[files.\"x_bump.png\"]\nhelper.role = \"bump\"\n";
    let m = fixture(d.path(), &files, toml);
    // The bump layer and the other set's helper are helpers, just not this channel's.
    let helpers: Vec<&str> = m
        .items
        .iter()
        .filter(|i| i.helper.is_some())
        .map(|i| i.name.as_str())
        .collect();
    assert!(
        helpers.contains(&"x_bump.png") && helpers.contains(&"y_ao.png"),
        "{helpers:?}"
    );
    let listed: Vec<String> = channel_helpers(&m, RMAOS)
        .into_iter()
        .map(|(_, f, _)| f)
        .collect();
    assert_eq!(listed, ["x_ao.png"]);
    let mut h = editor(m, egui::vec2(1400.0, 900.0));
    let opts = options(&mut h, AO_SOURCE);
    assert!(opts.iter().any(|o| o == "x_ao.png"), "{opts:?}");
    assert!(opts.iter().any(|o| o == "Value"), "{opts:?}");
    assert!(
        !opts.iter().any(|o| o == "x_bump.png" || o == "y_ao.png"),
        "{opts:?}"
    );
}

#[test]
fn picking_b_removes_channel_from_a() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path(), &TWO_AO, TWO_AO_TOML);
    assert_eq!(
        source_of(&m, 2).as_deref(),
        Some("x_ao.png"),
        "precondition"
    );
    let mut h = editor(m, egui::vec2(1400.0, 900.0));
    let combo = h.get_by_role_and_label(Role::ComboBox, AO_SOURCE);
    assert_eq!(
        combo.accesskit_node().value().as_deref(),
        Some("Source: x_ao.png")
    );
    pick(&mut h, AO_SOURCE, "x_cavity2.png");
    assert_eq!(value(&h, "x_ao.png", "helper.fills"), Some(json!([])));
    assert_eq!(
        value(&h, "x_cavity2.png", "helper.fills"),
        Some(json!(["rmaos.b"]))
    );
    assert_eq!(source_of(h.state(), 2).as_deref(), Some("x_cavity2.png"));
    // The roughness helper was not touched: its suggestion stays live.
    assert_eq!(value(&h, "x_roughness.png", "helper.fills"), None);
    let combo = h.get_by_role_and_label(Role::ComboBox, AO_SOURCE);
    assert_eq!(
        combo.accesskit_node().value().as_deref(),
        Some("Source: x_cavity2.png")
    );
}

#[test]
fn picking_value_removes_channel_from_all_helpers() {
    let d = tempfile::tempdir().unwrap();
    let m = fixture(d.path(), &["x.png", "x_roughness.png", "x_ao.png"], "");
    assert_eq!(source_of(&m, 2).as_deref(), Some("x_ao.png"), "suggested");
    let mut h = editor(m, egui::vec2(1400.0, 900.0));
    pick(&mut h, AO_SOURCE, "Value");
    assert_eq!(value(&h, "x_ao.png", "helper.fills"), Some(json!([])));
    assert_eq!(value(&h, "x_roughness.png", "helper.fills"), None);
    assert_eq!(source_of(h.state(), 2), None);
    let combo = h.get_by_role_and_label(Role::ComboBox, AO_SOURCE);
    assert_eq!(
        combo.accesskit_node().value().as_deref(),
        Some("Source: value")
    );
    // The drag box appears on the AO row.
    let row_y = combo.rect().center().y;
    assert!(
        h.query_all_by_role(Role::SpinButton)
            .any(|n| (n.rect().center().y - row_y).abs() < 6.0),
        "the AO value box"
    );
    h.state_mut()
        .set_pack_fill(RMAOS, engine::config::RawMap::Ao, Some(128));
    settle(&mut h);
    assert_eq!(value(&h, RMAOS, "packing.fill.ao"), Some(json!(128)));
    let pk = h.state().item(RMAOS).unwrap().job.as_ref().unwrap();
    assert!(
        matches!(
            pk.pack.as_ref().unwrap().channels[2],
            ChannelSource::Fill { code: 128, .. }
        ),
        "{:?}",
        pk.pack.as_ref().unwrap().channels[2]
    );
}

#[test]
fn picker_is_one_undo_step() {
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(
        fixture(d.path(), &TWO_AO, TWO_AO_TOML),
        egui::vec2(1400.0, 900.0),
    );
    assert!(!h.state().can_undo());
    pick(&mut h, AO_SOURCE, "x_cavity2.png");
    assert_eq!(
        value(&h, "x_cavity2.png", "helper.fills"),
        Some(json!(["rmaos.b"]))
    );
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    settle(&mut h);
    assert_eq!(
        value(&h, "x_ao.png", "helper.fills"),
        Some(json!(["rmaos.b"]))
    );
    assert_eq!(value(&h, "x_cavity2.png", "helper.fills"), None);
    assert!(!h.state().can_undo(), "one step");
}

/// Re-picking the channel's current source writes nothing.
#[test]
fn picking_the_current_source_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(
        fixture(d.path(), &["x.png", "x_roughness.png", "x_ao.png"], ""),
        egui::vec2(1400.0, 900.0),
    );
    pick(&mut h, AO_SOURCE, "x_ao.png");
    assert_eq!(value(&h, "x_ao.png", "helper.fills"), None);
    assert!(!h.state().can_undo());
}

#[test]
fn picker_combo_inside_card_at_1100() {
    use egui::containers::panel::PanelState;
    let d = tempfile::tempdir().unwrap();
    let mut h = editor(
        fixture(d.path(), &TWO_AO, TWO_AO_TOML),
        egui::vec2(1100.0, 800.0),
    );
    let panel = PanelState::load(&h.ctx, egui::Id::new("set-panel"))
        .expect("the set panel")
        .outer_rect;
    let names: Vec<String> = h
        .query_all_by_role(Role::ComboBox)
        .filter_map(|n| n.accesskit_node().label())
        .filter(|l| l.ends_with(" channel source"))
        .collect();
    assert_eq!(names.len(), 4, "R, G, B, A: {names:?}");
    for name in names {
        // The slot rows scroll sideways in a narrow centre column (Task 2.2): scroll to it as a user would.
        h.get_by_role_and_label(Role::ComboBox, &name)
            .scroll_to_me();
        h.run();
        let r = h.get_by_role_and_label(Role::ComboBox, &name).rect();
        assert!(
            r.max.x <= panel.min.x,
            "combo {r:?} runs under the set panel at {}",
            panel.min.x
        );
        // Not clipped by the card's scroll area: the clickable rect is the whole combo.
        let clickable = h.ctx.viewport(|v| {
            // Kittest ends a run between passes: the last drawn pass is `prev_pass`.
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, ws)| ws.iter())
                .filter(|w| {
                    (w.rect.min - r.min).length() < 1.0 && (w.rect.max - r.max).length() < 1.0
                })
                .map(|w| w.interact_rect)
                .next()
        });
        let clickable = clickable.expect("the combo's widget rect");
        assert!(
            (clickable.min - r.min).length() < 1.0 && (clickable.max - r.max).length() < 1.0,
            "combo {r:?} is clipped to {clickable:?}"
        );
    }
}

/// Review pictures of the card's pickers at 1100, 1280 and 1600 px, closed and open (egui-ui-rules step 5). Needs a
/// GPU and `SDC_SHOTS_DIR`: `cargo test -p sdc-gui --test helper_picker -- --ignored shots`.
#[test]
#[ignore]
fn shots() {
    let Ok(out) = std::env::var("SDC_SHOTS_DIR") else {
        return;
    };
    let d = tempfile::tempdir().unwrap();
    for (w, hgt) in [(1100.0, 800.0), (1280.0, 800.0), (1600.0, 900.0)] {
        for open in [false, true] {
            let mut m = fixture(d.path(), &TWO_AO, TWO_AO_TOML);
            let key = m
                .sets
                .iter()
                .find(|s| s.key.ends_with('x'))
                .unwrap()
                .key
                .clone();
            m.select_set(&key);
            let mut h = Harness::builder()
                .with_size(egui::vec2(w, hgt))
                .with_pixels_per_point(1.0)
                .wgpu()
                .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
            sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
            h.run_steps(10);
            if open {
                h.get_by_role_and_label(Role::ComboBox, AO_SOURCE)
                    .scroll_to_me();
                h.run_steps(4);
                h.get_by_role_and_label(Role::ComboBox, AO_SOURCE).click();
                h.run_steps(4);
            }
            let img = h.render().expect("render");
            let state = if open { "open" } else { "closed" };
            img.save(Path::new(&out).join(format!("helper-picker-{state}-{w}.png")))
                .expect("save png");
        }
    }
}
