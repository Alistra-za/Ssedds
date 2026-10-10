use engine::config::Subtype;
use engine::overrides::*;
use serde_json::json;

#[test]
fn parses_scopes_and_typed_values() {
    let o = parse_set("subtype:rmaos.format=bc1").unwrap();
    assert_eq!(
        (o.scope, o.field.as_str(), o.value),
        (Scope::Subtype(Subtype::Rmaos), "format", json!("bc1"))
    );
    let o = parse_set("file:*.png.alpha.policy=blend").unwrap();
    assert_eq!(
        (o.scope, o.field.as_str()),
        (Scope::File("*.png".into()), "alpha.policy")
    );
    let o = parse_set("file:**/wood01_*.png.alpha.threshold=100").unwrap();
    assert_eq!((o.field.as_str(), o.value), ("alpha.threshold", json!(100)));
    let o = parse_set("alpha.exact_opaque=true").unwrap();
    assert_eq!((o.scope, o.value), (Scope::All, json!(true)));
    assert_eq!(parse_set("mips.levels=none").unwrap().value, json!("none"));
    assert!(parse_set("colour.tag=srgb").is_err());
    assert!(parse_set("subtype:nope.format=bc7").is_err());
    assert!(parse_set("format").is_err());
    assert_eq!(
        to_patch("alpha.policy", &json!("data")),
        json!({"alpha": {"policy": "data"}})
    );
}

#[test]
fn subtype_values_are_checked() {
    assert_eq!(
        parse_set("file:*_n.png.subtype=normal").unwrap().value,
        json!("normal")
    );
    assert!(parse_set("subtype=nrmal").is_err());
    assert!(parse_set("file:*.png.subtype=nope").is_err());
    // a sub-type-scoped sub-type is meaningless
    assert!(parse_set("subtype:normal.subtype=msn").is_err());
}

/// M2.5: every packing field is settable, unscoped, per sub-type and per (packed) file name.
#[test]
fn packing_fields_are_settable() {
    for s in [
        "packing.enabled=false",
        "packing.roughness_default=0.6",
        "packing.prefer=raw",
        "packing.fill.metallic=255",
        "subtype:rmaos.packing.fill.ao=200",
        "file:**/iron_rmaos.png.packing.fill.specular=128",
    ] {
        assert!(parse_set(s).is_ok(), "{s}");
    }
    let o = parse_set("file:**/iron_rmaos.png.packing.fill.specular=128").unwrap();
    assert_eq!(
        (o.scope, o.field.as_str(), o.value),
        (
            Scope::File("**/iron_rmaos.png".into()),
            "packing.fill.specular",
            json!(128)
        )
    );
    // Role ids only, not exporter terms.
    assert!(parse_set("packing.fill.metal=1").is_err());
}
