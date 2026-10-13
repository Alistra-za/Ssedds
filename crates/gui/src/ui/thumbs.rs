//! Thumbnails as egui textures, created once per item and refreshed when the bytes change.
use crate::model::items::TextureItem;
use eframe::egui;
use std::collections::HashMap;

#[derive(Default)]
pub struct ThumbCache {
    map: HashMap<String, (u64, egui::TextureHandle)>,
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

impl ThumbCache {
    pub fn get(&mut self, ctx: &egui::Context, item: &TextureItem) -> Option<egui::TextureHandle> {
        let t = item.thumb.as_ref()?;
        let h = hash(&t.rgba);
        if let Some((old, tex)) = self.map.get(&item.key) {
            if *old == h {
                return Some(tex.clone());
            }
        }
        let img = egui::ColorImage::from_rgba_unmultiplied(
            [t.width as usize, t.height as usize],
            &t.rgba,
        );
        let tex = ctx.load_texture(
            format!("thumb:{}", item.key),
            img,
            egui::TextureOptions::LINEAR,
        );
        self.map.insert(item.key.clone(), (h, tex.clone()));
        Some(tex)
    }
}

impl ThumbCache {
    /// A texture for a thumbnail that has no item (an RNM layer or mask file); `invert` is the AO channel: shown as inverted grey
    /// (what the blend reads, `1 - AO`).
    pub fn get_raw(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        t: &engine::session::Thumbnail,
        invert: Option<usize>,
    ) -> egui::TextureHandle {
        let h = hash(&t.rgba);
        let id = format!("raw:{key}:{invert:?}");
        if let Some((old, tex)) = self.map.get(&id) {
            if *old == h {
                return tex.clone();
            }
        }
        let mut rgba = t.rgba.clone();
        if let Some(ch) = invert {
            for px in rgba.as_chunks_mut::<4>().0 {
                let g = 255 - px[ch];
                px[..3].fill(g);
            }
        }
        let img =
            egui::ColorImage::from_rgba_unmultiplied([t.width as usize, t.height as usize], &rgba);
        let tex = ctx.load_texture(format!("thumb:{id}"), img, egui::TextureOptions::LINEAR);
        self.map.insert(id, (h, tex.clone()));
        tex
    }
}
