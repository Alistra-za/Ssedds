//! Debug views (GUI suggestions item 1): one output block injected into the user's own `Lighting.hlsl` expansion
//! right before its single `return psout;`, so CS does all of its own sampling and remaps and every term is right for
//! every permutation. The block writes `psout.Diffuse.xyz` only (alpha test and blend behave as in Lit). It is used
//! only when the view is not Lit, so Lit program keys and the user's warm disk cache are untouched.
use nif::material::MaterialState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DebugOutput {
    #[default]
    Lit,
    Albedo,
    NormalWorld,
    Roughness,
    Metallic,
    Ao,
}

/// What a view shows for one material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Exact,
    /// Not the physical term: the named stand-in (shown in the GUI's hover text).
    StandIn(&'static str),
    /// Constant for this material: not worth a view.
    None,
}

impl DebugOutput {
    pub const ALL: [DebugOutput; 6] = [
        DebugOutput::Lit,
        DebugOutput::Albedo,
        DebugOutput::NormalWorld,
        DebugOutput::Roughness,
        DebugOutput::Metallic,
        DebugOutput::Ao,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DebugOutput::Lit => "Lit",
            DebugOutput::Albedo => "Albedo",
            DebugOutput::NormalWorld => "Normals (world)",
            DebugOutput::Roughness => "Roughness",
            DebugOutput::Metallic => "Metallic",
            DebugOutput::Ao => "AO",
        }
    }

    /// The `SDC_DEBUG` value of the view; `None` for Lit (no define, no injected source).
    pub fn define(self) -> Option<u32> {
        match self {
            DebugOutput::Lit => None,
            DebugOutput::Albedo => Some(1),
            DebugOutput::NormalWorld => Some(2),
            DebugOutput::Roughness => Some(3),
            DebugOutput::Metallic => Some(4),
            DebugOutput::Ao => Some(5),
        }
    }

    /// TruePBR: Exact for all. Non-PBR: Albedo and NormalWorld Exact; Roughness StandIn("1 − specular mask"); Metallic
    /// None (CS's non-PBR material has no metallic term); Ao StandIn("vertex AO"), None for shader types 4, 5, 6 and
    /// 16 (FaceGen, FaceGen tint, Hair, Eye: vertexAO is the constant 1, L:2673, 2689).
    pub fn availability(self, m: &MaterialState) -> Availability {
        if self == DebugOutput::Lit || m.is_pbr() {
            return Availability::Exact;
        }
        match self {
            DebugOutput::Lit | DebugOutput::Albedo | DebugOutput::NormalWorld => {
                Availability::Exact
            }
            DebugOutput::Roughness => Availability::StandIn("1 − specular mask"),
            DebugOutput::Metallic => Availability::None,
            DebugOutput::Ao if matches!(m.shader_type, 4 | 5 | 6 | 16) => Availability::None,
            DebugOutput::Ao => Availability::StandIn("vertex AO"),
        }
    }
}

/// The one statement the block goes before.
const ANCHOR: &str = "return psout;";
/// The `main`-scope declarations the block reads (installed CS: L:1828, 1642/1645, 1361, 2673/2689/2691). Checked as
/// declarations: a bare `MaterialProperties material` also matches include-file function parameters (plan R5).
const NAMES: [&str; 4] = [
    "MaterialProperties material =",
    "float3 worldNormal",
    "float glossiness",
    "float vertexAO",
];

/// The injected output. Data views are pre-decoded when the readback sRGB-encodes (`SDC_DEBUG_PRELINEAR`), so the
/// shown value is the data value; Albedo is a colour and is written as is. Non-PBR materials have no `Metallic`
/// field, so that view writes 0 there (plan R4). The EOTF is written out: no CS helper name is relied on.
pub const BLOCK: &str = "#if defined(SDC_DEBUG)
\t{
\t\tfloat3 sdc;
#\tif SDC_DEBUG == 1
\t\tsdc = material.BaseColor;
#\telse
#\t\tif SDC_DEBUG == 2
\t\tsdc = worldNormal * 0.5 + 0.5;
#\t\telif SDC_DEBUG == 3
#\t\t\tif defined(TRUE_PBR)
\t\tsdc = material.Roughness.xxx;
#\t\t\telse
\t\tsdc = (1 - glossiness).xxx;
#\t\t\tendif
#\t\telif SDC_DEBUG == 4
#\t\t\tif defined(TRUE_PBR)
\t\tsdc = material.Metallic.xxx;
#\t\t\telse
\t\tsdc = 0.0.xxx;
#\t\t\tendif
#\t\telse
#\t\t\tif defined(TRUE_PBR)
\t\tsdc = (material.AO * vertexAO).xxx;
#\t\t\telse
\t\tsdc = vertexAO.xxx;
#\t\t\tendif
#\t\tendif
#\t\tif defined(SDC_DEBUG_PRELINEAR)
\t\tsdc = sdc <= 0.04045 ? sdc / 12.92 : pow((saturate(sdc) + 0.055) / 1.055, 2.4);
#\t\tendif
#\tendif
\t\tpsout.Diffuse.xyz = sdc;
\t}
#endif
";

