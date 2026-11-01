//! The demo project, the harness and one function per documentation shot. Everything is generated here: no game or
//! mod assets (docs/DOCUMENTATION.md §5).

use crate::manifest;
use crate::support;
use eframe::egui::{self, accesskit::Role, Pos2, Rect};
use eframe::egui_wgpu::{self, wgpu};
use egui_kittest::kittest::{NodeT as _, Queryable};
use egui_kittest::Harness;
use image::{ImageBuffer, Luma, Rgb, RgbaImage};
use sdc_gui::app::SdcApp;
use sdc_gui::model::bottom::BottomTab;
use sdc_gui::model::rename::RenameMode;
use sdc_gui::model::{AfterExport, Answer, AppModel, Dialog, DocKind};
use sdc_gui::settings::GuiSettings;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub struct Frame {
    pub img: RgbaImage,
    /// The surface the shot shows; `None` keeps the whole window.
    pub crop: Option<Rect>,
}

/// The argument is a fresh, empty temp folder of the shot's own.
pub type Scenario = fn(&Path) -> Frame;

pub const SCENARIOS: &[(&str, Scenario)] = &[
    ("main_window", main_window),
    ("start_screen", start_screen),
    ("open_menu", open_menu),
    ("export_to_menu", export_to_menu),
    ("preset_combo", preset_combo),
    ("command_palette", command_palette),
    ("shortcut_sheet", shortcut_sheet),
    ("advanced_drawer", advanced_drawer),
    ("problems_tab", problems_tab),
    ("messages_tab", messages_tab),
    ("tile_menu", tile_menu),
    ("filter_popup", filter_popup),
    ("members_of_kind", members_of_kind),
    ("set_card_menu", set_card_menu),
    ("member_menu", member_menu),
    ("review_tab", review_tab),
    ("presets_tab", presets_tab),
    ("json_editor", json_editor),
    ("close_unsaved", close_unsaved),
    ("open_unsaved", open_unsaved),
    ("reload_unsaved", reload_unsaved),
    ("external_change", external_change),
    ("export_running", export_running),
    ("error_dialog", error_dialog),
    ("reset_all", reset_all),
    ("copy_settings", copy_settings),
    ("rename_inputs", rename_inputs),
    ("rename_outputs", rename_outputs),
    ("generate_dialog", generate_dialog),
    ("marker_dialog", marker_dialog),
    ("set_editor", set_editor),
    ("assign_menu", assign_menu),
    ("slot_menu", slot_menu),
    ("pgp_material", pgp_material),
    ("texture_panel", texture_panel),
    ("rnm_panel", rnm_panel),
    ("helper_panel", helper_panel),
    ("cube_panel", cube_panel),
    ("preview_encoded", preview_encoded),
    ("preview_normal", preview_normal),
    ("meshes_view", meshes_view),
    ("plugin_refs", plugin_refs),
    ("mo2_popup", mo2_popup),
    ("skin_card", skin_card),
    ("landscape_card", landscape_card),
];

type H = Harness<'static, SdcApp>;

static ADAPTER: OnceLock<String> = OnceLock::new();

/// The adapter the first harness chose, with a note when it is not a CPU adapter with BC textures.
pub fn adapter_name() -> String {
    ADAPTER.get().cloned().unwrap_or_default()
}

