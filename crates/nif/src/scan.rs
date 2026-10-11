//! LAYER 0: header → shape heads → follow shader/alpha/texture-set refs. One row per shape (00-decision §2 rule 1).
use crate::header::Header;
use crate::path::TexPath;
use crate::props::{
    decode_alpha, decode_effect, decode_lighting, decode_texture_set, Alpha, Effect, Lighting,
};
use crate::shape_head::{decode_head, ShapeHead, ShapeKind};
use crate::text::lossy;
use crate::NifError;

#[derive(Debug, Clone, PartialEq)]
pub enum Shader {
    None,
    Lighting(Lighting, Vec<TexPath>),
    Effect(Effect),
    Other { kind: String },
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
}

fn valid(h: &Header, r: i32) -> Option<usize> {
    usize::try_from(r).ok().filter(|&i| i < h.sizes.len())
}

pub fn scan(b: &[u8]) -> Result<NifScan, NifError> {
    let h = Header::parse(b)?;
    h.check_file_length(b)?;
    let mut out = NifScan {
        bsver: h.bsver,
        le_mesh: h.bsver == 83,
        shapes: vec![],
        problems: vec![],
    };
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
            (_, Some(s)) => match h.kind(s) {
                b"BSLightingShaderProperty" => match decode_lighting(&h, b, s) {
                    Ok(l) => {
                        let slots = match valid(&h, l.texture_set) {
                            Some(t) if h.kind(t) == b"BSShaderTextureSet" => {
                                match decode_texture_set(&h, b, t) {
                                    Ok(ts) => ts.slots,
                                    Err(e) => {
                                        out.problems.push(format!("block {t}: {e}"));
                                        vec![]
                                    }
                                }
                            }
                            _ => vec![],
                        };
                        Shader::Lighting(l, slots)
                    }
                    Err(e) => {
                        out.problems.push(format!("block {s}: {e}"));
                        Shader::None
                    }
                },
                b"BSEffectShaderProperty" => match decode_effect(&h, b, s) {
                    Ok(e) => Shader::Effect(e),
                    Err(e) => {
                        out.problems.push(format!("block {s}: {e}"));
                        Shader::None
                    }
                },
                other => Shader::Other { kind: lossy(other) },
            },
        };
        let alpha = valid(&h, head.alpha)
            .filter(|&a| h.kind(a) == b"NiAlphaProperty")
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
