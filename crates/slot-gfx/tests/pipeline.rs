use slot_gfx::{
    blue_light_gain, grille_mean, Compositor, Draw, HeadlessSurface, GAME_H, GAME_W, GAME_X,
    GAME_Y, OUT_H, OUT_W, SRC_H, SRC_W,
};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// `gl::load_with` writes global function pointers, so two GL tests must not overlap.
static GL: Mutex<()> = Mutex::new(());

/// A GL context is not available everywhere. Skip rather than fail, the same way the mGBA
/// test skips a missing dylib.
fn compositor() -> Option<(MutexGuard<'static, ()>, HeadlessSurface, Compositor)> {
    let guard = GL.lock().unwrap_or_else(PoisonError::into_inner);
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

/// Where `video_refresh` centres a Game Boy's 160x144 picture inside the 240x160 buffer this
/// whole path is built on. Spelled out here rather than imported, because the point of these
/// tests is that the compositor is told a sub-rect and honours it, whoever worked it out.
const GB_X: usize = 40;
const GB_Y: usize = 8;
const GB_W: usize = 160;
const GB_H: usize = 144;

/// That window as the game pass takes it: origin and size in texture coordinates.
const GB_RECT: [f32; 4] = [
    GB_X as f32 / SRC_W as f32,
    GB_Y as f32 / SRC_H as f32,
    GB_W as f32 / SRC_W as f32,
    GB_H as f32 / SRC_H as f32,
];

/// The whole texture, which is what the pass draws when nobody has asked for anything else.
const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// A core's frame: XRGB8888, which is B, G, R, unused in memory. `inside` paints the Game Boy
/// window and `margin` fills the border `video_refresh` leaves around it.
fn gb_shaped(inside: impl Fn(usize, usize) -> [u8; 3], margin: [u8; 3]) -> Vec<u8> {
    let mut buf = Vec::with_capacity((SRC_W * SRC_H * 4) as usize);
    for y in 0..SRC_H as usize {
        for x in 0..SRC_W as usize {
            let in_window = (GB_X..GB_X + GB_W).contains(&x) && (GB_Y..GB_Y + GB_H).contains(&y);
            let rgb = match in_window {
                true => inside(x - GB_X, y - GB_Y),
                false => margin,
            };
            buf.extend_from_slice(&[rgb[2], rgb[1], rgb[0], 0]);
        }
    }
    buf
}

/// A source pixel as the pass draws it with the grille off: shaded by the grille's average,
/// so a 640 panel is exactly as bright as the 3x picture the grille was drawn for. The
/// tolerance is the byte the shader rounds to, not slack in the relationship.
fn shaded(rgb: [u8; 3]) -> [i32; 3] {
    let gain = grille_mean();
    [0, 1, 2].map(|ch| (rgb[ch] as f32 * gain[ch]).round() as i32)
}

/// The panel pixel at the middle of a source pixel's cell, for a picture of `w` by `h` source
/// pixels stretched over the game area. The sharp filter only blends at a cell's edges, so
/// here the source comes through untouched.
fn centre_of(sx: usize, sy: usize, w: usize, h: usize) -> (usize, usize) {
    let x = GAME_X as f32 + (sx as f32 + 0.5) * GAME_W as f32 / w as f32;
    let y = GAME_Y as f32 + (sy as f32 + 0.5) * GAME_H as f32 / h as f32;
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
    c.upload_game(&grey);
    c.draw_game();
    let frame = c.read_frame();

    let want = shaded([0x80; 3]);
    let (top, bottom) = (GAME_Y as usize, (GAME_Y + GAME_H) as usize);
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
    c.upload_game(&src);
    c.draw_game();
    let frame = c.read_frame();

    let (x0, y0) = (GAME_X as usize, GAME_Y as usize);
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
        px(&frame, x0, (GAME_Y + GAME_H) as usize - 1),
        [0, 0, 0],
        "frame is upside down"
    );
    assert_eq!(
        px(&frame, (GAME_X + GAME_W) as usize - 1, y0),
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
        c.upload_game(&grey);
        c.draw_game();
        let frame = c.read_frame();
        let brightest = frame.chunks_exact(4).map(|p| p[0]).max().unwrap_or(0);
        (frame, brightest)
    };

    let (striking, bright) = peak(&mut c, 0.35);
    let lit = |frame: &[u8], y: usize| (0..OUT_W as usize).any(|x| px(frame, x, y) != [0, 0, 0]);
    let (top, bottom) = (GAME_Y as usize, (GAME_Y + GAME_H) as usize - 1);
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
    c.upload_game(&white);
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
    for (x, y) in [(0, GAME_Y), (320, 240), (639, GAME_Y + GAME_H - 1)] {
        let got = px(&lit, x as usize, y as usize);
        assert!(close(got, want), "{x},{y}: {got:?} against {want:?}");
    }
    assert_eq!(
        px(&lit, 320, GAME_Y as usize - 1),
        [0, 0, 0],
        "the shot is over the bar"
    );
}