/// A made-up mod: an architecture wall (diffuse, `_n`, `_m`), a PBR cobble set, a 200x120 banner (not a power of
/// two), vanilla armour sets with `_n`/`_p`/`_m`, `_bump` and `_detailnorm`, a PBR metal set with `_rmaos`, a body
/// skin and a fern with cut-out alpha. Returns its `textures` folder.
pub fn demo(dir: &Path) -> PathBuf {
    let t = dir.join("textures");
    let mk = |rel: &str| {
        let p = t.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    };
    let rgb = |rel: &str, w: u32, h: u32, f: &dyn Fn(u32, u32) -> [u8; 3]| {
        ImageBuffer::from_fn(w, h, |x, y| Rgb(f(x, y)))
            .save(mk(rel))
            .unwrap();
    };
    let mortar = |x: u32, y: u32| {
        y % 32 < 3 || (x + if (y / 32).is_multiple_of(2) { 0 } else { 32 }) % 64 < 3
    };
    let flat = |_: u32, _: u32| [128u8, 128, 255];
    rgb("architecture/stone/stonewall.png", 256, 256, &|x, y| {
        let n = ((x * 7 + y * 13 + (y / 32) * 29) % 23) as u8;
        if mortar(x, y) {
            [150, 146, 138]
        } else {
            [118 + n, 84 + n / 2, 66 + n / 3]
        }
    });
    rgb("architecture/stone/stonewall_n.png", 256, 256, &|x, y| {
        if mortar(x, y) {
            [128, 100, 220]
        } else {
            [128, 128, 255]
        }
    });
    rgb("architecture/stone/stonewall_m.png", 256, 256, &|x, _| {
        [(x / 2) as u8, 40, 40]
    });
    rgb("architecture/stone/cobble.png", 256, 256, &|x, y| {
        let c = ((x / 24 + y / 24) % 3) as u8 * 16;
        [90 + c, 88 + c, 84 + c]
    });
    rgb("architecture/stone/cobble_n.png", 256, 256, &flat);
    rgb("architecture/stone/cobble_rmaos.png", 256, 256, &|x, y| {
        [200, ((x + y) % 60) as u8, 255]
    });
    rgb("architecture/stone/cobble_p.png", 256, 256, &|x, _| {
        [(x % 128) as u8 * 2; 3]
    });
    rgb("architecture/stone/banner.png", 200, 120, &|_, y| {
        [160, 30 + (y / 4) as u8 % 60, 40]
    });
    rgb("architecture/stone/banner_n.png", 200, 120, &flat);

    let colour = |rel: &str, r: u8, g: u8, b: u8| {
        rgb(rel, 128, 128, &|x, y| {
            let v = ((x / 16 + y / 16) % 2) as u8 * 40;
            [
                r.saturating_add(v),
                g.saturating_add((x / 2) as u8),
                b.saturating_add((y / 3) as u8),
            ]
        })
    };
    let normal = |rel: &str| {
        rgb(rel, 128, 128, &|x, y| {
            [
                128u8.wrapping_add((x % 32) as u8),
                128u8.wrapping_add((y % 32) as u8),
                255,
            ]
        })
    };
    let mono = |rel: &str, v: u8| {
        ImageBuffer::from_fn(128, 128, |x, y| {
            Luma([v.wrapping_add(((x ^ y) % 64) as u8)])
        })
        .save(mk(rel))
        .unwrap()
    };
    for (n, c) in [
        ("steel", (90, 90, 110)),
        ("gold", (180, 140, 40)),
        ("iron", (70, 60, 55)),
    ] {
        colour(&format!("armor/{n}.png"), c.0, c.1, c.2);
        normal(&format!("armor/{n}_n.png"));
    }
    mono("armor/steel_p.png", 100);
    mono("armor/steel_m.png", 60);
    mono("armor/gold_m.png", 80);
    mono("armor/iron_bump.png", 120);
    normal("armor/iron_detailnorm.png");
    colour("pbr/metal.png", 150, 150, 160);
    normal("pbr/metal_n.png");
    rgb("pbr/metal_rmaos.png", 128, 128, &|x, y| {
        [(x * 2) as u8, 200, (y * 2) as u8]
    });
    colour("actors/body.png", 200, 150, 130);
    normal("actors/body_n.png");
    ImageBuffer::from_fn(128, 128, |x, y| {
        image::Rgba([60u8, 140, 60, if (x + y) % 20 < 10 { 255 } else { 0 }])
    })
    .save(mk("plants/fern.png"))
    .unwrap();
    t
}

/// kittest's adapter order (egui_kittest 0.36.2 `wgpu.rs`), CPU first, except that an adapter without BC textures
/// (which the 2D preview uploads) goes last.
fn cpu_first() -> egui_wgpu::NativeAdapterSelectorMethod {
    Arc::new(|adapters, _surface| {
        let mut adapters = adapters.iter().collect::<Vec<_>>();
        adapters.sort_by_key(|a| match a.get_info().backend {
            wgpu::Backend::Metal => 0,
            wgpu::Backend::Vulkan => 1,
            wgpu::Backend::Dx12 => 2,
            wgpu::Backend::Gl => 4,
            wgpu::Backend::BrowserWebGpu => 6,
            wgpu::Backend::Noop => 7,
        });
        adapters.sort_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::Cpu => 0,
            wgpu::DeviceType::DiscreteGpu => 1,
            wgpu::DeviceType::Other
            | wgpu::DeviceType::IntegratedGpu
            | wgpu::DeviceType::VirtualGpu => 2,
        });
        adapters.sort_by_key(|a| {
            !a.features()
                .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
        });
        adapters
            .first()
            .map(|a| (*a).clone())
            .ok_or_else(|| "No adapter found".to_owned())
    })
}

