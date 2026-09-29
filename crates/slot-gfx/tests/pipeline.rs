use slot_gfx::{
    blue_light_gain, game_rect, set_fit, set_grid, set_picture, set_scaler, Compositor, Draw, Fit,
    Grid, HeadlessSurface, Picture, Scaler, OUT_H, OUT_W, SRC_H, SRC_W,
};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// `gl::load_with` writes global function pointers, so two GL tests must not overlap.
static GL: Mutex<()> = Mutex::new(());

/// A GL context is not available everywhere. Skip rather than fail, the same way the mGBA
/// test skips a missing dylib.
///
/// In the 3:2 picture, the one with bars: most of what these check is where the picture ends,
/// and 4:3 ends at the panel's own edge. `in_four_three` is the other shape.
fn compositor() -> Option<(MutexGuard<'static, ()>, HeadlessSurface, Compositor)> {
    let guard = GL.lock().unwrap_or_else(PoisonError::into_inner);
    set_picture(Picture::ThreeTwo);
    set_fit(Fit::Gba);
    let surface = HeadlessSurface::new().ok()?;
    let compositor = Compositor::new(&surface).ok()?;
    Some((guard, surface, compositor))
}

fn px(frame: &[u8], x: usize, y: usize) -> [u8; 3] {
    let o = (y * OUT_W as usize + x) * 4;
    [frame[o], frame[o + 1], frame[o + 2]]
}

/// A screenshot as the switcher uploads one: RGBA at the source size, not the core's BGRA.
fn flat_shot(rgb: [u8; 3]) -> Vec<u8> {
    std::iter::repeat_n([rgb[0], rgb[1], rgb[2], 255], (SRC_W * SRC_H) as usize)
        .flatten()
        .collect()
}

/// A Game Boy's picture, which a core now hands over at its own size.
const GB_W: usize = 160;
const GB_H: usize = 144;
const GB: (u32, u32) = (GB_W as u32, GB_H as u32);
const GBA: (u32, u32) = (SRC_W, SRC_H);

/// A Game Boy frame: XRGB8888, which is B, G, R, unused in memory.
fn gb_frame(inside: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(GB_W * GB_H * 4);
    for y in 0..GB_H {
        for x in 0..GB_W {
            let rgb = inside(x, y);
            buf.extend_from_slice(&[rgb[2], rgb[1], rgb[0], 0]);
        }
    }
    buf
}

/// A source pixel as the pass draws it with the grille off: the core's own colour, undimmed.
/// Compared with a byte of tolerance, which is the rounding the shader does, not slack.
fn shaded(rgb: [u8; 3]) -> [i32; 3] {
    rgb.map(i32::from)
}

/// The panel pixel at the middle of a source pixel's cell, for a picture of `w` by `h` source
/// pixels stretched over the game area. The sharp filter only blends at a cell's edges, so
/// here the source comes through untouched.
fn centre_of(sx: usize, sy: usize, w: usize, h: usize) -> (usize, usize) {
    let x = game_rect().0 as f32 + (sx as f32 + 0.5) * game_rect().2 as f32 / w as f32;
    let y = game_rect().1 as f32 + (sy as f32 + 0.5) * game_rect().3 as f32 / h as f32;
    (x as usize, y as usize)
}

fn close(got: [u8; 3], want: [i32; 3]) -> bool {
    (0..3).all(|ch| (want[ch] - got[ch] as i32).abs() <= 1)
}

/// The LCD3x grille only lines up at exactly 3x, which a 640 panel is not, so the picture is
/// drawn without it: a flat frame comes out flat over the whole 640x427 game area, at the
/// grille's average brightness, and the bars above and below it stay black.
#[test]
fn a_flat_frame_fills_the_game_area_flat_and_nothing_else() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let grey = vec![0x80u8; (SRC_W * SRC_H * 4) as usize];
    c.begin_frame();
    c.upload_game(&grey, GBA);
    c.draw_game();
    let frame = c.read_frame();

    let want = shaded([0x80; 3]);
    let (top, bottom) = (
        game_rect().1 as usize,
        (game_rect().1 + game_rect().3) as usize,
    );
    for y in 0..OUT_H as usize {
        for x in 0..OUT_W as usize {
            let got = px(&frame, x, y);
            match (top..bottom).contains(&y) {
                true => assert!(close(got, want), "{x},{y} is {got:?}, not {want:?}"),
                false => assert_eq!(got, [0, 0, 0], "{x},{y} is outside the picture"),
            }
        }
    }
}

/// The FBO is stored bottom up and the source is top down, so a missing flip anywhere in
/// upload, draw or readback shows up as a frame that is upside down or mirrored.
#[test]
fn the_game_frame_keeps_its_orientation_from_upload_to_readback() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let mut src = vec![0u8; (SRC_W * SRC_H * 4) as usize];
    src[0..3].copy_from_slice(&[255, 255, 255]);
    c.begin_frame();
    c.upload_game(&src, GBA);
    c.draw_game();
    let frame = c.read_frame();

    let (x0, y0) = (game_rect().0 as usize, game_rect().1 as usize);
    for y in y0..y0 + 2 {
        for x in x0..x0 + 2 {
            assert!(
                px(&frame, x, y)[0] > 100,
                "top left source pixel missing at {x},{y}"
            );
        }
    }
    assert_eq!(px(&frame, x0 + 5, y0), [0, 0, 0], "bled one cell right");
    assert_eq!(px(&frame, x0, y0 + 5), [0, 0, 0], "bled one cell down");
    assert_eq!(
        px(&frame, x0, (game_rect().1 + game_rect().3) as usize - 1),
        [0, 0, 0],
        "frame is upside down"
    );
    assert_eq!(
        px(&frame, (game_rect().0 + game_rect().2) as usize - 1, y0),
        [0, 0, 0],
        "frame is mirrored"
    );
}

