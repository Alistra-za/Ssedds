//! `[layout]` in `gui.toml`: the window and the arrangement the app comes back to (DECISIONS AG3, AG17).
//! Every field is read on its own: a value of the wrong shape or range is ignored, never a whole-file failure.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What `[layout.widths]` may name: the Textures and Sets columns, the set 3D panel, the Meshes tree, the Advanced
/// drawer and the 2D preview's settings panel (fixed at 280 pt today, so its saved width changes nothing yet).
pub const WIDTH_IDS: [&str; 6] = [
    "textures",
    "sets",
    "set-3d",
    "meshes-tree",
    "advanced",
    "preview-settings",
];
/// The Textures filter toggles, by their `gui.toml` names.
pub const FILTER_NAMES: [&str; 4] = ["needs-decision", "warnings", "failed", "not-in-set"];
/// A saved panel width outside this range (points) is ignored.
pub const WIDTH_RANGE: (f32, f32) = (200.0, 8000.0);

/// Reads `T` out of any TOML value; a value of the wrong shape gives `T::default()` instead of failing the file.
pub(super) fn lenient<
    'de,
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
>(
    d: D,
) -> Result<T, D::Error> {
    Ok(toml::Value::deserialize(d)?.try_into().unwrap_or_default())
}

fn lenient_strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match toml::Value::deserialize(d)? {
        toml::Value::Array(a) => a
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    })
}

fn lenient_widths<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, f32>, D::Error> {
    let toml::Value::Table(t) = toml::Value::deserialize(d)? else {
        return Ok(BTreeMap::new());
    };
    Ok(t.into_iter()
        .filter_map(|(k, v)| {
            let w = v.as_float().or_else(|| v.as_integer().map(|i| i as f64))?;
            Some((k, w as f32))
        })
        .collect())
}

fn lenient_one<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    Ok(toml::Value::deserialize(d)?
        .try_into::<f32>()
        .unwrap_or(1.0))
}

fn one() -> f32 {
    1.0
}

/// `[layout.window]`: the inner size and outer position in egui points, and the scale they were taken at (monitors
/// are checked in pixels: points times `ppp`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSettings {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default = "one", deserialize_with = "lenient_one")]
    pub ppp: f32,
    #[serde(default, deserialize_with = "lenient")]
    pub maximized: bool,
}

/// `[layout.look]`: the 3D panel's View controls. A field that is missing or invalid keeps the default look.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LookSettings {
    #[serde(deserialize_with = "lenient")]
    pub hdri: Option<bool>,
    #[serde(deserialize_with = "lenient")]
    pub grey: Option<f32>,
    #[serde(deserialize_with = "lenient")]
    pub sun: Option<f32>,
    #[serde(deserialize_with = "lenient")]
    pub point: Option<f32>,
    #[serde(deserialize_with = "lenient")]
    pub radius: Option<f32>,
}

impl LookSettings {
    /// Every field into its range; a value that is not a finite number is dropped.
    fn clamp(&mut self) {
        let fix = |v: &mut Option<f32>, (lo, hi): (f32, f32)| {
            *v = v.filter(|f| f.is_finite()).map(|f| f.clamp(lo, hi));
        };
        fix(&mut self.grey, (0.0, 1.0));
        fix(&mut self.sun, (0.0, 4.0));
        fix(&mut self.point, (0.0, 4.0));
        fix(&mut self.radius, (0.1, 10.0));
    }
}

/// `[layout]`. Not the zoom, the selection or the open folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSettings {
    /// Dropped when it is not a whole, sensible rectangle.
    #[serde(deserialize_with = "lenient")]
    pub window: Option<WindowSettings>,
    /// Panel widths in points, by [`WIDTH_IDS`] name.
    #[serde(deserialize_with = "lenient_widths")]
    pub widths: BTreeMap<String, f32>,
    /// The Meshes view instead of the Textures view.
    #[serde(deserialize_with = "lenient")]
    pub meshes_view: bool,
    /// The Textures filters that are on, by [`FILTER_NAMES`] name.
    #[serde(deserialize_with = "lenient_strings")]
    pub filters: Vec<String>,
    /// The preview channel, as `Channel::shader_id`.
    #[serde(deserialize_with = "lenient")]
    pub preview_channel: u8,
    /// `side-by-side` or `split`.
    #[serde(deserialize_with = "lenient")]
    pub preview_layout: String,
    /// The Meshes view's 3D look and the set preview's.
    #[serde(deserialize_with = "lenient")]
    pub look: Option<LookSettings>,
    #[serde(deserialize_with = "lenient")]
    pub set_look: Option<LookSettings>,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        LayoutSettings {
            window: None,
            widths: BTreeMap::new(),
            meshes_view: false,
            filters: vec![],
            preview_channel: 0,
            preview_layout: "side-by-side".into(),
            look: None,
            set_look: None,
        }
    }
}

impl LayoutSettings {
    /// Ignores, field by field, what the app could not use: an unknown or out-of-range value goes back to its default.
    pub fn sanitize(&mut self) {
        let sane = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
        self.window = self.window.filter(|w| {
            sane(w.w, 200.0, 16000.0)
                && sane(w.h, 200.0, 16000.0)
                && sane(w.x, -32000.0, 32000.0)
                && sane(w.y, -32000.0, 32000.0)
                && sane(w.ppp, 0.5, 8.0)
        });
        self.widths.retain(|k, w| {
            WIDTH_IDS.contains(&k.as_str()) && sane(*w, WIDTH_RANGE.0, WIDTH_RANGE.1)
        });
        let mut seen: Vec<String> = vec![];
        self.filters.retain(|f| {
            let keep = FILTER_NAMES.contains(&f.as_str()) && !seen.contains(f);
            seen.push(f.clone());
            keep
        });
        if self.preview_channel > 5 {
            self.preview_channel = 0;
        }
        if !["side-by-side", "split"].contains(&self.preview_layout.as_str()) {
            self.preview_layout = "side-by-side".into();
        }
        for l in [&mut self.look, &mut self.set_look].into_iter().flatten() {
            l.clamp();
        }
    }
}