/// The real `SdcApp` through `eframe::App::ui`, with the app's device setup on kittest's adapter and the manifest's
/// render settings.
pub fn harness(m: AppModel) -> H {
    let render = manifest::load(&manifest::repo_root().join("docs/img/gui/shots.toml")).render;
    let mut setup = sdc_gui::render::wgpu_configuration().wgpu_setup;
    if let egui_wgpu::WgpuSetup::CreateNew(ref mut c) = setup {
        c.native_adapter_selector = Some(cpu_first());
    }
    // kittest takes only a RenderState no renderer has used, so each harness gets its own device.
    let rs =
        egui_kittest::wgpu::create_render_state(setup, egui_wgpu::RendererOptions::PREDICTABLE);
    let info = rs.adapter.get_info();
    let mut name = info.name.clone();
    if info.device_type != wgpu::DeviceType::Cpu {
        name.push_str(" (no CPU adapter: shots are not comparable with ones made on WARP)");
    }
    if !rs
        .adapter
        .features()
        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    {
        name.push_str(" (no BC textures: the 2D preview cannot draw)");
    }
    let _ = ADAPTER.set(name);
    let mut app = SdcApp::new(m).with_render_state(Some(&rs));
    app.meshes.procs = sdc_gui::model::meshes::no_procs;
    let mut frame = eframe::Frame::_new_kittest();
    let theme = if render.theme == "light" {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    let h = Harness::builder()
        .with_size(egui::vec2(render.size[0] as f32, render.size[1] as f32))
        .with_pixels_per_point(render.pixels_per_point)
        .with_theme(theme)
        .with_os(egui::os::OperatingSystem::Windows)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs))
        .build_ui_state(
            move |ui, app: &mut SdcApp| eframe::App::ui(app, ui, &mut frame),
            app,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Frames with real time between them, so workers (thumbnails, previews, plans) land.
pub fn settle(h: &mut H, frames: usize) {
    for _ in 0..frames {
        h.run_steps(1);
        std::thread::sleep(Duration::from_millis(40));
    }
    let _ = h.state_mut().model.wait_idle(Duration::from_secs(60));
    h.run_steps(3);
}

/// The first widget whose accessible label equals `label` (leftmost, then topmost).
fn rect_of(h: &H, label: &str) -> Rect {
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .min_by(|a, b| (a.min.x, a.min.y).partial_cmp(&(b.min.x, b.min.y)).unwrap())
        .unwrap_or_else(|| panic!("no widget labelled {label:?}"))
}

/// The topmost widget of `role` whose label starts with `start`.
fn rect_starting(h: &H, role: Role, start: &str) -> Rect {
    h.query_all_by_role(role)
        .filter(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with(start))
        })
        .map(|n| n.rect())
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap_or_else(|| {
            let labels: Vec<String> = h
                .query_all_by_role(role)
                .filter_map(|n| n.accesskit_node().label())
                .collect();
            panic!("no {role:?} starting {start:?}; there are {labels:?}")
        })
}

/// 3x5 digit glyphs, one row per `u8` (bits 2..0).
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 2, 2],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// A filled, white-ringed circle at `c` with the number `n` (1 or 2 digits) in white.
fn marker(img: &mut RgbaImage, c: Pos2, n: u32) {
    let (r, px) = (9.0f32, 2i32);
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (cx, cy) = (c.x.round() as i32, c.y.round() as i32);
    let mut put = |x: i32, y: i32, col: [u8; 4]| {
        if (0..w).contains(&x) && (0..h).contains(&y) {
            img.put_pixel(x as u32, y as u32, image::Rgba(col));
        }
    };
    for gy in -12..=12 {
        for gx in -12..=12 {
            let d = ((gx * gx + gy * gy) as f32).sqrt();
            if d <= r {
                put(cx + gx, cy + gy, [214, 40, 57, 255]);
            } else if d <= r + 2.0 {
                put(cx + gx, cy + gy, [255, 255, 255, 255]);
            }
        }
    }
    let digits: Vec<u32> = n
        .to_string()
        .chars()
        .map(|c| c.to_digit(10).unwrap())
        .collect();
    let tw = digits.len() as i32 * 4 * px - px; // 3 columns + 1 gap per digit
    let (x0, y0) = (cx - tw / 2, cy - 5 * px / 2);
    for (i, d) in digits.iter().enumerate() {
        for (ry, row) in DIGITS[*d as usize].iter().enumerate() {
            for rx in 0..3 {
                if row >> (2 - rx) & 1 == 1 {
                    for dy in 0..px {
                        for dx in 0..px {
                            put(
                                x0 + i as i32 * 4 * px + rx * px + dx,
                                y0 + ry as i32 * px + dy,
                                [255; 4],
                            );
                        }
                    }
                }
            }
        }
    }
}

const EXPORT: &str = "D:\\Mods\\StoneWork\\sdc-out";

