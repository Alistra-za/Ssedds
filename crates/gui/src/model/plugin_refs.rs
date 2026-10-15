//! "Used by plugins" in the set editor (esp scanner spec §4.1): the TXSTs whose slot paths match the set's match key
//! (PGPatcher's own rule, `set_preview::tex_key`, ruling R9) and the records whose alternate textures use them.
use crate::model::sets::SetView;
use modscan::plugin_store::{AltUse, TxstUse};
use modscan::store::{Store, TexKey};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::Path;

/// TXSTs listed; one more is read to say "only the first … are listed".
pub const LIMIT: usize = 200;
pub const NO_SCAN: &str = "No scan yet: scan the MO2 instance in the Meshes view.";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetPluginUse {
    pub txsts: Vec<TxstUse>,
    pub alts: Vec<AltUse>,
    pub truncated: bool,
    /// Inert reasons (filtering spec §3) of the listed TXSTs and alternate-texture users that are inert.
    pub inert: HashMap<u32, &'static str>,
}

/// The section's store reads, one per key (and on Refresh).
#[derive(Debug, Default)]
pub struct SetPluginRefs {
    /// The key the result answers, and the result.
    pub cached: Option<(TexKey, Result<SetPluginUse, String>)>,
    /// "Show inert" in the section; reset whenever the data is re-read.
    pub show_inert: bool,
}

impl SetPluginRefs {
    /// The result for `key`; read from the scan database `db` when the key changed or after `refresh`. Only called
    /// while the section is open.
    pub fn get(&mut self, db: Option<&Path>, key: &TexKey) -> &Result<SetPluginUse, String> {
        // An Err (no scan yet, old schema) is not cached: it is one stat away from changing after a scan.
        let stale = match &self.cached {
            Some((k, r)) => k != key || r.is_err(),
            None => true,
        };
        if stale {
            self.show_inert = false;
            self.cached = Some((key.clone(), query(db, key)));
        }
        match &self.cached {
            Some((_, r)) => r,
            None => unreachable!("set just above"),
        }
    }

    pub fn refresh(&mut self) {
        self.cached = None;
        self.show_inert = false;
    }
}

/// Its own read-only connection: a scan may be writing.
fn query(db: Option<&Path>, key: &TexKey) -> Result<SetPluginUse, String> {
    let path = db
        .filter(|p| p.is_file())
        .ok_or_else(|| NO_SCAN.to_string())?;
    let store = Store::open_read_only(path).map_err(|e| e.to_string())?;
    let mut txsts = store
        .txsts_matching(key, LIMIT + 1)
        .map_err(|e| e.to_string())?;
    let truncated = txsts.len() > LIMIT;
    txsts.truncate(LIMIT);
    let mut ids: Vec<u32> = txsts.iter().map(|t| t.formid).collect();
    let alts = store.alt_users_of(&ids).map_err(|e| e.to_string())?;
    ids.extend(alts.iter().map(|a| a.formid));
    let inert = store.inert_of(&ids).map_err(|e| e.to_string())?;
    Ok(SetPluginUse {
        txsts,
        alts,
        truncated,
        inert,
    })
}

/// The set's match key as PGPatcher reads it: the set's settings, `match_diffuse` defaulting to the derived match.
pub fn set_key(v: &SetView) -> Option<TexKey> {
    let mut entry: Map<String, Value> = v
        .settings
        .iter()
        .map(|(k, x)| (k.clone(), x.clone()))
        .collect();
    if let Some(d) = &v.derived_match {
        entry
            .entry("match_diffuse")
            .or_insert_with(|| Value::String(d.clone()));
        // AN1: a renamed entry is written with its `match_diffuse` only.
        if let Some((md, _)) = crate::pgp::prefix::json_rename(&v.settings, d) {
            entry.retain(|k, _| {
                k == "match_diffuse" || !(k.starts_with("match") || k == "path_contains")
            });
            entry.insert("match_diffuse".into(), Value::String(md));
        }
    }
    crate::model::set_preview::tex_key(&Value::Object(entry), &Value::Null).map(|(k, _)| k)
}
