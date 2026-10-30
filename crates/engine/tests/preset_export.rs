//! Note FC10 (T8): `presets export` copies the preset file's text, dropping only `[suffix_aliases]`.

use engine::presets::PresetRegistry;

const COMMENTED: &str = "# my tuned preset\nschema_version = 1\nid = \"ultra\" # keep this name short\nversion = \"2.0.0\"\nbase = \"cs-compact\"\n\n# diffuse gets a softer cut\n[subtypes.diffuse]\nalpha = { threshold = 100 }\n\n[suffix_aliases]\nrmaosultrahigh = \"rmaos\"\n";

#[test]
fn preset_export_keeps_comments_and_drops_aliases() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("ultra.toml");
    std::fs::write(&f, COMMENTED).unwrap();
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_file(&f).unwrap();
    let text = r.export("ultra", "ultra-copy").unwrap();
    assert!(text.contains("# my tuned preset"), "{text}");
    assert!(text.contains("# keep this name short"), "{text}");
    assert!(text.contains("# diffuse gets a softer cut"), "{text}");
    assert!(text.contains("id = \"ultra-copy\""), "{text}");
    assert!(
        !text.contains("suffix_aliases") && !text.contains("rmaosultrahigh"),
        "{text}"
    );
    // Order kept: schema_version, then id, then base, then the diffuse table.
    let at = |s: &str| text.find(s).unwrap();
    assert!(
        at("schema_version") < at("id =")
            && at("id =") < at("base =")
            && at("base =") < at("[subtypes.diffuse]")
    );
    // The copy loads beside the original: no alias clash.
    r.add_toml(&text).unwrap();
}
