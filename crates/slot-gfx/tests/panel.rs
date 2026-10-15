//! A panel larger than the composite: the RG CubeXX's 720x720. The menus keep their 640x480 in
//! the middle, and the live game is placed against the whole panel. One test in its own binary,
//! because the canvas is the process's.

use slot_gfx::{
    canvas, game_rect, live_rect, set_fit, set_source_size, ui_origin, Compositor, Fit,
    PbufferSurface,
};

#[test]
fn a_720_square_panel_draws_the_game_across_it_and_a_640x480_panel_is_unchanged() {
    let Ok(surface) = PbufferSurface::new((720, 720)) else {
        return;
    };
    let mut c = Compositor::for_panel(&surface).expect("a compositor");
    assert_eq!(canvas(), (720, 720));
    assert_eq!(ui_origin(), (40, 120), "the menus are not centred");

    // GBA: 240x160 at a whole 3x, the panel's full width, centred top to bottom.
    set_source_size((240, 160));
    set_fit(Fit::Gba);
    assert_eq!(live_rect(), (0, 120, 720, 480));
    // Game Boy: 160x144 at 4x.
    set_source_size((160, 144));
    set_fit(Fit::Whole);
    assert_eq!(live_rect(), (40, 72, 640, 576));
    // SNES, at its 8:7 across the full width, whichever picture the 640x480 panel was set to,
    // and whether the frame is 256x224 or hi-res.
    for fit in [Fit::Rows, Fit::Aspect(4.0 / 3.0)] {
        for size in [(256, 224), (512, 448)] {
            set_source_size(size);
            set_fit(fit);
            assert_eq!(live_rect(), (0, 45, 720, 630), "{fit:?} {size:?}");
        }
    }

    // A frame through the whole path, which the device runs every refresh.
    set_source_size((240, 160));
    set_fit(Fit::Gba);
    c.upload_game(&vec![0x80; 240 * 160 * 4], (240, 160));
    c.begin_frame();
    c.draw_game();
    c.end_frame((720, 720));
    assert_eq!(
        c.read_frame().len(),
        640 * 480 * 4,
        "the menus' area read back"
    );
    drop(c);

    // The RG35XXSP's own panel is the composite's size, and nothing about it changes.
    let surface = PbufferSurface::new((640, 480)).expect("a 640x480 surface");
    let _c = Compositor::for_panel(&surface).expect("a compositor");
    assert_eq!(canvas(), (640, 480));
    assert_eq!(ui_origin(), (0, 0));
    set_source_size((256, 224));
    set_fit(Fit::Rows);
    assert_eq!(live_rect(), game_rect());
}
