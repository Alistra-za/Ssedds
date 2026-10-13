//! Thumbnails as egui textures, created once per shared thumbnail and freed when the plan's revision changes.
use crate::model::items::TextureItem;
use eframe::egui;
use engine::session::Thumbnail;
use std::collections::HashMap;
use std::sync::Arc;

/// The entry holds the `Arc` it was made from, so the pointer it is compared by cannot be reused by a new thumbnail.
#[derive(Default)]
pub struct ThumbCache {
    map: HashMap<String, (Arc<Thumbnail>, egui::TextureHandle)>,
    rev: u64,
}

impl ThumbCache {
    /// Drop every texture when the items changed (a new plan): the handles free their GPU memory.
    pub fn sweep(&mut self, rev: u64) {
        if self.rev != rev {
            // An Arc only this cache holds is gone from the plan (shared ones are still current): drop its texture.
            self.map.retain(|_, (t, _)| Arc::strong_count(t) > 1);
            self.rev = rev;
        }
    }

    pub fn get(&mut self, ctx: &egui::Context, item: &TextureItem) -> Option<egui::TextureHandle> {
        let t = item.thumb.as_ref()?;
        if let Some((old, tex)) = self.map.get(&item.key) {
            if Arc::ptr_eq(old, t) {
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
        self.map.insert(item.key.clone(), (t.clone(), tex.clone()));
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
        t: &Arc<Thumbnail>,
        invert: Option<usize>,
    ) -> egui::TextureHandle {
        let id = format!("raw:{key}:{invert:?}");
        if let Some((old, tex)) = self.map.get(&id) {
            if Arc::ptr_eq(old, t) {
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
        self.map.insert(id, (t.clone(), tex.clone()));
        tex
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::session::Thumbnail;
    use std::sync::Arc;

    #[test]
    fn same_thumb_reuses_the_texture_and_a_new_plan_frees_them() {
        let ctx = egui::Context::default();
        let mut c = ThumbCache::default();
        let t = Arc::new(Thumbnail {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 255],
        });
        let a = c.get_raw(&ctx, "k", &t, None);
        assert_eq!(a.id(), c.get_raw(&ctx, "k", &t, None).id());
        let t2 = Arc::new(Thumbnail {
            rgba: vec![9, 9, 9, 255],
            ..(*t).clone()
        });
        assert_ne!(
            a.id(),
            c.get_raw(&ctx, "k", &t2, None).id(),
            "a new thumbnail uploads again"
        );
        drop(t);
        c.sweep(1);
        assert_eq!(
            c.map.len(),
            1,
            "a thumbnail still shared is kept, not re-uploaded"
        );
        drop(t2);
        c.sweep(1);
        assert_eq!(c.map.len(), 1, "the same revision keeps the textures");
        c.sweep(2);
        assert!(c.map.is_empty(), "nothing holds it any more");
    }
}
