//! Window and layout memory (item 3, DECISIONS AG3, AG17-AG20): what is saved in `gui.toml` `[layout]`, how the saved
//! window is fitted to the monitors, and the debounce that keeps the file from being written every frame.
use crate::model::texture_view::{ColumnPrefs, SetFilter, SortKey};
use crate::model::view::{Channel, Layout, ViewState};
use crate::model::viewport3d::Look;
use crate::model::Filter;
use crate::settings::{
    LayoutSettings, LookSettings, WindowSettings, DETAILS_NAMES, FILTER_NAMES, WIDTH_IDS,
    WIDTH_RANGE,
};
use eframe::egui::{self, containers::panel::PanelState, pos2, vec2, Rect};
use std::time::{Duration, Instant};

/// The window size of a first start, and of a `gui.toml` without a usable window.
pub const DEFAULT_SIZE: [f32; 2] = [1400.0, 900.0];
/// A saved window must show at least this many pixels (both ways) on some monitor.
const MIN_VISIBLE: f32 = 64.0;
/// The top of the window that must be on a monitor so it can be dragged.
const TITLE_BAR: f32 = 32.0;

/// The saved rectangle (pixels) fitted to the monitors (pixels, the primary first), AG18. With no monitor info it is
/// returned as it is. A rectangle that shows less than 64 x 64 px on every monitor is centred on the primary monitor
/// at its saved size, at most 90% of the monitor. One that shows enough stays where it is, except that a side bigger
/// than all the monitors together shrinks to them and a title bar off every monitor moves onto one.
pub fn fit_window(saved: Rect, monitors: &[Rect]) -> Rect {
    let Some(primary) = monitors.first() else {
        return saved;
    };
    let shown = monitors.iter().any(|m| {
        let i = m.intersect(saved);
        i.width() >= MIN_VISIBLE && i.height() >= MIN_VISIBLE
    });
    if !shown {
        let size = vec2(
            saved.width().min(primary.width() * 0.9),
            saved.height().min(primary.height() * 0.9),
        );
        return Rect::from_center_size(primary.center(), size);
    }
    let all = monitors.iter().fold(*primary, |a, m| a.union(*m));
    let (mut min, mut size) = (saved.min, saved.size());
    if size.x > all.width() {
        (size.x, min.x) = (all.width(), all.min.x);
    }
    if size.y > all.height() {
        (size.y, min.y) = (all.height(), all.min.y);
    }
    let r = Rect::from_min_size(min, size);
    // The title bar must be on a monitor, or the window cannot be dragged: else it comes down (or up) onto the monitor
    // that shows the most of the window.
    let bar = Rect::from_min_size(min, vec2(size.x, TITLE_BAR));
    let bar_shown = monitors.iter().any(|m| {
        let i = m.intersect(bar);
        i.width() >= MIN_VISIBLE && i.height() >= TITLE_BAR
    });
    if !bar_shown {
        let shown = |m: &&Rect| {
            let i = m.intersect(r);
            i.width().max(0.0) * i.height().max(0.0)
        };
        if let Some(m) = monitors.iter().max_by(|a, b| shown(a).total_cmp(&shown(b))) {
            min.y = min.y.clamp(m.min.y, (m.max.y - TITLE_BAR).max(m.min.y));
        }
    }
    Rect::from_min_size(min, size)
}

/// What `ViewportBuilder` is given at start, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InitialWindow {
    pub size: [f32; 2],
    /// `None`: the OS places the window.
    pub pos: Option<[f32; 2]>,
    pub maximized: bool,
}

/// The saved window, fitted to `monitors` (pixels). The window is saved in points with the scale it had, so the check
/// runs in pixels (points x scale) and the answer goes back to points.
pub fn initial_window(layout: &LayoutSettings, monitors: &[Rect]) -> InitialWindow {
    let Some(w) = layout.window else {
        return InitialWindow {
            size: DEFAULT_SIZE,
            pos: None,
            maximized: false,
        };
    };
    if monitors.is_empty() {
        return InitialWindow {
            size: [w.w, w.h],
            pos: None,
            maximized: w.maximized,
        };
    }
    let px = Rect::from_min_size(
        pos2(w.x * w.ppp, w.y * w.ppp),
        vec2(w.w * w.ppp, w.h * w.ppp),
    );
    let r = fit_window(px, monitors);
    InitialWindow {
        size: [r.width() / w.ppp, r.height() / w.ppp],
        pos: Some([r.min.x / w.ppp, r.min.y / w.ppp]),
        maximized: w.maximized,
    }
}

