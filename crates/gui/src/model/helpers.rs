//! Note DX17: the helper panel's edits. Each writes one `helper.*` selector of the helper file's own `[files]` entry in
//! `sdc.toml` as one undo step, then plans again; a read-only `sdc.toml` takes none (`AppModel::edit`).
use super::AppModel;
use engine::helper::report::HelperReport;
use engine::helper::{FillTarget, HelperRead, HelperRole};
use engine::helper::{FILLS_FIELD, INVERT_FIELD, READ_FIELD, ROLE_FIELD};
use serde_json::{json, Value};
use std::path::Path;

/// The set's base name that fill targets are labelled with ("x" in "x_rmaos R").
pub fn set_base(h: &HelperReport) -> String {
    Path::new(&h.set_key)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The `helper.fills` entries of the report's fills: layer blends (`Rnm`) are reports only, never written.
pub fn written_fills(h: &HelperReport) -> Vec<FillTarget> {
    h.fills
        .iter()
        .copied()
        .filter(|t| !matches!(t, FillTarget::Rnm(_)))
        .collect()
}

impl AppModel {
    fn helper_edit(&mut self, label: &str, key: &str, field: &'static str, v: Option<Value>) {
        let k = key.to_string();
        self.edit(label, move |p| match v {
            Some(v) => p.set_file_value(&k, field, &v),
            None => p.clear_file_value(&k, field),
        });
    }

    /// `None` clears `helper.role` (the role guessed from the name again).
    pub fn set_helper_role(&mut self, key: &str, role: Option<HelperRole>) {
        let v = role.map(|r| json!(r.id()));
        self.helper_edit("Helper role", key, ROLE_FIELD, v);
    }

    /// `None` clears `helper.fills` (the suggestion again); `Some(vec![])` writes `[]` (not used).
    pub fn set_helper_fills(&mut self, key: &str, fills: Option<Vec<FillTarget>>) {
        let v = fills.map(|f| json!(f.iter().map(FillTarget::id).collect::<Vec<_>>()));
        self.helper_edit("Helper fills", key, FILLS_FIELD, v);
    }

    /// `None` clears `helper.read` (R, or the recipe's luma).
    pub fn set_helper_read(&mut self, key: &str, read: Option<HelperRead>) {
        let v = read.map(|r| json!(r.id()));
        self.helper_edit("Helper read from", key, READ_FIELD, v);
    }

    /// `false` clears `helper.invert`.
    pub fn set_helper_invert(&mut self, key: &str, on: bool) {
        self.helper_edit(
            "Helper invert",
            key,
            INVERT_FIELD,
            on.then_some(json!(true)),
        );
    }
}
