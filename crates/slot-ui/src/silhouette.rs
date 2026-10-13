use std::sync::OnceLock;

use crate::cart::{Mould, CART_H, CART_W, GB_CART_H, GB_CART_W, SNES_CART_H, SNES_CART_W};

const CART_SVG: &str = include_str!("../assets/cart.svg");
const DETAIL_SVG: &str = include_str!("../assets/cart_detail.svg");
const GB_CART_SVG: &str = include_str!("../assets/gb_cart.svg");
const GBC_CART_SVG: &str = include_str!("../assets/gbc_cart.svg");
const GB_DETAIL_SVG: &str = include_str!("../assets/gb_cart_detail.svg");
const GBC_DETAIL_SVG: &str = include_str!("../assets/gbc_cart_detail.svg");
const SNES_CART_SVG: &str = include_str!("../assets/snes_cart.svg");
const SNES_DETAIL_SVG: &str = include_str!("../assets/snes_cart_detail.svg");

/// Moulded lettering, one greyscale mask a cart, at the size of the cart's own canvas: white is
/// the raised letters. Slot's, which it rasterised from Gill Sans and Futura; cut to ezGBA's
/// canvases rather than scaled to them, since the two outlines agree everywhere above the label
/// and the lettering sits in the shoulder there.
const GBA_LETTERING: &[u8] = include_bytes!("../assets/lettering_gba.png");
const GB_LETTERING: &[u8] = include_bytes!("../assets/lettering_gb.png");
const GBC_LETTERING: &[u8] = include_bytes!("../assets/lettering_gbc.png");

/// Which of the two Game Pak shell moulds a cart came out of. Nintendo's typology names three
/// classes and slot draws three plastics, but there are only two shells: a grey 0x00 pak and a
/// black 0x80 pak share one mould, and a clear 0xc0 pak has its own. The arms are named for
/// what separates them rather than for the flags that pick them, because the notch is the
/// difference that does something — it is what a Game Boy's power switch needs somewhere to go.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum GbShell {
    /// Classes A and B: the power-switch notch cut out of the top right corner.
    Notched,
    /// Class C: no notch, and the top corners rounded rather than stepped.
    Rounded,
}

/// The SVG outline and moulding of each shape, at its own canvas size.
fn art(m: Mould) -> (&'static str, &'static str, u32, u32) {
    match m {
        Mould::Gba => (CART_SVG, DETAIL_SVG, CART_W, CART_H),
        Mould::Snes => (SNES_CART_SVG, SNES_DETAIL_SVG, SNES_CART_W, SNES_CART_H),
        Mould::Gb(GbShell::Notched) => (GB_CART_SVG, GB_DETAIL_SVG, GB_CART_W, GB_CART_H),
        Mould::Gb(GbShell::Rounded) => (GBC_CART_SVG, GBC_DETAIL_SVG, GB_CART_W, GB_CART_H),
    }
}

/// Coverage of a cart's outline, one byte per pixel, row major. Every cart of a shape is the
/// same shape, so each is rasterised once and multiplied into faces.
///
/// The Game Boy pak has an outline of its own rather than the GBA one at a taller size: its
/// sides are parallel where the GBA cart's taper into a grip ridge.
pub fn silhouette(m: Mould) -> &'static [u8] {
    static MASKS: [OnceLock<Vec<u8>>; Mould::COUNT] = [const { OnceLock::new() }; Mould::COUNT];
    MASKS[m.index()].get_or_init(|| {
        let (svg, _, w, h) = art(m);
        rasterise_svg(svg, w, h)
    })
}

/// How far inside the outline each pixel sits, in city block steps, saturating at 255. A
/// translucent shell fades from its edge inward and needs the distance, not the coverage.
pub(crate) fn depth(m: Mould) -> &'static [u8] {
    static DEPTHS: [OnceLock<Vec<u8>>; Mould::COUNT] = [const { OnceLock::new() }; Mould::COUNT];
    DEPTHS[m.index()].get_or_init(|| {
        let (_, _, w, h) = art(m);
        depth_map(silhouette(m), w as usize, h as usize)
    })
}

/// Two pass chamfer. Everything off the edge of the buffer counts as outside, so a pixel on
/// the top row is one step in rather than unreachable.
fn depth_map(mask: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut d: Vec<u8> = mask
        .iter()
        .map(|c| if *c > 127 { 255 } else { 0 })
        .collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if d[i] == 0 {
                continue;
            }
            let up = if y == 0 { 0 } else { d[i - w] };
            let left = if x == 0 { 0 } else { d[i - 1] };
            d[i] = d[i].min(up.saturating_add(1)).min(left.saturating_add(1));
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if d[i] == 0 {
                continue;
            }
            let down = if y + 1 == h { 0 } else { d[i + w] };
            let right = if x + 1 == w { 0 } else { d[i + 1] };
            d[i] = d[i]
                .min(down.saturating_add(1))
                .min(right.saturating_add(1));
        }
    }
    d
}

