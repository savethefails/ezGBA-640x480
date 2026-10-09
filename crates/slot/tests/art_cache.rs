mod common;

use std::collections::VecDeque;
use std::path::Path;

use common::{clocked, tmp_root_with_carts};
use slot::art_cache::{box_art, cache_path, label_art};
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface, OUT_W};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;
use slot_store::{scan, Cart};
use slot_ui::label_size;

fn write_png(path: &Path, w: u32, h: u32, rgb: [u8; 3]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let px: Vec<u8> = (0..w * h)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 0xff])
        .collect();
    e.write_header().unwrap().write_image_data(&px).unwrap();
}

fn cart(root: &Path, stem: &str) -> Cart {
    scan(root)
        .expect("scan")
        .into_iter()
        .find(|c| c.stem == stem)
        .expect("the cart is on the card")
}

fn label_png(root: &Path, stem: &str) -> std::path::PathBuf {
    root.join("Labels/GBA").join(format!("{stem}.png"))
}

fn box_art_png(root: &Path, stem: &str) -> std::path::PathBuf {
    root.join("Images/GBA").join(format!("{stem}.png"))
}

#[test]
fn a_label_is_scaled_once_and_read_back_from_the_card() {
    let d = tmp_root_with_carts(&["Emerald"]);
    write_png(&label_png(d.path(), "Emerald"), 64, 64, [200, 30, 30]);
    let c = cart(d.path(), "Emerald");
    let (w, h) = label_size(&c);

    let built = label_art(d.path(), &c).expect("no label art");
    assert_eq!(built.len(), (w * h * 4) as usize);
    let entry = cache_path(d.path(), "Labels", &c);
    assert_eq!(
        std::fs::metadata(&entry).unwrap().len(),
        32 + u64::from(w * h * 4)
    );

    // The entry, not the source, is what is read now: the same bytes come back, and only the
    // entry was read (its access is the only thing that changed).
    let again = label_art(d.path(), &c).expect("no label art the second time");
    assert_eq!(again, built);
}

#[test]
fn a_changed_label_is_scaled_again() {
    let d = tmp_root_with_carts(&["Emerald"]);
    write_png(&label_png(d.path(), "Emerald"), 64, 64, [200, 30, 30]);
    let c = cart(d.path(), "Emerald");
    let red = label_art(d.path(), &c).unwrap();
    // A different size as well as a different colour: the stamp is the length and the
    // modification time, and a test cannot wait out a filesystem's coarse mtime.
    write_png(&label_png(d.path(), "Emerald"), 80, 64, [30, 30, 200]);
    let blue = label_art(d.path(), &c).unwrap();
    assert_ne!(red, blue, "the cache handed back the old label");
    assert_eq!(&blue[..3], &[30, 30, 200]);
}

#[test]
fn a_torn_entry_is_rebuilt_rather_than_trusted() {
    let d = tmp_root_with_carts(&["Emerald"]);
    write_png(&label_png(d.path(), "Emerald"), 64, 64, [200, 30, 30]);
    let c = cart(d.path(), "Emerald");
    let built = label_art(d.path(), &c).unwrap();
    let entry = cache_path(d.path(), "Labels", &c);
    let bytes = std::fs::read(&entry).unwrap();
    std::fs::write(&entry, &bytes[..bytes.len() / 2]).unwrap();
    assert_eq!(label_art(d.path(), &c).unwrap(), built);
    assert_eq!(
        std::fs::read(&entry).unwrap(),
        bytes,
        "the entry was not rewritten"
    );
}

#[test]
fn a_label_the_decoder_refuses_is_remembered_as_refused() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let png = label_png(d.path(), "Emerald");
    std::fs::create_dir_all(png.parent().unwrap()).unwrap();
    std::fs::write(&png, b"not a png").unwrap();
    let c = cart(d.path(), "Emerald");
    assert!(label_art(d.path(), &c).is_none());
    let entry = cache_path(d.path(), "Labels", &c);
    assert_eq!(std::fs::metadata(&entry).unwrap().len(), 32);
    assert!(label_art(d.path(), &c).is_none());
}

