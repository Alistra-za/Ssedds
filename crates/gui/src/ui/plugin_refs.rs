//! "Used by plugins" in the set editor (esp scanner spec §4.1). Information only: no PGPatcher warning (spec §1).
use crate::model::meshes::overridden_text;
use crate::model::plugin_refs::{set_key, SetPluginRefs, LIMIT};
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
            model.plugin_refs.get(model.scan_db.as_deref(), &key);
            // Split borrows: the cached rows and the toggle are separate fields.
            let SetPluginRefs {
                cached,
                show_inert: show,
            } = &mut model.plugin_refs;
            let Some((_, result)) = cached else {
                return;
            };
            match result {
                Err(e) => {
                    ui.weak(e.as_str());
                }
                Ok(u) => {
                    let n_inert = u
                        .txsts
                        .iter()
                        .filter(|t| u.inert.contains_key(&t.formid))
                        .count()
                        + u.alts
                            .iter()
                            .filter(|a| u.inert.contains_key(&a.formid))
                            .count();
                    if n_inert > 0 {
                        ui.checkbox(show, format!("Show inert ({n_inert})"));
                    }
                    if u.txsts
                        .iter()
                        .all(|t| !*show && u.inert.contains_key(&t.formid))
                    {
                        ui.label("No texture set of the latest scan matches this set.");
                    }
                    for t in &u.txsts {
                        let text = format!(
                            "{} · slot {}: {}{}",
                            record_label(t.edid.as_deref(), t.formid, "TXST", &t.plugin),
                            // Note F?18 (GH7): slots count from 1, as in the NIF edit form.
                            t.slot + 1,
                            t.path,
                            overridden_text(t.n_overrides)
                        );
                        match u.inert.get(&t.formid) {
                            Some(why) if *show => {
                                ui.weak(format!("{text} · {why}"));
                            }
                            Some(_) => {}
                            None => {
                                ui.label(text);
                            }
                        }
                    }
                    if u.truncated {
                        ui.weak(format!("… only the first {LIMIT} are listed"));
                    }
                    let alts: Vec<_> = u
                        .alts
                        .iter()
                        .filter(|a| *show || !u.inert.contains_key(&a.formid))
                        .collect();
                    if !alts.is_empty() {
                        ui.label(egui::RichText::new("Alternate textures using them").strong());
                        for a in alts {
                            let text = format!(
                                "{} · {} → {}",
                                record_label(a.edid.as_deref(), a.formid, &a.sig, &a.plugin),
                                a.swap.shape_name,
                                a.swap.txst_edid.as_deref().unwrap_or("(no EDID)")
                            );
                            match u.inert.get(&a.formid) {
                                Some(why) => {
                                    ui.weak(format!("{text} · {why}"));
                                }
                                None => {
                                    ui.label(text);
                                }
                            }
                        }
                    }
                }
            }
        });
}