/// The power on is a uniform on the game pass, so only a readback says whether the picture
/// is genuinely being squeezed and flashed rather than the curve merely being computed.
#[test]
fn the_picture_strikes_as_a_band_at_the_centre_before_it_fills_the_frame() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let grey = vec![0x80u8; (SRC_W * SRC_H * 4) as usize];
    let peak = |c: &mut Compositor, t: f32| {
        c.set_screen_power(t);
        c.begin_frame();
        c.upload_game(&grey, GBA);
        c.draw_game();
        let frame = c.read_frame();
        let brightest = frame.chunks_exact(4).map(|p| p[0]).max().unwrap_or(0);
        (frame, brightest)
    };

    let (striking, bright) = peak(&mut c, 0.35);
    let lit = |frame: &[u8], y: usize| (0..OUT_W as usize).any(|x| px(frame, x, y) != [0, 0, 0]);
    let (top, bottom) = (
        game_rect().1 as usize,
        (game_rect().1 + game_rect().3) as usize - 1,
    );
    assert!(lit(&striking, (top + bottom) / 2), "nothing at the centre");
    assert!(
        !lit(&striking, top + 1),
        "the picture already reaches the top"
    );
    assert!(
        !lit(&striking, bottom - 1),
        "the picture already reaches the bottom"
    );

    let (settled, normal) = peak(&mut c, 1.0);
    assert!(
        lit(&settled, top) && lit(&settled, bottom),
        "the settled picture is short"
    );
    assert!(
        !lit(&settled, top - 1) && !lit(&settled, bottom + 1),
        "the settled picture runs into the bars"
    );
    assert!(
        bright > normal,
        "the strike at {bright} is no brighter than the settled picture at {normal}"
    );
}

/// The game layer is an item in the draw list rather than a pass before it, which is the
/// only way the picture can come up in front of a cart that is already seated. Both
/// directions matter: what is listed before the marker is covered by the picture, and what
/// is listed after it is drawn over the picture.
#[test]
fn the_game_marker_draws_the_picture_where_it_sits_in_the_list() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let white = vec![0xffu8; (SRC_W * SRC_H * 4) as usize];
    let full = |colour: [f32; 4]| Draw::Rect {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: OUT_H as f32,
        colour,
    };
    c.set_screen_power(1.0);

    c.begin_frame();
    c.upload_game(&white, GBA);
    c.draw_list(&[full([1.0, 0.0, 0.0, 1.0]), Draw::Game]);
    let under = c.read_frame();
    let blue = under.chunks_exact(4).map(|p| p[2]).max().unwrap_or(0);
    assert!(blue > 100, "the picture never drew over the red rect");

    c.begin_frame();
    c.draw_list(&[Draw::Game, full([0.0, 0.0, 1.0, 1.0])]);
    let over = c.read_frame();
    assert_eq!(
        px(&over, OUT_W as usize / 2, OUT_H as usize / 2),
        [0, 0, 255],
        "the picture was drawn over the item listed after it"
    );
}

/// The switcher shows a still of the same panel, so it goes through the same pass into the same
/// game area as the live picture. Blitted flat over the whole panel it reads as a different
/// machine to the game behind it.
#[test]
fn a_saved_shot_is_drawn_through_the_game_pass() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let shot = flat_shot([200, 200, 200]);
    let tex = c.create_texture_nearest(SRC_W, SRC_H, &shot);
    c.set_screen_power(1.0);

    c.begin_frame();
    c.draw_list(&[Draw::Tex {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: OUT_H as f32,
        tex,
        alpha: 1.0,
    }]);
    let plain = c.read_frame();

    c.begin_frame();
    c.draw_list(&[Draw::Shot { tex }]);
    let lit = c.read_frame();

    assert_ne!(plain, lit, "the shot is not going through the game pass");
    let want = shaded([200; 3]);
    for (x, y) in [
        (0, game_rect().1),
        (320, 240),
        (639, game_rect().1 + game_rect().3 - 1),
    ] {
        let got = px(&lit, x as usize, y as usize);
        assert!(close(got, want), "{x},{y}: {got:?} against {want:?}");
    }
    assert_eq!(
        px(&lit, 320, game_rect().1 as usize - 1),
        [0, 0, 0],
        "the shot is over the bar"
    );
}

