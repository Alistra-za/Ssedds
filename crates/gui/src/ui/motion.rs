//! Reduce motion (Note CF1): one flag on the egui context, so any painter reads it without the model.
//! Movement takes 0 s and fades are capped at [`FADE_CAP`]; egui's stock motion is left alone.
use eframe::egui;

/// The longest a fade or colour change runs when motion is reduced, in seconds.
pub const FADE_CAP: f32 = 0.12;

fn flag() -> egui::Id {
    egui::Id::new("reduce-motion")
}

pub fn reduce_motion(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(flag())).unwrap_or(false)
}

pub fn set_reduce_motion(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(flag(), on));
}

/// Duration of an offset, scale or glide: `secs`, or 0 when motion is reduced.
pub fn move_dur(ctx: &egui::Context, secs: f32) -> f32 {
    if reduce_motion(ctx) {
        0.0
    } else {
        secs
    }
}

/// Duration of a fade or colour change: `secs`, or at most [`FADE_CAP`] when motion is reduced.
pub fn fade_dur(ctx: &egui::Context, secs: f32) -> f32 {
    if reduce_motion(ctx) {
        secs.min(FADE_CAP)
    } else {
        secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_motion_drops_movement_and_caps_fades() {
        let ctx = egui::Context::default();
        assert_eq!(move_dur(&ctx, 0.3), 0.3);
        assert_eq!(fade_dur(&ctx, 0.25), 0.25);
        set_reduce_motion(&ctx, true);
        assert_eq!(move_dur(&ctx, 0.3), 0.0);
        assert_eq!(fade_dur(&ctx, 0.25), FADE_CAP);
        assert_eq!(fade_dur(&ctx, 0.1), 0.1);
    }
}
