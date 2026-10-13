//! M6 Track F2: the RNM rows of a base normal (F2 spec §5). Layer files have no items; their rows come from the
//! base's `ResolvedJob::rnm`.
use engine::config::{LayerId, RnmTarget};
use engine::group::{vocab_of, VocabRole};
use engine::policy::ResolvedJob;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const AO_NOTE: &str =
    "sdc inverts this for you. Supply normal AO (white = open); don't invert it yourself.";

pub struct RnmRow {
    pub id: LayerId,
    pub label: &'static str,
    pub path: PathBuf,
    pub key: String,
    pub strength: f64,
    pub flip_green: bool,
    /// The mask file and whether it is used.
    pub mask: Option<(PathBuf, bool)>,
    pub target: RnmTarget,
}

fn default_target(path: &std::path::Path) -> RnmTarget {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match vocab_of(&stem).map(|v| v.role) {
        Some(VocabRole::NormalLayer(t)) => t.prefix.unwrap_or(RnmTarget::N),
        _ => RnmTarget::N,
    }
}

pub fn rnm_rows(
    job: &ResolvedJob,
    keys: &BTreeMap<PathBuf, String>,
    targets: &dyn Fn(&str) -> Option<RnmTarget>,
) -> Vec<RnmRow> {
    let Some(r) = &job.rnm else { return vec![] };
    let spec = &job.profile.rnm;
    spec.effective_order()
        .into_iter()
        .filter_map(|id| {
            let l = r.layers.iter().find(|l| l.id == id)?;
            let v = spec.layer(id);
            let key = keys.get(&l.file.path).cloned().unwrap_or_default();
            Some(RnmRow {
                id,
                label: id.label(),
                target: targets(&key).unwrap_or_else(|| default_target(&l.file.path)),
                path: l.file.path.clone(),
                key,
                strength: v.strength,
                // A bump never follows the base (Note W8).
                flip_green: v.flip_green.unwrap_or(
                    !matches!(id, LayerId::Bump | LayerId::Bump2)
                        && job.profile.channels.flip_green,
                ),
                mask: l.mask.as_ref().map(|m| (m.path.clone(), v.mask)),
            })
        })
        .collect()
}

/// Swap `id` with its neighbour among `order` (the present layers); unchanged at either end.
pub fn moved(order: &[LayerId], id: LayerId, up: bool) -> Vec<LayerId> {
    let mut v = order.to_vec();
    if let Some(i) = v.iter().position(|x| *x == id) {
        let j = if up {
            i.checked_sub(1)
        } else {
            Some(i + 1).filter(|j| *j < v.len())
        };
        if let Some(j) = j {
            v.swap(i, j);
        }
    }
    v
}

/// The full `rnm.order` after moving `id`: the present layers moved, then the absent ids in effective order.
pub fn order_after_move(job: &ResolvedJob, id: LayerId, up: bool) -> Vec<LayerId> {
    let eff = job.profile.rnm.effective_order();
    let present: Vec<LayerId> = eff
        .iter()
        .copied()
        .filter(|i| {
            job.rnm
                .as_ref()
                .is_some_and(|r| r.layers.iter().any(|l| l.id == *i))
        })
        .collect();
    let mut out = moved(&present, id, up);
    out.extend(eff.into_iter().filter(|i| !present.contains(i)));
    out
}

pub fn rnm_active(job: &ResolvedJob) -> bool {
    job.active_rnm().is_some()
}

/// The AO source, its channel, and whether the bent layer reads it.
pub fn ao_line(job: &ResolvedJob) -> Option<(PathBuf, u8, bool)> {
    let a = job.rnm.as_ref()?.ao.as_ref()?;
    Some((a.file.path.clone(), a.channel, job.profile.rnm.bent.ao))
}
