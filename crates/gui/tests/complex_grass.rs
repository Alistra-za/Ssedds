//! Complex grass in the GUI (design `2026-10-12-complex-grass-design.md`, rulings R1, R12).
use engine::config::{Subtype, Target};
use sdc_gui::ui::settings_panel::{offered_groups, subtype_groups};

/// R1 (Review Focus 4): offered under cs and cpm (ENB), not under vanilla; grouped with the packed maps.
#[test]
fn pickers_hide_complex_grass_under_vanilla() {
    for (t, want) in [
        (Target::Cs, true),
        (Target::Cpm, true),
        (Target::Vanilla, false),
    ] {
        let listed = offered_groups(t)
            .into_iter()
            .any(|(_, m)| m.contains(&Subtype::ComplexGrass));
        assert_eq!(listed, want, "{t:?}");
    }
    let groups = subtype_groups(|_| true);
    let (heading, _) = groups
        .iter()
        .find(|(_, m)| m.contains(&Subtype::ComplexGrass))
        .unwrap();
    assert_eq!(*heading, "Packed and data");
}
