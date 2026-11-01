//! Pixel compare and difference overlay (docs/DOCUMENTATION.md §5, step 2 and 3).

use image::{Rgba, RgbaImage};

/// A pixel is changed when any channel differs by more than this.
pub const CHANNEL_TOL: u8 = 8;

pub struct Compare {
    pub changed_px: u64,
    pub total_px: u64,
    pub size_changed: bool,
}

impl Compare {
    /// A shot is changed when its size changed or more than 0.1 % of its pixels did.
    pub fn changed(&self) -> bool {
        self.size_changed || self.changed_px * 1000 > self.total_px
    }

    pub fn percent(&self) -> f64 {
        if self.size_changed {
            return 100.0;
        }
        if self.total_px == 0 {
            return 0.0;
        }
        self.changed_px as f64 * 100.0 / self.total_px as f64
    }
}

fn differs(a: &Rgba<u8>, b: &Rgba<u8>) -> bool {
    a.0.iter()
        .zip(b.0.iter())
        .any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOL)
}

pub fn compare(old: &RgbaImage, new: &RgbaImage) -> Compare {
    let total_px = u64::from(new.width()) * u64::from(new.height());
    if old.dimensions() != new.dimensions() {
        return Compare {
            changed_px: total_px,
            total_px,
            size_changed: true,
        };
    }
    let changed_px = old
        .pixels()
        .zip(new.pixels())
        .filter(|(a, b)| differs(a, b))
        .count() as u64;
    Compare {
        changed_px,
        total_px,
        size_changed: false,
    }
}

/// The old image dimmed to 35 % with every changed pixel magenta; the new image itself when the sizes differ.
pub fn overlay(old: &RgbaImage, new: &RgbaImage) -> RgbaImage {
    if old.dimensions() != new.dimensions() {
        return new.clone();
    }
    let mut out = RgbaImage::new(old.width(), old.height());
    for (o, (a, b)) in out.pixels_mut().zip(old.pixels().zip(new.pixels())) {
        *o = if differs(a, b) {
            Rgba([255, 0, 255, 255])
        } else {
            let d = |c: u8| (f32::from(c) * 0.35) as u8;
            Rgba([d(a[0]), d(a[1]), d(a[2]), 255])
        };
    }
    out
}
