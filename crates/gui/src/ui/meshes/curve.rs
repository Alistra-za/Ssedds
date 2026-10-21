//! The curve graph of the Controllers tab (spec §7, v2 B6): the keys of one controller (or clip) sampled as B2
//! evaluates them, the keys as handles, QUADRATIC tangent handles and the clock's cursor.
use eframe::egui;
use nif::anim::{sample_f32, sample_vec3};
use nif::keys::{encode_data, Key, KeyData, KeyGroup, KeyType};

/// Inset of the plot inside the graph's rect, so end handles are not clipped (tests map times with it).
pub(super) const PAD: f32 = 6.0;
const HEIGHT: f32 = 120.0;
const HANDLE: f32 = 4.0; // drawn radius; the hit box is 3×
const TIME_STRIP: f32 = 10.0;
const RGB: [&str; 3] = ["R", "G", "B"];
const RGB_COLORS: [egui::Color32; 3] = [
    egui::Color32::from_rgb(220, 80, 80),
    egui::Color32::from_rgb(80, 170, 80),
    egui::Color32::from_rgb(90, 120, 230),
];

/// What one frame of the graph did.
pub(super) struct CurveOut {
    /// The keys after this frame's handle drag; Some only when a float's bits changed.
    pub keys: Option<KeyData>,
    /// A handle drag ended this frame: the caller flushes (one undo step per drag).
    pub released: bool,
    /// A click or drag on the background: the key time under the pointer.
    pub scrub: Option<f32>,
}

/// Which float of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Value,
    Forward,
    Backward,
}

/// A key value with float channels: f32 (1) or an RGB / XYZ triple (3).
trait Chan: Copy + Default {
    const N: usize;
    fn get(&self, c: usize) -> f32;
    fn set(&mut self, c: usize, v: f32);
}

impl Chan for f32 {
    const N: usize = 1;
    fn get(&self, _: usize) -> f32 {
        *self
    }
    fn set(&mut self, _: usize, v: f32) {
        *self = v;
    }
}

impl Chan for [f32; 3] {
    const N: usize = 3;
    fn get(&self, c: usize) -> f32 {
        self[c]
    }
    fn set(&mut self, c: usize, v: f32) {
        self[c] = v;
    }
}

fn get_in<T: Chan>(k: &Key<T>, c: usize, p: Part) -> f32 {
    match p {
        Part::Value => k.value.get(c),
        // A missing tangent reads as 0, as `encode_group` writes it.
        Part::Forward => k.forward.unwrap_or_default().get(c),
        Part::Backward => k.backward.unwrap_or_default().get(c),
    }
}

fn set_in<T: Chan>(k: &mut Key<T>, c: usize, p: Part, v: f32) {
    match p {
        Part::Value => k.value.set(c, v),
        Part::Forward => k.forward.get_or_insert_with(T::default).set(c, v),
        Part::Backward => k.backward.get_or_insert_with(T::default).set(c, v),
    }
}

/// Float channels of `d`: 1 for Float, 3 for Pos, 0 for Bool (not drawn, not edited).
pub(super) fn channels(d: &KeyData) -> usize {
    match d {
        KeyData::Float(_) => 1,
        KeyData::Pos(_) => 3,
        KeyData::Bool(_) => 0,
    }
}

pub(super) fn len(d: &KeyData) -> usize {
    match d {
        KeyData::Float(g) => g.keys.len(),
        KeyData::Pos(g) => g.keys.len(),
        KeyData::Bool(g) => g.keys.len(),
    }
}

pub(super) fn key_type(d: &KeyData) -> Option<u32> {
    match d {
        KeyData::Float(g) => g.key_type,
        KeyData::Pos(g) => g.key_type,
        KeyData::Bool(g) => g.key_type,
    }
}

pub(super) fn time(d: &KeyData, i: usize) -> f32 {
    match d {
        KeyData::Float(g) => g.keys[i].time,
        KeyData::Pos(g) => g.keys[i].time,
        KeyData::Bool(g) => g.keys[i].time,
    }
}

