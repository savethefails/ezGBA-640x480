//! The game layer coming up and going out. A panel does not fade in: it strikes as a line
//! and blooms, and it dies back down to a dot. `t` is 0.0 dark and 1.0 fully on throughout,
//! so power off is the same curve walked backwards.

use crate::surface::game_rect;

/// The line the picture opens from and closes to. Two pixels rather than one: an odd height
/// centred in an even frame lands on a half pixel and the line reads as grey.
const LINE_PX: f32 = 2.0;

/// How much of the travel the horizontal collapse gets. Only the tail of it, or the whole
/// thing reads as an iris closing rather than a screen going out.
const DOT_T: f32 = 0.12;

/// How far past normal the strike goes. The picture is a line when it is brightest, so this
/// is a flash on a few hundred pixels rather than on the whole frame.
const OVERSHOOT: f32 = 0.6;

/// Height of the picture as a fraction of the frame. Ease out: it snaps open and settles,
/// which is a panel striking rather than a blind going up.
pub fn screen_scale(t: f32) -> f32 {
    let (_, _, _, game_h) = game_rect();
    let left = 1.0 - t.clamp(0.0, 1.0);
    1.0 - (1.0 - LINE_PX / game_h as f32) * left * left
}

/// Width, and only over the last of the collapse. This is the dot the line closes to.
pub fn screen_width(t: f32) -> f32 {
    let (_, _, game_w, _) = game_rect();
    let left = 1.0 - (t.clamp(0.0, 1.0) / DOT_T).min(1.0);
    1.0 - (1.0 - LINE_PX / game_w as f32) * left
}

/// Gain on the game layer. Brightest as the line appears and settling to exactly 1.0, so a
/// screen that is up is not a screen that is being graded.
pub fn screen_brightness(t: f32) -> f32 {
    let left = 1.0 - t.clamp(0.0, 1.0);
    1.0 + OVERSHOOT * left * left
}

/// The rect the game layer fills, in offscreen pixels: the game area, collapsing about its
/// own centre. The line is at the vertical middle of the picture, not at the slot.
pub fn screen_rect(t: f32) -> (f32, f32, f32, f32) {
    let (x, y, game_w, game_h) = game_rect();
    let w = game_w as f32 * screen_width(t);
    let h = game_h as f32 * screen_scale(t);
    (
        x as f32 + (game_w as f32 - w) / 2.0,
        y as f32 + (game_h as f32 - h) / 2.0,
        w,
        h,
    )
}