/// Settings that read the same on every machine: a fixed export folder and recent list, an NVTT path that does not
/// exist (so the drawer never shows this machine's NVTT) and no motion.
fn settings() -> GuiSettings {
    GuiSettings {
        export_folder: Some(EXPORT.into()),
        recent: recent(),
        nvtt_path: Some("D:\\Tools\\NVTT\\nvcompress.exe".into()),
        reduce_motion: true,
        ..GuiSettings::default()
    }
}

fn recent() -> Vec<PathBuf> {
    vec![
        "D:\\Mods\\StoneWork\\textures".into(),
        "D:\\Mods\\Banners\\textures".into(),
    ]
}

/// The demo project opened with the cobble set on PBR and the export folder chosen, then `f`, then a settled harness.
fn opened_with(dir: &Path, f: impl FnOnce(&mut AppModel)) -> H {
    opened_extra(dir, &[], f)
}

/// `opened_with` with extra 128x128 grey files (paths under `textures`) written before the folder opens.
fn opened_extra(dir: &Path, extra: &[&str], f: impl FnOnce(&mut AppModel)) -> H {
    let root = demo(dir);
    for rel in extra {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        ImageBuffer::from_fn(128, 128, |x, y| Rgb([(x + 60) as u8, (y + 60) as u8, 120]))
            .save(p)
            .unwrap();
    }
    let mut m = AppModel::new(settings(), None, support::session());
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m.set_shader(
        "architecture/stone/cobble",
        Some(engine::shader::Shader::Pbr),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_export_folder(EXPORT.into());
    assert!(m.wait_idle(Duration::from_secs(60)));
    f(&mut m);
    assert!(m.wait_idle(Duration::from_secs(60)));
    // Opening put the temp folder first in the recent list.
    m.settings.recent = recent();
    let mut h = harness(m);
    settle(&mut h, 10);
    h
}

type Ids = std::collections::HashSet<String>;

fn ids(h: &H) -> Ids {
    h.query_all(egui_kittest::kittest::by())
        .map(|n| format!("{:?}", n.accesskit_node().id()))
        .collect()
}

/// The union of the nodes that `act` made appear: the menu, popup, window or panel it opened.
fn reveal(h: &mut H, act: impl FnOnce(&mut H)) -> Rect {
    let before = ids(h);
    act(h);
    settle(h, 4);
    let r = h
        .query_all(egui_kittest::kittest::by())
        .filter(|n| !before.contains(&format!("{:?}", n.accesskit_node().id())))
        .filter_map(|n| n.accesskit_node().bounding_box())
        .map(|b| {
            Rect::from_min_max(
                Pos2::new(b.x0 as f32, b.y0 as f32),
                Pos2::new(b.x1 as f32, b.y1 as f32),
            )
        })
        .filter(|r| r.is_positive())
        .fold(Rect::NOTHING, |a, b| a.union(b));
    assert!(r.is_positive(), "nothing appeared");
    r
}

/// The first widget labelled `label` (leftmost, then topmost).
fn node<'a>(h: &'a H, label: &'a str) -> egui_kittest::Node<'a> {
    h.query_all_by_label(label)
        .min_by(|a, b| {
            (a.rect().min.x, a.rect().min.y)
                .partial_cmp(&(b.rect().min.x, b.rect().min.y))
                .unwrap()
        })
        .unwrap_or_else(|| panic!("no widget labelled {label:?}; labels: {:?}", labels(h)))
}

fn labels(h: &H) -> Vec<String> {
    h.query_all(egui_kittest::kittest::by())
        .filter_map(|n| n.accesskit_node().label())
        .filter(|l| l.len() < 40)
        .collect()
}

/// The topmost button whose label starts with `start`.
fn button_starting<'a>(h: &'a H, start: &str) -> egui_kittest::Node<'a> {
    h.query_all_by_role(Role::Button)
        .filter(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with(start))
        })
        .min_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .unwrap_or_else(|| panic!("no button starting {start:?}"))
}

fn shot(h: &mut H, crop: Rect) -> Frame {
    Frame {
        img: h.render().expect("render"),
        crop: Some(crop),
    }
}

/// GUI-Main-Window: the window before a folder is open.
fn start_screen(_dir: &Path) -> Frame {
    let m = AppModel::new(settings(), None, support::session());
    let mut h = harness(m);
    settle(&mut h, 6);
    Frame {
        img: h.render().expect("render"),
        crop: None,
    }
}

/// GUI-Main-Window: the Open menu with two recent folders.
fn open_menu(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| node(h, "Open").click());
    let b = node(&h, "Open").rect();
    shot(&mut h, r.union(b))
}

/// GUI-Main-Window: the Export to ▾ menu.
fn export_to_menu(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| button_starting(h, "Export to").click());
    let b = button_starting(&h, "Export to").rect();
    shot(&mut h, r.union(b))
}