/// Box art keeps its own shape: a square box in a wide space is as tall as the space and no
/// wider than it is tall, and a wide title screen is as wide as the space allows.
#[test]
fn box_art_fits_the_space_without_losing_its_shape() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_png(&box_art_png(d.path(), "Emerald"), 500, 500, [10, 200, 10]);
    write_png(&box_art_png(d.path(), "Fusion"), 1600, 400, [200, 10, 10]);
    let bound = (600, 250);

    let (rgba, w, h) = box_art(d.path(), &cart(d.path(), "Emerald"), bound).expect("no art");
    assert_eq!((w, h), (250, 250));
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    assert_eq!(&rgba[..3], &[10, 200, 10]);

    let (_, w, h) = box_art(d.path(), &cart(d.path(), "Fusion"), bound).unwrap();
    assert_eq!((w, h), (600, 150));

    // Small art is grown to the space rather than left a postage stamp.
    write_png(&box_art_png(d.path(), "Emerald"), 64, 80, [10, 200, 10]);
    let (_, w, h) = box_art(d.path(), &cart(d.path(), "Emerald"), bound).unwrap();
    assert_eq!((w, h), (200, 250));
}

/// The space is asked of the layout, so an entry fitted to an older layout is fitted again.
#[test]
fn box_art_fitted_to_another_space_is_fitted_again() {
    let d = tmp_root_with_carts(&["Emerald"]);
    write_png(&box_art_png(d.path(), "Emerald"), 500, 500, [10, 200, 10]);
    let c = cart(d.path(), "Emerald");
    let (_, w, _) = box_art(d.path(), &c, (600, 250)).unwrap();
    assert_eq!(w, 250);
    let (_, w, h) = box_art(d.path(), &c, (600, 200)).unwrap();
    assert_eq!((w, h), (200, 200));
}

/// It stands in the space over the selected cart: clear of the top of the screen, clear of
/// the cart, centred across the screen.
#[test]
fn box_art_floats_between_the_top_of_the_screen_and_the_cart() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let c = cart(d.path(), "Emerald");
    let (bw, bh) = slot::box_art::bound(&c);
    let (x, y) = slot::box_art::place(&c, bh, bh);
    assert!(y > 0.0, "the art touches the top of the screen");
    let cart_top = slot_ui::rest_y(slot_ui::CART_H as f32) + slot::app::SHELF_ROW_LOWER;
    assert!(y + (bh as f32) < cart_top, "the art runs into the cart");
    assert!(
        cart_top - (y + bh as f32) < 20.0,
        "the art leaves a gap over the cart"
    );
    assert_eq!(x + bh as f32 / 2.0, OUT_W as f32 / 2.0);
    assert!(bw < OUT_W);
}

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

/// Ten carts with box art each: the first frame already shows the selection's, and only the
/// selection's neighbours ever hold one, however far the row turns.
#[test]
fn only_the_box_art_around_the_selection_is_loaded() {
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let stems: Vec<String> = (0..10).map(|i| format!("Cart{i}")).collect();
    let refs: Vec<&str> = stems.iter().map(String::as_str).collect();
    let d = tmp_root_with_carts(&refs);
    for (i, stem) in stems.iter().enumerate() {
        let shade = 40 + 20 * i as u8;
        write_png(
            &box_art_png(d.path(), stem),
            16,
            12,
            [shade, 0, 255 - shade],
        );
    }
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    f.advance(&mut input);

    let loaded = |f: &Frontend| (0..10).filter(|&i| f.app().box_art_loaded((0, i))).count();
    assert!(
        f.app().box_art_loaded((0, 0)),
        "the selection's box art was not up for the first frame"
    );
    assert!(loaded(&f) <= 5, "{} pictures loaded", loaded(&f));

    for _ in 0..6 {
        input.0.push_back(vec![RawEvent::Down(Btn::Right)]);
        input.0.push_back(vec![RawEvent::Up(Btn::Right)]);
        for _ in 0..40 {
            f.advance(&mut input);
            f.compose(&mut c);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(loaded(&f) <= 5, "{} pictures loaded", loaded(&f));
    }
    // Six carts along, with time for the loader to catch up.
    for _ in 0..200 {
        f.compose(&mut c);
        if f.app().box_art_loaded((0, 6)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        f.app().box_art_loaded((0, 6)),
        "the new selection's box art never came"
    );
    assert!(
        !f.app().box_art_loaded((0, 0)),
        "box art far behind was kept"
    );
}