/// `text` has `name` as a whole identifier: not followed by an identifier character, so a renamed `worldNormalWS`
/// does not pass for `worldNormal` (the block would then fail to compile instead of the view being refused).
fn declares(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(i, _)| {
        !text[i + name.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// `text` with [`BLOCK`] inserted right before its single `return psout;`. Err when the text does not have exactly
/// one, or lacks one of the declarations the block reads.
pub fn inject(text: &str) -> Result<String, String> {
    let n = text.matches(ANCHOR).count();
    if n != 1 {
        return Err(format!(
            "Lighting.hlsl has {n} `{ANCHOR}`; the debug views need exactly one"
        ));
    }
    if let Some(name) = NAMES.iter().find(|name| !declares(text, name)) {
        return Err(format!(
            "Lighting.hlsl has no `{name}`; the debug views read it"
        ));
    }
    let at = text.find(ANCHOR).expect("counted once above");
    // Before the anchor's own line, so its indentation stays with it.
    let line = text[..at].rfind('\n').map_or(0, |i| i + 1);
    Ok(format!("{}{BLOCK}{}", &text[..line], &text[line..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = "MaterialProperties material = (MaterialProperties)0;\nfloat3 worldNormal = n;\nfloat glossiness = 0;\nfloat vertexAO = 1;\n\treturn psout;\n}\n";

    #[test]
    fn the_block_goes_right_before_the_one_return() {
        let out = inject(MINI).unwrap();
        let at = out.find("\treturn psout;").unwrap();
        assert!(out[..at].ends_with(BLOCK), "{out}");
        assert_eq!(out.matches("return psout;").count(), 1);
    }

    #[test]
    fn zero_or_two_returns_or_a_missing_name_is_an_error() {
        assert!(inject(&MINI.replace("return psout;", "")).is_err());
        assert!(inject(&format!("{MINI}\treturn psout;\n")).is_err());
        for name in [
            "MaterialProperties material =",
            "float3 worldNormal",
            "float glossiness",
            "float vertexAO",
        ] {
            let e = inject(&MINI.replace(name, "x")).unwrap_err();
            assert!(e.contains(name), "{e}");
        }
    }

    #[test]
    fn a_suffix_renamed_declaration_is_an_error() {
        for (name, renamed) in [
            (
                "MaterialProperties material =",
                "MaterialProperties materialX =",
            ),
            ("float3 worldNormal", "float3 worldNormalWS"),
            ("float glossiness", "float glossinessX"),
            ("float vertexAO", "float vertexAO2"),
        ] {
            let e = inject(&MINI.replace(name, renamed)).unwrap_err();
            assert!(e.contains(name), "{e}");
        }
    }

    #[test]
    fn the_standin_shader_has_no_anchor() {
        assert!(inject(include_str!("../tests/fixtures/standin/Lighting.hlsl")).is_err());
    }

    #[test]
    fn availability_table() {
        use Availability::*;
        let mut v = MaterialState::builtin_default();
        let mut p = v.clone();
        p.slsf2 |= nif::material::SLSF2_TRUE_PBR;
        for d in DebugOutput::ALL {
            assert_eq!(d.availability(&p), Exact, "{d:?} TruePBR");
        }
        assert_eq!(DebugOutput::Albedo.availability(&v), Exact);
        assert_eq!(DebugOutput::NormalWorld.availability(&v), Exact);
        assert_eq!(
            DebugOutput::Roughness.availability(&v),
            StandIn("1 − specular mask")
        );
        assert_eq!(DebugOutput::Metallic.availability(&v), None);
        assert_eq!(DebugOutput::Ao.availability(&v), StandIn("vertex AO"));
        for t in [4, 5, 6, 16] {
            v.shader_type = t;
            assert_eq!(
                DebugOutput::Ao.availability(&v),
                None,
                "type {t}: vertexAO is constant 1"
            );
        }
        assert_eq!(DebugOutput::Lit.define(), Option::None);
        assert_eq!(DebugOutput::Ao.define(), Some(5));
    }
}