/// A still is a photograph, taken at some earlier moment: a stretched Game Boy's polaroids
/// stay the shape the game is. The same stored image rendering differently because of a
/// preference set after it was taken would mean a state saved before the player ever pressed L
/// comes back stretched.
///
/// The live game beside it is the control: without one, a build that had simply stopped
/// honouring the fit at all would pass this.
#[test]
fn a_still_is_not_stretched_by_the_picture_mode() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let mut shot: Vec<u8> = (0..GB_W * GB_H)
        .flat_map(|i| [(i % 200) as u8, 120, 120, 255])
        .collect();
    shot[..4].copy_from_slice(&[255, 0, 0, 255]);
    let tex = c.create_texture_nearest(GB.0, GB.1, &shot);
    let src = gb_frame(|x, y| [(x * 3) as u8, (y * 5) as u8, 200]);
    c.set_screen_power(1.0);

    let framed = |c: &mut Compositor, fit: Fit| {
        set_fit(fit);
        c.begin_frame();
        c.upload_game(&src, GB);
        c.draw_list(&[Draw::Shot { tex }]);
        let still = c.read_frame();
        c.begin_frame();
        c.draw_game();
        (still, c.read_frame())
    };

    let (still_actual, game_actual) = framed(&mut c, Fit::Whole);
    let (still_full, game_full) = framed(&mut c, Fit::Fill);
    set_fit(Fit::Gba);

    assert!(
        still_actual == still_full,
        "the polaroid changed shape when the panel did"
    );
    assert!(
        game_actual != game_full,
        "the game did not stretch, so this proves nothing about the still"
    );
}

#[test]
fn draw_list_rects_land_in_top_left_pixel_space() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    c.begin_frame();
    c.draw_list(&[Draw::Rect {
        x: 2.0,
        y: 1.0,
        w: 4.0,
        h: 2.0,
        colour: [1.0, 0.0, 0.0, 1.0],
    }]);
    let frame = c.read_frame();

    assert_eq!(px(&frame, 2, 1), [255, 0, 0]);
    assert_eq!(px(&frame, 5, 2), [255, 0, 0]);
    assert_ne!(px(&frame, 1, 1), [255, 0, 0], "rect starts one pixel early");
    assert_ne!(px(&frame, 6, 2), [255, 0, 0], "rect runs one pixel long");
    assert_ne!(px(&frame, 2, 3), [255, 0, 0], "rect runs one row long");
}

/// The switcher reuses one texture per ring slot instead of minting a fresh set on every
/// opening. A replace that quietly kept the old contents would show the previous ring.
#[test]
fn a_replaced_texture_draws_its_new_contents() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let tex = c.create_texture(1, 1, &[255, 0, 0, 255]);
    c.update_texture(tex, 1, 1, &[0, 0, 255, 255]);
    c.begin_frame();
    c.draw_list(&[Draw::Tex {
        x: 0.0,
        y: 0.0,
        w: 4.0,
        h: 4.0,
        tex,
        alpha: 1.0,
    }]);
    let frame = c.read_frame();
    assert_eq!(px(&frame, 1, 1), [0, 0, 255]);
}

#[test]
fn blue_light_warms_monotonically_and_clamps_at_the_last_step() {
    assert_eq!(blue_light_gain(0), [1.0, 1.0, 1.0]);
    let warmest = blue_light_gain(9);
    assert!(
        (warmest[1] - 0.82).abs() < 0.005 && (warmest[2] - 0.62).abs() < 0.005,
        "warmest step is {warmest:?}"
    );
    assert_eq!(blue_light_gain(200), warmest, "step must clamp, not wrap");
    for step in 0..9 {
        let a = blue_light_gain(step);
        let b = blue_light_gain(step + 1);
        assert_eq!(b[0], 1.0, "red must not move");
        assert!(b[1] < a[1] && b[2] < a[2], "step {step} did not warm");
        assert!(b[2] < b[1], "blue must fall faster than green");
    }
}

/// A turn of nothing is the image as it was. The rotation is extra arithmetic in the vertex
/// shader every sprite goes through, so this is what proves the rest of the chrome did not
/// move by a single value.
#[test]
fn an_unturned_image_draws_exactly_as_a_plain_one() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let rgba: Vec<u8> = (0..16u32 * 8)
        .flat_map(|i| [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255])
        .collect();
    let tex = c.create_texture(16, 8, &rgba);

    c.begin_frame();
    c.draw_list(&[Draw::Tex {
        x: 101.3,
        y: 57.6,
        w: 37.0,
        h: 19.0,
        tex,
        alpha: 0.8,
    }]);
    let plain = c.read_frame();

    c.begin_frame();
    c.draw_list(&[Draw::Turned {
        x: 101.3,
        y: 57.6,
        w: 37.0,
        h: 19.0,
        tex,
        alpha: 0.8,
        turn: 0.0,
    }]);
    let turned = c.read_frame();

    assert!(plain == turned, "a turn of zero moved something");
}

/// Positive is clockwise on the panel, since y runs down. A quarter turn about the centre
/// takes the texture's top left corner to the top right.
#[test]
fn a_quarter_turn_takes_the_top_left_corner_to_the_top_right() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    // Four by two, each texel 10 px once drawn: the top left red, the rest blue.
    let mut rgba = [0u8, 0, 255, 255].repeat(8);
    rgba[0..4].copy_from_slice(&[255, 0, 0, 255]);
    let tex = c.create_texture_nearest(4, 2, &rgba);

    c.begin_frame();
    c.draw_list(&[Draw::Turned {
        x: 100.0,
        y: 100.0,
        w: 40.0,
        h: 20.0,
        tex,
        alpha: 1.0,
        turn: std::f32::consts::FRAC_PI_2,
    }]);
    let frame = c.read_frame();

    // Turned, the quad stands 20 wide and 40 tall about the same centre, (120, 110).
    assert_eq!(
        px(&frame, 127, 93),
        [255, 0, 0],
        "the red texel is not top right"
    );
    assert_eq!(
        px(&frame, 113, 93),
        [0, 0, 255],
        "the top left did not move"
    );
    assert_eq!(px(&frame, 113, 127), [0, 0, 255]);
}