/// GUI-Main-Window: the preset combo open.
fn preset_combo(dir: &Path) -> Frame {
    fn combo<'a>(h: &'a H) -> egui_kittest::Node<'a> {
        let all: Vec<_> = h.query_all_by_role(Role::ComboBox).collect();
        let found = all.iter().position(|n| {
            let a = n.accesskit_node();
            a.value()
                .or_else(|| a.label())
                .is_some_and(|v| v.starts_with("default ("))
        });
        match found {
            Some(i) => all.into_iter().nth(i).unwrap(),
            None => panic!(
                "no preset combo; combos: {:?}",
                all.iter()
                    .map(|n| (n.accesskit_node().label(), n.accesskit_node().value()))
                    .collect::<Vec<_>>()
            ),
        }
    }
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| combo(h).click());
    let c = combo(&h).rect();
    shot(&mut h, r.union(c))
}

/// GUI-Keyboard: the command palette (Ctrl+K) with a search typed.
fn command_palette(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| {
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    });
    shot(&mut h, r)
}

/// GUI-Keyboard: the F1 shortcut sheet.
fn shortcut_sheet(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| h.key_press(egui::Key::F1));
    shot(&mut h, r)
}

/// GUI-Advanced-Drawer: the drawer open beside a selected texture.
fn advanced_drawer(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| m.select_item("architecture/stone/stonewall.png"));
    let r = reveal(&mut h, |h| {
        h.state_mut().model.settings.advanced_open = true;
    });
    shot(&mut h, r)
}

/// GUI-Problems: the Problems tab with the demo's rows.
fn problems_tab(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| {
        h.state_mut().model.toggle_tab(BottomTab::Problems)
    });
    shot(&mut h, r)
}

/// GUI-Bottom-Tabs: the Messages tab after a save.
fn messages_tab(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| {
        m.messages.clock = || "14:02:37".into();
        // Real notices: undo and redo of the shader change opened_with made.
        m.undo();
        assert!(m.wait_idle(Duration::from_secs(60)));
        m.redo();
    });
    let r = reveal(&mut h, |h| {
        h.state_mut().model.toggle_tab(BottomTab::Messages)
    });
    shot(&mut h, r)
}

/// GUI-Textures-Column: a tile's right-click menu.
fn tile_menu(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| m.select_item("architecture/stone/stonewall.png"));
    let r = reveal(&mut h, |h| node(h, "stonewall.png").click_secondary());
    shot(&mut h, r)
}

/// GUI-Textures-Column: the Filter ▾ popup.
fn filter_popup(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| node(h, "Filter ▾").click());
    let b = node(&h, "Filter ▾").rect();
    shot(&mut h, r.union(b))
}

/// GUI-Bulk-and-Rename: two sets selected, their menu with Select members of kind open.
fn members_of_kind(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| {
        let a = set_key(m, "architecture/stone/stonewall");
        let b = set_key(m, "architecture/stone/cobble");
        m.click_set(&a, false, false);
        m.click_set(&b, true, false);
    });
    let r = reveal(&mut h, |h| {
        node(h, "architecture/stone/stonewall").click_secondary();
        settle(h, 3);
        node(h, "Select members of kind ⏵").hover();
    });
    shot(&mut h, r)
}

/// GUI-Sets-Column: a set card's right-click menu.
fn set_card_menu(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| select_set(m, "architecture/stone/stonewall"));
    let r = reveal(&mut h, |h| {
        node(h, "architecture/stone/stonewall").click_secondary()
    });
    shot(&mut h, r)
}

/// GUI-Sets-Column: a member chip's right-click menu.
fn member_menu(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| node(h, "N").click_secondary());
    let b = node(&h, "N").rect();
    shot(&mut h, r.union(b))
}

/// GUI-Review-and-Export: Review in the bottom panel.
fn review_tab(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| h.state_mut().model.open_review());
    shot(&mut h, r)
}

/// GUI-Presets: the preset creator on a built-in preset.
fn presets_tab(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| {
        let m = &mut h.state_mut().model;
        m.open_presets();
        let id = engine::presets::default_preset_for(m.target).to_string();
        m.presets_model().select(&id);
    });
    shot(&mut h, r)
}

/// GUI-PGPatcher-JSON: the TruePBR JSON editor.
fn json_editor(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| h.state_mut().model.open_json_editor());
    shot(&mut h, r)
}

/// The demo project with `f` run on the model after the first frames: the dialog (or window) it opens, cropped.
fn dialog(dir: &Path, pre: impl FnOnce(&mut AppModel), f: impl FnOnce(&mut AppModel)) -> Frame {
    let mut h = opened_with(dir, pre);
    let r = reveal(&mut h, |h| f(&mut h.state_mut().model));
    shot(&mut h, r)
}

