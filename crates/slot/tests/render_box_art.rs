//! Box art floating over the shelf, read off the panel. `SCRATCH_PNG_DIR` writes the frames out
//! to be looked at.

mod common;

use std::collections::VecDeque;
use std::path::Path;

use common::{clocked, tmp_root_with_carts};
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

/// A box of `w` by `h`: a pale face inside a dark frame, so its edges show where it stands.
fn write_box(path: &Path, w: u32, h: u32, face: [u8; 3]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let edge = w.min(h) / 20;
    let px: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let frame = x < edge || y < edge || x >= w - edge || y >= h - edge;
            let [r, g, b] = if frame { [20, 20, 60] } else { face };
            [r, g, b, 0xff]
        })
        .collect();
    e.write_header().unwrap().write_image_data(&px).unwrap();
}

fn composed(f: &mut Frontend, c: &mut Compositor, name: &str) -> Vec<u8> {
    f.compose(c);
    let px = c.read_frame();
    if let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") {
        let path = format!("{dir}/box-art-{name}.png");
        let file = std::fs::File::create(&path).expect("create png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&px).unwrap();
    }
    px
}

fn at(px: &[u8], x: u32, y: u32) -> [u8; 3] {
    let o = ((y * OUT_W + x) * 4) as usize;
    [px[o], px[o + 1], px[o + 2]]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 12)
}

/// The selected cart's box stands over it at full strength, not behind a scrim, and in its own
/// shape; it goes when the row turns to a cart that has none, and fades as a cart goes in.
#[test]
fn the_box_floats_over_the_selected_cart() {
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    const FACE: [u8; 3] = [230, 200, 60];
    let d = tmp_root_with_carts(&["Advance Wars", "Bare", "Wario Ware"]);
    write_box(
        &d.path().join("Images/GBA/Advance Wars.png"),
        500,
        500,
        FACE,
    );
    write_box(
        &d.path().join("Images/GBA/Wario Ware.png"),
        1200,
        300,
        [90, 200, 230],
    );
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    f.advance(&mut input);

    let px = composed(&mut f, &mut c, "square");
    let mid = OUT_W / 2;
    assert!(
        near(at(&px, mid, 130), FACE),
        "no box over the cart: {:?}",
        at(&px, mid, 130)
    );
    // Square: as wide as it is tall, so well inside the screen's sides.
    assert!(
        !near(at(&px, 100, 130), FACE),
        "the box was stretched across the screen"
    );
    // Above the cart, not over it: the selected GBA cart starts at 272.
    assert!(!near(at(&px, mid, 268), FACE), "the box runs into the cart");
    assert!(
        !near(at(&px, mid, 4), FACE),
        "the box runs off the top of the screen"
    );

    // The cart with no box: nothing there.
    input.0.push_back(vec![RawEvent::Down(Btn::Right)]);
    input.0.push_back(vec![RawEvent::Up(Btn::Right)]);
    for _ in 0..60 {
        f.advance(&mut input);
        f.compose(&mut c);
    }
    let px = composed(&mut f, &mut c, "none");
    assert!(
        !near(at(&px, mid, 130), FACE),
        "the last cart's box stayed up"
    );

    // The wide one: wider than the square box, and shorter.
    input.0.push_back(vec![RawEvent::Down(Btn::Right)]);
    input.0.push_back(vec![RawEvent::Up(Btn::Right)]);
    let mut px = Vec::new();
    for _ in 0..200 {
        f.advance(&mut input);
        px = composed(&mut f, &mut c, "wide");
        if near(at(&px, mid, 130), [90, 200, 230]) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        near(at(&px, mid, 130), [90, 200, 230]),
        "the wide box never came"
    );
    assert!(
        near(at(&px, 40, 130), [90, 200, 230]),
        "the wide box was not fitted to the width"
    );
    assert!(
        !near(at(&px, mid, 40), [90, 200, 230]),
        "the wide box was stretched to the height"
    );
}
