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
use sdc_gui::model::AppModel;
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

pub const SCENARIOS: &[(&str, Scenario)] = &[("main_window", main_window)];

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
