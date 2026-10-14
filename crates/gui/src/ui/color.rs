//! The shared colour input (DECISIONS AG2): egui's swatch with its colour-square popup, and a `#RRGGBB` box.
use eframe::egui;

/// `#1A2B3C`, `1a2b3c`, with the `#` and surrounding spaces optional; anything else is `None`.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// The box's text for `rgb`: uppercase `#RRGGBB`.
pub fn format_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// A 0..=1 float channel as a byte, rounded and clamped.
pub fn unit_to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A byte as a 0..=1 float.
pub fn u8_to_unit(b: u8) -> f32 {
    f32::from(b) / 255.0
}

/// Swatch + hex box for `rgb` (sRGB bytes). Returns the new value when the popup changed it, or when the hex was
/// committed (Enter or focus loss) as a valid, different colour. Invalid text is outlined red and never returned;
/// on focus loss it reverts to `rgb`. The caller stores the result through its usual commit path.
pub fn color_field(ui: &mut egui::Ui, id: egui::Id, rgb: [u8; 3]) -> Option<[u8; 3]> {
    let mut out = None;
    let mut picked = rgb;
    if ui.color_edit_button_srgb(&mut picked).changed() {
        out = Some(picked);
    }
    let buf = id.with("hex");
    let mut text = ui
        .data(|d| d.get_temp::<String>(buf))
        .unwrap_or_else(|| format_hex(rgb));
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(64.0));
    if parse_hex(&text).is_none() {
        ui.painter().rect_stroke(
            r.rect,
            2.0,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(210, 60, 60)),
            egui::StrokeKind::Outside,
        );
    }
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(buf, text.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(buf));
        if let Some(v) = parse_hex(&text).filter(|v| *v != rgb) {
            out.get_or_insert(v);
        }
    }
    out
}