/// A moulded feature has two sides, and one mask can only ever cut into the shell. Carrying the
/// lit side as well is what separates moulded plastic from a scratch on it: a ridge catches the
/// light along one edge and casts a shadow along the other, and drawing only the shadow leaves
/// every feature looking drawn on rather than moulded in. It matters most on a dark shell,
/// where a darker line has nowhere left to go.
///
/// Both are coverage, one byte a pixel, and they never overlap: each pixel of the asset is
/// split between them by its luminance, so their sum is that pixel's own coverage.
pub(crate) struct Detail {
    pub shadow: Vec<u8>,
    pub highlight: Vec<u8>,
}

/// A cart's moulding, shaded into the shell rather than drawn in a fixed colour, so it belongs
/// to whatever colour the cart is: the GBA cart's grip ridge and thumb notch; the Game Boy pak's
/// shoulder ribbing, lettering plate, side grooves and arrow; the SNES pak's own.
///
/// The two Game Pak shells differ in their moulding as well as their corners: a class C
/// shoulder has no lines across it, its header panel being smooth with the ribbing surviving
/// only as short ridges on the outer edges. See the note at the top of `gbc_cart_detail.svg`.
pub(crate) fn detail(m: Mould) -> &'static Detail {
    static DETAILS: [OnceLock<Detail>; Mould::COUNT] = [const { OnceLock::new() }; Mould::COUNT];
    DETAILS[m.index()].get_or_init(|| {
        let (_, svg, w, h) = art(m);
        let mut detail = rasterise_detail(svg, w, h);
        let lettering = match m {
            Mould::Gba => Some(GBA_LETTERING),
            Mould::Gb(GbShell::Notched) => Some(GB_LETTERING),
            Mould::Gb(GbShell::Rounded) => Some(GBC_LETTERING),
            Mould::Snes => None,
        };
        if let Some(png) = lettering {
            emboss(&mut detail, png, w, h);
        }
        // The class C shoulder is smooth, but the plate the lettering sits on ends in one
        // moulded step beneath it.
        if m == Mould::Gb(GbShell::Rounded) {
            let y = GBC_PLATE_STEP;
            for x in GBC_PLATE_X.0..GBC_PLATE_X.1 {
                detail.shadow[(y * w + x) as usize] = 200;
                detail.highlight[((y + 1) * w + x) as usize] = 160;
            }
        }
        detail
    })
}

/// Where the step under the class C lettering runs, in canvas pixels.
const GBC_PLATE_STEP: u32 = 52;
const GBC_PLATE_X: (u32, u32) = (72, 168);

/// Raises `png`'s letters out of the shell: each letter is lit along its upper left and shaded
/// along its lower right, which is how a moulded feature standing proud of the plastic reads
/// with the light where every other feature has it.
fn emboss(detail: &mut Detail, png: &[u8], w: u32, h: u32) {
    let mask = decode_mask(png, w, h);
    let at = |x: i32, y: i32| -> u8 {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            0
        } else {
            mask[(y as u32 * w + x as u32) as usize]
        }
    };
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let (here, before) = (at(x, y), at(x - 1, y - 1));
            let i = (y as u32 * w + x as u32) as usize;
            detail.highlight[i] = detail.highlight[i].saturating_add(here.saturating_sub(before));
            detail.shadow[i] = detail.shadow[i].saturating_add(before.saturating_sub(here));
        }
    }
}

/// A built-in 8-bit greyscale mask the size of its cart's canvas.
fn decode_mask(png: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .expect("a built-in lettering mask");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .expect("a built-in lettering mask");
    assert_eq!(
        (info.color_type, info.bit_depth, info.width, info.height),
        (png::ColorType::Grayscale, png::BitDepth::Eight, w, h),
        "a lettering mask of the wrong kind"
    );
    buf.truncate((w * h) as usize);
    buf
}

/// Splits one drawn asset into its shadow and its light by luminance: black is shadow, white is
/// light, and the two come back as separate coverage masks. Authoring them as one file rather
/// than two keeps a feature's lit edge and its dark edge from ever drifting apart, since they
/// are the same shape drawn twice in the same document.
fn rasterise_detail(svg: &str, w: u32, h: u32) -> Detail {
    let px = render(svg, w, h);
    let mut shadow = Vec::with_capacity((w * h) as usize);
    let mut highlight = Vec::with_capacity((w * h) as usize);
    for p in px.data().chunks_exact(4) {
        // The pixmap is premultiplied, so each channel is already scaled by coverage and the
        // split needs no division: a white pixel's luminance *is* its alpha. The weights are
        // Rec. 709 over 256, and `min` only guards against rounding pushing light past cover.
        let lit = ((p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) / 256) as u8;
        let lit = lit.min(p[3]);
        highlight.push(lit);
        shadow.push(p[3] - lit);
    }
    Detail { shadow, highlight }
}

fn rasterise_svg(svg: &str, w: u32, h: u32) -> Vec<u8> {
    render(svg, w, h)
        .data()
        .iter()
        .skip(3)
        .step_by(4)
        .copied()
        .collect()
}

/// A built-in SVG at a canvas size of ours, so nothing here can fail but a bug.
fn render(svg: &str, w: u32, h: u32) -> resvg::tiny_skia::Pixmap {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("a built-in SVG");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).expect("a canvas size");
    let size = tree.size();
    let scale =
        resvg::tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, scale, &mut pixmap.as_mut());
    pixmap
}
