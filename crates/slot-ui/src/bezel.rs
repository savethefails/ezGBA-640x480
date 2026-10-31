//! The bars either side of a PICO-8 game. The game is a square, 480 x 480 in the middle of the
//! 640 x 480 panel, which leaves 80 px each side that were black. Drawn once at boot, at the
//! panel's size, in PICO-8's own 16 colours and its alternate palette, in blocks of 4 px: close
//! to the game's own pixel, 3.75 px, so the bars read as part of the same picture. Neither design
//! names the console or carries a light.
//!
//! Reflect is the exception: it is drawn from the game, a frame at a time, by `reflect_face`.

use slot_store::Pico8Bezel;

use crate::draw::{OUT_H, OUT_W};

/// PICO-8's palette, colours 0 to 15.
const PICO8: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x1d, 0x2b, 0x53],
    [0x7e, 0x25, 0x53],
    [0x00, 0x87, 0x51],
    [0xab, 0x52, 0x36],
    [0x5f, 0x57, 0x4f],
    [0xc2, 0xc3, 0xc7],
    [0xff, 0xf1, 0xe8],
    [0xff, 0x00, 0x4d],
    [0xff, 0xa3, 0x00],
    [0xff, 0xec, 0x27],
    [0x00, 0xe4, 0x36],
    [0x29, 0xad, 0xff],
    [0x83, 0x76, 0x9c],
    [0xff, 0x77, 0xa8],
    [0xff, 0xcc, 0xaa],
];

/// Its alternate palette, colours 128 to 143.
const ALTERNATE: [[u8; 3]; 16] = [
    [0x29, 0x18, 0x14],
    [0x11, 0x1d, 0x35],
    [0x42, 0x21, 0x36],
    [0x12, 0x53, 0x59],
    [0x74, 0x2f, 0x29],
    [0x49, 0x33, 0x3b],
    [0xa2, 0x88, 0x79],
    [0xf3, 0xef, 0x7d],
    [0xbe, 0x12, 0x50],
    [0xff, 0x6c, 0x24],
    [0xa8, 0xe7, 0x2e],
    [0x00, 0xb5, 0x43],
    [0x06, 0x5a, 0xb5],
    [0x75, 0x46, 0x65],
    [0xff, 0x6e, 0x59],
    [0xff, 0x9d, 0x81],
];

const BLACK: [u8; 3] = PICO8[0];
const DARK_BLUE: [u8; 3] = PICO8[1];
const NAVY: [u8; 3] = ALTERNATE[1];
/// Between black and the alternate palette's navy, for the middle band.
const MIDNIGHT: [u8; 3] = [0x0b, 0x10, 0x20];

const BLOCK: u32 = 4;
/// Where the game starts: the width of each bar.
const BAR: u32 = (OUT_W - OUT_H) / 2;
const BAR_BLOCKS: u32 = BAR / BLOCK;

/// The bezel as an RGBA picture the size of the panel, with the game's square left black for
/// the game to be drawn over. `None` for `Off`, which draws nothing, and for `Reflect`, which
/// `reflect_face` draws from the game instead.
pub fn bezel_face(bezel: Pico8Bezel) -> Option<Vec<u8>> {
    let mut face = Face(vec![0; (OUT_W * OUT_H * 4) as usize]);
    face.fill(0, 0, OUT_W, OUT_H, BLACK);
    match bezel {
        Pico8Bezel::Off | Pico8Bezel::Reflect => return None,
        Pico8Bezel::Quiet => quiet(&mut face),
        Pico8Bezel::Palette => palette(&mut face),
        Pico8Bezel::Soft => soft(&mut face),
    }
    Some(face.0)
}

/// Dark blue beside the game, dithered out to black at the panel's edges, and a rim of dark blue
/// against the game.
fn quiet(face: &mut Face) {
    for (x0, left) in [(0, true), (OUT_W - BAR, false)] {
        for bx in 0..BAR_BLOCKS {
            // 0 at the panel's edge, 1 beside the game; squared, so the blue stays near the game.
            let across = (bx as f32 + 0.5) / BAR_BLOCKS as f32;
            let t = if left { across } else { 1.0 - across };
            for by in 0..OUT_H / BLOCK {
                let colour = if bayer(bx, by) < t * t { NAVY } else { BLACK };
                face.block(x0 + bx * BLOCK, by * BLOCK, colour);
            }
        }
    }
    for x in [BAR - BLOCK, OUT_W - BAR] {
        face.fill(x, 0, BLOCK, OUT_H, DARK_BLUE);
    }
}