/// The monitors' work areas in pixels, the primary first. Empty when Windows cannot say (AG18: the Windows API
/// through `windows-sys`, which eframe already pulls in; winit has no monitor list before the window exists).
#[cfg(windows)]
pub fn monitors() -> Vec<Rect> {
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    use windows_sys::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> i32 {
        // SAFETY: `out` is the `&mut Vec` passed below, alive for the whole enumeration.
        let list = unsafe { &mut *(out as *mut Vec<(bool, Rect)>) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(m, &mut info) } != 0 {
            let r = info.rcWork;
            list.push((
                info.dwFlags & 1 != 0, // MONITORINFOF_PRIMARY
                Rect::from_min_max(
                    pos2(r.left as f32, r.top as f32),
                    pos2(r.right as f32, r.bottom as f32),
                ),
            ));
        }
        1
    }
    let mut list: Vec<(bool, Rect)> = vec![];
    // SAFETY: plain Win32 calls. The process is made per-monitor DPI aware first (winit does the same a moment later and
    // then finds it done), so the rectangles are real pixels and not scaled ones.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(each),
            &mut list as *mut _ as LPARAM,
        );
    }
    list.sort_by_key(|(primary, _)| !primary);
    list.into_iter().map(|(_, r)| r).collect()
}

#[cfg(not(windows))]
pub fn monitors() -> Vec<Rect> {
    vec![]
}

/// Writes at most once per [`SaveDebounce::DELAY`] (AG19): the first change starts the wait, later changes inside it
/// ride along, and `due` is true once when it is over.
#[derive(Debug, Default)]
pub struct SaveDebounce {
    since: Option<Instant>,
}

impl SaveDebounce {
    pub const DELAY: Duration = Duration::from_secs(2);
    pub fn changed(&mut self, now: Instant) {
        self.since.get_or_insert(now);
    }
    pub fn pending(&self) -> bool {
        self.since.is_some()
    }
    /// True once per wait, when it has run out.
    pub fn due(&mut self, now: Instant) -> bool {
        match self.since {
            Some(t) if now.saturating_duration_since(t) >= Self::DELAY => {
                self.since = None;
                true
            }
            _ => false,
        }
    }
}

fn chosen_id(name: &str) -> egui::Id {
    egui::Id::new(name).with("chosen")
}

/// The width a panel has now, in points: the Textures and Sets columns keep the width the user dragged to apart from
/// egui's own (see `column_panel`).
fn panel_width(ctx: &egui::Context, name: &str) -> Option<f32> {
    if matches!(name, "textures" | "sets") {
        return ctx.data_mut(|d| d.get_persisted::<f32>(chosen_id(name)));
    }
    PanelState::load(ctx, egui::Id::new(name)).map(|s| s.outer_rect.width())
}

/// Everything [`capture`] reads, in a form that is cheap to build and compare: the app captures (which allocates) only
/// on a frame where this changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    rects: [Option<Rect>; 2],
    states: [Option<bool>; 3],
    ppp: Option<f32>,
    widths: [Option<f32>; WIDTH_IDS.len()],
    meshes_view: bool,
    filter: Filter,
    prefs: ColumnPrefs,
    channel: Channel,
    layout: Layout,
    looks: (Look, Look),
    settings_window: Option<Rect>,
    presets_window: Option<Rect>,
    details: [bool; DETAILS_NAMES.len()],
}

/// The 3D settings window's rectangle this frame, if it has been shown (AG132).
fn settings_window_now(ctx: &egui::Context) -> Option<Rect> {
    ctx.memory(|m| m.area_rect(crate::ui::set_preview::settings_window_id()))
}

