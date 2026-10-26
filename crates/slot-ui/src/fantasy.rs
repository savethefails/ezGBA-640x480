//! PICO-8 is a fantasy console: there is no real machine for its carts to go into, so the slot
//! does not have to behave like one. The other carts go into an SP's slot the way plastic does.
//! A PICO-8 cart dresses the machine in its own colour, is swallowed in a gulp with a burst of
//! confetti out of the slot, and is spat back out with a spin and a puff of sparkles.

use slot_gfx::Draw;

use crate::slot_chrome::{edge, housing, opening, recess};

/// PICO-8's palette, for the trim either side of the slot and the confetti out of it.
pub const PICO8_PALETTE: [[u8; 3]; 16] = [
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

/// The device's face: the colours the slot is drawn in, and how much of PICO-8's palette trims
/// it. The theme's own for every real cart; candy for a PICO-8 one.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Case {
    pub housing: [f32; 4],
    pub recess: [f32; 4],
    pub opening: [f32; 4],
    pub edge: [f32; 4],
    /// The rainbow trim either side of the slot, 0 for none.
    pub trim: f32,
}

impl Case {
    /// The theme's palette, as the slot has always been drawn.
    pub fn theme() -> Case {
        Case {
            housing: housing(),
            recess: recess(),
            opening: opening(),
            edge: edge(),
            trim: 0.0,
        }
    }

    /// A fantasy console in `colour`'s plastic: deep enough that the case's white type still
    /// reads on it, darker again into the bay, nearly black in the opening, and lit at the edge.
    /// A light colour is taken down further than a dark one, so yellow's face is as readable as
    /// blue's.
    pub fn candy(colour: [u8; 3]) -> Case {
        let c = colour.map(|v| v as f32 / 255.0);
        let toward = |to: f32, t: f32| {
            let [r, g, b] = c.map(|v| v + (to - v) * t);
            [r, g, b, 1.0]
        };
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let face = (1.0 - FACE_LUMA / luma.max(1e-3)).max(0.45);
        Case {
            housing: toward(0.0, face),
            recess: toward(0.0, 0.64),
            opening: toward(0.0, 0.86),
            edge: toward(1.0, 0.4),
            trim: 1.0,
        }
    }

    /// `self` a fraction `t` of the way to `to`.
    pub fn mix(self, to: Case, t: f32) -> Case {
        let t = t.clamp(0.0, 1.0);
        let m = |a: [f32; 4], b: [f32; 4]| [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t);
        Case {
            housing: m(self.housing, to.housing),
            recess: m(self.recess, to.recess),
            opening: m(self.opening, to.opening),
            edge: m(self.edge, to.edge),
            trim: self.trim + (to.trim - self.trim) * t,
        }
    }
}

/// The trim: PICO-8's sixteen colours in a run along the top of the case either side of the bay,
/// never across it, where it would be a bar the cart slides behind.
pub fn draw_trim(case: &Case, left: f32, right: f32, y: f32, alpha: f32, out: &mut Vec<Draw>) {
    let a = case.trim * alpha;
    if a <= 0.0 {
        return;
    }
    for (x0, x1) in [(0.0, left), (right, slot_gfx::OUT_W as f32)] {
        let step = (x1 - x0) / PICO8_PALETTE.len() as f32;
        for (i, c) in PICO8_PALETTE.iter().enumerate() {
            let x = (x0 + step * i as f32).round();
            let w = (x0 + step * (i + 1) as f32).round() - x;
            out.push(Draw::Rect {
                x,
                y,
                w,
                h: TRIM_H,
                colour: [
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                    a,
                ],
            });
        }
    }
}

const TRIM_H: f32 = 4.0;

/// The brightest a candy face may be, as luma: under it the case's white type reads.
const FACE_LUMA: f32 = 0.36;

/// A PICO-8 cart going in or coming out, for `SlotChrome` to play the fantasy over the travel.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Whimsy {
    /// The cart's own plastic: the slot flashes it and the confetti carries it.
    pub colour: [u8; 3],
    pub ejecting: bool,
    /// Seconds into the insert or the eject, which the confetti keeps flying on after the travel
    /// has finished.
    pub t: f32,
    /// How long the travel takes, in seconds, so the confetti bursts on its beat.
    pub travel_s: f32,
}