/// The default sub-rect is the whole texture, which is the arithmetic the pass did before
/// there was a sub-rect to ask for: every source pixel in its own cell of the 640x427 picture. Checked against that arithmetic recomputed here rather than against a
/// recorded frame, so it pins the relationship and not one capture of it — and then asked for
/// explicitly as well, because a default that only happens to agree is one that can drift.
///
/// This is the whole of what keeps a GBA picture unmoved by a change made for the Game Boy.
#[test]
fn the_default_source_rect_draws_exactly_as_before() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    // Three channels that differ everywhere, so a swap or a half-pixel slide shows up.
    let src: Vec<u8> = (0..(SRC_W * SRC_H) as usize)
        .flat_map(|i| {
            let (x, y) = (i % SRC_W as usize, i / SRC_W as usize);
            let rgb = [
                (x * 7 + y * 3) as u8,
                (x * 13 + y * 29) as u8,
                (x * 31 + y * 11) as u8,
            ];
            [rgb[2], rgb[1], rgb[0], 0]
        })
        .collect();
    c.set_screen_power(1.0);

    c.begin_frame();
    c.upload_game(&src, GBA);
    c.draw_game();
    let default = c.read_frame();

    for (sx, sy) in [(0, 0), (1, 0), (0, 1), (113, 37), (120, 80), (239, 159)] {
        let o = (sy * SRC_W as usize + sx) * 4;
        let want = shaded([src[o + 2], src[o + 1], src[o]]);
        let (x, y) = centre_of(sx, sy, SRC_W as usize, SRC_H as usize);
        let got = px(&default, x, y);
        assert!(
            close(got, want),
            "source {sx},{sy}: {got:?} against {want:?}"
        );
    }
}

/// A Game Boy picture at actual size is 3x, square pixels, centred: 480x432 with 80 px either
/// side and 24 above and below, the panel around it left dark. Stretched, the same picture's
/// corner pixels land in the panel's own corners.
#[test]
fn a_game_boy_picture_is_whole_at_actual_size_and_fills_the_panel_stretched() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let corner = |x: usize, y: usize| match (x, y) {
        (0, 0) => Some([255, 0, 0]),
        (x, 0) if x == GB_W - 1 => Some([0, 255, 0]),
        (0, y) if y == GB_H - 1 => Some([0, 0, 255]),
        (x, y) if x == GB_W - 1 && y == GB_H - 1 => Some([255, 255, 0]),
        _ => None,
    };
    let src = gb_frame(|x, y| corner(x, y).unwrap_or([90, 90, 90]));
    c.set_screen_power(1.0);

    for (name, fit, (x0, y0, w, h)) in [
        (
            "actual size",
            Fit::Whole,
            (80usize, 24usize, 480usize, 432usize),
        ),
        (
            "stretched",
            Fit::Fill,
            (0, 0, OUT_W as usize, OUT_H as usize),
        ),
    ] {
        set_fit(fit);
        c.begin_frame();
        c.upload_game(&src, GB);
        c.draw_game();
        let frame = c.read_frame();
        assert_eq!(
            game_rect(),
            (x0 as u32, y0 as u32, w as u32, h as u32),
            "{name}"
        );
        let (x1, y1) = (x0 + w - 1, y0 + h - 1);
        for (corner, (x, y), rgb) in [
            ("top left", (x0, y0), [255, 0, 0]),
            ("top right", (x1, y0), [0, 255, 0]),
            ("bottom left", (x0, y1), [0, 0, 255]),
            ("bottom right", (x1, y1), [255, 255, 0]),
        ] {
            let got = px(&frame, x, y);
            assert!(close(got, shaded(rgb)), "{name}, {corner}: {got:?}");
        }
        if x0 > 0 {
            assert_eq!(
                px(&frame, x0 - 1, y0),
                [0, 0, 0],
                "{name}: lit left of the picture"
            );
            assert_eq!(px(&frame, x1 + 1, y1), [0, 0, 0], "{name}: lit right of it");
            assert_eq!(px(&frame, x0, y0 - 1), [0, 0, 0], "{name}: lit above it");
        }
    }
    set_fit(Fit::Gba);
}

/// At 3x each Game Boy pixel is exactly three panel pixels square, so nothing is blended: a
/// checkerboard comes out as pure black and white blocks three pixels on a side.
#[test]
fn a_game_boy_picture_at_actual_size_is_blended_nowhere() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let src = gb_frame(|x, y| match (x + y) % 2 {
        0 => [255, 255, 255],
        _ => [0, 0, 0],
    });
    c.set_screen_power(1.0);
    set_fit(Fit::Whole);
    c.begin_frame();
    c.upload_game(&src, GB);
    c.draw_game();
    let frame = c.read_frame();
    set_fit(Fit::Gba);
    for y in (24..456).step_by(7) {
        for x in 80..560 {
            let want = match ((x - 80) / 3 + (y - 24) / 3) % 2 {
                0 => 255,
                _ => 0,
            };
            let got = px(&frame, x, y)[0] as i32;
            // A few levels of the shader's float rounding, against the 128 or so a real blend
            // between the two would be.
            assert!((got - want).abs() <= 4, "({x}, {y}) is {got}, not {want}");
        }
    }
}