pub fn probe(
    ctx: &egui::Context,
    filter: &Filter,
    prefs: &ColumnPrefs,
    view: &ViewState,
    looks: (&Look, &Look),
) -> Probe {
    let (rects, states, ppp) = ctx.input(|i| {
        let v = i.viewport();
        (
            [v.inner_rect, v.outer_rect],
            [v.maximized, v.minimized, v.fullscreen],
            v.native_pixels_per_point,
        )
    });
    Probe {
        rects,
        states,
        ppp,
        widths: WIDTH_IDS.map(|n| panel_width(ctx, n)),
        meshes_view: crate::ui::header::meshes_view_active(ctx),
        filter: filter.clone(),
        prefs: *prefs,
        channel: view.channel,
        layout: view.layout,
        looks: (*looks.0, *looks.1),
        settings_window: settings_window_now(ctx),
        presets_window: ctx.memory(|m| m.area_rect(crate::ui::presets::window_id())),
        details: DETAILS_NAMES.map(|n| crate::ui::viewport3d::details_open(ctx, n)),
    }
}

/// What the app shows now, as `[layout]`. A part that cannot be read this frame (no viewport info yet, a panel not
/// shown yet) keeps its value in `prev`, and a change under half a point is noise and keeps `prev` too.
pub fn capture(
    ctx: &egui::Context,
    prev: &LayoutSettings,
    filter: &Filter,
    prefs: &ColumnPrefs,
    view: &ViewState,
    looks: (&Look, &Look),
) -> LayoutSettings {
    let mut l = prev.clone();
    l.window = window_now(ctx, prev.window);
    for name in WIDTH_IDS {
        let Some(w) =
            panel_width(ctx, name).filter(|w| (WIDTH_RANGE.0..=WIDTH_RANGE.1).contains(w))
        else {
            continue;
        };
        if prev.widths.get(name).is_none_or(|p| (p - w).abs() >= 0.5) {
            l.widths.insert(name.to_string(), w);
        }
    }
    l.meshes_view = crate::ui::header::meshes_view_active(ctx);
    let on = [
        filter.needs_decision,
        filter.warnings,
        filter.failed,
        filter.not_in_set,
    ];
    l.filters = FILTER_NAMES
        .iter()
        .zip(on)
        .filter(|(_, on)| *on)
        .map(|(n, _)| n.to_string())
        .collect();
    l.filter_subtype = filter.subtype.clone();
    l.filter_format = filter.format.clone();
    l.filter_encoder = filter.encoder.clone();
    l.filter_set = filter.set.name().to_string();
    l.sort = prefs.sort.name().to_string();
    l.tile = prefs.tile;
    l.preview_channel = view.channel.shader_id() as u8;
    l.preview_layout = view.layout.name().to_string();
    l.look = look_now(looks.0, &prev.look);
    l.set_look = look_now(looks.1, &prev.set_look);
    if let Some(r) = settings_window_now(ctx) {
        let now = [r.min.x, r.min.y, r.width(), r.height()];
        let moved = prev
            .settings_window
            .is_none_or(|p| p.iter().zip(now).any(|(a, b)| (a - b).abs() >= 0.5));
        if moved {
            l.settings_window = Some(now);
        }
    }
    if let Some(r) = ctx.memory(|m| m.area_rect(crate::ui::presets::window_id())) {
        let now = [r.min.x, r.min.y, r.width(), r.height()];
        if prev
            .presets_window
            .is_none_or(|p| p.iter().zip(now).any(|(a, b)| (a - b).abs() >= 0.5))
        {
            l.presets_window = Some(now);
        }
    }
    l.details = DETAILS_NAMES
        .iter()
        .filter(|n| crate::ui::viewport3d::details_open(ctx, n))
        .map(|n| n.to_string())
        .collect();
    l
}

/// The saved look when it still means `look` (a default look saves as nothing).
fn look_now(look: &Look, prev: &Option<LookSettings>) -> Option<LookSettings> {
    if Look::from_settings(prev.as_ref()) == *look {
        return *prev;
    }
    Some(LookSettings::of(look))
}