/// How the cart is squashed, turned and lifted at `seat` of the travel: a width and a height to
/// scale it by about its foot, a turn in radians, and pixels up off the travel's own path.
pub struct Pose {
    pub sx: f32,
    pub sy: f32,
    pub turn: f32,
    pub lift: f32,
}

/// The insert's beats, as `slot_chrome::travel` has them: the cart lands on the lip at `catch_in`
/// and is pushed through from `catch_out`.
pub fn pose(w: &Whimsy, seat: f32, catch_in: f32, catch_out: f32) -> Pose {
    let mut p = Pose {
        sx: 1.0,
        sy: 1.0,
        turn: 0.0,
        lift: 0.0,
    };
    let window = |from: f32, len: f32| ((seat - from) / len).clamp(0.0, 1.0);
    if !w.ejecting {
        // Lands on the lip: squashed, then stretched back past itself, then still.
        let u = window(catch_in, 0.16);
        if u > 0.0 && u < 1.0 {
            let b = (u * std::f32::consts::TAU).sin() * (1.0 - u);
            p.sy = 1.0 - 0.13 * b;
            p.sx = 1.0 + 0.10 * b;
        }
        // Tasted: a wiggle while it sits on the lip, dying away before the gulp.
        let v = window(catch_in + 0.04, catch_out - catch_in - 0.04);
        if v > 0.0 && v < 1.0 {
            p.turn = 0.05 * (v * 2.0 * std::f32::consts::TAU).sin() * (1.0 - v);
        }
        // Gulped: drawn thin and long as it goes, like something sucked down a straw.
        let g = window(catch_out, 1.0 - catch_out);
        if g > 0.0 && g < 1.0 {
            let b = (g * std::f32::consts::PI).sin();
            p.sx = 1.0 - 0.12 * b;
            p.sy = 1.0 + 0.10 * b;
        }
    } else {
        // Spat out: `seat` runs from 1 down to 0, so the way out counts up.
        let u = 1.0 - seat;
        // Up past its place on the shelf and back down onto it, turning as it flies.
        let f = ((u - 0.5) / 0.5).clamp(0.0, 1.0);
        p.lift = 46.0 * (f * std::f32::consts::PI).sin();
        p.turn = -0.14 * (f * std::f32::consts::PI).sin();
        // Squeezed coming through the slot, and squashed landing.
        let s = ((u - 0.15) / 0.35).clamp(0.0, 1.0);
        if s > 0.0 && s < 1.0 {
            let b = (s * std::f32::consts::PI).sin();
            p.sx = 1.0 - 0.10 * b;
            p.sy = 1.0 + 0.10 * b;
        }
        let l = ((u - 0.9) / 0.1).clamp(0.0, 1.0);
        if l > 0.0 && l < 1.0 {
            let b = (l * std::f32::consts::PI).sin();
            p.sy = 1.0 - 0.10 * b;
            p.sx = 1.0 + 0.08 * b;
        }
    }
    p
}

/// How lit the slot is with the cart's colour: a flash, on the clock rather than on the travel,
/// as the cart is gulped or spat. The travel stops at the seat and a flash keyed to it stayed lit.
pub fn glow(w: &Whimsy, burst: f32) -> f32 {
    let u = (w.t - burst + 0.04) / 0.32;
    if (0.0..1.0).contains(&u) {
        (u * std::f32::consts::PI).sin()
    } else {
        0.0
    }
}

