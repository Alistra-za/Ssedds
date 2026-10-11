//! The DDS header-patch choice for a DDS item (DECISIONS S7). Never automatic: the user picks the tag and presses
//! Patch; the patched copy goes to the export folder, never over the source.
use engine::dds_patch::{plan_patch, PatchRequest};
use engine::policy::SourceStamp;
use std::collections::BTreeMap;
use std::path::Path;

/// What the panel shows for one DDS item.
#[derive(Debug, Clone, PartialEq)]
pub struct DdsHeaderView {
    /// `check_header` findings of the file as it is.
    pub issues: Vec<String>,
    /// The fixes a patch would make (`Ok(None)`: nothing to patch), or why it cannot be patched.
    pub patch: Result<Option<Vec<String>>, String>,
    /// The tag choice: `None` keeps the file's.
    pub tag: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct DdsPatchState {
    /// key → (stamp, tag choice, view); recomputed when the file or the choice changes.
    cache: BTreeMap<String, (Option<SourceStamp>, Option<bool>, DdsHeaderView)>,
    /// The key whose "Patch header…" was pressed and awaits "Patch" or "Cancel".
    pub confirming: Option<String>,
    /// The last result line, shown under the panel.
    pub last: Option<String>,
}

impl DdsPatchState {
    pub(crate) fn view(&mut self, key: &str, path: &Path) -> DdsHeaderView {
        let stamp = SourceStamp::of(path);
        let tag = self.cache.get(key).and_then(|(_, t, _)| *t);
        if let Some((s, t, v)) = self.cache.get(key) {
            if *s == stamp && *t == tag {
                return v.clone();
            }
        }
        let view = compute(path, tag);
        self.cache
            .insert(key.to_string(), (stamp, tag, view.clone()));
        view
    }

    pub(crate) fn set_tag(&mut self, key: &str, tag: Option<bool>) {
        let e = self
            .cache
            .entry(key.to_string())
            .or_insert((None, None, empty()));
        e.0 = None; // force a recompute
        e.1 = tag;
    }
}

fn empty() -> DdsHeaderView {
    DdsHeaderView {
        issues: vec![],
        patch: Ok(None),
        tag: None,
    }
}

fn compute(path: &Path, tag: Option<bool>) -> DdsHeaderView {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return DdsHeaderView {
                issues: vec![],
                patch: Err(e.to_string()),
                tag,
            }
        }
    };
    let issues = engine::dds::read_dds(&bytes)
        .map(|f| engine::dds::check_header(&f))
        .unwrap_or_default();
    let patch = plan_patch(
        &bytes,
        &PatchRequest {
            srgb: tag,
            ..Default::default()
        },
    )
    .map(|p| p.map(|p| p.fixes))
    .map_err(|u| u.0);
    DdsHeaderView { issues, patch, tag }
}

/// Write the patched copy of `src` to `dest` (DECISIONS S7); the caller has resolved both paths.
pub(crate) fn write_patch(src: &Path, dest: &Path, tag: Option<bool>) -> Result<String, String> {
    if engine::dds::same_file(src, dest) {
        return Err("the export folder is the mod folder: patching would overwrite the source; choose another export folder".into());
    }
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    match plan_patch(
        &bytes,
        &PatchRequest {
            srgb: tag,
            ..Default::default()
        },
    ) {
        Ok(Some(p)) => {
            engine::dds::write_atomic(dest, &p.bytes).map_err(|e| e.to_string())?;
            let w = p.warning();
            Ok(format!("{}: {}", w.code.as_str(), w.message))
        }
        Ok(None) => Ok("header already valid; nothing to patch".into()),
        // Only a header refusal is DDS-HEADER-UNPATCHABLE; I/O and same-file errors stay plain.
        Err(u) => Err(format!(
            "{}: {}",
            u.warning().code.as_str(),
            u.warning().message
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fix round 1 (DECISIONS S6 parity): a target naming the source, even by another spelling, is refused and the
    /// source is untouched; the refusal is not labelled DDS-HEADER-UNPATCHABLE.
    #[test]
    fn write_patch_refuses_a_target_that_is_its_source() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.dds");
        std::fs::write(&src, b"not read").unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        for dest in [src.clone(), d.path().join("sub/../A.DDS")] {
            let e = write_patch(&src, &dest, None).unwrap_err();
            assert!(e.contains("would overwrite the source"), "{e}");
            assert!(!e.starts_with("DDS-HEADER-UNPATCHABLE"), "{e}");
        }
        assert_eq!(std::fs::read(&src).unwrap(), b"not read");
    }
}