fn set_dialog(d: Dialog) -> impl FnOnce(&mut AppModel) {
    move |m| m.dialog = Some(d)
}

fn close_unsaved(dir: &Path) -> Frame {
    dialog(dir, |_| {}, set_dialog(Dialog::CloseUnsaved))
}
fn open_unsaved(dir: &Path) -> Frame {
    dialog(
        dir,
        |_| {},
        set_dialog(Dialog::OpenUnsaved("D:\\Mods\\Banners\\textures".into())),
    )
}
fn reload_unsaved(dir: &Path) -> Frame {
    dialog(dir, |_| {}, set_dialog(Dialog::ReloadUnsaved))
}
fn external_change(dir: &Path) -> Frame {
    dialog(
        dir,
        |_| {},
        set_dialog(Dialog::ExternalChange(DocKind::Project)),
    )
}
fn export_running(dir: &Path) -> Frame {
    dialog(
        dir,
        |_| {},
        set_dialog(Dialog::ExportRunning(AfterExport::Close)),
    )
}
fn error_dialog(dir: &Path) -> Frame {
    dialog(
        dir,
        |_| {},
        set_dialog(Dialog::Error(
            "Can't read D:\\Mods\\StoneWork\\textures\\sdc.toml: the file is locked by another program.".into(),
        )),
    )
}
fn reset_all(dir: &Path) -> Frame {
    dialog(
        dir,
        |_| {},
        set_dialog(Dialog::ConfirmResetAll(vec![
            "architecture/stone/stonewall.png".into(),
            "architecture/stone/cobble.png".into(),
        ])),
    )
}
fn copy_settings(dir: &Path) -> Frame {
    dialog(
        dir,
        |m| {
            m.select_item("architecture/stone/stonewall.png");
            m.click_item("architecture/stone/cobble.png", true, false);
        },
        |m| m.open_copy_settings(),
    )
}
fn rename_inputs(dir: &Path) -> Frame {
    dialog(
        dir,
        |m| {
            m.select_item("architecture/stone/stonewall.png");
            m.click_item("architecture/stone/cobble.png", true, false);
        },
        |m| m.open_rename(RenameMode::Inputs),
    )
}
fn rename_outputs(dir: &Path) -> Frame {
    dialog(
        dir,
        |m| select_set(m, "architecture/stone/stonewall"),
        |m| m.open_rename(RenameMode::Outputs),
    )
}
fn generate_dialog(dir: &Path) -> Frame {
    dialog(
        dir,
        |_| {},
        |m| m.open_gen("architecture/stone/stonewall", None),
    )
}
fn marker_dialog(dir: &Path) -> Frame {
    dialog(dir, |_| {}, |m| m.open_marker_dialog())
}

fn set_key(m: &AppModel, id: &str) -> String {
    m.sets
        .iter()
        .find(|s| s.key.ends_with(id))
        .unwrap_or_else(|| panic!("no set {id}"))
        .key
        .clone()
}

fn select_set(m: &mut AppModel, id: &str) {
    let k = set_key(m, id);
    m.select_set(&k);
}

/// The rightmost widget labelled `label` (the set editor's, not the Sets column's).
fn rightmost<'a>(h: &'a H, label: &'a str) -> egui_kittest::Node<'a> {
    h.query_all_by_label(label)
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap_or_else(|| panic!("no widget labelled {label:?}"))
}

/// GUI-Set-Editor: the set editor of the stone wall.
fn set_editor(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| {
        select_set(&mut h.state_mut().model, "architecture/stone/stonewall")
    });
    shot(&mut h, r)
}

/// GUI-Set-Editor: an empty slot's Assign… menu.
fn assign_menu(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| select_set(m, "architecture/stone/cobble"));
    let r = reveal(&mut h, |h| node(h, "Assign…").click());
    let b = node(&h, "Assign…").rect();
    shot(&mut h, r.union(b))
}

/// GUI-Sets-Column: a set-editor slot row's right-click menu (the slot menus).
fn slot_menu(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| select_set(m, "architecture/stone/cobble"));
    let r = reveal(&mut h, |h| rightmost(h, "RMAOS").click_secondary());
    let b = rightmost(&h, "RMAOS").rect();
    shot(&mut h, r.union(b))
}

/// GUI-PGPatcher-JSON: the Material tab of a TruePBR set.
fn pgp_material(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| {
        select_set(&mut h.state_mut().model, "architecture/stone/cobble")
    });
    shot(&mut h, r)
}

