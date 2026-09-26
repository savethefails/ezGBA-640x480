use slot_gfx::{blit_is_whole, blit_rect, blit_rect_fit, fit_rect, fit_scale};

#[test]
fn integer_scale_never_fractional() {
    assert_eq!(fit_scale(1280, 960), 2);
    assert_eq!(fit_scale(1400, 1000), 2); // rounds down, never 2.08
    assert_eq!(fit_scale(630, 400), 1); // clamps to 1 below native
    assert_eq!(fit_scale(1920, 1440), 3);
}

#[test]
fn integer_scale_is_limited_by_the_tighter_axis() {
    assert_eq!(fit_scale(1920, 960), 2);
    assert_eq!(fit_scale(1280, 1440), 2);
}

#[test]
fn blit_rect_centres_the_scaled_output_in_the_window() {
    assert_eq!(fit_rect(1280, 960), (0, 0, 1280, 960));
    assert_eq!(fit_rect(1340, 1000), (30, 20, 1280, 960));
    assert_eq!(fit_rect(1920, 960), (320, 0, 1280, 960));
}

#[test]
fn blit_rect_overflows_symmetrically_when_the_window_is_too_small() {
    assert_eq!(fit_rect(620, 400), (-10, -40, 640, 480));
}

#[test]
fn a_panel_smaller_than_the_composite_fits_rather_than_crops() {
    let (x, y, w, h) = blit_rect_fit((480, 320), 0.0);
    assert!(
        w <= 480 && h <= 320,
        "{w}x{h} does not fit a 480x320 window"
    );
    assert!(
        (w as f32 / h as f32 - 4.0 / 3.0).abs() < 0.01,
        "aspect was not preserved"
    );
    assert!(x >= 0 && y >= 0);
}

#[test]
fn the_panel_takes_the_composite_one_to_one() {
    // The RG35XXSP: the composite is the panel's own size, so the blit is a copy.
    assert_eq!(blit_rect((640, 480), 0.0), (0, 0, 640, 480));
    assert_eq!(blit_rect((480, 320), 0.0), blit_rect_fit((480, 320), 0.0));
    assert_eq!(blit_rect((1340, 1000), 0.0), fit_rect(1340, 1000));
}

#[test]
fn the_fit_is_centred_and_shakes_with_the_picture() {
    let (x, y, w, h) = blit_rect_fit((600, 360), 0.0);
    assert_eq!((y, h), (0, 360));
    assert_eq!((x, w), ((600 - w) / 2, 480));
    assert!(blit_rect_fit((600, 360), 6.0).0 > x);
}

#[test]
fn only_a_target_that_holds_the_composite_whole_is_blitted_sharp() {
    assert!(blit_is_whole((640, 480)));
    assert!(blit_is_whole((1280, 960)));
    assert!(!blit_is_whole((480, 320)));
    assert!(!blit_is_whole((640, 400)));
}