/// A source pixel's edge is one panel pixel of blend and no more, in both modes and on both
/// axes, although fullscreen stretches a Game Boy's picture 4x across and under 3x down. The
/// filter scaled for the wrong axis blends a wider band, which at the edge of a Game Boy
/// picture means the border around it bleeding in.
#[test]
fn a_hard_edge_softens_by_one_panel_pixel_in_both_modes() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    // Columns and rows alternating black and white, so every cell boundary is an edge.
    let src = gb_frame(|x, y| match (x + y) % 2 {
        0 => [255, 255, 255],
        _ => [0, 0, 0],
    });
    c.set_screen_power(1.0);
    for (name, fit, w, h) in [
        ("actual size", Fit::Whole, GB_W, GB_H),
        ("fullscreen", Fit::Fill, GB_W, GB_H),
    ] {
        set_fit(fit);
        c.begin_frame();
        c.upload_game(&src, GB);
        c.draw_game();
        let frame = c.read_frame();
        // Along one row through the middle of the picture: every pixel is either one of the
        // two flat values, or a lone blend between them. Two blends side by side is a band.
        let (_, y) = centre_of(w / 2, h / 2, w, h);
        let (x0, x1) = (
            game_rect().0 as usize + 8,
            (game_rect().0 + game_rect().2) as usize - 8,
        );
        let white = shaded([255; 3])[0];
        let grey = |x: usize| {
            let v = px(&frame, x, y)[0] as i32;
            v > 2 && v < white - 2
        };
        let wide = (x0..x1).find(|&x| grey(x) && grey(x + 1));
        assert_eq!(wide, None, "{name}: a blend two pixels wide at x {wide:?}");
        // And down one column.
        let (x, _) = centre_of(w / 2, h / 2, w, h);
        let (y0, y1) = (
            game_rect().1 as usize + 8,
            (game_rect().1 + game_rect().3) as usize - 8,
        );
        let grey = |y: usize| {
            let v = px(&frame, x, y)[0] as i32;
            v > 2 && v < white - 2
        };
        let tall = (y0..y1).find(|&y| grey(y) && grey(y + 1));
        assert_eq!(tall, None, "{name}: a blend two pixels tall at y {tall:?}");
    }
    set_fit(Fit::Gba);
}

/// Sharp-shimmerless blends by area: a panel pixel that a source pixel's edge crosses takes
/// each side's colour in proportion to how much of it that side covers. At 640 across 240 a
/// source pixel is 8/3 panel pixels wide, so a lone white column at the left edge lights the
/// first two panel pixels fully, two thirds of the third, and none of the fourth. And the total
/// light it leaves across the row is its width whichever texel it is in, which is what stops a
/// scrolling picture shimmering.
#[test]
fn a_panel_pixel_is_blended_by_the_area_each_source_pixel_covers() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let column = |at: usize| -> Vec<u8> {
        (0..(SRC_W * SRC_H) as usize)
            .flat_map(|i| match i % SRC_W as usize == at {
                true => [255, 255, 255, 0],
                false => [0, 0, 0, 0],
            })
            .collect()
    };
    // Sharp-shimmerless's own arithmetic, which mixes stored values: Pixel AA's linear light
    // version of the same has a test of its own below.
    set_scaler(Scaler::SharpShimmerless);
    c.set_screen_power(1.0);
    let row = |c: &mut Compositor, at: usize| {
        c.begin_frame();
        c.upload_game(&column(at), GBA);
        c.draw_game();
        let frame = c.read_frame();
        let y = (game_rect().1 + game_rect().3 / 2) as usize;
        (0..OUT_W as usize)
            .map(|x| px(&frame, x, y)[1] as f32)
            .collect::<Vec<_>>()
    };
    let white = shaded([255; 3])[1] as f32;

    let first = row(&mut c, 0);
    assert!((first[0] - white).abs() <= 1.0 && (first[1] - white).abs() <= 1.0);
    assert!(
        (first[2] - white * 2.0 / 3.0).abs() <= 2.0,
        "the pixel the edge crosses is {} rather than two thirds of {white}",
        first[2]
    );
    assert_eq!(first[3], 0.0, "the column bled past its edge");

    for at in [1, 2, 3, 100, 239] {
        let lit: f32 = row(&mut c, at).iter().sum::<f32>() / white;
        assert!(
            (lit - 8.0 / 3.0).abs() < 0.05,
            "column {at} leaves {lit} pixels of light, not 8/3"
        );
    }
    set_scaler(Scaler::default());
}

