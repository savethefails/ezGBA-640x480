#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use slot::thumb;
use slot_gfx::{
    game_rect, set_picture, Compositor, Draw, HeadlessSurface, Picture, OUT_H, OUT_W, SRC_H, SRC_W,
};
use slot_retro::{ButtonMask, MockCore, RetroCore, GBA_H, GBA_W};
use slot_store::{Core, Platform, StateRing};
use slot_ui::{photo_face, Polaroids, Printed};

/// `gl::load_with` writes global function pointers, so two GL tests must not overlap.
static GL: Mutex<()> = Mutex::new(());

fn compositor() -> Option<(MutexGuard<'static, ()>, HeadlessSurface, Compositor)> {
    let guard = GL.lock().unwrap_or_else(PoisonError::into_inner);
    // The 3:2 picture, whose bars these tests check stay black.
    set_picture(Picture::ThreeTwo);
    let surface = HeadlessSurface::new().ok()?;
    let compositor = Compositor::new(&surface).ok()?;
    Some((guard, surface, compositor))
}

fn px(frame: &[u8], x: usize, y: usize) -> [u8; 3] {
    let o = (y * OUT_W as usize + x) * 4;
    [frame[o], frame[o + 1], frame[o + 2]]
}

/// The panel pixel at the middle of a source pixel's cell in the 640x427 picture. The sharp
/// bilinear filter only blends at a cell's edges, so here the source comes through untouched.
fn centre(sx: usize, sy: usize) -> (usize, usize) {
    let x = game_rect().0 as f32 + (sx as f32 + 0.5) * game_rect().2 as f32 / SRC_W as f32;
    let y = game_rect().1 as f32 + (sy as f32 + 0.5) * game_rect().3 as f32 / SRC_H as f32;
    (x as usize, y as usize)
}

/// Worst channel error between each listed source pixel and the panel at the middle of its
/// cell. The picture is the core's own colours: no grille, and no dimming in its place.
fn worst_at_centres(frame: &[u8], src: &[u8], at: &[(usize, usize)]) -> i32 {
    let mut worst = 0;
    for &(sx, sy) in at {
        let o = (sy * SRC_W as usize + sx) * 4;
        let want_rgb = [src[o + 2], src[o + 1], src[o]];
        let (x, y) = centre(sx, sy);
        let got = px(frame, x, y);
        for ch in 0..3 {
            let want = want_rgb[ch] as i32;
            worst = worst.max((want - got[ch] as i32).abs());
        }
    }
    worst
}

fn save_png(frame: &[u8], name: &str) {
    let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") else {
        return;
    };
    let file = std::fs::File::create(Path::new(&dir).join(name)).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .and_then(|mut w| w.write_image_data(frame))
        .expect("write png");
}

/// The switcher's photo item alone, with the plates left off so the picture can be read at
/// every corner. Only the compositor can mint a `TexId`, so this is the first place the
/// wiring is visible at all: a flat sprite here is the switcher looking like a different
/// machine to the game behind it.
fn switcher_photo(p: &Polaroids) -> Vec<Draw> {
    let mut out = Vec::new();
    p.draw(None, Printed::default(), None, Printed::default(), &mut out);
    let photo = *out.first().expect("the switcher drew nothing");
    assert!(
        matches!(photo, Draw::Shot { .. }),
        "the screenshot is not drawn on the panel"
    );
    vec![photo]
}

fn mock_frame(frames: u32) -> Vec<u8> {
    let mut core = MockCore::new();
    core.load(Path::new("mock")).expect("mock load");
    for _ in 0..frames {
        core.run_frame(ButtonMask::default());
    }
    core.video_xrgb8888().to_vec()
}

/// The core hands over XRGB8888, which is B, G, R in memory. A channel swap anywhere in that
/// path is invisible against the grey the mask test uses, so check it against a frame whose
/// three channels genuinely differ.
#[test]
fn a_mock_frame_keeps_its_colours_through_the_game_pass() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let src = mock_frame(7);

    c.begin_frame();
    c.upload_game(&src, (GBA_W, GBA_H));
    c.draw_game();
    let frame = c.read_frame();

    save_png(&frame, "game-mock.png");
    let worst = worst_at_centres(
        &frame,
        &src,
        &[(0, 0), (5, 1), (113, 37), (120, 80), (239, 159)],
    );
    assert!(
        worst <= 1,
        "max channel deviation {worst} from the mock frame"
    );
}

/// The whole picture path in one pass: a core frame is PNG encoded on the save, decoded
/// back into a screenshot, uploaded and drawn. A swapped channel or a stray rescale anywhere
/// along it leaves the switcher showing something the game never showed.
#[test]
fn a_saved_frame_arrives_intact_on_its_screenshot() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let src = mock_frame(13);
    let d = tempfile::tempdir().expect("tempdir");
    let ring = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Mock");
    let stamp = "2026-08-09_14-32-05";
    ring.push(
        b"state",
        &thumb::png(&src, (GBA_W, GBA_H)).expect("encode"),
        stamp,
    )
    .expect("push");
    let entries = ring.list().expect("list");

    let face = photo_face(&entries[0]);
    let tex = c.create_texture(face.w, face.h, &face.rgba);
    c.begin_frame();
    c.draw_list(&[Draw::Tex {
        x: 0.0,
        y: 0.0,
        w: face.w as f32,
        h: face.h as f32,
        tex,
        alpha: 1.0,
    }]);
    let frame = c.read_frame();

    for (sx, sy) in [(0, 0), (113, 37), (239, 159)] {
        let o = (sy * SRC_W as usize + sx) * 4;
        let want = [src[o + 2], src[o + 1], src[o]];
        let got = px(&frame, sx, sy);
        assert_eq!(got, want, "screenshot pixel {sx},{sy}");
    }
}

/// The switcher's screenshot goes through the same sharp filter, into the same 640x427 area,
/// as the game it was taken of. A plain linear tap would soften every edge across the whole
/// cell; a different rect would make the paused game jump as the switcher opens.
#[test]
fn the_switcher_magnifies_its_screenshot_without_resampling_it() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let src = mock_frame(21);
    let d = tempfile::tempdir().expect("tempdir");
    let ring = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Mock");
    ring.push(
        b"state",
        &thumb::png(&src, (GBA_W, GBA_H)).expect("encode"),
        "2026-08-09_14-32-05",
    )
    .expect("push");
    let entries = ring.list().expect("list");

    let face = photo_face(&entries[0]);
    let tex = c.create_texture_nearest(face.w, face.h, &face.rgba);
    let mut switcher = Polaroids::new(entries);
    switcher.set_faces(vec![tex]);
    c.set_screen_power(1.0);
    c.begin_frame();
    c.draw_list(&switcher_photo(&switcher));
    let frame = c.read_frame();

    let worst = worst_at_centres(
        &frame,
        &src,
        &[
            (0, 0),
            (1, 0),
            (113, 37),
            (SRC_W as usize - 1, SRC_H as usize - 1),
        ],
    );
    assert!(
        worst <= 1,
        "the screenshot differs from the game by {worst}"
    );
    // And nothing of it outside the game area: the bars above and below stay black.
    assert_eq!(px(&frame, 320, game_rect().1 as usize - 1), [0, 0, 0]);
    assert_eq!(
        px(&frame, 320, (game_rect().1 + game_rect().3) as usize),
        [0, 0, 0]
    );
}