/// The confetti out of the slot: pieces of PICO-8's brights and the cart's own colour thrown up
/// from the slot's mouth, falling back and fading. Every piece is a function of time alone, from
/// a seed of its own, so the burst is the same every time and costs no state.
pub fn draw_confetti(w: &Whimsy, mouth: (f32, f32), lip: f32, starts: f32, out: &mut Vec<Draw>) {
    let (count, life, speed) = if w.ejecting {
        (36, 0.5, 560.0)
    } else {
        (80, 0.55, 720.0)
    };
    let (left, width) = mouth;
    for i in 0..count {
        let h = seed(i as u32 + if w.ejecting { 1000 } else { 0 });
        let born = starts + (h % 97) as f32 / 97.0 * 0.12;
        let age = w.t - born;
        if !(0.0..life).contains(&age) {
            continue;
        }
        let r = |shift: u32| ((h >> shift) % 1000) as f32 / 1000.0;
        let x0 = left + width * r(3);
        // Thrown up and outwards from the middle of the slot, the further out the wider.
        let spread = (x0 - (left + width / 2.0)) / (width / 2.0);
        let vx = spread * 220.0 + (r(13) - 0.5) * 140.0;
        let vy = -speed * (0.3 + 0.7 * r(23));
        let x = x0 + vx * age;
        let y = lip + vy * age + 0.5 * GRAVITY * age * age;
        let size = if (h >> 7).is_multiple_of(3) { 8.0 } else { 5.0 };
        let colour = if (h >> 9).is_multiple_of(4) {
            w.colour
        } else {
            BRIGHTS[((h >> 17) % BRIGHTS.len() as u32) as usize]
        };
        let fade = 1.0 - (age / life).powi(2);
        out.push(Draw::Rect {
            x: x.round(),
            y: y.round(),
            w: size,
            h: size,
            colour: [
                colour[0] as f32 / 255.0,
                colour[1] as f32 / 255.0,
                colour[2] as f32 / 255.0,
                fade,
            ],
        });
    }
}

const GRAVITY: f32 = 1700.0;

/// The palette's brights, which are the confetti's colours.
const BRIGHTS: [[u8; 3]; 7] = [
    PICO8_PALETTE[7],
    PICO8_PALETTE[8],
    PICO8_PALETTE[9],
    PICO8_PALETTE[10],
    PICO8_PALETTE[11],
    PICO8_PALETTE[12],
    PICO8_PALETTE[14],
];

fn seed(i: u32) -> u32 {
    let mut h = i.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::PICO8_CANDY;

    const CATCH_IN: f32 = 0.42;
    const CATCH_OUT: f32 = 0.62;

    fn whimsy(ejecting: bool) -> Whimsy {
        Whimsy {
            colour: PICO8_CANDY[0],
            ejecting,
            t: 0.0,
            travel_s: 0.45,
        }
    }

    fn still(p: &Pose) -> bool {
        (p.sx - 1.0).abs() < 1e-4
            && (p.sy - 1.0).abs() < 1e-4
            && p.turn.abs() < 1e-4
            && p.lift.abs() < 1e-3
    }

    /// The fantasy is all in between: at either end of either travel the cart is exactly where
    /// and as the shelf and the slot draw it, so nothing jumps as the chrome hands it over.
    #[test]
    fn a_cart_is_posed_as_itself_at_both_ends_of_both_travels() {
        for ejecting in [false, true] {
            let w = whimsy(ejecting);
            for seat in [0.0, 1.0] {
                assert!(
                    still(&pose(&w, seat, CATCH_IN, CATCH_OUT)),
                    "ejecting {ejecting} at seat {seat}"
                );
            }
        }
    }

    /// And it does something in between: lands squashed on the insert, and flies up on the eject.
    #[test]
    fn the_insert_squashes_on_the_lip_and_the_eject_flies() {
        let landed = pose(&whimsy(false), CATCH_IN + 0.04, CATCH_IN, CATCH_OUT);
        assert!(
            landed.sy < 0.95 && landed.sx > 1.04,
            "{} x {}",
            landed.sx,
            landed.sy
        );
        let flying = pose(&whimsy(true), 0.25, CATCH_IN, CATCH_OUT);
        assert!(flying.lift > 30.0 && flying.turn.abs() > 0.05);
    }

    /// The case's type is white, so every candy face has to stay dark enough to carry it.
    #[test]
    fn every_candy_face_is_dark_enough_for_the_cases_white_type() {
        for colour in PICO8_CANDY {
            let [r, g, b, _] = Case::candy(colour).housing;
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            assert!(luma < 0.42, "{colour:02x?} makes a face of luma {luma}");
        }
    }

    /// The flash comes and goes on the clock: lit just after the burst, dark well before it and
    /// well after, however long the cart then sits seated.
    #[test]
    fn the_slot_flashes_once_and_goes_dark_again() {
        let at = |t: f32| glow(&Whimsy { t, ..whimsy(false) }, 0.28);
        assert_eq!(at(0.1), 0.0);
        assert!(at(0.4) > 0.5);
        assert_eq!(at(0.7), 0.0);
    }
}
