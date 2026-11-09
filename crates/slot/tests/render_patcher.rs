//! The quick menu's Patcher through the real frontend, composited on the GPU and written out for
//! judging by eye.
//!
//! `SCRATCH_PNG_DIR=/tmp cargo test -p slot --test render_patcher -- --nocapture`

#![cfg(any(target_os = "macos", target_os = "linux"))]

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
    let path = format!("{dir}/patcher-{name}.png");
    let file = std::fs::File::create(&path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
    println!("wrote {path}");
}

#[test]
fn render_patcher() {
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
    let games = slot::pico8_patch::games(d.path());
    std::fs::create_dir_all(&games).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pico8");
    for name in ["multi_demo-2", "a_very_long_cart_name_from_the_bbs-12"] {
        std::fs::copy(
            fixture.join("multi_demo-2.p8.png"),
            games.join(format!("{name}.p8.png")),
        )
        .unwrap();
    }
    std::fs::copy(fixture.join("multi_demo-2.p8"), games.join("Renamed.p8")).unwrap();
    std::fs::write(
        games.join("celeste-1.p8"),
        "pico-8 cartridge\nversion 43\n__lua__\nprint(\"hi\")\n",
    )
    .unwrap();
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    run(&mut f, &mut input, 20);
    tap(&mut f, &mut input, Btn::Menu);
    run(&mut f, &mut input, 20);
    // Down to Patcher: it sits after Wi-Fi, before About.
    for _ in 0..slot_ui::QuickRow::Patcher.index() {
        tap(&mut f, &mut input, Btn::Down);
    }
    shot(&mut f, &mut c, &dir, "quick-menu");
    tap(&mut f, &mut input, Btn::A);
    run(&mut f, &mut input, 10);
    shot(&mut f, &mut c, &dir, "list");
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::Down);
    shot(&mut f, &mut c, &dir, "on-multicart");
    tap(&mut f, &mut input, Btn::A);
    run(&mut f, &mut input, 10);
    shot(&mut f, &mut c, &dir, "patched");
}

#[test]
fn render_shelf_patch_hint() {
    let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") else {
        return;
    };
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let d = tmp_root_with_carts(&[]);
    clocked(d.path());
    let games = slot::pico8_patch::games(d.path());
    std::fs::create_dir_all(&games).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pico8");
    std::fs::copy(
        fixture.join("multi_demo-2.p8.png"),
        games.join("multi_demo-2.p8.png"),
    )
    .unwrap();
    std::fs::write(
        games.join("celeste-1.p8"),
        "pico-8 cartridge\nversion 43\n__lua__\nprint(\"hi\")\n",
    )
    .unwrap();
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    run(&mut f, &mut input, 20);
    shot(&mut f, &mut c, &dir, "shelf-fine");
    tap(&mut f, &mut input, Btn::Right);
    run(&mut f, &mut input, 40);
    shot(&mut f, &mut c, &dir, "shelf-wifi-off");
    f.app_mut().assume_online();
    f.app_mut()
        .set_offline_fetch(std::sync::Arc::new(|| Box::new(SlowBbs)));
    run(&mut f, &mut input, 5);
    shot(&mut f, &mut c, &dir, "shelf-needs-patch");
    input.0.push_back(vec![RawEvent::Down(Btn::Select)]);
    run(&mut f, &mut input, 3);
    tap(&mut f, &mut input, Btn::Start);
    input.0.push_back(vec![RawEvent::Up(Btn::Select)]);
    run(&mut f, &mut input, 20);
    shot(&mut f, &mut c, &dir, "shelf-downloading");
    run(&mut f, &mut input, 120);
    shot(&mut f, &mut c, &dir, "shelf-ready");
    run(&mut f, &mut input, 220);
    shot(&mut f, &mut c, &dir, "shelf-done");
    assert_eq!(
        slot::pico8_patch::status(&games.join("multi_demo-2.p8.png")),
        slot::pico8_patch::Status::Patched
    );
}

/// Serves the demo cart for every id, a little slowly, so the progress can be seen.
struct SlowBbs;

impl slot::pico8_cache::Fetch for SlowBbs {
    fn fetch(&mut self, url: &str, out: &Path) -> bool {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let lid = url.rsplit("lid=").next().unwrap();
        if url.contains("nfo=1") {
            return std::fs::write(out, format!("lid:{lid}-1\n")).is_ok();
        }
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pico8");
        std::fs::copy(fixture.join("multi_demo-2.p8.png"), out).is_ok()
    }
}
