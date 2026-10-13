//! "Used by plugins" in the set editor (esp scanner spec §4.1). Information only: no PGPatcher warning (spec §1).
use crate::model::meshes::overridden_text;
use crate::model::plugin_refs::{set_key, LIMIT};
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui;
use modscan::plugin_store::record_label;

pub fn set_section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    egui::CollapsingHeader::new("Used by plugins")
        .id_salt(("set-used-by-plugins", v.id.as_str()))
        .show(ui, |ui| {
            let Some(key) = set_key(v) else {
                ui.weak("This set has no match key.");
                return;
            };
            if ui.small_button("Refresh").clicked() {
                model.plugin_refs.refresh();
            }
            match model.plugin_refs.get(model.scan_db.as_deref(), &key) {
                Err(e) => {
                    ui.weak(e.as_str());
                }
                Ok(u) => {
                    if u.txsts.is_empty() {
                        ui.label("No texture set of the latest scan matches this set.");
                    }
                    for t in &u.txsts {
                        ui.label(format!(
                            "{} · slot {}: {}{}",
                            record_label(t.edid.as_deref(), t.formid, "TXST", &t.plugin),
                            t.slot,
                            t.path,
                            overridden_text(t.n_overrides)
                        ));
                    }
                    if u.truncated {
                        ui.weak(format!("… only the first {LIMIT} are listed"));
                    }
                    if !u.alts.is_empty() {
                        ui.label(egui::RichText::new("Alternate textures using them").strong());
                        for a in &u.alts {
                            ui.label(format!(
                                "{} · {} → {}",
                                record_label(a.edid.as_deref(), a.formid, &a.sig, &a.plugin),
                                a.swap.shape_name,
                                a.swap.txst_edid.as_deref().unwrap_or("(no EDID)")
                            ));
                        }
                    }
                }
            }
        });
}