/// `bands`, a dark blue line a block off the game, the two palettes as chips low on either bar,
/// and the logo's sparkle at the top right, where the logo keeps it.
fn palette(face: &mut Face) {
    bands(face);
    for x in [BAR - 2 * BLOCK, OUT_W - BAR + BLOCK] {
        face.fill(x, 0, BLOCK, OUT_H, DARK_BLUE);
    }
    for x in [BAR - BLOCK, OUT_W - BAR] {
        face.fill(x, 0, BLOCK, OUT_H, BLACK);
    }
    // Colour 0 at the foot, a block apart, so up close each is its own chip and at arm's length
    // the two runs are a faint dotted line.
    for (x, colours) in [(38, PICO8), (OUT_W - 42, ALTERNATE)] {
        for (i, colour) in colours.iter().enumerate() {
            face.block(x, 440 - i as u32 * 2 * BLOCK, *colour);
        }
    }
    sparkle(face, OUT_W - 40, 40);
}

/// Palette's bands and nothing on them, running straight up to the game, with the game's own
/// shadow on them: three blocks of midnight dithered thinner away from the game. A line or a
/// lip at the game's edge makes the bezel a frame standing over the picture; the shadow puts it
/// underneath.
fn soft(face: &mut Face) {
    bands(face);
    for (x0, left) in [(0, true), (OUT_W - BAR, false)] {
        for (k, density) in [0.75, 0.5, 0.25].into_iter().enumerate() {
            let bx = if left {
                BAR_BLOCKS - 1 - k as u32
            } else {
                k as u32
            };
            for by in 0..OUT_H / BLOCK {
                if bayer(bx, by) < density {
                    face.block(x0 + bx * BLOCK, by * BLOCK, MIDNIGHT);
                }
            }
        }
    }
}

/// Three flat bands from black at the panel's edges to navy beside the game, each joined to the
/// next by a column of PICO-8's checker fill.
fn bands(face: &mut Face) {
    let bands = [BLACK, MIDNIGHT, NAVY];
    let width = BAR_BLOCKS / bands.len() as u32;
    for (x0, left) in [(0, true), (OUT_W - BAR, false)] {
        for bx in 0..BAR_BLOCKS {
            // Counted from the panel's edge on both sides, so the two bars mirror.
            let out = if left { bx } else { BAR_BLOCKS - 1 - bx };
            let band = ((out / width) as usize).min(bands.len() - 1);
            let joint = band > 0 && out % width == 0;
            for by in 0..OUT_H / BLOCK {
                let colour = if joint && by % 2 == 0 {
                    bands[band - 1]
                } else {
                    bands[band]
                };
                face.block(x0 + bx * BLOCK, by * BLOCK, colour);
            }
        }
    }
}

/// The logo's sparkle: a white heart with the palette's brights around it, and a dark blue
/// shadow a block down and right, as the logo's lettering has.
fn sparkle(face: &mut Face, cx: u32, cy: u32) {
    let p = PICO8;
    let points: [((i32, i32), [u8; 3]); 13] = [
        ((0, -2), p[8]),
        ((-1, -1), p[9]),
        ((0, -1), p[7]),
        ((1, -1), p[15]),
        ((-2, 0), p[10]),
        ((-1, 0), p[7]),
        ((0, 0), p[7]),
        ((1, 0), p[7]),
        ((2, 0), p[14]),
        ((-1, 1), p[11]),
        ((0, 1), p[7]),
        ((1, 1), p[13]),
        ((0, 2), p[12]),
    ];
    let at = |d: i32, c: u32| (c as i32 + d * BLOCK as i32) as u32;
    for &((dx, dy), _) in &points {
        face.block(at(dx + 1, cx), at(dy + 1, cy), DARK_BLUE);
    }
    for &((dx, dy), colour) in &points {
        face.block(at(dx, cx), at(dy, cy), colour);
    }
}

/// Reflect's picture: a texel for each block of the panel, so 160 x 120, drawn over the whole
/// panel with nearest filtering. Small enough to redraw from every frame and upload whole.
pub const REFLECT_W: u32 = OUT_W / BLOCK;
pub const REFLECT_H: u32 = OUT_H / BLOCK;

/// How bright the glass is beside the game, as a share of the picture's own light.
const GLASS: f32 = 0.38;
/// Blocks out from the game for the reflection to fall to about a third.
const FALLOFF: f32 = 5.0;
/// How much wider the blur grows with each game pixel the glass is from the game.
const SPREAD: f32 = 0.6;
/// The bezel slopes away from the screen, so it mirrors more of the picture than its own width:
/// each block out reaches this many blocks into the game.
const SQUEEZE: f32 = 1.6;
/// How much of the picture's colour the glass keeps.
const SATURATION: f32 = 0.8;

