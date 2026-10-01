mod common;

use common::{app_playing_with, tmp_root_with_carts, tmp_root_with_gb_carts, StubSnapshot};
use slot_input::Action;
use slot_store::{paint_path, read_paint, scan, write_paint, Platform};

/// A 240×160 frame, as `Snapshot::thumb` hands one over: `top` over the upper three quarters
/// and `bottom` under them, through the same encoder the worker uses.
fn frame(top: [u8; 3], bottom: [u8; 3]) -> Vec<u8> {
    let (w, h) = (240u32, 160u32);
    let mut xrgb = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let [r, g, b] = if y < h * 3 / 4 { top } else { bottom };
        for _ in 0..w {
            xrgb.extend_from_slice(&[b, g, r, 0]);
        }
    }
    slot::thumb::png(&xrgb, (w, h)).expect("encode")
}

fn showing(png: Vec<u8>) -> Box<dyn slot::persist::Snapshot> {
    let (_, loaded) = StubSnapshot::pair();
    Box::new(StubSnapshot {
        state: vec![9u8; 1024],
        sav: None,
        thumb: Some(png),
        loaded,
    })
}

fn hue_of([r, g, b]: [u8; 3]) -> &'static str {
    match (r > g && r > b, g > r && g > b, b > r && b > g) {
        (true, _, _) => "red",
        (_, true, _) => "green",
        (_, _, true) => "blue",
        _ => "none",
    }
}

#[test]
fn leaving_a_game_paints_its_label_the_colour_it_was_showing() {
    let d = tmp_root_with_carts(&["Emerald"]);
    // A faded sky over three quarters of the screen and vivid grass under it: the grass is
    // what the picture is, and what the label takes.
    let png = frame([170, 190, 215], [40, 200, 60]);
    let mut a = app_playing_with(d.path(), "Emerald", showing(png));
    a.apply(Action::Eject);

    let kept = read_paint(&paint_path(d.path(), Platform::Gba, "Emerald")).expect("no file");
    assert_eq!(hue_of(kept), "green");
    let cart = a.carts().find(|c| c.stem == "Emerald").unwrap();
    assert_eq!(
        cart.paint,
        Some(kept),
        "the shelf is not showing what was kept"
    );
    // And the next boot reads the same colour back, rather than reading the game again.
    assert_eq!(scan(d.path()).unwrap()[0].paint, Some(kept));
}

#[test]
fn a_painted_label_keeps_its_colour_however_the_game_ends_next_time() {
    let d = tmp_root_with_carts(&["Emerald"]);
    write_paint(d.path(), Platform::Gba, "Emerald", [200, 40, 40]).unwrap();
    let mut a = app_playing_with(
        d.path(),
        "Emerald",
        showing(frame([20, 40, 230], [20, 40, 230])),
    );
    a.apply(Action::Eject);
    assert_eq!(
        read_paint(&paint_path(d.path(), Platform::Gba, "Emerald")),
        Some([200, 40, 40])
    );
}

#[test]
fn a_frame_with_no_colour_in_it_paints_nothing() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_with(
        d.path(),
        "Emerald",
        showing(frame([0, 0, 0], [128, 128, 128])),
    );
    a.apply(Action::Eject);
    assert!(!paint_path(d.path(), Platform::Gba, "Emerald").exists());
    assert_eq!(a.carts().next().unwrap().paint, None);
}

#[test]
fn a_cart_with_label_art_is_never_painted() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let label = d.path().join("Labels/GBA/Emerald.png");
    std::fs::create_dir_all(label.parent().unwrap()).unwrap();
    std::fs::write(&label, frame([1, 2, 3], [1, 2, 3])).unwrap();
    let mut a = app_playing_with(
        d.path(),
        "Emerald",
        showing(frame([40, 200, 60], [40, 200, 60])),
    );
    a.apply(Action::Eject);
    assert!(!paint_path(d.path(), Platform::Gba, "Emerald").exists());
}

/// A Game Boy's picture is the screen's own tint, so every cart would come out one green.
#[test]
fn a_game_boy_cart_keeps_its_hashed_colour() {
    let d = tmp_root_with_gb_carts(&["Tetris"]);
    let mut a = app_playing_with(
        d.path(),
        "Tetris",
        showing(frame([155, 188, 15], [48, 98, 48])),
    );
    a.apply(Action::Eject);
    assert!(!paint_path(d.path(), Platform::Gb, "Tetris").exists());
}
