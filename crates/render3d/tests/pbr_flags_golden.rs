//! `pbr_flags` on real PGPatcher output NIFs (read-only owner data). Ignored: needs the MO2 instance.
//! `$env:SDC_ACHERUS='<your MO2 instance>'; cargo test -p render3d --test pbr_flags_golden -- --ignored`
//! Without SDC_ACHERUS it panics rather than passing silently. Only the class bits of 01 §3.4 are asserted.
use render3d::material::{
    PBR_FUZZ, PBR_GLINT, PBR_HAS_DISPLACEMENT, PBR_HAS_EMISSIVE, PBR_TWO_LAYER,
};
use render3d::textures::register_for_slot;
use render3d::{parse, pbr_flags, Bound};

#[test]
#[ignore = "needs SDC_ACHERUS (MO2 instance root)"]
fn pgpatcher_output_class_bits() {
    let root = std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root");
    // (path under mods/ParallaxGen_Output, shape name, class bits that must be set)
    let cases: [(&str, &str, u32); 4] = [
        (
            "meshes/clutter/eyeofmagnus/eyeofmagnus01.nif",
            "SectionD10:0",
            PBR_HAS_EMISSIVE | PBR_HAS_DISPLACEMENT,
        ),
        (
            "meshes/dungeons/caves/ice/largehall/caveilhall1way128u01.nif",
            "CaveILHall1Way128U01:6",
            PBR_TWO_LAYER,
        ),
        (
            "meshes/dlc01/clothes/vampire/vampirerobesfkha_1.nif",
            "robes_bodice",
            PBR_FUZZ,
        ),
        (
            "meshes/architecture/sovengarde/sovstairspiece01.nif",
            "SOVStairsPiece01:0",
            PBR_GLINT,
        ),
    ];
    for (rel, shape, want) in cases {
        let path = format!("{root}/mods/ParallaxGen_Output/{rel}");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let nif = parse(&bytes);
        let s = nif
            .shapes
            .iter()
            .find(|s| &*s.name == shape)
            .unwrap_or_else(|| panic!("{rel}: no shape {shape}"));
        let m = &s.material;
        // Every register the non-empty slots would bind (the `load` rule, minus the PS-reads filter).
        let regs = m
            .slots
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.norm.is_empty())
            .filter_map(|(i, _)| {
                register_for_slot(m.shader_type, m.is_pbr(), m.slsf1 & (1 << 12) != 0, i)
            });
        let flags = pbr_flags(
            m,
            &Bound {
                regs: regs.collect(),
            },
        );
        println!("{shape}: PBRFlags = {flags} ({flags:#b})");
        assert_eq!(flags & want, want, "{rel} {shape}: flags {flags:#b}");
    }
}
