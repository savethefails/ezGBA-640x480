use std::path::Path;

use slot_gfx::{Draw, TexId, OUT_H, OUT_W};

use crate::art;
use crate::slot_chrome::scrim;

/// How much of the picture is taken back out again. 0.62 was tuned for a full-height
/// row sitting directly on top of the picture, needing heavy contrast to read over it;
/// the row now stands in its own clear band below the picture (see SHELF_ROW_LOWER in
/// app.rs) with nothing drawn over the artwork itself, so that much dimming only muted
/// a picture nothing was competing with. Kept low rather than at 0.0 so the theme's own
/// scrim colour still ties the picture into the rest of the card's palette.
const SCRIM: f32 = 0.18;

/// Cover the whole panel, centre cropped. PNG only, as the labels are.
pub fn wallpaper_face(path: &Path) -> Option<Vec<u8>> {
    art::cover(path, OUT_W, OUT_H)
}

/// Warm paper at the top to a faint peach at the foot, with PICO-8's brights scattered over it
/// as confetti, washed most of the way back into the paper so they read as a pattern rather than
/// as things to look at.
pub fn bright_backdrop_face() -> Vec<u8> {
    const TOP: [u8; 3] = [0xfb, 0xf6, 0xea];
    const FOOT: [u8; 3] = [0xf6, 0xe4, 0xd6];
    const CONFETTI: [[u8; 3]; 8] = [
        [0xff, 0x00, 0x4d],
        [0xff, 0xa3, 0x00],
        [0xff, 0xec, 0x27],
        [0x00, 0xe4, 0x36],
        [0x29, 0xad, 0xff],
        [0x83, 0x76, 0x9c],
        [0xff, 0x77, 0xa8],
        [0x8a, 0x4f, 0xff],
    ];
    let mut rgba = vec![0u8; (OUT_W * OUT_H * 4) as usize];
    for y in 0..OUT_H {
        let c = mix(TOP, FOOT, y as f32 / (OUT_H - 1) as f32);
        for x in 0..OUT_W {
            let at = ((y * OUT_W + x) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&[c[0], c[1], c[2], 0xff]);
        }
    }
    // One piece per 48 px cell, nudged about inside it, so they spread evenly without lining up.
    const CELL: u32 = 48;
    for cy in 0..OUT_H / CELL {
        for cx in 0..OUT_W / CELL {
            let h = hash(cx, cy);
            if h.is_multiple_of(3) {
                continue;
            }
            let x = cx * CELL + 8 + (h >> 4) % (CELL - 20);
            let y = cy * CELL + 8 + (h >> 12) % (CELL - 20);
            let colour = CONFETTI[((h >> 20) % CONFETTI.len() as u32) as usize];
            // A square, or a plus like the PICO-8 logo's sparkle, in 3 px blocks.
            let blocks: &[(u32, u32)] = if (h >> 24).is_multiple_of(2) {
                &[(0, 0), (1, 0), (0, 1), (1, 1)]
            } else {
                &[(1, 0), (0, 1), (1, 1), (2, 1), (1, 2)]
            };
            for &(bx, by) in blocks {
                for py in 0..3 {
                    for px in 0..3 {
                        let (xx, yy) = (x + bx * 3 + px, y + by * 3 + py);
                        let at = ((yy * OUT_W + xx) * 4) as usize;
                        let paper = [rgba[at], rgba[at + 1], rgba[at + 2]];
                        let c = mix(paper, colour, 0.28);
                        rgba[at..at + 3].copy_from_slice(&c);
                    }
                }
            }
        }
    }
    rgba
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// Stable across boots, so the confetti falls the same way every time.
fn hash(x: u32, y: u32) -> u32 {
    let mut h = x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b) ^ 0x5bd1_e995;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h
}

/// The picture and the scrim over it, as the first two things on the screen. With no wallpaper,
/// `bright`, the ground `bright_backdrop_face` drew, when the shelf is dressed bright, and
/// otherwise nothing: the clear colour is already the ground.
pub fn draw_backdrop(face: Option<TexId>, bright: Option<TexId>, out: &mut Vec<Draw>) {
    let Some(tex) = face else {
        if let Some(tex) = bright {
            out.push(Draw::Tex {
                x: 0.0,
                y: 0.0,
                w: OUT_W as f32,
                h: OUT_H as f32,
                tex,
                alpha: 1.0,
            });
        }
        return;
    };
    out.push(Draw::Tex {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: OUT_H as f32,
        tex,
        alpha: 1.0,
    });
    let [r, g, b] = scrim();
    out.push(Draw::Rect {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: OUT_H as f32,
        colour: [r, g, b, SCRIM],
    });
}