/// 4:3, the default: the picture is the whole panel, and 480 rows over 160 is exactly 3, so a
/// flat frame fills every pixel and no row of a hard horizontal edge is blended.
#[test]
fn in_four_three_the_picture_fills_the_panel_and_rows_are_whole() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    set_picture(Picture::FourThree);
    assert_eq!(game_rect(), (0, 0, OUT_W, OUT_H));
    c.set_screen_power(1.0);

    let grey = vec![0x80u8; (SRC_W * SRC_H * 4) as usize];
    c.begin_frame();
    c.upload_game(&grey, GBA);
    c.draw_game();
    let frame = c.read_frame();
    for (x, y) in [(0, 0), (639, 0), (0, 479), (639, 479), (320, 240)] {
        assert!(
            close(px(&frame, x, y), [0x80; 3]),
            "{x},{y} is not the picture"
        );
    }

    // Rows alternating white and black: every panel row is one or the other, never between.
    let rows: Vec<u8> = (0..(SRC_W * SRC_H) as usize)
        .flat_map(|i| match (i / SRC_W as usize) % 2 {
            0 => [255, 255, 255, 0],
            _ => [0, 0, 0, 0],
        })
        .collect();
    c.begin_frame();
    c.upload_game(&rows, GBA);
    c.draw_game();
    let frame = c.read_frame();
    for y in 0..OUT_H as usize {
        let v = px(&frame, 320, y)[0];
        let want = if (y / 3) % 2 == 0 { 255 } else { 0 };
        // Three levels, not one: at exactly 3x every third row samples on the very edge of
        // Pixel AA's transition, and the rounding there comes back out of linear light
        // magnified near black — 2/255, as in the original shader, and not a blended row.
        assert!(
            v.abs_diff(want) <= 3,
            "row {y} is {v}, not {want}: a row was blended at 3x"
        );
    }
    set_picture(Picture::ThreeTwo);
}

/// Pixel AA at sharpness 1.0 places the same boundary sharp-shimmerless does, but mixes the two
/// sides in linear light: a panel pixel two thirds covered by white carries two thirds of the
/// *light*, which stored as sRGB-ish gamma is (2/3)^(1/2.2) of full, about 212 rather than the
/// 170 a straight mix of the stored values gives. Sharper settings narrow the blend, so the
/// crossing pixel is brighter still, and every setting keeps the solid pixels solid.
#[test]
fn pixel_aa_blends_the_crossing_pixel_in_linear_light() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let src: Vec<u8> = (0..(SRC_W * SRC_H) as usize)
        .flat_map(|i| match i % SRC_W as usize == 0 {
            true => [255, 255, 255, 0],
            false => [0, 0, 0, 0],
        })
        .collect();
    c.set_screen_power(1.0);
    let row = |c: &mut Compositor, scaler: Scaler| {
        set_scaler(scaler);
        c.begin_frame();
        c.upload_game(&src, GBA);
        c.draw_game();
        let frame = c.read_frame();
        let y = (game_rect().1 + game_rect().3 / 2) as usize;
        (0..5).map(|x| px(&frame, x, y)[1]).collect::<Vec<_>>()
    };
    let flat = row(&mut c, Scaler::SharpShimmerless);
    let soft = row(&mut c, Scaler::PixelAa(1.0));
    let sharp = row(&mut c, Scaler::PixelAa(1.5));
    set_scaler(Scaler::default());

    let linear = ((2.0f32 / 3.0).powf(1.0 / 2.2) * 255.0).round() as i32;
    assert!((flat[2] as i32 - 170).abs() <= 2, "shimmerless {flat:?}");
    assert!(
        (soft[2] as i32 - linear).abs() <= 3,
        "pixel aa at 1.0 crossed at {} rather than {linear}: {soft:?}",
        soft[2]
    );
    assert!(
        sharp[2] > soft[2],
        "sharper did not narrow the blend: {sharp:?}"
    );
    for r in [&flat, &soft, &sharp] {
        assert!(
            r[0] >= 254 && r[1] >= 254,
            "a solid pixel was blended: {r:?}"
        );
        assert!(
            r[3] <= 1 && r[4] <= 1,
            "the column bled past its edge: {r:?}"
        );
    }
}

/// A grid at 4:3, strong enough that its lines can be measured.
const GRID_43: Grid = Grid {
    gap: [0.5, 0.5],
    keep: 1.0,
    even: false,
};

fn srgb_to_lin(v: u8) -> f32 {
    (v as f32 / 255.0).powf(2.2)
}

/// Draws `src` (BGRX, 240x160) at 4:3 through the grid and hands the frame back.
fn through_grid(c: &mut Compositor, src: &[u8], grid: Grid) -> Vec<u8> {
    set_picture(Picture::FourThree);
    set_grid(grid);
    c.set_screen_power(1.0);
    c.begin_frame();
    c.upload_game(src, GBA);
    c.draw_game();
    let frame = c.read_frame();
    set_grid(Grid::default());
    set_picture(Picture::ThreeTwo);
    frame
}

fn flat(v: u8) -> Vec<u8> {
    vec![v; (SRC_W * SRC_H * 4) as usize]
}

/// A flat colour carries the same light through the grid as without it: the gaps take light,
/// and the gain on what is left lit gives exactly that back. Measured as light (linear) over
/// one whole repeat of the grid, 8 columns by 3 rows.
#[test]
fn the_grid_keeps_the_light_of_a_flat_colour() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    for v in [60u8, 128, 180] {
        let frame = through_grid(&mut c, &flat(v), GRID_43);
        let mut light = 0.0;
        for y in 240..243 {
            for x in 320..328 {
                light += srgb_to_lin(px(&frame, x, y)[1]);
            }
        }
        let want = srgb_to_lin(v) * 24.0;
        assert!(
            (light - want).abs() / want < 0.02,
            "grey {v}: {light:.3} of light against {want:.3} without the grid"
        );
    }
}