pub(super) fn set_time(d: &mut KeyData, i: usize, t: f32) {
    match d {
        KeyData::Float(g) => g.keys[i].time = t,
        KeyData::Pos(g) => g.keys[i].time = t,
        KeyData::Bool(g) => g.keys[i].time = t,
    }
}

/// Channel `c` of key `i`'s `p`. Bool keys read as 0 (never edited).
pub(super) fn get(d: &KeyData, i: usize, c: usize, p: Part) -> f32 {
    match d {
        KeyData::Float(g) => get_in(&g.keys[i], c, p),
        KeyData::Pos(g) => get_in(&g.keys[i], c, p),
        KeyData::Bool(_) => 0.0,
    }
}

pub(super) fn set(d: &mut KeyData, i: usize, c: usize, p: Part, v: f32) {
    match d {
        KeyData::Float(g) => set_in(&mut g.keys[i], c, p, v),
        KeyData::Pos(g) => set_in(&mut g.keys[i], c, p, v),
        KeyData::Bool(_) => {}
    }
}

/// The curve's channel values at key time `t`; None for 0 keys or Bool.
pub(super) fn sample(d: &KeyData, t: f32) -> Option<[f32; 3]> {
    match d {
        KeyData::Float(g) => sample_f32(g, t).map(|v| [v, 0.0, 0.0]),
        KeyData::Pos(g) => sample_vec3(g, t),
        KeyData::Bool(_) => None,
    }
}

/// `g` with QUADRATIC tangents that keep its LINEAR shape (D8), or LINEAR without tangents.
fn convert_group<T: Chan>(g: &KeyGroup<T>, quad: bool) -> KeyGroup<T> {
    let mut out = g.clone();
    let n = out.keys.len();
    out.key_type = Some(if quad { 2 } else { 1 });
    // The segment slope v_{i+1} − v_i, per channel.
    let slope = |i: usize| {
        let mut s = T::default();
        for c in 0..T::N {
            s.set(c, g.keys[i + 1].value.get(c) - g.keys[i].value.get(c));
        }
        s
    };
    for (i, k) in out.keys.iter_mut().enumerate() {
        k.tbc = None;
        // backward_i = forward_{i+1} = the slope; the first key's forward and the last key's backward are 0.
        k.forward = quad.then(|| if i > 0 { slope(i - 1) } else { T::default() });
        k.backward = quad.then(|| if i + 1 < n { slope(i) } else { T::default() });
    }
    out
}

/// D8: Linear ↔ Quadratic with the same shape (Linear drops the tangents).
pub(super) fn convert(d: &KeyData, quad: bool) -> KeyData {
    match d {
        KeyData::Float(g) => KeyData::Float(convert_group(g, quad)),
        KeyData::Pos(g) => KeyData::Pos(convert_group(g, quad)),
        KeyData::Bool(_) => d.clone(),
    }
}

/// The line drawn instead of a graph whose keys cannot be mapped (B-P35).
pub(super) const OUT_OF_RANGE: &str = "These keys are out of range for the graph.";

/// `[a, b]` as a mapping range whose span is finite and strictly positive: `pad` × the span on each side, or, for a
/// flat range (within ~80 ulps), a span of max(|v| × 1e-3, 0.5) around its middle (so 1e8 ± 0.5 cannot round back
/// to 1e8). None when f32 cannot hold it.
fn range(a: f32, b: f32, pad: f32) -> Option<(f32, f32)> {
    let mag = a.abs().max(b.abs());
    let (lo, hi) = if b - a > mag * 1e-5 {
        let p = (b - a) * pad;
        (a - p, b + p)
    } else {
        let (mid, half) = (a + (b - a) / 2.0, (mag * 1e-3).max(0.5) / 2.0);
        (mid - half, mid + half)
    };
    let d = hi - lo;
    (lo.is_finite() && hi.is_finite() && d.is_finite() && d > 0.0).then_some((lo, hi))
}