/// A still is a photograph, taken at some earlier moment, and `thumb::png` encodes the whole
/// 240x160 buffer — so a Game Boy polaroid is the centred picture with black at its sides
/// whatever the panel is set to now. It must not take the picture mode: the same stored image
/// rendering differently because of a preference set after it was taken would mean a state
/// saved before the player ever pressed L comes back stretched.
///
/// The live game beside it is the control: without one, a build that had simply stopped
/// honouring the sub-rect at all would pass this.
#[test]
fn a_still_is_not_cropped_by_the_picture_mode() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    // A shot with its corners marked, so a crop moves something a comparison can see.
    let mut shot = flat_shot([120, 120, 120]);
    for (x, y) in [(0, 0), (SRC_W as usize - 1, SRC_H as usize - 1)] {
        let o = (y * SRC_W as usize + x) * 4;
        shot[o..o + 3].copy_from_slice(&[255, 0, 0]);
    }
    let tex = c.create_texture_nearest(SRC_W, SRC_H, &shot);
    let src = gb_shaped(|x, y| [(x * 3) as u8, (y * 5) as u8, 200], [0, 0, 0]);
    c.set_screen_power(1.0);

    let framed = |c: &mut Compositor, rect: [f32; 4]| {
        c.set_game_source_rect(rect);
        c.begin_frame();
        c.upload_game(&src);
        c.draw_list(&[Draw::Shot { tex }]);
        let still = c.read_frame();
        c.begin_frame();
        c.draw_game();
        (still, c.read_frame())
    };

    let (still_actual, game_actual) = framed(&mut c, WHOLE);
    let (still_full, game_full) = framed(&mut c, GB_RECT);

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
    c.upload_game(&src);
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

    c.set_game_source_rect(WHOLE);
    c.begin_frame();
    c.upload_game(&src);
    c.draw_game();
    assert!(
        c.read_frame() == default,
        "the whole texture asked for is not the whole texture by default"
    );
}

