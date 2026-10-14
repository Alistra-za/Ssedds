//! GUI tweaks G1/G5: a searchable list of known paths, drawn inside a menu or popup. Drawing only.
use crate::model::known_paths::filter;
use eframe::egui;
use std::sync::Arc;

/// Paths listed at most; typing narrows them.
pub const LIMIT: usize = 200;

/// A filter box over `paths`, the matches, and (with `typed`) a "Use" row for the text as typed. Returns the pick.
pub fn list(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    paths: &[String],
    typed: bool,
) -> Option<String> {
    let id = egui::Id::new(("path-picker", salt));
    let mut q = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
    let mut picked = None;
    let r = ui.add(
        egui::TextEdit::singleline(&mut q)
            .hint_text("Filter: words of the path")
            .desired_width(360.0),
    );
    if r.changed() {
        ui.data_mut(|d| d.insert_temp(id, q.clone()));
    }
    let hits = filter(paths, &q, LIMIT);
    if paths.is_empty() {
        ui.weak("No known paths yet: scan your mods to list them.");
    } else if hits.len() == LIMIT {
        ui.weak(format!(
            "The first {LIMIT} matches; type more to narrow them."
        ));
    } else {
        ui.weak(format!("{} of {} paths", hits.len(), paths.len()));
    }
    if typed && !q.trim().is_empty() && ui.button(format!("Use “{}”", q.trim())).clicked() {
        picked = Some(q.trim().to_string());
    }
    egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .max_height(280.0)
        .show(ui, |ui| {
            for p in hits {
                if ui.selectable_label(false, p).clicked() {
                    picked = Some(p.to_string());
                }
            }
        });
    if picked.is_some() {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    picked
}

/// `make()` cached in egui memory under `salt` while `key` stays the same (the lists can be large).
pub fn cached(
    ui: &egui::Ui,
    salt: &str,
    key: (usize, u64),
    make: impl FnOnce() -> Vec<String>,
) -> Arc<Vec<String>> {
    let id = egui::Id::new(("path-picker-cache", salt));
    if let Some((k, v)) = ui.data(|d| d.get_temp::<((usize, u64), Arc<Vec<String>>)>(id)) {
        if k == key {
            return v;
        }
    }
    let v = Arc::new(make());
    ui.data_mut(|d| d.insert_temp(id, (key, v.clone())));
    v
}
