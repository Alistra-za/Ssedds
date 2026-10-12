//! Reflection-driven cbuffer writer: fields are set by name, so a CS build that moves or drops a variable
//! never needs a hand-written struct.
use crate::compile::CbLayout;

/// A resolved cbuffer variable: byte offset and size in one writer's layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub off: u32,
    pub size: u32,
}

pub struct CbWriter {
    vars: Vec<(String, u32, u32)>,
    data: Vec<u8>,
}

/// `full` is `var`, or `var` behind namespace qualifiers: fxc reports namespaced cbuffer members with their
/// namespace (`SharedData::InInterior`, `FrameBuffer::CameraPosAdjust`). Struct members stay dotted
/// (`SharedData::linearLightingSettings.diffuseGamma` matches `linearLightingSettings.diffuseGamma`).
fn name_matches(full: &str, var: &str) -> bool {
    full == var || full.strip_suffix(var).is_some_and(|ns| ns.ends_with("::"))
}

impl CbWriter {
    /// Zero-initialised: every field the caller does not set reads 0 (no stale values, answers.md §Surprises 3).
    /// At least one register: D3D11 rejects a zero-byte constant buffer.
    pub fn new(layout: &CbLayout) -> CbWriter {
        // D3D11 constant buffers are sized in 16-byte multiples.
        let size = (layout.size as usize).div_ceil(16).max(1) * 16;
        CbWriter {
            vars: layout.vars.clone(),
            data: vec![0; size],
        }
    }

    fn find(&self, var: &str) -> Option<(usize, usize)> {
        self.vars
            .iter()
            .find(|v| name_matches(&v.0, var))
            .map(|v| (v.1 as usize, v.2 as usize))
    }

    /// Whether this build's layout has `var` (namespace-tolerant, see [`name_matches`]).
    pub fn has(&self, var: &str) -> bool {
        self.find(var).is_some()
    }

    fn put(&mut self, var: &str, words: impl Iterator<Item = [u8; 4]>) -> bool {
        let Some(f) = self.resolve(var) else {
            return false;
        };
        self.put_at(f, words);
        true
    }

    fn put_at(&mut self, f: Field, words: impl Iterator<Item = [u8; 4]>) {
        let (off, size) = (f.off as usize, f.size as usize);
        for (i, w) in words.take(size / 4).enumerate() {
            let at = off + 4 * i;
            if let Some(dst) = self.data.get_mut(at..at + 4) {
                dst.copy_from_slice(&w);
            }
        }
    }

    /// Where `var` lives in this layout (namespace-tolerant, see [`name_matches`]); resolve once, then
    /// [`CbWriter::set_at`] every frame without a name search.
    pub fn resolve(&self, var: &str) -> Option<Field> {
        self.find(var).map(|(off, size)| Field {
            off: off as u32,
            size: size as u32,
        })
    }

    /// As [`CbWriter::set`] at a resolved field.
    pub fn set_at(&mut self, f: Field, data: &[f32]) {
        self.put_at(f, data.iter().map(|x| x.to_le_bytes()));
    }

    /// As [`CbWriter::set_u32`] at a resolved field.
    pub fn set_u32_at(&mut self, f: Field, data: &[u32]) {
        self.put_at(f, data.iter().map(|x| x.to_le_bytes()));
    }

    /// Writes `data` at `var`'s offset, truncated to the variable's size. False if the variable is not in this
    /// build (the caller logs it). Floats are written contiguously, so a `float4 X[N]` array takes 4·N floats
    /// and a `float3x4` takes its three 16-byte rows in order.
    pub fn set(&mut self, var: &str, data: &[f32]) -> bool {
        self.put(var, data.iter().map(|f| f.to_le_bytes()))
    }

    /// As [`CbWriter::set`] for `uint`/`bool` fields (HLSL `bool` in a cbuffer is a 32-bit 0/1).
    pub fn set_u32(&mut self, var: &str, data: &[u32]) -> bool {
        self.put(var, data.iter().map(|u| u.to_le_bytes()))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Every byte back to 0, as [`CbWriter::new`] leaves it. A material rewrite starts here, so no field of the
    /// previous material survives (answers.md §Surprises 3; 03 T-E2).
    pub fn zero(&mut self) {
        self.data.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_zero_layout_still_gets_one_register() {
        let layout = CbLayout {
            name: "Empty".into(),
            slot: 0,
            size: 0,
            vars: vec![],
        };
        assert_eq!(CbWriter::new(&layout).bytes().len(), 16);
    }

    #[test]
    fn namespace_tolerant_names() {
        assert!(name_matches("SharedData::InInterior", "InInterior"));
        assert!(name_matches("InInterior", "InInterior"));
        assert!(!name_matches("NotInInterior", "InInterior"));
        assert!(name_matches(
            "SharedData::linearLightingSettings.diffuseGamma",
            "linearLightingSettings.diffuseGamma"
        ));
    }
}