/// Reflect's bars from one frame of the game, `xrgb` being `size` pixels of XRGB8888 as the core
/// draws them, into `out` as `REFLECT_W` x `REFLECT_H` RGBA. Each bar mirrors the game's edge
/// beside it, as the glass round a CRT does: blurred wider and dimmed the further out it is, a
/// little short of the picture's colour, and black for the block touching the game, so the
/// picture keeps its edge. The game's square is left black; a frame that is no picture leaves
/// all of it black.
pub fn reflect_face(xrgb: &[u8], (w, h): (u32, u32), out: &mut Vec<u8>) {
    out.clear();
    out.extend([0, 0, 0, 0xff].repeat((REFLECT_W * REFLECT_H) as usize));
    let (fw, fh) = (w as usize, h as usize);
    if fw == 0 || fh == 0 || xrgb.len() < fw * fh * 4 {
        return;
    }
    // Light adds in linear terms, not in the bytes' gamma, or the blur goes muddy.
    let decode: [f32; 256] = std::array::from_fn(|v| (v as f32 / 255.0).powf(2.2));
    // Each pixel's sum with everything above and left of it, so any box's mean is four reads.
    let stride = fw + 1;
    let mut sum = vec![[0f32; 3]; stride * (fh + 1)];
    for y in 0..fh {
        let mut row = [0f32; 3];
        for x in 0..fw {
            let p = &xrgb[(y * fw + x) * 4..];
            // Bytes in memory are B, G, R, X.
            for (c, byte) in [p[2], p[1], p[0]].into_iter().enumerate() {
                row[c] += decode[byte as usize];
                sum[(y + 1) * stride + x + 1][c] = sum[y * stride + x + 1][c] + row[c];
            }
        }
    }
    let mean = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let cx = |v: f32| (v.max(0.0) as usize).min(fw);
        let cy = |v: f32| (v.max(0.0) as usize).min(fh);
        let (x0, x1, y0, y1) = (cx(x0.floor()), cx(x1.ceil()), cy(y0.floor()), cy(y1.ceil()));
        let n = ((x1 - x0) * (y1 - y0)).max(1) as f32;
        std::array::from_fn::<f32, 3, _>(|c| {
            (sum[y1 * stride + x1][c] - sum[y0 * stride + x1][c] - sum[y1 * stride + x0][c]
                + sum[y0 * stride + x0][c])
                / n
        })
    };
    // The game is drawn OUT_H square, whatever its own size.
    let (across, down) = (fw as f32 / OUT_H as f32, fh as f32 / OUT_H as f32);
    for (x0, left) in [(0, true), (REFLECT_W - BAR_BLOCKS, false)] {
        for bx in 0..BAR_BLOCKS {
            let d = if left { BAR_BLOCKS - 1 - bx } else { bx };
            if d == 0 {
                continue;
            }
            let into = d as f32 * (BLOCK as f32 * across) * SQUEEZE;
            let reach = 0.6 + SPREAD * into;
            let cx = if left { into } else { fw as f32 - into };
            let glass = GLASS * (-(d as f32) / FALLOFF).exp();
            for by in 0..REFLECT_H {
                let cy = (by * BLOCK) as f32 * down + BLOCK as f32 * down / 2.0;
                let c = mean(cx - reach, cy - reach, cx + reach, cy + reach);
                let grey = (c[0] + c[1] + c[2]) / 3.0;
                let at = ((by * REFLECT_W + x0 + bx) * 4) as usize;
                for (ch, v) in c.into_iter().enumerate() {
                    let v = (grey + (v - grey) * SATURATION) * glass;
                    out[at + ch] = (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8;
                }
            }
        }
    }
}

/// The 4 x 4 ordered dither's threshold for a block, 0 to 15/16.
fn bayer(x: u32, y: u32) -> f32 {
    const M: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    f32::from(M[(y % 4) as usize][(x % 4) as usize]) / 16.0
}

struct Face(Vec<u8>);

impl Face {
    fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, [r, g, b]: [u8; 3]) {
        for yy in y..(y + h).min(OUT_H) {
            for xx in x..(x + w).min(OUT_W) {
                let at = ((yy * OUT_W + xx) * 4) as usize;
                self.0[at..at + 4].copy_from_slice(&[r, g, b, 0xff]);
            }
        }
    }

    fn block(&mut self, x: u32, y: u32, colour: [u8; 3]) {
        self.fill(x, y, BLOCK, BLOCK, colour);
    }
}