/// A single column slid across the panel gives out the same light at every position: every
/// source pixel loses the same share of itself to the grid wherever it lands, and gets it back.
#[test]
fn a_column_sliding_under_the_grid_does_not_pulse() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let mut lights = vec![];
    for at in 100..109 {
        let src: Vec<u8> = (0..(SRC_W * SRC_H) as usize)
            .flat_map(|i| match i % SRC_W as usize == at {
                true => [120, 120, 120, 0],
                false => [0, 0, 0, 0],
            })
            .collect();
        let frame = through_grid(&mut c, &src, GRID_43);
        let light: f32 = (0..3)
            .map(|dy| {
                (0..OUT_W as usize)
                    .map(|x| srgb_to_lin(px(&frame, x, 240 + dy)[1]))
                    .sum::<f32>()
            })
            .sum();
        lights.push(light);
    }
    let (lo, hi) = lights
        .iter()
        .fold((f32::MAX, 0f32), |(a, b), &v| (a.min(v), b.max(v)));
    assert!(
        (hi - lo) / hi < 0.02,
        "the column pulses by {:.1}% as it moves: {lights:?}",
        (hi - lo) / hi * 100.0
    );
}

/// Every line sits on the edge it marks, so the lines are evenly spaced at the true pitch of 8/3
/// panel pixels rather than snapping to whole pixels in a 3-2-3 rhythm. On a flat colour that
/// shows as a pattern that repeats exactly every 8 columns (three pixels) across the whole
/// picture, and within each repeat is its own mirror image both about its ends, where an edge
/// falls between two columns, and about its middle, which the other two edges straddle at 4/3
/// either side. Hard lines snapped to columns are neither.
#[test]
fn the_grid_lines_are_evenly_spaced_at_the_true_pitch() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let frame = through_grid(&mut c, &flat(128), GRID_43);
    let col = |x: usize| px(&frame, x, 241)[1];
    let repeat: Vec<u8> = (0..8).map(col).collect();
    for x in 0..OUT_W as usize {
        assert!(
            col(x).abs_diff(repeat[x % 8]) <= 1,
            "column {x} is {}, not {} as every 8 columns before it: {repeat:?}",
            col(x),
            repeat[x % 8]
        );
    }
    for m in 0..4 {
        assert!(
            repeat[3 - m].abs_diff(repeat[4 + m]) <= 1,
            "not a mirror about its middle: {repeat:?}"
        );
        assert!(
            repeat[m].abs_diff(repeat[7 - m]) <= 1,
            "not a mirror about its ends: {repeat:?}"
        );
    }
    let lightest = *repeat.iter().max().unwrap();
    assert!(
        repeat.iter().any(|&v| v + 8 < lightest),
        "no lines at all: {repeat:?}"
    );
}

/// The pattern is its own mirror image about the middle of the picture: whatever the lines do at
/// one side they do at the other, so nothing drifts from left to right or top to bottom.
#[test]
fn the_grid_is_symmetric_about_the_middle() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let frame = through_grid(&mut c, &flat(128), GRID_43);
    for x in 0..OUT_W as usize {
        let (a, b) = (
            px(&frame, x, 241)[1],
            px(&frame, OUT_W as usize - 1 - x, 241)[1],
        );
        assert!(a.abs_diff(b) <= 1, "column {x} is {a}, its mirror {b}");
    }
    for y in 0..OUT_H as usize {
        let (a, b) = (
            px(&frame, 320, y)[1],
            px(&frame, 320, OUT_H as usize - 1 - y)[1],
        );
        assert!(a.abs_diff(b) <= 1, "row {y} is {a}, its mirror {b}");
    }
}

/// The grid never washes a colour out. Its gain is the same on all three channels and never
/// takes a channel past white, so a colour keeps its balance — hue and saturation — exactly.
/// Checked on flat colours including a fully saturated orange, the case with no headroom: every
/// panel pixel keeps the source's channel ratios, and the light over a repeat of the grid is the
/// source's (strict) or within 5% of it (`grid on`, which keeps some grid on such a colour).
#[test]
fn the_grid_keeps_every_colour_s_balance() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let lin = |v: u8| (v as f32 / 255.0).powf(2.2);
    for rgb in [
        [255u8, 128, 0],
        [96, 176, 232],
        [230, 40, 40],
        [240, 200, 160],
        [120, 80, 40],
    ] {
        for grid in [
            Grid {
                gap: [0.25, 0.25],
                keep: 0.8,
                even: false,
            },
            Grid {
                gap: [0.25, 0.25],
                keep: 1.0,
                even: false,
            },
        ] {
            let src: Vec<u8> = (0..(SRC_W * SRC_H) as usize)
                .flat_map(|_| [rgb[2], rgb[1], rgb[0], 0])
                .collect();
            let frame = through_grid(&mut c, &src, grid);
            let want = [lin(rgb[0]), lin(rgb[1]), lin(rgb[2])];
            let wmax = want.iter().cloned().fold(0.0, f32::max);
            let mut light = [0.0f32; 3];
            for y in 240..243 {
                for x in 320..328 {
                    let p = px(&frame, x, y).map(lin);
                    let pmax = p.iter().cloned().fold(0.0, f32::max);
                    for k in 0..3 {
                        light[k] += p[k] / 24.0;
                        let shift = (p[k] / pmax - want[k] / wmax).abs();
                        assert!(
                            shift < 0.01,
                            "{rgb:?} at {x},{y}: channel {k} is off balance by {shift:.3}"
                        );
                    }
                }
            }
            // Strict gives every colour its full light back. Below strict a colour with no
            // headroom keeps some of its lines, at the cost of a little of its light: at 0.8,
            // no more than 5%, and never more than it had.
            let floor = if grid.keep >= 1.0 { 0.99 } else { 0.95 };
            for k in 0..3 {
                assert!(
                    light[k] >= floor * want[k] - 0.005 && light[k] <= want[k] + 0.01 * wmax,
                    "{rgb:?} keep {}: channel {k} carries {:.3} against {:.3}",
                    grid.keep,
                    light[k],
                    want[k]
                );
            }
        }
    }
}

