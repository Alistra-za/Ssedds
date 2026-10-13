//! M6 Track F2 Task 2: normal-layer vocabulary and grouping (F2 spec §2.1–2.2).
use engine::config::{LayerId, RnmTarget};
use engine::discover::{InputFile, InputKind};
use engine::group::{group_sets, split_suffix_with, vocab_of, VocabRole};
use engine::rnm::LayerTerm;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn term(stem: &str) -> Option<LayerTerm> {
    match vocab_of(stem)?.role {
        VocabRole::NormalLayer(t) => Some(t),
        _ => None,
    }
}

fn t(layer: LayerId, mask: bool, prefix: Option<RnmTarget>) -> Option<LayerTerm> {
    Some(LayerTerm {
        layer,
        mask,
        prefix,
    })
}

#[test]
fn owner_suffixes_and_rnm_py_aliases() {
    use LayerId::*;
    for (stem, want) in [
        ("rock_bump", t(Bump, false, None)),
        ("rock_bump2", t(Bump2, false, None)),
        ("rock_bumpmask", t(Bump, true, None)),
        ("rock_bumpmask2", t(Bump2, true, None)),
        ("rock_detailnorm", t(Detail, false, None)),
        ("rock_detailnorm2", t(Detail2, false, None)),
        ("rock_detailnormmask", t(Detail, true, None)),
        ("rock_detailnormmask2", t(Detail2, true, None)),
        ("rock_bent", t(Bent, false, None)),
        ("rock_bentmask", t(Bent, true, None)),
        ("rock_detail", t(Detail, false, None)),
        ("rock_detail2", t(Detail2, false, None)),
        ("rock_detail_mask", t(Detail, true, None)),
        ("rock_detail2_mask", t(Detail2, true, None)),
        ("rock_bump_mask", t(Bump, true, None)),
        ("rock_bump2_mask", t(Bump2, true, None)),
        ("rock_bent_mask", t(Bent, true, None)),
        ("rock_cnr_bump", t(Bump, false, Some(RnmTarget::Cnr))),
        ("rock_n_detailnorm2", t(Detail2, false, Some(RnmTarget::N))),
        ("rock_cnr_bentmask", t(Bent, true, Some(RnmTarget::Cnr))),
        (
            "Rock_CNR_DetailNormMask",
            t(Detail, true, Some(RnmTarget::Cnr)),
        ),
    ] {
        assert_eq!(term(stem), want, "{stem}");
    }
}

#[test]
fn layers_split_to_their_set_base_and_canonical_suffix() {
    let none = BTreeMap::new();
    let s = split_suffix_with("rock_cnr_detail_mask", &none);
    assert_eq!(
        (s.base.as_str(), s.suffix.as_deref()),
        ("rock", Some("cnr_detailnormmask"))
    );
    let s = split_suffix_with("rock_bump", &none);
    assert_eq!(
        (s.base.as_str(), s.suffix.as_deref()),
        ("rock", Some("bump"))
    );
    // `_norm` is an alias of `_n` (rnm.py), green convention unstated like `_normal`.
    assert_eq!(vocab_of("rock_norm").unwrap().role, VocabRole::Normal(None));
    assert_eq!(
        split_suffix_with("rock_norm", &none).suffix.as_deref(),
        Some("n")
    );
}

fn file(rel: &str) -> InputFile {
    InputFile {
        path: PathBuf::from("in").join(rel),
        rel: PathBuf::from(rel),
        game_path: rel.to_string(),
        output: PathBuf::from("out").join(rel).with_extension("dds"),
        kind: InputKind::Image,
    }
}

/// The Track E bug: `foo_bump.png` was its own set (a bare stem, auto-accepted as a diffuse). It now joins `foo`.
#[test]
fn bump_joins_the_base_set() {
    let files = vec![
        file("foo.png"),
        file("foo_n.png"),
        file("foo_bump.png"),
        file("foo_cnr_bumpmask.png"),
    ];
    let (sets, dups) = group_sets(&files);
    assert!(dups.is_empty());
    assert_eq!(sets.len(), 1, "{sets:?}");
    assert_eq!(sets[0].members.len(), 4);
    assert!(sets[0].suffixes.contains("bump") && sets[0].suffixes.contains("cnr_bumpmask"));
}