/// The view: (time lo, time hi, value lo, value hi), every span finite and > 0 (B-P35). None when the keys cannot
/// be mapped: a non-finite key time or value, a curve that samples to a non-finite value, or spans past f32.
fn view(keys: &KeyData, span: (f32, f32), samples: usize) -> Option<[f32; 4]> {
    let (n, nch) = (len(keys), channels(keys));
    for i in 0..n {
        if !time(keys, i).is_finite() || (0..nch).any(|c| !get(keys, i, c, Part::Value).is_finite())
        {
            return None;
        }
    }
    // A non-finite start or stop (B3's invalid times) is ignored: the keys still give the extent.
    let ends = [span.0, span.1]
        .into_iter()
        .filter(|t| t.is_finite())
        .chain(
            (n > 0)
                .then(|| [time(keys, 0), time(keys, n - 1)])
                .into_iter()
                .flatten(),
        );
    let (a, b) = ends.fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), t| {
        (a.min(t), b.max(t))
    });
    let (lo, hi) = if a <= b {
        range(a, b, 0.0)?
    } else {
        range(0.0, 0.0, 0.0)?
    };
    let (mut a, mut b) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut add = |v: f32| {
        a = a.min(v);
        b = b.max(v);
    };
    for i in 0..n {
        (0..nch).for_each(|c| add(get(keys, i, c, Part::Value)));
    }
    for s in 0..samples {
        let t = lo + (hi - lo) * s as f32 / (samples - 1).max(1) as f32;
        for &v in sample(keys, t).iter().flatten().take(nch) {
            if !v.is_finite() {
                return None;
            }
            add(v);
        }
    }
    if n == 0 {
        (a, b) = (0.0, 0.0);
    }
    // QUADRATIC handles widen the view, capped at half the curve's own span past it: a huge tangent leaves its
    // handle outside the graph (where it cannot start a drag) instead of flattening the curve.
    if key_type(keys) == Some(KeyType::Quadratic as u32) {
        let cap = (b - a).max((a.abs().max(b.abs()) * 1e-3).max(0.5)) / 2.0;
        let (a0, b0) = (a, b);
        for (i, j) in (1..n).map(|j| (j - 1, j)) {
            for c in 0..nch {
                let out_h = get(keys, i, c, Part::Value) + get(keys, i, c, Part::Backward) / 3.0;
                let in_h = get(keys, j, c, Part::Value) - get(keys, j, c, Part::Forward) / 3.0;
                for h in [out_h, in_h].into_iter().filter(|h| h.is_finite()) {
                    a = a.min(h.max(a0 - cap));
                    b = b.max(h.min(b0 + cap));
                }
            }
        }
    }
    let (vlo, vhi) = range(a, b, 0.1)?;
    Some([lo, hi, vlo, vhi])
}

