//! "Make marker cube…" (cubemap GUI step, Task 6): the dialog's draft and the write. Both go through
//! `engine::cube::marker`, so the bytes and the overwrite guard are those of `sdc cubemap-marker`.
use super::messages::Severity;
use super::picker::{PickerKind, PickerPurpose, PickerSpec};
use super::{AppModel, Dialog};
use eframe::egui;
use engine::cube::marker::{alpha_of, write_marker};
use std::path::Path;

/// What the dialog edits: the raw RGB codes (F0 = (code/255)^1.6 in CS) and the optional roughness (off = alpha 255).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarkerDraft {
    pub rgb: [u8; 3],
    pub roughness: Option<f64>,
}

/// The engine's result, one to one: `Kept` carries the engine's note (a file Tessalith did not write).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerOutcome {
    Written,
    Kept(String),
    Failed(String),
}

/// Write the marker cube for `d` at `path`, as `sdc cubemap-marker` does.
pub fn make_marker(path: &Path, d: &MarkerDraft) -> MarkerOutcome {
    let alpha = match alpha_of(d.roughness) {
        Ok(a) => a,
        Err(e) => return MarkerOutcome::Failed(e),
    };
    match write_marker(path, d.rgb, alpha) {
        Ok(Ok(())) => MarkerOutcome::Written,
        Ok(Err(note)) => MarkerOutcome::Kept(note),
        Err(e) => MarkerOutcome::Failed(e),
    }
}

/// The Save dialog's default name.
pub const MARKER_FILE: &str = "marker_e.dds";

impl AppModel {
    /// The menu entry: open the dialog.
    pub fn open_marker_dialog(&mut self) {
        self.dialog = Some(Dialog::MakeMarker);
    }

    /// The dialog's Save…: ask for the file, starting in `<project textures>\cubemaps` (else the textures folder).
    pub fn pick_marker_file(&self, ctx: &egui::Context) -> bool {
        let dir = self.root.as_ref().map(|r| {
            let c = r.join("cubemaps");
            if c.is_dir() {
                c
            } else {
                r.clone()
            }
        });
        let kind = PickerKind::Save {
            file_name: MARKER_FILE,
            filters: &[("DDS texture", &["dds"])],
            dir,
        };
        self.pick(
            ctx,
            PickerPurpose::MakeMarker,
            PickerSpec {
                title: Some("Save marker cube"),
                kind,
            },
        )
    }

    /// A finished Save dialog: write, report, and close the dialog only when the file was written (a kept or failed
    /// write leaves it open to pick another name).
    pub fn marker_picked(&mut self, path: &Path) {
        let (sev, text) = match make_marker(path, &self.marker) {
            MarkerOutcome::Written => {
                self.dialog = None;
                (
                    Severity::Info,
                    format!("Wrote marker cube: {}", path.display()),
                )
            }
            MarkerOutcome::Kept(note) => (Severity::Warning, note),
            MarkerOutcome::Failed(e) => (
                Severity::Error,
                format!("Could not write the marker cube: {e}"),
            ),
        };
        self.status_line = text.clone();
        self.messages.notify(sev, text);
    }
}