/// Fullscreen draws only the Game Boy's own picture, over the whole game area. The picture's
/// corner texels land in that area's corners, and the margin `video_refresh` leaves around it
/// is nowhere on screen at all — which is the difference between a stretch and a crop that
/// merely moved.
#[test]
fn fullscreen_puts_the_pictures_corners_in_the_panels_corners() {
    let Some((_g, _s, mut c)) = compositor() else {
        return;
    };
    // Four corners that cannot be confused with each other or with the margin, and a body
    // that is none of the five.
    let corner = |x: usize, y: usize| match (x, y) {
        (0, 0) => Some([255, 0, 0]),
        (x, 0) if x == GB_W - 1 => Some([0, 255, 0]),
        (0, y) if y == GB_H - 1 => Some([0, 0, 255]),
        (x, y) if x == GB_W - 1 && y == GB_H - 1 => Some([255, 255, 0]),
        _ => None,
    };
    const MARGIN: [u8; 3] = [0, 255, 255];
    let src = gb_shaped(|x, y| corner(x, y).unwrap_or([90, 90, 90]), MARGIN);
    c.set_screen_power(1.0);

    // The control: at actual size the panel's own corner is the margin, not the picture.
    c.set_game_source_rect(WHOLE);
    c.begin_frame();
    c.upload_game(&src);
    c.draw_game();
    let actual = c.read_frame();
    let got = px(&actual, GAME_X as usize, GAME_Y as usize);
    assert!(
        close(got, shaded(MARGIN)),
        "at actual size the picture's corner is not the margin: {got:?}"
    );

    c.set_game_source_rect(GB_RECT);
    c.begin_frame();
    c.upload_game(&src);
    c.draw_game();
    let full = c.read_frame();

    let (first_x, first_y) = (GAME_X as usize, GAME_Y as usize);
    let last_x = (GAME_X + GAME_W) as usize - 1;
    let last_y = (GAME_Y + GAME_H) as usize - 1;
    for (name, (x, y), rgb) in [
        ("top left", (first_x, first_y), [255, 0, 0]),
        ("top right", (last_x, first_y), [0, 255, 0]),
        ("bottom left", (first_x, last_y), [0, 0, 255]),
        ("bottom right", (last_x, last_y), [255, 255, 0]),
    ] {
        let want = shaded(rgb);
        let got = px(&full, x, y);
        assert!(close(got, want), "{name}: {got:?} against {want:?}");
    }

    // Cyan is the margin and nothing in the picture is cyan, so one cyan-dominant pixel
    // anywhere means the border came along with the stretch.
    let strayed = (0..OUT_H as usize).any(|y| {
        (0..OUT_W as usize).any(|x| {
            let p = px(&full, x, y);
            p[1] > 60 && p[2] > 60 && p[0] < 20
        })
    });
    assert!(!strayed, "the margin is still on screen in fullscreen");
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
    let src = gb_shaped(
        |x, y| match (x + y) % 2 {
            0 => [255, 255, 255],
            _ => [0, 0, 0],
        },
        [0, 0, 0],
    );
    c.set_screen_power(1.0);
    for (name, rect, w, h) in [
        ("actual size", WHOLE, SRC_W as usize, SRC_H as usize),
        ("fullscreen", GB_RECT, GB_W, GB_H),
    ] {
        c.set_game_source_rect(rect);
        c.begin_frame();
        c.upload_game(&src);
        c.draw_game();
        let frame = c.read_frame();
        // Along one row through the middle of the picture: every pixel is either one of the
        // two flat values, or a lone blend between them. Two blends side by side is a band.
        let (_, y) = centre_of(w / 2, h / 2, w, h);
        let (x0, x1) = (GAME_X as usize + 8, (GAME_X + GAME_W) as usize - 8);
        let white = shaded([255; 3])[0];
        let grey = |x: usize| {
            let v = px(&frame, x, y)[0] as i32;
            v > 2 && v < white - 2
        };
        let wide = (x0..x1).find(|&x| grey(x) && grey(x + 1));
        assert_eq!(wide, None, "{name}: a blend two pixels wide at x {wide:?}");
        // And down one column.
        let (x, _) = centre_of(w / 2, h / 2, w, h);
        let (y0, y1) = (GAME_Y as usize + 8, (GAME_Y + GAME_H) as usize - 8);
        let grey = |y: usize| {
            let v = px(&frame, x, y)[0] as i32;
            v > 2 && v < white - 2
        };
        let tall = (y0..y1).find(|&y| grey(y) && grey(y + 1));
        assert_eq!(tall, None, "{name}: a blend two pixels tall at y {tall:?}");
    }
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
    c.set_screen_power(1.0);
    let row = |c: &mut Compositor, at: usize| {
        c.begin_frame();
        c.upload_game(&column(at));
        c.draw_game();
        let frame = c.read_frame();
        let y = (GAME_Y + GAME_H / 2) as usize;
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
}
