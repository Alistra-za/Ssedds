//! The include-expanded entry file (`Lighting.hlsl`) of one CS `Shaders` folder, expanded once and memoised,
//! with the SHA-256 of the expansion: the content half of the shader cache key (03 §5). The expansion does not
//! depend on the defines (the expander follows every `#include`, also inside `#if 0`), so one `ExpandedSource`
//! serves every permutation. The memo lives as long as the `SourceSet`: `Renderer::set_shaders` (a moved or
//! updated CS, which the GUI sends for `Change::Updated` too) makes a new one.
use crate::includes::expand;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// One entry file with every include inlined.
pub struct ExpandedSource {
    pub text: Arc<str>,
    /// SHA-256 of `text`. `#line` names are root-relative, so this does not depend on where the folder is.
    pub sha256: [u8; 32],
    /// The entry file first, then every included file found, as absolute paths.
    pub files: Vec<PathBuf>,
}

pub struct SourceSet {
    root: PathBuf,
    memo: Mutex<HashMap<PathBuf, Arc<ExpandedSource>>>,
    /// [`SourceSet::get_debug`]: the debug-block expansion (or why there is none), per entry file.
    debug_memo: Mutex<HashMap<PathBuf, Result<Arc<ExpandedSource>, String>>>,
}

impl SourceSet {
    pub fn new(root: PathBuf) -> SourceSet {
        SourceSet {
            root,
            memo: Mutex::new(HashMap::new()),
            debug_memo: Mutex::new(HashMap::new()),
        }
    }

    /// `rel` (root-relative, e.g. `Lighting.hlsl`) expanded, from the memo after the first call. Errors are
    /// text (`path: io error`), not memoised.
    pub fn get(&self, rel: &Path) -> Result<Arc<ExpandedSource>, String> {
        let mut memo = self.memo.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(e) = memo.get(rel) {
            return Ok(e.clone());
        }
        let path = self.root.join(rel);
        let e = expand(&path, std::slice::from_ref(&self.root))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let sha256: [u8; 32] = Sha256::digest(e.text.as_bytes()).into();
        let out = Arc::new(ExpandedSource {
            text: e.text.into(),
            sha256,
            files: e.files,
        });
        memo.insert(rel.to_path_buf(), out.clone());
        Ok(out)
    }

    /// The expansion of `rel` with [`crate::debug::BLOCK`] before its one `return psout;`, with its own SHA-256 (so
    /// debug programs key apart from Lit ones). Memoised separately, Err included (the anchor check is a property of
    /// the text); an I/O error from [`SourceSet::get`] is not memoised.
    pub fn get_debug(&self, rel: &Path) -> Result<Arc<ExpandedSource>, String> {
        let base = self.get(rel)?;
        let mut memo = self
            .debug_memo
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(r) = memo.get(rel) {
            return r.clone();
        }
        let r = crate::debug::inject(&base.text).map(|text| {
            let sha256: [u8; 32] = Sha256::digest(text.as_bytes()).into();
            Arc::new(ExpandedSource {
                text: text.into(),
                sha256,
                files: base.files.clone(),
            })
        });
        memo.insert(rel.to_path_buf(), r.clone());
        r
    }
}
