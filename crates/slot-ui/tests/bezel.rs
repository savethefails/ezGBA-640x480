//! The PICO-8 bezels: what they leave for the game, and that they stay in the two palettes.
//!
//! `SCRATCH_PNG=/tmp/bezel cargo test -p slot-ui --test bezel` also writes each one out, as
//! `/tmp/bezel-quiet.png` and `/tmp/bezel-palette.png`, for judging by eye. With
//! `REFLECT_FRAME=<a 128 x 128 png>` as well, Reflect is written out around that picture as
//! `/tmp/bezel-reflect.png`, the panel as a PICO-8 game would show it.

use slot_store::Pico8Bezel;
use slot_ui::{bezel_face, reflect_face, OUT_H, OUT_W, REFLECT_H, REFLECT_W};

const BEZELS: [(Pico8Bezel, &str); 3] = [
    (Pico8Bezel::Quiet, "quiet"),
    (Pico8Bezel::Palette, "palette"),
    (Pico8Bezel::Soft, "soft"),
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
fn reflect_has_no_face_of_its_own() {
    assert!(bezel_face(Pico8Bezel::Reflect).is_none());
}

/// A 128 x 128 XRGB8888 frame, `colour` as R, G, B wherever `lit` says.
fn frame(lit: impl Fn(u32, u32) -> Option<[u8; 3]>) -> Vec<u8> {
    let mut f = Vec::new();
    for y in 0..128 {
        for x in 0..128 {
            let [r, g, b] = lit(x, y).unwrap_or([0, 0, 0]);
            f.extend([b, g, r, 0xff]);
        }
    }
    f
}

fn reflected(f: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    reflect_face(f, (128, 128), &mut out);
    out
}

/// A block of Reflect's picture, as R, G, B.
fn texel(face: &[u8], x: u32, y: u32) -> [u8; 3] {
    let at = ((y * REFLECT_W + x) * 4) as usize;
    face[at..at + 3].try_into().unwrap()
}

const BAR_TEXELS: u32 = (REFLECT_W - REFLECT_H) / 2;

#[test]
fn reflect_is_one_opaque_texel_a_block_and_black_for_a_black_frame() {
    let face = reflected(&frame(|_, _| None));
    assert_eq!(face.len(), (REFLECT_W * REFLECT_H * 4) as usize);
    assert!(face.chunks(4).all(|p| p == [0, 0, 0, 0xff]));
    // Nor does a frame too short for its size draw anything, or panic.
    let face = reflected(&[0xff; 64]);
    assert!(face.chunks(4).all(|p| p == [0, 0, 0, 0xff]));
}

/// Red down the game's left half and blue down its right: the left bar glows red and the right
/// blue, each dimmer away from the game, with the game's square and the block against it black.
#[test]
fn each_bar_reflects_its_own_edge_of_the_game_and_fades_outward() {
    let face = reflected(&frame(|x, _| {
        Some(if x < 64 { [255, 0, 77] } else { [41, 173, 255] })
    }));
    let y = REFLECT_H / 2;
    for x in BAR_TEXELS..REFLECT_W - BAR_TEXELS {
        assert_eq!(texel(&face, x, y), [0, 0, 0], "under the game at {x}");
    }
    assert_eq!(texel(&face, BAR_TEXELS - 1, y), [0, 0, 0]);
    assert_eq!(texel(&face, REFLECT_W - BAR_TEXELS, y), [0, 0, 0]);
    let near_left = texel(&face, BAR_TEXELS - 2, y);
    let near_right = texel(&face, REFLECT_W - BAR_TEXELS + 1, y);
    assert!(
        near_left[0] > near_left[2],
        "left bar not red: {near_left:?}"
    );
    assert!(
        near_right[2] > near_right[0],
        "right bar not blue: {near_right:?}"
    );
    // Dim: glass, not more picture.
    assert!(
        near_left[0] < 200,
        "left bar as bright as the game: {near_left:?}"
    );
    let far_left = texel(&face, 0, y);
    let far_right = texel(&face, REFLECT_W - 1, y);
    assert!(
        far_left[0] < near_left[0] / 2,
        "{far_left:?} against {near_left:?}"
    );
    assert!(
        far_right[2] < near_right[2] / 2,
        "{far_right:?} against {near_right:?}"
    );
}

/// The glass beside a row of the game shows that row: light along the top of the picture lights
/// the bars at the top and leaves them dark at the foot.
#[test]
fn the_reflection_follows_the_picture_down_the_bars() {
    let face = reflected(&frame(|_, y| (y < 16).then_some([255, 241, 232])));
    let x = BAR_TEXELS - 3;
    let top = texel(&face, x, 4)[0];
    let foot = texel(&face, x, REFLECT_H - 4)[0];
    assert!(top > 40, "top of the bar is dark: {top}");
    assert_eq!(foot, 0, "foot of the bar is lit: {foot}");
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
    let Ok(picture) = std::env::var("REFLECT_FRAME") else {
        return;
    };
    let mut d = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(picture).unwrap(),
    ));
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = d.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!(
        (info.width, info.height),
        (128, 128),
        "REFLECT_FRAME is not 128 x 128"
    );
    let n = info.color_type.samples();
    let f = frame(|x, y| {
        let at = ((y * 128 + x) as usize) * n;
        Some([buf[at], buf[at + 1], buf[at + 2]])
    });
    let bars = reflected(&f);
    // The panel: each block of the bars at 4 px, and the game nearest-scaled over the middle.
    let bar = (OUT_W - OUT_H) / 2;
    let mut panel = vec![0u8; (OUT_W * OUT_H * 4) as usize];
    for y in 0..OUT_H {
        for x in 0..OUT_W {
            let rgb = if (bar..OUT_W - bar).contains(&x) {
                let at = (((y * 128 / OUT_H) * 128 + (x - bar) * 128 / OUT_H) * 4) as usize;
                [f[at + 2], f[at + 1], f[at]]
            } else {
                texel(&bars, x / 4, y / 4)
            };
            let at = ((y * OUT_W + x) * 4) as usize;
            panel[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 0xff]);
        }
    }
    let f = std::fs::File::create(format!("{out}-reflect.png")).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), OUT_W, OUT_H);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&panel).unwrap();
}
