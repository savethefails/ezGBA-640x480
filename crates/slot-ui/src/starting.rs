//! The "Starting" pill the boot picture carries: a small dark capsule, the word in spaced
//! capitals, and three dots. It is what tells the picture the device opens on apart from a
//! screen that has stopped: the same frame the device powered off on, marked as a start.
//!
//! Drawn straight into the frame, with no texture and no animation, because the frame is
//! written to the boot partition and the bootloader shows it as it is. Every edge is
//! anti-aliased from its own distance field, so it needs no supersampling either.

use crate::text::{coverage, label_font, line_width, Layout};
use slot_gfx::{OUT_H, OUT_W};

/// Where the pill goes, which is wherever the picture under it has least to say.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PillAt {
    /// Low and centred over a game: what a game puts at the bottom of the screen is usually the
    /// least of it.
    Game,
    /// In the open space above the carts, clear of the row and the clock.
    Shelf,
}

impl PillAt {
    fn centre_y(self) -> f32 {
        match self {
            PillAt::Game => 440.0,
            PillAt::Shelf => 150.0,
        }
    }
}

const WORD: &str = "STARTING";
const TEXT_PX: f32 = 13.0;
const TRACKING: f32 = 2.0;
const HEIGHT: f32 = 34.0;
const SIDE: f32 = 22.0;
const DOT_GAP: f32 = 11.0;
const DOT_STEP: f32 = 9.0;
const DOT_R: f32 = 2.2;
const FILL: [u8; 3] = [22, 22, 27];
const FILL_ALPHA: f32 = 0.80;
const EDGE_ALPHA: f32 = 0.16;
const INK: [u8; 3] = [236, 236, 242];
const SHADOW_ALPHA: f32 = 0.45;
const SHADOW_DROP: f32 = 3.0;

/// Stamps the pill into a frame laid out as `Compositor::read_frame` gives it: RGBA, top row
/// first, the size of the panel. A frame of any other size is left as it is.
pub fn stamp_starting(rgba: &mut [u8], at: PillAt) {
    let (w, h) = (OUT_W as usize, OUT_H as usize);
    if rgba.len() != w * h * 4 {
        return;
    }
    let font = label_font();
    let text_w = font.map_or(0.0, |f| line_width(f, WORD, TEXT_PX, TRACKING));
    let dots_w = DOT_GAP + 2.0 * DOT_STEP + DOT_R;
    let pill_w = SIDE + text_w + dots_w + SIDE;
    let (cx, cy) = (w as f32 / 2.0, at.centre_y());
    let (hw, hh) = (pill_w / 2.0, HEIGHT / 2.0);
    let left = cx - hw;

    // Every pixel the shadow can reach, and nothing else is visited.
    let x0 = (left - 12.0).floor().max(0.0) as usize;
    let x1 = ((cx + hw + 12.0).ceil() as usize).min(w);
    let y0 = (cy - hh - 12.0).floor().max(0.0) as usize;
    let y1 = ((cy + hh + SHADOW_DROP + 12.0).ceil() as usize).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let i = (y * w + x) * 4;
            // A soft shadow under the capsule, so it lifts off whatever is behind it.
            let ds = capsule(px - cx, py - cy - SHADOW_DROP, hw, hh);
            let shadow = SHADOW_ALPHA * (1.0 - smoothstep(-3.0, 7.0, ds));
            over(&mut rgba[i..i + 4], [0, 0, 0], shadow);
            // The capsule, and a hairline just inside its edge to keep it crisp on dark ground.
            let d = capsule(px - cx, py - cy, hw, hh);
            let inside = (0.5 - d).clamp(0.0, 1.0);
            over(&mut rgba[i..i + 4], FILL, FILL_ALPHA * inside);
            let edge = inside - (0.5 - (d + 1.0)).clamp(0.0, 1.0);
            over(&mut rgba[i..i + 4], [255, 255, 255], EDGE_ALPHA * edge);
            // Three dots after the word.
            for k in 0..3 {
                let dx = left + SIDE + text_w + DOT_GAP + k as f32 * DOT_STEP;
                let dd = ((px - dx).powi(2) + (py - cy).powi(2)).sqrt() - DOT_R;
                over(&mut rgba[i..i + 4], INK, 0.92 * (0.5 - dd).clamp(0.0, 1.0));
            }
        }
    }

    // The word, laid out by the same type the labels use, centred in its own box.
    if font.is_some() {
        let box_w = text_w.ceil() as u32 + 2;
        let box_h = HEIGHT as u32;
        let layout = Layout {
            lines: vec![WORD.to_string()],
            px: TEXT_PX,
            tracking: TRACKING,
        };
        let cov = coverage(box_w, box_h, &layout);
        let bx = (left + SIDE).round() as i32 - 1;
        let by = (cy - hh).round() as i32;
        for (j, a) in cov.into_iter().enumerate() {
            if a == 0 {
                continue;
            }
            let x = bx + (j as u32 % box_w) as i32;
            let y = by + (j as u32 / box_w) as i32;
            if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                continue;
            }
            let i = (y as usize * w + x as usize) * 4;
            over(&mut rgba[i..i + 4], INK, a as f32 / 255.0);
        }
    }
}

/// Distance from a point to a capsule centred on the origin, half `hw` wide and `hh` tall,
/// whose ends are full half circles: negative inside.
fn capsule(x: f32, y: f32, hw: f32, hh: f32) -> f32 {
    let r = hh;
    let qx = x.abs() - (hw - r);
    let qy = y.abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `colour` at `alpha` over an opaque pixel, which stays opaque.
fn over(px: &mut [u8], colour: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let a = alpha.min(1.0);
    for c in 0..3 {
        px[c] = (px[c] as f32 * (1.0 - a) + colour[c] as f32 * a).round() as u8;
    }
}
