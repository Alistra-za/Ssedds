//! Reflection-driven cbuffer writer: fields are set by name, so a CS build that moves or drops a variable
//! never needs a hand-written struct.
use crate::compile::CbLayout;

pub struct CbWriter {
    vars: Vec<(String, u32, u32)>,
    data: Vec<u8>,
}

impl CbWriter {
    pub fn new(layout: &CbLayout) -> CbWriter {
        // D3D11 constant buffers are sized in 16-byte multiples.
        let size = (layout.size as usize).div_ceil(16) * 16;
        CbWriter {
            vars: layout.vars.clone(),
            data: vec![0; size],
        }
    }

    /// Writes `data` at `var`'s offset, truncated to the variable's size. False if the variable is not in this
    /// build (the caller logs it).
    pub fn set(&mut self, var: &str, data: &[f32]) -> bool {
        let Some(&(_, off, size)) = self.vars.iter().find(|v| v.0 == var) else {
            return false;
        };
        let (off, size) = (off as usize, size as usize);
        for (i, f) in data.iter().take(size / 4).enumerate() {
            let at = off + 4 * i;
            if let Some(dst) = self.data.get_mut(at..at + 4) {
                dst.copy_from_slice(&f.to_le_bytes());
            }
        }
        true
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
}
