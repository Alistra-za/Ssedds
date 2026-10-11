//! Shared fixtures for the pgpsim integration tests.
#![allow(dead_code)]
use nif::material::MaterialState;

pub const D: &str = "textures\\landscape\\rock01_d.dds";
pub const N: &str = "textures\\landscape\\rock01_n.dds";
pub const NIF: &str = "meshes\\landscape\\rocks\\rock01.nif";

/// A shape whose slots are `slots` (raw NIF strings, slot 0 first); everything else is `builtin_default`.
pub fn shape(slots: &[&str]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    for (i, s) in slots.iter().enumerate() {
        m.set_slot_raw(i, s);
    }
    m
}

/// The usual fixture: a vanilla rock with diffuse and normal.
pub fn rock() -> MaterialState {
    shape(&[D, N])
}
