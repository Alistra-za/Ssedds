//! Note AR (crash 2026-10-15): egui-wgpu's "Failed to create staging buffer for index data" panic is what a lost
//! wgpu device looks like, the app records the loss, and the panic hook keeps unsaved edits.
mod support;
use eframe::egui::epaint::{ClippedPrimitive, Mesh, Primitive, Vertex};
use eframe::egui::{pos2, Rect};
use eframe::egui_wgpu::{self, wgpu, RenderState, RendererOptions};
use sdc_gui::crash_log;
use support::*;

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

fn triangle() -> Vec<ClippedPrimitive> {
    let mut mesh = Mesh::default();
    for p in [pos2(0.0, 0.0), pos2(4.0, 0.0), pos2(0.0, 4.0)] {
        mesh.vertices.push(Vertex {
            pos: p,
            uv: pos2(0.0, 0.0),
            color: eframe::egui::Color32::WHITE,
        });
    }
    mesh.indices = vec![0, 1, 2];
    vec![ClippedPrimitive {
        clip_rect: Rect::EVERYTHING,
        primitive: Primitive::Mesh(mesh),
    }]
}

/// One test (one device per process: several all-backend instances at once deadlock in Vulkan + GL init).
#[test]
fn a_lost_device_is_logged_and_is_what_the_staging_buffer_panic_means() {
    let instance = wgpu::Instance::default();
    let Ok(rs) = block_on(RenderState::create(
        &sdc_gui::render::wgpu_configuration(),
        &instance,
        None,
        RendererOptions::default(),
    )) else {
        return; // no adapter
    };
    eprintln!("adapter: {:?}", rs.adapter.get_info());
    let RenderState { device, queue, .. } = &rs;
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("crash.log");
    crash_log::watch_device(device, Some(log.clone()));
    let mut renderer =
        egui_wgpu::Renderer::new(device, rs.target_format, RendererOptions::default());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [8, 8],
        pixels_per_point: 1.0,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    // A live device takes the frame.
    renderer.update_buffers(device, queue, &mut encoder, &triangle(), &screen);
    drop(encoder);

    device.destroy();
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        text.contains("wgpu device lost (Destroyed)"),
        "the loss is in the crash log: {text:?}"
    );

    // The same frame on the lost device: the panic of the crash.
    let mut encoder = device.create_command_encoder(&Default::default());
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        renderer.update_buffers(device, queue, &mut encoder, &triangle(), &screen);
    }))
    .expect_err("egui-wgpu panics on a lost device");
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.starts_with("Failed to create staging buffer for index data"),
        "{message}"
    );
}

#[test]
fn unsaved_edits_are_the_recovery_snapshot_and_a_panic_writes_them_beside_the_file() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    assert!(m.recovery_snapshot().is_empty(), "nothing unsaved");
    m.set_target(engine::config::Target::Vanilla);
    let snap = m.recovery_snapshot();
    let project = m.project.as_ref().unwrap();
    assert_eq!(snap, vec![(project.path().to_path_buf(), project.text())]);

    crash_log::write_recovery(&snap);
    let recovery = crash_log::recovery_path(project.path());
    assert_eq!(std::fs::read_to_string(&recovery).unwrap(), project.text());
    assert!(
        std::fs::read_to_string(project.path()).ok() != Some(project.text()),
        "the original is untouched"
    );
}

/// Hypothesis 3 of the crash: egui textures that grow per frame. Switching items, the JSON editor and the expanded
/// preview over and over must leave as many textures allocated as one round did.
#[test]
fn egui_textures_do_not_grow_over_frames() {
    use egui_kittest::Harness;
    use sdc_gui::model::AppModel;
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let keys: Vec<String> = m.items.iter().map(|i| i.key.clone()).collect();
    assert!(keys.len() > 1);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    let round = |h: &mut Harness<'static, AppModel>| {
        for (i, k) in keys.iter().enumerate() {
            h.state_mut().select_item(k);
            h.state_mut().json_editor_open = i % 2 == 0;
            h.state_mut().expanded = (i % 3 == 0).then(|| k.clone());
            h.run();
            assert!(h.state_mut().wait_idle(std::time::Duration::from_secs(60)));
            h.run();
        }
    };
    let count = |h: &Harness<'static, AppModel>| h.ctx.tex_manager().read().num_allocated();
    round(&mut h);
    let first = count(&h);
    for _ in 0..5 {
        round(&mut h);
    }
    assert_eq!(count(&h), first, "textures after 6 rounds vs after 1");
}