/// `keys` over at least `span` (the controller's start and stop); `cursor` is the clock's time in key time;
/// `editable` false draws handles that do not move. `name` is the controller's title (accessible names).
pub(super) fn curve(
    ui: &mut egui::Ui,
    id: egui::Id,
    name: &str,
    keys: &KeyData,
    span: (f32, f32),
    cursor: Option<f32>,
    editable: bool,
) -> CurveOut {
    let mut out = CurveOut {
        keys: None,
        released: false,
        scrub: None,
    };
    let nch = channels(keys);
    if nch == 0 {
        return out;
    }
    let mut next = keys.clone();
    let w = (ui.available_width() - 8.0).max(160.0); // rule 3.7: a margin, or the scroll bar flips
    let n = (((w - 2.0 * PAD) / 2.0) as usize).clamp(2, 400);
    // D6: while a handle is dragged the axes stay as they were at its start.
    let view_id = id.with("view");
    let frozen = ui
        .data(|d| d.get_temp::<[f32; 4]>(view_id))
        // a drag that ended unseen (its handle went away) does not keep the axes frozen
        .filter(|_| ui.input(|i| i.pointer.any_down()));
    let Some([lo, hi, vlo, vhi]) = frozen.or_else(|| view(keys, span, n)) else {
        ui.weak(OUT_OF_RANGE);
        return out;
    };
    let (rect, bg) = ui.allocate_exact_size(egui::vec2(w, HEIGHT), egui::Sense::click_and_drag());
    // PAD on every side, plus a strip under the plot for the time extents (x keeps PAD: tests map times with it).
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(PAD, PAD),
        rect.max - egui::vec2(PAD, PAD + TIME_STRIP),
    );
    let x = |t: f32| plot.left() + (t - lo) / (hi - lo) * plot.width();
    let y = |v: f32| plot.bottom() - (v - vlo) / (vhi - vlo) * plot.height();
    let t_of = |px: f32| lo + (px - plot.left()) / plot.width() * (hi - lo);
    let dt = (hi - lo) / plot.width();
    let dv = (vhi - vlo) / plot.height();

    // Background: a click or drag scrubs.
    if bg.clicked() || bg.dragged() {
        if let Some(p) = bg.interact_pointer_pos() {
            out.scrub = Some(t_of(p.x).clamp(lo, hi));
        }
    }
    let shown = cursor.filter(|c| c.is_finite()).unwrap_or(lo) as f64;
    let label = format!("{name} curve");
    bg.widget_info(|| egui::WidgetInfo::slider(true, shown, &label));
    bg.on_hover_text("Click or drag to move the preview time.");

    // Paint: frame, curve, cursor, extents.
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals().clone();
    painter.rect_stroke(
        rect,
        2.0,
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let color = |c: usize| match nch {
        1 => visuals.widgets.active.fg_stroke.color,
        _ => RGB_COLORS[c],
    };
    if len(keys) > 0 {
        let mut lines = vec![Vec::with_capacity(n); nch];
        for s in 0..n {
            let t = lo + (hi - lo) * s as f32 / (n - 1) as f32;
            if let Some(v) = sample(keys, t) {
                for (c, line) in lines.iter_mut().enumerate() {
                    if v[c].is_finite() {
                        line.push(egui::pos2(x(t), y(v[c])));
                    }
                }
            }
        }
        for (c, line) in lines.into_iter().enumerate() {
            painter.add(egui::Shape::line(line, egui::Stroke::new(1.5, color(c))));
        }
    }
    if let Some(c) = cursor.filter(|c| c.is_finite() && *c >= lo && *c <= hi) {
        painter.line_segment(
            [
                egui::pos2(x(c), rect.top()),
                egui::pos2(x(c), rect.bottom()),
            ],
            visuals.selection.stroke,
        );
    }
    let font = egui::TextStyle::Small.resolve(ui.style());
    let weak = visuals.weak_text_color();
    // Value extents inside the plot's left edge; time extents in the strip under it.
    for (pos, align, v) in [
        (plot.left_top(), egui::Align2::LEFT_TOP, vhi),
        (plot.left_bottom(), egui::Align2::LEFT_BOTTOM, vlo),
        (
            egui::pos2(plot.left(), rect.bottom() - 1.0),
            egui::Align2::LEFT_BOTTOM,
            lo,
        ),
        (
            egui::pos2(plot.right(), rect.bottom() - 1.0),
            egui::Align2::RIGHT_BOTTOM,
            hi,
        ),
    ] {
        painter.text(pos, align, format!("{v:.3}"), font.clone(), weak);
    }

    // One draggable handle; returns its drag delta in (time, value) units.
    let sense = if editable {
        egui::Sense::drag()
    } else {
        egui::Sense::hover()
    };
    let mut handle = |ui: &mut egui::Ui,
                      centre: egui::Pos2,
                      hid: egui::Id,
                      c: usize,
                      value: f32,
                      label: String| {
        // B-P35: a handle projected outside the graph (a huge tangent) cannot start a drag there, over other
        // controls; a drag already under way continues wherever the pointer goes.
        let live = rect.contains(centre) || ui.ctx().is_being_dragged(hid);
        let r = ui.interact(
            egui::Rect::from_center_size(centre, egui::vec2(3.0 * HANDLE, 3.0 * HANDLE)),
            hid,
            if live { sense } else { egui::Sense::hover() },
        );
        let fill = if r.dragged() {
            visuals.widgets.active.bg_fill
        } else {
            color(c)
        };
        painter.circle(centre, HANDLE, fill, egui::Stroke::new(1.0, color(c)));
        r.widget_info(|| egui::WidgetInfo::slider(editable, value as f64, &label));
        if r.drag_started() {
            ui.data_mut(|d| d.insert_temp(view_id, [lo, hi, vlo, vhi]));
        }
        if r.drag_stopped() {
            ui.data_mut(|d| d.remove::<[f32; 4]>(view_id));
            out.released = true;
        }
        let d = if r.dragged() {
            r.drag_delta()
        } else {
            egui::Vec2::ZERO
        };
        (d.x * dt, -d.y * dv)
    };
    let suffix = |c: usize| {
        if nch == 1 {
            String::new()
        } else {
            format!(" {}", RGB[c])
        }
    };
    let nk = len(keys);
    for k in 0..nk {
        let t = time(keys, k);
        for c in 0..nch {
            let v = get(keys, k, c, Part::Value);
            if !t.is_finite() || !v.is_finite() {
                continue;
            }
            let label = format!("{name} key {}{}", k + 1, suffix(c));
            let (ddt, ddv) = handle(
                ui,
                egui::pos2(x(t), y(v)),
                id.with(("key", k, c)),
                c,
                v,
                label,
            );
            // D5: no clamp between neighbours; only a moved axis is written (−0.0 + 0.0 would flip its bits).
            if ddt != 0.0 {
                set_time(&mut next, k, t + ddt);
            }
            if ddv != 0.0 {
                set(&mut next, k, c, Part::Value, v + ddv);
            }
        }
    }
    // D7: the cubic Bézier handles of B2's unscaled Hermite, vertical only.
    if key_type(keys) == Some(KeyType::Quadratic as u32) {
        for i in 0..nk.saturating_sub(1) {
            let j = i + 1;
            let (ti, tj) = (time(keys, i), time(keys, j));
            let d = tj - ti;
            if !d.is_finite() || d <= 0.0 {
                continue;
            }
            for c in 0..nch {
                let (vi, vj) = (get(keys, i, c, Part::Value), get(keys, j, c, Part::Value));
                let (bi, fj) = (
                    get(keys, i, c, Part::Backward),
                    get(keys, j, c, Part::Forward),
                );
                if !(vi.is_finite() && vj.is_finite() && bi.is_finite() && fj.is_finite()) {
                    continue;
                }
                let stroke = egui::Stroke::new(1.0, weak);
                let out_h = (ti + d / 3.0, vi + bi / 3.0);
                let in_h = (tj - d / 3.0, vj - fj / 3.0);
                painter.line_segment(
                    [egui::pos2(x(ti), y(vi)), egui::pos2(x(out_h.0), y(out_h.1))],
                    stroke,
                );
                painter.line_segment(
                    [egui::pos2(x(tj), y(vj)), egui::pos2(x(in_h.0), y(in_h.1))],
                    stroke,
                );
                let label = format!("{name} key {} backward handle{}", i + 1, suffix(c));
                let (_, ddv) = handle(
                    ui,
                    egui::pos2(x(out_h.0), y(out_h.1)),
                    id.with(("back", i, c)),
                    c,
                    bi,
                    label,
                );
                if ddv != 0.0 {
                    set(&mut next, i, c, Part::Backward, 3.0 * (out_h.1 + ddv - vi));
                }
                let label = format!("{name} key {} forward handle{}", j + 1, suffix(c));
                let (_, ddv) = handle(
                    ui,
                    egui::pos2(x(in_h.0), y(in_h.1)),
                    id.with(("fwd", j, c)),
                    c,
                    fj,
                    label,
                );
                if ddv != 0.0 {
                    set(&mut next, j, c, Part::Forward, 3.0 * (vj - (in_h.1 + ddv)));
                }
            }
        }
    }
    // F19: compared as bytes, so a NaN key is "unchanged".
    if encode_data(&next) != encode_data(keys) {
        out.keys = Some(next);
    }
    out
}
