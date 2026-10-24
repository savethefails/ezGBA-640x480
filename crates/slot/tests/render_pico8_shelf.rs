//! A shelf of PICO-8 carts through the real frontend, composited on the GPU and written out, for
//! judging the shelf's look by eye.
//!
//! `SCRATCH_PNG_DIR=/tmp cargo test -p slot --test render_pico8_shelf -- --nocapture`

#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::VecDeque;
use std::path::Path;

use common::{clocked, tmp_root};
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

const PICO8: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x1d, 0x2b, 0x53],
    [0x7e, 0x25, 0x53],
    [0x00, 0x87, 0x51],
    [0xab, 0x52, 0x36],
    [0x5f, 0x57, 0x4f],
    [0xc2, 0xc3, 0xc7],
    [0xff, 0xf1, 0xe8],
    [0xff, 0x00, 0x4d],
    [0xff, 0xa3, 0x00],
    [0xff, 0xec, 0x27],
    [0x00, 0xe4, 0x36],
    [0x29, 0xad, 0xff],
    [0x83, 0x76, 0x9c],
    [0xff, 0x77, 0xa8],
    [0xff, 0xcc, 0xaa],
];

/// A `.p8.png` as PICO-8 saves one: 160 x 205, the game's 128 x 128 picture at (16, 24), drawn
/// here as a sky, a ground and a few blocks in PICO-8's colours.
fn p8_png(path: &Path, sky: usize, ground: usize, blocks: &[(u32, u32, u32, u32, usize)]) {
    let (w, h) = (160u32, 205u32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    let mut put = |x: u32, y: u32, c: usize| {
        let at = ((y * w + x) * 4) as usize;
        px[at..at + 3].copy_from_slice(&PICO8[c]);
        px[at + 3] = 0xff;
    };
    for y in 0..h {
        for x in 0..w {
            put(x, y, 5);
        }
    }
    for y in 0..128 {
        for x in 0..128 {
            put(16 + x, 24 + y, if y < 92 { sky } else { ground });
        }
    }
    for &(bx, by, bw, bh, c) in blocks {
        for y in by..by + bh {
            for x in bx..bx + bw {
                put(16 + x, 24 + y, c);
            }
        }
    }
    let file = std::fs::File::create(path).expect("create cart");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
}

fn put_pico8_carts(root: &Path) {
    let dir = root.join("Games/PICO8");
    std::fs::create_dir_all(&dir).unwrap();
    p8_png(
        &dir.join("Celeste.p8.png"),
        1,
        4,
        &[
            (20, 60, 30, 6, 7),
            (70, 40, 40, 6, 7),
            (60, 80, 6, 12, 8),
            (94, 10, 14, 14, 15),
        ],
    );
    p8_png(
        &dir.join("Earth Apple.p8.png"),
        0,
        9,
        &[
            (20, 20, 88, 14, 11),
            (24, 40, 80, 14, 8),
            (40, 84, 10, 8, 6),
            (70, 80, 8, 8, 10),
        ],
    );
    p8_png(
        &dir.join("Minesweeper.p8.png"),
        0,
        0,
        &[(10, 50, 108, 12, 7), (30, 76, 68, 4, 7)],
    );
    p8_png(
        &dir.join("Paper Plane.p8.png"),
        1,
        1,
        &[
            (30, 30, 6, 6, 0),
            (80, 50, 6, 6, 0),
            (60, 70, 4, 4, 8),
            (20, 100, 40, 3, 12),
        ],
    );
}

fn composed(f: &mut Frontend, c: &mut Compositor, name: &str) {
    f.compose(c);
    let px = c.read_frame();
    let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") else {
        return;
    };
    let path = format!("{dir}/pico8-shelf-{name}.png");
    let file = std::fs::File::create(&path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png header")
        .write_image_data(&px)
        .expect("png data");
    println!("wrote {path}");
}

/// `n` frames at the panel's rate: the frontend runs on the wall clock, so frames fed faster than
/// that leave every animation where it started.
fn run(f: &mut Frontend, input: &mut Script, n: usize) {
    for _ in 0..n {
        std::thread::sleep(std::time::Duration::from_millis(16));
        f.advance(input);
    }
}

#[test]
fn render_pico8_shelf() {
    if std::env::var("SCRATCH_PNG_DIR").is_err() {
        return;
    }
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let d = tmp_root();
    put_pico8_carts(d.path());
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    run(&mut f, &mut input, 30);
    composed(&mut f, &mut c, "first");
    input.0.push_back(vec![RawEvent::Down(Btn::Right)]);
    input.0.push_back(vec![RawEvent::Up(Btn::Right)]);
    run(&mut f, &mut input, 60);
    composed(&mut f, &mut c, "second");
    // In, and out again, every frame of both. With no PICO-8 core on the machine the game is the
    // mock's test pattern.
    if std::env::var("SCRATCH_INSERT").is_err() {
        return;
    }
    input.0.push_back(vec![RawEvent::Down(Btn::A)]);
    input.0.push_back(vec![RawEvent::Up(Btn::A)]);
    for i in 0..60 {
        run(&mut f, &mut input, 1);
        composed(&mut f, &mut c, &format!("insert-{i:03}"));
    }
    // MENU held: out it comes.
    input.0.push_back(vec![RawEvent::Down(Btn::Menu)]);
    for i in 0..80 {
        run(&mut f, &mut input, 1);
        if i == 6 {
            input.0.push_back(vec![RawEvent::Up(Btn::Menu)]);
        }
        composed(&mut f, &mut c, &format!("eject-{i:03}"));
    }
}
