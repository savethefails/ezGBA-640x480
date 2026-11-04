//! The quick menu's Wi-Fi pages and the keyboard through the real frontend with the fake radio,
//! composited on the GPU and written out for judging by eye.
//!
//! `SCRATCH_PNG_DIR=/tmp cargo test -p slot --test render_wifi -- --nocapture`

#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::VecDeque;

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

fn run(f: &mut Frontend, input: &mut Script, n: usize) {
    for _ in 0..n {
        std::thread::sleep(std::time::Duration::from_millis(16));
        f.advance(input);
    }
}

fn tap(f: &mut Frontend, input: &mut Script, btn: Btn) {
    input.0.push_back(vec![RawEvent::Down(btn)]);
    input.0.push_back(vec![RawEvent::Up(btn)]);
    run(f, input, 6);
}

fn shot(f: &mut Frontend, c: &mut Compositor, dir: &str, name: &str) {
    f.compose(c);
    let px = c.read_frame();
    let path = format!("{dir}/wifi-{name}.png");
    let file = std::fs::File::create(&path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
    println!("wrote {path}");
}

#[test]
fn render_wifi() {
    let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") else {
        return;
    };
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.set_wifi(slot::wifi::Wifi::spawn(
        Box::new(slot::wifi::Fake::joined()),
    ));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    run(&mut f, &mut input, 20);
    tap(&mut f, &mut input, Btn::Menu);
    run(&mut f, &mut input, 20);
    // Down to Wi-Fi: it sits after the SNES emulator, before About.
    for _ in 0..12 {
        tap(&mut f, &mut input, Btn::Down);
    }
    shot(&mut f, &mut c, &dir, "quick-menu");
    tap(&mut f, &mut input, Btn::A);
    run(&mut f, &mut input, 10);
    shot(&mut f, &mut c, &dir, "page-off");
    tap(&mut f, &mut input, Btn::Right);
    run(&mut f, &mut input, 120);
    shot(&mut f, &mut c, &dir, "page-on");
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::A);
    run(&mut f, &mut input, 120);
    shot(&mut f, &mut c, &dir, "scan");
    tap(&mut f, &mut input, Btn::A);
    run(&mut f, &mut input, 10);
    for b in [Btn::A, Btn::Right, Btn::A, Btn::Select, Btn::Right, Btn::A] {
        tap(&mut f, &mut input, b);
    }
    run(&mut f, &mut input, 10);
    shot(&mut f, &mut c, &dir, "keyboard");
}