/// GUI-Texture-Panel: the settings of a diffuse with cut-out alpha.
fn texture_panel(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| {
        h.state_mut().model.select_item("plants/fern.png")
    });
    shot(&mut h, r)
}

/// GUI-Helpers-RNM-and-Cubemaps: a normal map with two RNM layers.
fn rnm_panel(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| {
        h.state_mut().model.select_item("armor/iron_n.png")
    });
    shot(&mut h, r)
}

/// GUI-Helpers-RNM-and-Cubemaps: a roughness helper's panel.
fn helper_panel(dir: &Path) -> Frame {
    let mut h = opened_extra(
        dir,
        &[
            "props/crate.png",
            "props/crate_roughness.png",
            "props/crate_opacity.png",
        ],
        |_| {},
    );
    let r = reveal(&mut h, |h| {
        h.state_mut().model.select_item("props/crate_roughness.png")
    });
    shot(&mut h, r)
}

/// GUI-Helpers-RNM-and-Cubemaps: a six-face cube's settings.
fn cube_panel(dir: &Path) -> Frame {
    let faces = ["px", "nx", "py", "ny", "pz", "nz"].map(|f| format!("sky/env_{f}.png"));
    let extra: Vec<&str> = faces.iter().map(|s| s.as_str()).collect();
    let mut h = opened_extra(dir, &extra, |m| {
        for f in &faces {
            m.decide(f, Answer::Subtype(engine::config::Subtype::Cubemap));
            assert!(m.wait_idle(Duration::from_secs(60)));
        }
    });
    let r = reveal(&mut h, |h| {
        h.state_mut().model.select_item("sky/env_px.png")
    });
    shot(&mut h, r)
}

/// GUI-Preview: the 2D preview of the stone wall after Encode now.
fn preview_encoded(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| m.select_item("architecture/stone/stonewall.png"));
    let r = reveal(&mut h, |h| {
        let m = &mut h.state_mut().model;
        m.expand("architecture/stone/stonewall.png");
        m.encode_now();
    });
    settle(&mut h, 10);
    shot(&mut h, r)
}

/// GUI-Preview: a normal map expanded, not yet encoded.
fn preview_normal(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |m| m.select_item("architecture/stone/stonewall_n.png"));
    let r = reveal(&mut h, |h| {
        h.state_mut()
            .model
            .expand("architecture/stone/stonewall_n.png")
    });
    shot(&mut h, r)
}

fn nif_row(copy_id: i64, path: &str) -> modscan::store::NifRow {
    modscan::store::NifRow {
        copy_id,
        path: path.into(),
        wins: true,
        overridden_by: None,
        n_copies: 1,
        derived: false,
        derive_check: None,
        le_mesh: false,
        problem: None,
        patched_in_game: false,
    }
}

/// The demo project in the Meshes view, with a made-up scan: five meshes from three mods.
fn meshes(dir: &Path, f: impl FnOnce(&mut sdc_gui::model::meshes::MeshesModel)) -> H {
    use modscan::store::{ModRow, NifRow};
    let mut h = opened_with(dir, |_| {});
    {
        let m = &mut h.state_mut().meshes;
        let r#mod = |id, name: &str, derived| ModRow {
            id,
            name: name.into(),
            kind: 1,
            priority: id as i32,
            derived,
            nifs: 2,
        };
        m.mods = vec![
            r#mod(1, "StoneWork", false),
            r#mod(2, "Banners of the Hold", false),
            r#mod(3, "PGPatcher output", true),
        ];
        m.winners = Arc::new(vec![
            (
                nif_row(1, "meshes/architecture/stone/wall01.nif"),
                "StoneWork".into(),
            ),
            (
                NifRow {
                    n_copies: 2,
                    ..nif_row(2, "meshes/architecture/stone/wall02.nif")
                },
                "StoneWork".into(),
            ),
            (
                NifRow {
                    derived: true,
                    patched_in_game: true,
                    ..nif_row(3, "meshes/architecture/stone/cobblefloor.nif")
                },
                "PGPatcher output".into(),
            ),
            (
                nif_row(4, "meshes/clutter/banner01.nif"),
                "Banners of the Hold".into(),
            ),
            (
                NifRow {
                    le_mesh: true,
                    ..nif_row(5, "meshes/clutter/banner02.nif")
                },
                "Banners of the Hold".into(),
            ),
        ]);
        f(m);
    }
    sdc_gui::ui::header::set_meshes_view(&h.ctx, true);
    settle(&mut h, 6);
    h
}

/// GUI-Meshes: the Meshes view with its mesh list.
fn meshes_view(dir: &Path) -> Frame {
    let mut h = meshes(dir, |_| {});
    Frame {
        img: h.render().expect("render"),
        crop: None,
    }
}

