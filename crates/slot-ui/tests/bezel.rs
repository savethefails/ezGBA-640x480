//! The PICO-8 bezels: what they leave for the game, and that they stay in the two palettes.
//!
//! `SCRATCH_PNG=/tmp/bezel cargo test -p slot-ui --test bezel` also writes each one out, as
//! `/tmp/bezel-quiet.png` and `/tmp/bezel-palette.png`, for judging by eye.

use slot_store::Pico8Bezel;
use slot_ui::{bezel_face, OUT_H, OUT_W};

const BEZELS: [(Pico8Bezel, &str); 2] = [
    (Pico8Bezel::Quiet, "quiet"),
    (Pico8Bezel::Palette, "palette"),
];

fn px(face: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * OUT_W + x) * 4) as usize;
    face[at..at + 4].try_into().unwrap()
}

#[test]
fn off_draws_nothing() {
    assert!(bezel_face(Pico8Bezel::Off).is_none());
}

#[test]
fn each_is_the_panel_opaque_with_the_games_square_left_black() {
    let bar = (OUT_W - OUT_H) / 2;
    for (bezel, name) in BEZELS {
        let face = bezel_face(bezel).unwrap();
        assert_eq!(face.len(), (OUT_W * OUT_H * 4) as usize, "{name}");
        assert!(
            face.chunks(4).all(|p| p[3] == 0xff),
            "{name} has a see-through pixel"
        );
        for y in 0..OUT_H {
            for x in bar..OUT_W - bar {
                assert_eq!(
                    px(&face, x, y),
                    [0, 0, 0, 0xff],
                    "{name} draws under the game at {x},{y}"
                );
            }
        }
        // Something is drawn in each bar, or there is no bezel to speak of.
        for xs in [0..bar, OUT_W - bar..OUT_W] {
            let lit = xs
                .flat_map(|x| (0..OUT_H).map(move |y| (x, y)))
                .any(|(x, y)| px(&face, x, y) != [0, 0, 0, 0xff]);
            assert!(lit, "{name} leaves a bar black");
        }
    }
}

#[test]
fn quiet_is_dark_blue_beside_the_game_and_black_at_the_panels_edges() {
    let face = bezel_face(Pico8Bezel::Quiet).unwrap();
    let bar = (OUT_W - OUT_H) / 2;
    let blue = |xs: std::ops::Range<u32>| {
        let n = xs.len() as u32 * OUT_H;
        let lit = xs
            .flat_map(|x| (0..OUT_H).map(move |y| (x, y)))
            .filter(|&(x, y)| px(&face, x, y) != [0, 0, 0, 0xff])
            .count();
        lit as f32 / n as f32
    };
    for (edge, inner) in [
        (0..16, bar - 24..bar - 8),
        (OUT_W - 16..OUT_W, OUT_W - bar + 8..OUT_W - bar + 24),
    ] {
        assert!(
            blue(edge.clone()) < 0.1,
            "the panel's edge is {:.0}% lit",
            blue(edge) * 100.0
        );
        assert!(
            blue(inner.clone()) > 0.6,
            "beside the game is {:.0}% lit",
            blue(inner) * 100.0
        );
    }
}

#[test]
fn write_for_judging() {
    let Ok(out) = std::env::var("SCRATCH_PNG") else {
        return;
    };
    for (bezel, name) in BEZELS {
        let face = bezel_face(bezel).unwrap();
        let f = std::fs::File::create(format!("{out}-{name}.png")).unwrap();
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), OUT_W, OUT_H);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&face).unwrap();
    }
}
