//! The on-disk side of [`crate::session::AnalysisCache`] (DECISIONS Note DU): each source's (or packed image's)
//! analysis, source info and thumbnail, kept between sessions so an unchanged image is not decoded again.
//!
//! One file per entry, named by the sha256 of its key: a JSON header line, then the thumbnail's RGBA8 bytes. The
//! key holds everything an entry's meaning depends on, and the header repeats it, so a lookup trusts only an entry
//! whose stored key equals the wanted one. Every failure is a miss: the cache is an optimisation, never an error.
use crate::analyse::{AnalyseHints, Analysis};
use crate::dds::write_atomic;
use crate::image::SourceInfo;
use crate::policy::SourceStamp;
use crate::session::{PackEntry, Thumbnail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Bump whenever `analyse_with`, `make_thumbnail`, pack assembly's analysed image or any `Analysis` field changes
/// meaning: entries written under another value are ignored. The pin test (analysis_version.rs) catches the first,
/// second and last; pack assembly is not covered (Note DU4).
pub const ANALYSIS_VERSION: u32 = 1;

const EXT: &str = "ent";

pub struct AnalysisStore {
    dir: PathBuf,
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct Head {
    id: String,
    analysis: Analysis,
    info: SourceInfo,
    #[serde(default)]
    non_gray: Vec<PathBuf>,
    /// Width and height of the RGBA8 bytes after the header line.
    thumb: Option<(u32, u32)>,
}

impl AnalysisStore {
    pub fn new(dir: PathBuf) -> AnalysisStore {
        AnalysisStore {
            dir,
            version: ANALYSIS_VERSION,
        }
    }
    /// A store that behaves as another analysis version (tests).
    #[doc(hidden)]
    pub fn with_version(mut self, version: u32) -> AnalysisStore {
        self.version = version;
        self
    }

    fn id(&self, hints: AnalyseHints, what: &str) -> String {
        format!("{}|{hints:?}|{what}", self.version)
    }
    fn file_id(&self, path: &Path, stamp: SourceStamp, hints: AnalyseHints) -> String {
        let what = format!("file|{}|{}|{}", path.display(), stamp.size, stamp.mtime_ns);
        self.id(hints, &what)
    }
    fn pack_id(&self, key: &str, hints: AnalyseHints) -> String {
        self.id(hints, &format!("pack|{key}"))
    }
    fn path_of(&self, id: &str) -> PathBuf {
        self.dir
            .join(format!("{}.{EXT}", hex::encode(Sha256::digest(id))))
    }

    /// The entry stored under `id`; a missing, unreadable, corrupt or mismatched file is `None`.
    fn get(&self, id: &str) -> Option<(Head, Option<Arc<Thumbnail>>)> {
        let bytes = std::fs::read(self.path_of(id)).ok()?;
        let nl = bytes.iter().position(|&b| b == b'\n')?;
        let head: Head = serde_json::from_slice(&bytes[..nl]).ok()?;
        if head.id != id {
            return None;
        }
        let rgba = &bytes[nl + 1..];
        let thumb = match head.thumb {
            Some((width, height)) if rgba.len() == width as usize * height as usize * 4 => {
                Some(Arc::new(Thumbnail {
                    width,
                    height,
                    rgba: rgba.to_vec(),
                }))
            }
            None if rgba.is_empty() => None,
            _ => return None,
        };
        Some((head, thumb))
    }
    /// Store an entry; errors are ignored.
    fn put(&self, head: Head, thumb: Option<&Thumbnail>) {
        let Ok(mut bytes) = serde_json::to_vec(&head) else {
            return;
        };
        // serde_json writes a non-finite float as `null`, which does not read back: such an analysis is not stored.
        match serde_json::from_slice::<Head>(&bytes) {
            Ok(back) if back.analysis == head.analysis => {}
            _ => return,
        }
        bytes.push(b'\n');
        if let Some(t) = thumb {
            bytes.extend_from_slice(&t.rgba);
        }
        let _ = write_atomic(&self.path_of(&head.id), &bytes);
    }

    pub(crate) fn get_file(
        &self,
        path: &Path,
        stamp: SourceStamp,
        hints: AnalyseHints,
    ) -> Option<(Analysis, SourceInfo, Option<Arc<Thumbnail>>)> {
        let (h, t) = self.get(&self.file_id(path, stamp, hints))?;
        Some((h.analysis, h.info, t))
    }
    pub(crate) fn put_file(
        &self,
        path: &Path,
        stamp: SourceStamp,
        hints: AnalyseHints,
        analysis: &Analysis,
        info: SourceInfo,
        thumb: Option<&Thumbnail>,
    ) {
        let head = Head {
            id: self.file_id(path, stamp, hints),
            analysis: analysis.clone(),
            info,
            non_gray: Vec::new(),
            thumb: thumb.map(|t| (t.width, t.height)),
        };
        self.put(head, thumb);
    }
    pub(crate) fn get_pack(&self, key: &str, hints: AnalyseHints) -> Option<PackEntry> {
        let (h, thumb) = self.get(&self.pack_id(key, hints))?;
        Some(PackEntry {
            analysis: h.analysis,
            info: h.info,
            non_gray: h.non_gray,
            thumb,
            hints,
        })
    }
    pub(crate) fn put_pack(&self, key: &str, hints: AnalyseHints, e: &PackEntry) {
        let head = Head {
            id: self.pack_id(key, hints),
            analysis: e.analysis.clone(),
            info: e.info,
            non_gray: e.non_gray.clone(),
            thumb: e.thumb.as_ref().map(|t| (t.width, t.height)),
        };
        self.put(head, e.thumb.as_deref());
    }

    /// Delete the oldest entries (by modification time) until the folder holds at most `max_bytes`.
    // shortcut: an entry's mtime is when it was written, not last used, so a busy old entry goes first; touch
    // entries on a hit if that ever costs reloads.
    pub fn prune(&self, max_bytes: u64) {
        let Ok(dir) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut all: Vec<_> = dir
            .filter_map(|e| {
                let e = e.ok()?;
                let path = e.path();
                (path.extension()? == EXT).then_some(())?;
                let m = e.metadata().ok()?;
                Some((m.modified().ok()?, m.len(), path))
            })
            .collect();
        let mut total: u64 = all.iter().map(|e| e.1).sum();
        all.sort();
        for (_, len, path) in all {
            if total <= max_bytes {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= len;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyse::analyse;
    use crate::image::Rgba32F;

    fn sample() -> (Analysis, SourceInfo) {
        let info = SourceInfo {
            bit_depth: 8,
            channels: 4,
            has_alpha: true,
            is_float: false,
        };
        (
            analyse(&Rgba32F::new(2, 2, [0.5, 0.25, 1.0, 1.0]), &info),
            info,
        )
    }
    const STAMP: SourceStamp = SourceStamp {
        size: 7,
        mtime_ns: 9,
    };

    #[test]
    fn a_stored_entry_reads_back_and_another_version_misses() {
        let dir = tempfile::tempdir().unwrap();
        let (a, info) = sample();
        let thumb = Thumbnail {
            width: 1,
            height: 2,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let (p, h) = (Path::new("a.png"), AnalyseHints::default());
        let s = AnalysisStore::new(dir.path().into());
        s.put_file(p, STAMP, h, &a, info, Some(&thumb));
        let (a2, i2, t2) = s.get_file(p, STAMP, h).unwrap();
        assert_eq!((a2, i2, t2.as_deref()), (a.clone(), info, Some(&thumb)));
        let other = AnalysisStore::new(dir.path().into()).with_version(ANALYSIS_VERSION + 1);
        assert!(other.get_file(p, STAMP, h).is_none());
    }

    #[test]
    fn a_non_finite_analysis_is_not_stored() {
        let dir = tempfile::tempdir().unwrap();
        let (mut a, info) = sample();
        a.corner_err = f32::INFINITY;
        let (p, h) = (Path::new("a.png"), AnalyseHints::default());
        let s = AnalysisStore::new(dir.path().into());
        s.put_file(p, STAMP, h, &a, info, None);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(s.get_file(p, STAMP, h).is_none());
    }

    #[test]
    fn prune_keeps_the_newest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let t0 = std::time::SystemTime::now();
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            let f = std::fs::File::create(dir.path().join(format!("{name}.{EXT}"))).unwrap();
            f.set_len(100).unwrap();
            f.set_modified(t0 + std::time::Duration::from_secs(i as u64))
                .unwrap();
        }
        AnalysisStore::new(dir.path().into()).prune(250);
        assert!(!dir.path().join("a.ent").exists());
        assert!(dir.path().join("b.ent").exists() && dir.path().join("c.ent").exists());
    }
}