/// A viewport rectangle in zoomed points (points of native x egui zoom) as points of the native scale, which is what
/// `WindowSettings` and `ViewportBuilder` use.
pub fn native_points(r: Rect, zoom: f32) -> Rect {
    Rect::from_min_max(r.min * zoom, r.max * zoom)
}

/// The window from the viewport info (points). Maximised keeps the last normal rectangle, so leaving the maximised
/// state at the next start gives that rectangle back; minimised and fullscreen change nothing.
fn window_now(ctx: &egui::Context, prev: Option<WindowSettings>) -> Option<WindowSettings> {
    let zoom = ctx.zoom_factor();
    let (inner, outer, maximized, minimized, fullscreen, ppp) = ctx.input(|i| {
        let v = i.viewport();
        (
            v.inner_rect,
            v.outer_rect,
            v.maximized.unwrap_or(false),
            v.minimized.unwrap_or(false),
            v.fullscreen.unwrap_or(false),
            v.native_pixels_per_point,
        )
    });
    if minimized || fullscreen {
        return prev;
    }
    if maximized {
        return prev.map(|w| WindowSettings {
            maximized: true,
            ..w
        });
    }
    let (Some(inner), Some(outer)) = (inner, outer) else {
        return prev;
    };
    let (inner, outer) = (native_points(inner, zoom), native_points(outer, zoom));
    let now = WindowSettings {
        x: outer.min.x,
        y: outer.min.y,
        w: inner.width(),
        h: inner.height(),
        ppp: ppp.unwrap_or(1.0),
        maximized: false,
    };
    // A minimised window reports far-off coordinates; a window too small to use is not worth remembering.
    let usable = now.x > -30000.0 && now.y > -30000.0 && now.w >= 200.0 && now.h >= 200.0;
    match prev {
        _ if !usable => prev,
        Some(p)
            if !p.maximized
                && p.ppp == now.ppp
                && [p.x - now.x, p.y - now.y, p.w - now.w, p.h - now.h]
                    .iter()
                    .all(|d| d.abs() < 0.5) =>
        {
            prev
        }
        _ => Some(now),
    }
}

/// Puts the saved arrangement into egui before the first frame: the panel widths and the Textures | Meshes view.
pub fn restore_ctx(ctx: &egui::Context, l: &LayoutSettings) {
    for (name, w) in &l.widths {
        if matches!(name.as_str(), "textures" | "sets") {
            ctx.data_mut(|d| d.insert_persisted(chosen_id(name), *w));
        } else {
            let outer_rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(*w, 100.0));
            ctx.data_mut(|d| d.insert_persisted(egui::Id::new(name), PanelState { outer_rect }));
        }
    }
    crate::ui::header::set_meshes_view(ctx, l.meshes_view);
    for n in DETAILS_NAMES {
        crate::ui::viewport3d::set_details_open(ctx, n, l.details.iter().any(|d| d == n));
    }
}

/// The Textures filters, the preview channel and layout the file names; an unknown one is skipped.
pub fn restore_model(
    l: &LayoutSettings,
    filter: &mut Filter,
    prefs: &mut ColumnPrefs,
    view: &mut ViewState,
) {
    let has = |n: &str| l.filters.iter().any(|f| f == n);
    *filter = Filter {
        needs_decision: has("needs-decision"),
        warnings: has("warnings"),
        failed: has("failed"),
        not_in_set: has("not-in-set"),
        subtype: l.filter_subtype.clone(),
        format: l.filter_format.clone(),
        encoder: l.filter_encoder.clone(),
        set: SetFilter::from_name(&l.filter_set).unwrap_or_default(),
    };
    *prefs = ColumnPrefs {
        sort: SortKey::from_name(&l.sort).unwrap_or_default(),
        tile: l.tile,
    };
    view.channel = Channel::from_id(u32::from(l.preview_channel)).unwrap_or_default();
    view.layout = Layout::from_name(&l.preview_layout).unwrap_or_default();
}
