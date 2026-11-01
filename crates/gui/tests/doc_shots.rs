//! Documentation screenshots (docs/DOCUMENTATION.md §5). The unit tests run in CI; `render_doc_shots` is the GPU render.

#[allow(dead_code)]
#[path = "doc_shots/compare.rs"]
mod compare;
#[allow(dead_code)]
#[path = "doc_shots/manifest.rs"]
mod manifest;

use compare::{compare, overlay};
use image::{Rgba, RgbaImage};

fn grey(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([128, 128, 128, 255]))
}

#[test]
fn identical_images_are_unchanged() {
    let c = compare(&grey(4, 4), &grey(4, 4));
    assert_eq!(c.changed_px, 0);
    assert!(!c.changed());
}

#[test]
fn a_step_of_8_is_noise_and_9_is_a_change() {
    let old = grey(4, 4);
    let mut new = grey(4, 4);
    new.put_pixel(1, 1, Rgba([128, 136, 128, 255]));
    assert_eq!(compare(&old, &new).changed_px, 0);
    new.put_pixel(1, 1, Rgba([128, 137, 128, 255]));
    assert_eq!(compare(&old, &new).changed_px, 1);
}

#[test]
fn a_thousandth_is_noise_and_more_counts() {
    let old = grey(1000, 1);
    let mut new = grey(1000, 1);
    new.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
    assert!(!compare(&old, &new).changed());
    new.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
    assert!(compare(&old, &new).changed());
}

#[test]
fn overlay_dims_old_and_marks_changes_magenta() {
    let old = RgbaImage::from_pixel(2, 1, Rgba([200, 100, 50, 255]));
    let mut new = old.clone();
    new.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
    let o = overlay(&old, &new);
    assert_eq!(o.get_pixel(0, 0), &Rgba([70, 35, 17, 255]));
    assert_eq!(o.get_pixel(1, 0), &Rgba([255, 0, 255, 255]));
}

#[test]
fn a_size_change_is_a_change() {
    let old = grey(4, 4);
    let new = grey(4, 5);
    let c = compare(&old, &new);
    assert!(c.size_changed);
    assert!(c.changed());
    assert_eq!(overlay(&old, &new), new);
}

#[test]
fn the_manifest_parses() {
    let m = manifest::load(&manifest::repo_root().join("docs/img/gui/shots.toml"));
    assert_eq!(m.render.size, [1600, 900]);
    assert_eq!(m.render.theme, "dark");
}
