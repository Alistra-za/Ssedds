//! LAYER 0: header → shape heads → follow shader/alpha/texture-set refs. One row per shape (00-decision §2 rule 1).
use crate::header::Header;
use crate::path::TexPath;
use crate::props::{
    decode_alpha, decode_effect, decode_lighting, decode_texture_set, Alpha, Effect, Lighting,
};
use crate::shape_head::{decode_head, ShapeHead, ShapeKind};
use crate::text::{label, lossy};
use crate::NifError;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The slots and the effect paths are shared by every shape that names the same property (Note GB4).
#[derive(Debug, Clone, PartialEq)]
pub enum Shader {
    None,
    Lighting(Lighting, Arc<[TexPath]>),
    Effect(Arc<Effect>),
    Other { kind: Arc<str> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRow {
    pub block: u32,
    pub head: ShapeHead,
    pub shader: Shader,
    pub alpha: Option<Alpha>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NifScan {
    pub bsver: u32,
    pub le_mesh: bool,
    pub shapes: Vec<ShapeRow>,
    pub problems: Vec<String>,
    /// Block → PGPatcher 3D index ([`crate::graph::index3d`]); a shape no node reaches is absent.
    pub index3d: BTreeMap<u32, u32>,
}

fn valid(h: &Header, r: i32) -> Option<usize> {
    usize::try_from(r).ok().filter(|&i| i < h.sizes.len())
}

/// A ref that must point at a block of type `want`. -1 is "none"; anything else that is out of range or of
/// another type is recorded as a problem of block `from` and treated as none.
fn typed_ref(
    h: &Header,
    r: i32,
    want: &[u8],
    what: &str,
    from: usize,
    problems: &mut Vec<String>,
) -> Option<usize> {
    if r == -1 {
        return None;
    }
    match valid(h, r) {
        None => {
            problems.push(format!("block {from}: {what} ref {r} out of range"));
            None
        }
        Some(t) if h.kind(t) != want => {
            problems.push(format!(
                "block {from}: {what} ref {r} points at {}, not {}",
                label(&lossy(h.kind(t))),
                lossy(want)
            ));
            None
        }
        Some(t) => Some(t),
    }
}

/// A block's error as a problem line; a long kind name inside it is cut, so a line stays short.
fn problem(block: usize, e: &NifError) -> String {
    label(&format!("block {block}: {e}")).into_owned()
}

/// The shader property at block `s`, its texture set followed, and the problems that raised.
fn shader_of(h: &Header, b: &[u8], s: usize) -> (Shader, Vec<String>) {
    let mut problems = vec![];
    let shader = match h.kind(s) {
        b"BSLightingShaderProperty" => match decode_lighting(h, b, s) {
            Ok(l) => {
                let slots = match typed_ref(
                    h,
                    l.texture_set,
                    b"BSShaderTextureSet",
                    "texture set",
                    s,
                    &mut problems,
                ) {
                    Some(t) => match decode_texture_set(h, b, t) {
                        Ok(ts) => ts.slots,
                        Err(e) => {
                            problems.push(problem(t, &e));
                            vec![]
                        }
                    },
                    None => vec![],
                };
                Shader::Lighting(l, slots.into())
            }
            Err(e) => {
                problems.push(problem(s, &e));
                Shader::None
            }
        },
        b"BSEffectShaderProperty" => match decode_effect(h, b, s) {
            Ok(e) => Shader::Effect(Arc::new(e)),
            Err(e) => {
                problems.push(problem(s, &e));
                Shader::None
            }
        },
        _ => Shader::Other {
            kind: h.kind_arc(s),
        },
    };
    (shader, problems)
}

pub fn scan(b: &[u8]) -> Result<NifScan, NifError> {
    let h = Header::parse(b)?;
    h.check_file_length(b)?;
    let mut out = NifScan {
        bsver: h.bsver,
        le_mesh: h.bsver == 83,
        shapes: vec![],
        problems: vec![],
        index3d: crate::graph::index3d(&h, b),
    };
    let mut shaders: HashMap<usize, (Shader, Vec<String>)> = HashMap::new();
    for i in 0..h.sizes.len() {
        let Some(kind) = ShapeKind::from_name(h.kind(i)) else {
            continue;
        };
        let head = match decode_head(&h, b, i, kind) {
            Ok(hd) => hd,
            Err(e) => {
                out.problems.push(format!("block {i}: {e}"));
                continue;
            }
        };
        let shader = match (head.shader, valid(&h, head.shader)) {
            (-1, _) => Shader::None,
            (r, None) => {
                out.problems
                    .push(format!("block {i}: shader ref {r} out of range"));
                Shader::None
            }
            (_, Some(s)) => {
                // Decoded once per property however many shapes name it; the problems it raised are repeated per
                // shape (as before), each already short (Note GB4).
                let (shader, problems) = shaders.entry(s).or_insert_with(|| shader_of(&h, b, s));
                out.problems.extend(problems.iter().cloned());
                shader.clone()
            }
        };
        let alpha = typed_ref(
            &h,
            head.alpha,
            b"NiAlphaProperty",
            "alpha",
            i,
            &mut out.problems,
        )
        .and_then(|a| match decode_alpha(&h, b, a) {
            Ok(x) => Some(x),
            Err(e) => {
                out.problems.push(format!("block {a}: {e}"));
                None
            }
        });
        out.shapes.push(ShapeRow {
            block: i as u32,
            head,
            shader,
            alpha,
        });
    }
    Ok(out)
}
