use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Float RGBA image, row-major. Values are in the stage's own domain (stored, linear light, vector, …).
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba32F {
    pub width: u32,
    pub height: u32,
    pub px: Vec<[f32; 4]>,
}

impl Rgba32F {
    pub fn new(width: u32, height: u32, fill: [f32; 4]) -> Self {
        Rgba32F {
            width,
            height,
            px: vec![fill; (width as usize) * (height as usize)],
        }
    }
    pub fn at(&self, x: u32, y: u32) -> [f32; 4] {
        self.px[(y * self.width + x) as usize]
    }
    pub fn at_mut(&mut self, x: u32, y: u32) -> &mut [f32; 4] {
        &mut self.px[(y * self.width + x) as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceInfo {
    pub bit_depth: u8,
    pub channels: u8,
    pub has_alpha: bool,
    pub is_float: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexing_is_row_major() {
        let mut img = Rgba32F::new(3, 2, [0.0; 4]);
        *img.at_mut(2, 1) = [1.0, 0.5, 0.25, 1.0];
        assert_eq!(img.px[5], [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(img.at(2, 1)[1], 0.5);
    }
}