/// `grid lcd` darkens every colour alike, as a backlit LCD's gaps do: over one repeat of the
/// grid, the darkest panel pixel carries the same share of the lightest one's light on white,
/// which cannot be brightened, as on a mid grey, which can. `grid on` keeps the lines on white
/// shallower, so there the two differ.
#[test]
fn grid_lcd_draws_lines_of_the_same_depth_on_every_colour() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let depth = |frame: &[u8]| {
        let lin: Vec<f32> = (240..243)
            .flat_map(|y| (320..328).map(move |x| (x, y)))
            .map(|(x, y)| srgb_to_lin(px(frame, x, y)[1]))
            .collect();
        let hi = lin.iter().cloned().fold(0.0, f32::max);
        let lo = lin.iter().cloned().fold(f32::MAX, f32::min);
        lo / hi
    };
    let white = depth(&through_grid(&mut c, &flat(255), Grid::lcd()));
    let grey = depth(&through_grid(&mut c, &flat(120), Grid::lcd()));
    assert!(
        (white - grey).abs() < 0.03,
        "lines take {:.0}% on white but {:.0}% on grey",
        (1.0 - white) * 100.0,
        (1.0 - grey) * 100.0
    );
    let on_white = depth(&through_grid(&mut c, &flat(255), Grid::on()));
    assert!(
        on_white > white + 0.05,
        "`grid on` should keep lines on white shallower than `grid lcd`: {on_white} vs {white}"
    );
}

/// `grid-depth` is how dark a line gets: a deeper setting darkens the darkest panel pixel of a
/// repeat further, on a colour that can be brightened back, and leaves no grid alone.
#[test]
fn a_deeper_grid_draws_darker_lines() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    let depth = |frame: &[u8]| {
        let lin: Vec<f32> = (240..243)
            .flat_map(|y| (320..328).map(move |x| (x, y)))
            .map(|(x, y)| srgb_to_lin(px(frame, x, y)[1]))
            .collect();
        let hi = lin.iter().cloned().fold(0.0, f32::max);
        let lo = lin.iter().cloned().fold(f32::MAX, f32::min);
        1.0 - lo / hi
    };
    let shallow = depth(&through_grid(
        &mut c,
        &flat(120),
        Grid::lcd().with_depth(20.0),
    ));
    let deep = depth(&through_grid(
        &mut c,
        &flat(120),
        Grid::lcd().with_depth(60.0),
    ));
    assert!(
        deep > shallow * 2.0,
        "60% lines took {:.0}%, no more than twice 20% lines' {:.0}%",
        deep * 100.0,
        shallow * 100.0
    );
    assert_eq!(Grid::default().with_depth(60.0), Grid::default());
}

/// `grid lcd` dims every colour by the same share and lifts none: over one repeat of the grid,
/// white, a bright colour, a mid grey and a dark one each keep the same fraction of the light
/// they have with no grid, so no colour is pushed up to meet a brighter one and the picture's
/// tones keep their order and their spacing. This is what flattened the highlights when the
/// light the lines took was given back instead.
#[test]
fn grid_lcd_dims_every_colour_alike_and_lifts_none() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    // Mean linear light of the green channel over whole repeats: 3 source pixels across is 8
    // panel pixels, 1 down is 3.
    let mean = |frame: &[u8]| {
        let lin: Vec<f32> = (240..246)
            .flat_map(|y| (320..336).map(move |x| (x, y)))
            .map(|(x, y)| srgb_to_lin(px(frame, x, y)[1]))
            .collect();
        lin.iter().sum::<f32>() / lin.len() as f32
    };
    let kept: Vec<f32> = [255u8, 215, 120, 40]
        .iter()
        .map(|&v| {
            let with = mean(&through_grid(&mut c, &flat(v), Grid::lcd()));
            let without = mean(&through_grid(&mut c, &flat(v), Grid::default()));
            with / without
        })
        .collect();
    for k in &kept {
        assert!(
            (k - kept[0]).abs() < 0.03,
            "colours keep different shares of their light: {kept:?}"
        );
    }
    assert!(
        kept[0] < 0.9,
        "the lines took almost nothing: every colour kept {:.0}%",
        kept[0] * 100.0
    );
}
