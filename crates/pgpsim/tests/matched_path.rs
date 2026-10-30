//! 01 §1.3 `matchedPath` (the PBR prefix), `rename`, and the ignored `pbr` key (Note GE3).
mod common;
use common::{shape, D, N};
use pgpsim::{matched_path, prepare};
use serde_json::{json, Value};

fn case(name: &str, entry: Value, slots: &[&str], want: Option<&str>) {
    let m = shape(slots);
    let got = matched_path(&prepare(&entry, &Value::Null), &m.slots);
    assert_eq!(got.as_deref(), want, "{name}");
}

#[test]
fn matched_path_table() {
    case(
        "diffuse",
        json!({"match_diffuse": "rock01_d"}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "already PBR",
        json!({"match_diffuse": "rock01_d"}),
        &["textures\\pbr\\landscape\\rock01_d.dds"],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "upper-case NIF path is lower-cased",
        json!({"match_diffuse": "rock01_d"}),
        &["Textures\\Landscape\\Rock01_D.dds"],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "normal",
        json!({"match_normal": "rock01"}),
        &[D, N],
        Some("textures\\pbr\\landscape\\rock01"),
    );
    case(
        "the normal rule runs first",
        json!({"match_normal": "rock01", "match_diffuse": "rock01_d"}),
        &[D, N],
        Some("textures\\pbr\\landscape\\rock01"),
    );
    case(
        "blind erase by the match_normal length when the diffuse rule hit",
        json!({"match_normal": "rock01", "match_diffuse": "rock01_d"}),
        &[D, "textures\\landscape\\other_n.dds"],
        Some("textures\\pbr\\landscape\\r\\rock01"),
    );
    case(
        "rename (golden: Windhelm bridge)",
        json!({"match_diffuse": "nicola89b\\windhelm_bridge\\sndiffuse", "rename": "landscape\\snow01"}),
        &["textures\\nicola89b\\windhelm_bridge\\sndiffuse.dds"],
        Some("textures\\pbr\\landscape\\snow01"),
    );
    case(
        "rename equal ignoring case keeps the field",
        json!({"match_diffuse": "rock01_d", "rename": "ROCK01_D"}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "rename is lower-cased",
        json!({"match_diffuse": "rock01_d", "rename": "Snow\\Snow01"}),
        &[D],
        Some("textures\\pbr\\landscape\\snow\\snow01"),
    );
    case(
        "path_contains keeps the whole diffuse",
        json!({"path_contains": "landscape"}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "match1 cuts the slot-1 suffix",
        json!({"match1": "landscape\\rock01_d.dds"}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01"),
    );
    case(
        "match2 cuts _n",
        json!({"match2": "landscape\\rock01_n.dds"}),
        &[D, N],
        Some("textures\\pbr\\landscape\\rock01"),
    );
    case(
        "pbr:false is ignored since PGPatcher 2.0 (Note GE3)",
        json!({"match_diffuse": "rock01_d", "pbr": false}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "no rule hits",
        json!({"match_diffuse": "other"}),
        &[D],
        None,
    );
}

/// Non-ASCII (`shape` cannot be used: it feeds UTF-8 bytes, but NIF strings are decoded as Windows-1252).
/// The slot is built from its cp1252 bytes, so `é` is 0xE9, as in a real NIF.
#[test]
fn non_ascii_never_panics() {
    let mut m = shape(&["", N]);
    m.slots[0] = nif::path::canon_texture(b"textures\\\xe9_d.dds");
    let got = matched_path(
        &prepare(
            &json!({"match_normal": "aa", "match_diffuse": "é_d"}),
            &Value::Null,
        ),
        &m.slots,
    );
    assert_eq!(
        got.as_deref(),
        Some("textures\\pbr\\\\aa"),
        "cut backs up to a char boundary"
    );
}