/// GUI-Meshes: the Plugin file references window.
fn plugin_refs(dir: &Path) -> Frame {
    let mut h = meshes(dir, |_| {});
    let r = reveal(&mut h, |h| {
        let m = &mut h.state_mut().meshes;
        m.file_refs = Some(sdc_gui::model::meshes::FileRefs {
            missing: vec![modscan::plugin_store::MissingRef {
                path: "textures/clutter/banner_missing.dds".into(),
                n_records: 2,
                records: vec!["BannerTXST [00000AB0] TXST — Banners.esp".into()],
            }],
            unused: vec![
                "textures/architecture/stone/oldwall.dds".into(),
                "textures/clutter/banner03.dds".into(),
            ],
        });
        m.refs_open = true;
    });
    let _ = r;
    let w = h
        .query_all_by_role(Role::Window)
        .find(|n| n.accesskit_node().label().as_deref() == Some("Plugin file references"))
        .expect("the window")
        .rect();
    shot(&mut h, w)
}

/// GUI-Main-Window: the MO2… popup.
fn mo2_popup(dir: &Path) -> Frame {
    let mut h = opened_with(dir, |_| {});
    let r = reveal(&mut h, |h| node(h, "MO2…").click());
    let b = node(&h, "MO2…").rect();
    shot(&mut h, r.union(b))
}

/// A set's Material tab after giving it `shader`.
fn material_tab(dir: &Path, set: &'static str, shader: engine::shader::Shader) -> Frame {
    let mut h = opened_with(dir, |m| {
        m.set_shader(set, Some(shader));
        assert!(m.wait_idle(Duration::from_secs(60)));
    });
    let r = reveal(&mut h, |h| select_set(&mut h.state_mut().model, set));
    shot(&mut h, r)
}

/// GUI-Set-Materials: the Advanced skin card.
fn skin_card(dir: &Path) -> Frame {
    material_tab(dir, "actors/body", engine::shader::Shader::AdvancedSkin)
}

/// GUI-Set-Materials: the PBR Landscape card.
fn landscape_card(dir: &Path) -> Frame {
    material_tab(dir, "pbr/metal", engine::shader::Shader::PbrLandscape)
}

/// The annotated main window of GUI-Guide.md. The numbers match its key ("Main window").
fn main_window(dir: &Path) -> Frame {
    let root = demo(dir);
    let settings = GuiSettings {
        // Shown in the header; nothing is exported, so the folder is never created.
        export_folder: Some("D:\\Mods\\StoneWork\\sdc-out".into()),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m.set_shader(
        "architecture/stone/cobble",
        Some(engine::shader::Shader::Pbr),
    );
    assert!(m.wait_idle(Duration::from_secs(60)));
    // Note FB1: the header names the export folder once one is chosen for the project.
    m.set_export_folder("D:\\Mods\\StoneWork\\sdc-out".into());
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.select_item("architecture/stone/stonewall.png");
    m.toggle_tab(BottomTab::Problems);
    let mut h = harness(m);
    settle(&mut h, 10);

    // Each marker sits just outside the top-left corner of its widget so it hides as little as possible.
    let tl = |r: Rect| r.left_top() + egui::vec2(-3.0, -3.0);
    let tile = h
        .query_all_by_role(Role::Image)
        .map(|n| n.rect())
        .min_by(|a, b| (a.min.y, a.min.x).partial_cmp(&(b.min.y, b.min.x)).unwrap())
        .expect("a thumbnail");
    let marks: Vec<(u32, Pos2)> = vec![
        (1, tl(rect_of(&h, "Open"))),
        (2, tl(rect_of(&h, "MO2…"))),
        (3, tl(rect_of(&h, "Textures view"))),
        (4, tl(rect_starting(&h, Role::Button, "Export to"))),
        (5, tl(rect_of(&h, "Save"))),
        // Export reads "Review N files" while Review would skip something (Note FB1).
        (6, tl(rect_starting(&h, Role::Button, "Review "))),
        (7, tl(rect_starting(&h, Role::Button, "Undecided"))),
        (8, tl(tile)),
        (9, tl(rect_starting(&h, Role::Button, "Not ready"))),
        // The first slot chip of the first set card (a "D" button; the Textures column has none).
        (10, tl(rect_of(&h, "D")) + egui::vec2(-4.0, 0.0)),
        (11, tl(rect_of(&h, "Type"))),
        (12, tl(rect_of(&h, "Advanced"))),
        (13, tl(rect_starting(&h, Role::Button, "Problems"))),
    ];
    let mut img = h.render().expect("render");
    for (n, p) in marks {
        marker(&mut img, p, n);
    }
    Frame { img, crop: None }
}
