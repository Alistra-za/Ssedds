//! Generates `docs/img/gui-main.png`, the annotated main-window picture of the GUI guide. Needs a GPU:
//! `cargo test -p sdc-gui --test gui_guide_screenshot -- --ignored`. Marker positions come from the widgets' rects, so a
//! layout change moves them; check the PNG by eye before committing it. The numbers match the key in
//! `docs/wiki/GUI-Guide.md` ("Main window").
mod support;
use eframe::egui::{self, accesskit::Role, Pos2, Rect};
use egui_kittest::kittest::{NodeT as _, Queryable};
use egui_kittest::Harness;
use image::{Rgba, RgbaImage};
use sdc_gui::model::bottom::BottomTab;
use sdc_gui::model::meshes::MeshesModel;
use sdc_gui::model::AppModel;
use sdc_gui::settings::{GuiSettings, MeshesSettings};
use sdc_gui::ui::State;
use std::path::Path;
use std::time::Duration;

const W: f32 = 1600.0;
const H: f32 = 900.0;

type Hn = Harness<'static, (AppModel, State, MeshesModel)>;

/// A small made-up mod: a plain diffuse + normal + envmask, a PBR cobble set and a banner. Generated, never real assets.
fn demo_folder(dir: &Path) -> std::path::PathBuf {
    let r = dir.join("textures/architecture/stone");
    std::fs::create_dir_all(&r).unwrap();
    let rgb = |name: &str, w: u32, h: u32, f: &dyn Fn(u32, u32) -> [u8; 3]| {
        image::ImageBuffer::from_fn(w, h, |gx, gy| image::Rgb(f(gx, gy)))
            .save(r.join(name))
            .unwrap();
    };
    let mortar = |gx: u32, gy: u32| {
        gy % 32 < 3 || (gx + if (gy / 32).is_multiple_of(2) { 0 } else { 32 }) % 64 < 3
    };
    let flat = |_: u32, _: u32| [128u8, 128, 255];
    rgb("stonewall.png", 256, 256, &|gx, gy| {
        let n = ((gx * 7 + gy * 13 + (gy / 32) * 29) % 23) as u8;
        if mortar(gx, gy) {
            [150, 146, 138]
        } else {
            [118 + n, 84 + n / 2, 66 + n / 3]
        }
    });
    rgb("stonewall_n.png", 256, 256, &|gx, gy| {
        if mortar(gx, gy) {
            [128, 100, 220]
        } else {
            [128, 128, 255]
        }
    });
    rgb("stonewall_m.png", 256, 256, &|gx, _| {
        [(gx / 2) as u8, 40, 40]
    });
    rgb("cobble.png", 256, 256, &|gx, gy| {
        let c = ((gx / 24 + gy / 24) % 3) as u8 * 16;
        [90 + c, 88 + c, 84 + c]
    });
    rgb("cobble_n.png", 256, 256, &flat);
    rgb("cobble_rmaos.png", 256, 256, &|gx, gy| {
        [200, ((gx + gy) % 60) as u8, 255]
    });
    rgb("cobble_p.png", 256, 256, &|gx, _| [(gx % 128) as u8 * 2; 3]);
    // 200x120 is not a power of two: a blocker for the Problems tab.
    rgb("banner.png", 200, 120, &|_, gy| {
        [160, 30 + (gy / 4) as u8 % 60, 40]
    });
    rgb("banner_n.png", 200, 120, &flat);
    dir.join("textures")
}

/// The first widget whose accessible label equals `label` (leftmost, then topmost).
fn rect_of(h: &Hn, label: &str) -> Rect {
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .min_by(|a, b| (a.min.x, a.min.y).partial_cmp(&(b.min.x, b.min.y)).unwrap())
        .unwrap_or_else(|| panic!("no widget labelled {label:?}"))
}

/// The topmost widget of `role` whose label starts with `start`.
fn rect_starting(h: &Hn, role: Role, start: &str) -> Rect {
    h.query_all_by_role(role)
        .filter(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.starts_with(start))
        })
        .map(|n| n.rect())
        .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap_or_else(|| panic!("no {role:?} starting {start:?}"))
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

/// A filled, white-ringed circle at `c` with the number `n` (1 or 2 digits) in white, on `img` (alpha-blend free).
fn marker(img: &mut RgbaImage, c: Pos2, n: u32) {
    let (r, px) = (9.0f32, 2i32);
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (cx, cy) = (c.x.round() as i32, c.y.round() as i32);
    for gy in -12..=12 {
        for gx in -12..=12 {
            let d = ((gx * gx + gy * gy) as f32).sqrt();
            let col = if d <= r {
                Rgba([214, 40, 57, 255])
            } else if d <= r + 2.0 {
                Rgba([255, 255, 255, 255])
            } else {
                continue;
            };
            let (gx, gy) = (cx + gx, cy + gy);
            if (0..w).contains(&gx) && (0..h).contains(&gy) {
                img.put_pixel(gx as u32, gy as u32, col);
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
                            let (gx, gy) = (
                                x0 + i as i32 * 4 * px + rx * px + dx,
                                y0 + ry as i32 * px + dy,
                            );
                            if (0..w).contains(&gx) && (0..h).contains(&gy) {
                                img.put_pixel(gx as u32, gy as u32, Rgba([255, 255, 255, 255]));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore]
fn gui_guide_screenshot() {
    let tmp = tempfile::tempdir().unwrap();
    // Nothing here may reach the real user data folders.
    std::env::set_var("APPDATA", tmp.path());
    std::env::set_var("LOCALAPPDATA", tmp.path());
    let root = demo_folder(tmp.path());
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
    m.select_item("architecture/stone/stonewall.png");
    m.toggle_tab(BottomTab::Problems);

    let mut meshes = MeshesModel::new(None, MeshesSettings::default());
    meshes.procs = sdc_gui::model::meshes::no_procs;
    let mut h: Hn = Harness::builder()
        .with_size(egui::vec2(W, H))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_ui_state(
            |ui, (m, s, me): &mut (AppModel, State, MeshesModel)| {
                sdc_gui::ui::draw_with_scan(ui, m, s, Some(me))
            },
            (m, State::default(), meshes),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run_steps(10);

    // (number, widget rect, which point of it the marker sits on). Keep in step with the key in GUI-Guide.md.
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
        (6, tl(rect_of(&h, "Export"))),
        (7, tl(rect_starting(&h, Role::Button, "Needs decision"))),
        (8, tl(tile)),
        (9, tl(rect_starting(&h, Role::Button, "Needs attention"))),
        // The first slot chip of the first set card (a "D" button; the Textures column has none).
        (10, tl(rect_of(&h, "D")) + egui::vec2(-4.0, 0.0)),
        (11, tl(rect_of(&h, "Type"))),
        (12, tl(rect_of(&h, "Advanced"))),
        (13, tl(rect_starting(&h, Role::Button, "Problems"))),
    ];

    let img = h.render().expect("render");
    let mut img: RgbaImage = img;
    for (n, p) in marks {
        marker(&mut img, p, n);
    }
    let out =
        std::env::var("GUI_GUIDE_PNG").unwrap_or_else(|_| "../../docs/img/gui-main.png".into());
    img.save(&out).expect("save png");
}
