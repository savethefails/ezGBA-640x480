//! A PICO-8 cart through the real frontend and the real core: on the shelf as a faux cart, in
//! the slot at 480x480, and back where it was after an eject.
//!
//! PICO-8 is closed source and never ships with slot., so this only runs when it is pointed at
//! a copy and at a built core:
//!
//! ```sh
//! make -C cores/pico8 OUT=/tmp/p8core
//! PICO8_DIR=~/pico-8 PICO8_CORE_DIR=/tmp/p8core SCRATCH_PNG_DIR=/tmp \
//!     cargo test --release -p slot --test pico8 -- --nocapture
//! ```
//!
//! `PICO8_DIR` holds `pico8_dyn` (or `pico8_64`) and `pico8.dat`; `PICO8_CORE_DIR` is the
//! `OUT` the core's Makefile wrote. Without both the test passes having checked nothing.

#![cfg(target_os = "linux")]

mod common;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::clocked;
use slot::app::Phase;
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_input::{InputSource, Millis, RawEvent};
use slot_power::SimPlatform;
use slot_store::{Core, Platform, StateRing};

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

/// Sixteen dots going round, and a label of its own so the shelf has a picture to show.
const CART: &str = "pico-8 cartridge // http://www.pico-8.com
version 42
__lua__
f=0
function _update() f+=1 end
function _draw()
 cls(1)
 for i=0,15 do
  local a=i/16+f/240
  circfill(64+cos(a)*40,64+sin(a)*40,6,8+i%8)
 end
 print(\"frame \"..f,40,60,7)
end
";

fn label() -> String {
    let mut out = String::from("__label__\n");
    for y in 0..128i32 {
        for x in 0..128i32 {
            let (dx, dy) = ((x - 64) as f32, (y - 64) as f32);
            let d = (dx * dx + dy * dy).sqrt();
            let c = if d > 34.0 && d < 46.0 {
                8 + ((dy.atan2(dx) + std::f32::consts::PI) / std::f32::consts::TAU * 8.0) as u32 % 8
            } else if d < 20.0 {
                if (x / 4 + y / 4) % 2 == 1 {
                    7
                } else {
                    12
                }
            } else {
                1
            };
            out.push(char::from_digit(c, 16).unwrap());
        }
        out.push('\n');
    }
    out
}

struct Setup {
    pico8: PathBuf,
    core: PathBuf,
}

fn setup() -> Option<Setup> {
    let pico8 = PathBuf::from(std::env::var_os("PICO8_DIR")?);
    let core = PathBuf::from(std::env::var_os("PICO8_CORE_DIR")?);
    Some(Setup { pico8, core })
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), &dest).unwrap();
        }
    }
}

/// Lays the card out as a user would: PICO-8 in `BIOS/pico8`, the core in `System`, and the
/// cart in `Games/PICO8` as a `.p8.png` exported by the user's own PICO-8 — or as the `.p8`
/// text if that PICO-8 cannot export here.
fn card(root: &Path, s: &Setup) -> String {
    for f in ["pico8_dyn", "pico8_64", "pico8.dat"] {
        let from = s.pico8.join(f);
        if from.exists() {
            std::fs::copy(&from, root.join("BIOS/pico8").join(f)).unwrap();
        }
    }
    copy_dir(&s.core, &root.join("System"));
    let games = root.join("Games/PICO8");
    let text = games.join("orbit.p8");
    std::fs::write(&text, format!("{CART}{}", label())).unwrap();
    let exe = ["pico8_dyn", "pico8_64"]
        .iter()
        .map(|f| s.pico8.join(f))
        .find(|p| p.exists());
    if let Some(exe) = exe {
        let home = root.join("p8export");
        let _ = std::process::Command::new(exe)
            .current_dir(&games)
            .env("SDL_VIDEODRIVER", "dummy")
            .env("SDL_AUDIODRIVER", "dummy")
            .args([
                "-home",
                home.to_str().unwrap(),
                "orbit.p8",
                "-export",
                "orbit.p8.png",
            ])
            .output();
    }
    if games.join("orbit.p8.png").exists() {
        std::fs::remove_file(&text).unwrap();
        "orbit.p8".into()
    } else {
        "orbit".into()
    }
}

fn png(px: &[u8], w: u32, h: u32, name: &str) {
    let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") else {
        return;
    };
    let path = format!("{dir}/pico8-{name}.png");
    let file = std::fs::File::create(&path).expect("create png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png header")
        .write_image_data(px)
        .expect("png data");
    println!("wrote {path}");
}

/// The core's XRGB8888 as the RGBA a PNG wants.
fn core_png(xrgb: &[u8], name: &str) {
    let rgba: Vec<u8> = xrgb
        .chunks(4)
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    png(&rgba, 128, 128, name);
}

struct Run {
    f: Frontend,
    c: Compositor,
    input: Script,
}

impl Run {
    fn step(&mut self) {
        self.f.advance(&mut self.input);
        self.f.kick_and_wait(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(8));
    }

    fn event(&mut self, ev: RawEvent) {
        self.input.0.push_back(vec![ev]);
        self.step();
    }

    fn until(&mut self, what: &str, secs: u64, cond: impl Fn(&Phase) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while !cond(self.f.session().app().phase()) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            self.step();
        }
    }

    fn shot(&mut self, name: &str) -> Vec<u8> {
        self.f.compose(&mut self.c);
        let px = self.c.read_frame();
        png(&px, OUT_W, OUT_H, name);
        px
    }

    /// The core's own picture, 128x128 and not yet scaled.
    fn game(&mut self) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(fr) = self.f.session().frame() {
                assert_eq!(fr.size(), (128, 128), "PICO-8 drew at another size");
                return fr.to_vec();
            }
            assert!(Instant::now() < deadline, "no picture from PICO-8");
            self.step();
        }
    }
}

fn at(px: &[u8], x: u32, y: u32) -> [u8; 3] {
    let o = ((y * OUT_W + x) * 4) as usize;
    [px[o], px[o + 1], px[o + 2]]
}

fn differ(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| p[..3] != q[..3])
        .count()
}

#[test]
fn a_pico8_cart_plays_at_480_and_resumes_after_an_eject() {
    let Some(s) = setup() else {
        eprintln!("PICO8_DIR and PICO8_CORE_DIR are not set: nothing checked");
        return;
    };
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(c) = Compositor::new(&surface) else {
        return;
    };
    let _lock = common::core_lock();
    // A GBA cart beside it: a card of one cart seats it at boot, and the shelf is the point.
    let d = common::tmp_root_with_carts(&["Emerald"]);
    let root = d.path();
    let stem = card(root, &s);
    clocked(root);
    // The player's switch for the trace: the core appends how PICO-8 kept up to it.
    let trace = root.join(slot::latency::TRACE_FILE);
    std::fs::write(&trace, "").unwrap();

    let mut r = Run {
        f: Frontend::boot(Box::new(SimPlatform::at(root.to_path_buf()))),
        c,
        input: Script(VecDeque::new()),
    };
    r.f.upload_faces(&mut r.c);
    for _ in 0..30 {
        r.step();
    }
    assert!(matches!(r.f.session().app().phase(), Phase::Shelf));
    r.shot("shelf");

    // One to the right of the GBA cart, in, and running: the dots move.
    r.event(RawEvent::Down(slot_input::Btn::Right));
    r.event(RawEvent::Up(slot_input::Btn::Right));
    for _ in 0..30 {
        r.step();
    }
    r.shot("shelf-picked");
    r.event(RawEvent::Down(slot_input::Btn::A));
    r.event(RawEvent::Up(slot_input::Btn::A));
    r.until("the cart to seat", 20, |p| {
        matches!(p, Phase::Playing { .. })
    });
    let first = r.game();
    for _ in 0..240 {
        r.step();
    }
    let later = r.game();
    assert!(differ(&first, &later) > 100, "the picture did not move");

    // 480x480 in the middle of the 640x480 panel: the cart's own background (cls(1), PICO-8's
    // dark blue) reaches the left and right edges of that square and stops there.
    core_png(&later, "core");
    // `game` took the latest picture for itself, so one more frame for the compositor to draw.
    r.step();
    let px = r.shot("playing");
    let blue = [0x1d, 0x2b, 0x53];
    for y in [4, 240, 475] {
        assert_eq!(at(&px, 84, y), blue, "x=84 y={y} is not the game");
        assert_eq!(at(&px, 555, y), blue, "x=555 y={y} is not the game");
        assert_ne!(
            at(&px, 76, y),
            blue,
            "x=76 y={y} is the game: wider than 480"
        );
        assert_ne!(
            at(&px, 563, y),
            blue,
            "x=563 y={y} is the game: wider than 480"
        );
    }

    // MENU held ejects, and the state is on the card before the cart is out.
    for _ in 0..30 {
        r.step();
    }
    let ejected = r.game();
    core_png(&ejected, "ejected");
    r.event(RawEvent::Down(slot_input::Btn::Menu));
    r.until("the eject", 60, |p| !matches!(p, Phase::Playing { .. }));
    r.event(RawEvent::Up(slot_input::Btn::Menu));
    let ring = StateRing::new(root, Platform::Pico8, Core::Pico8, &stem);
    let resume = ring.read_resume().expect("read resume");
    let resume = resume.expect("no resume state after the eject");
    println!("resume state: {} bytes", resume.len());
    r.until("the shelf", 20, |p| matches!(p, Phase::Shelf));
    assert!(!r.f.session().has_core(), "PICO-8 outlived the cart");

    // Back in: the first picture is where it was left, not PICO-8 starting over.
    r.event(RawEvent::Down(slot_input::Btn::A));
    r.event(RawEvent::Up(slot_input::Btn::A));
    r.until("the cart to seat again", 60, |p| {
        matches!(p, Phase::Playing { .. })
    });
    for _ in 0..10 {
        r.step();
    }
    let resumed = r.game();
    core_png(&resumed, "resumed-core");
    r.shot("resumed");
    let from_save = differ(&resumed, &ejected);
    let from_start = differ(&resumed, &first);
    println!("pixels unlike the eject: {from_save}, unlike a fresh start: {from_start}");
    assert!(
        from_save < from_start,
        "the cart started over instead of resuming"
    );

    // SELECT+R1 saves into the ring like any other game, just more slowly.
    r.event(RawEvent::Down(slot_input::Btn::Select));
    r.event(RawEvent::Down(slot_input::Btn::R1));
    r.event(RawEvent::Up(slot_input::Btn::R1));
    r.event(RawEvent::Up(slot_input::Btn::Select));
    let deadline = Instant::now() + Duration::from_secs(30);
    while ring.list().expect("list the ring").is_empty() {
        assert!(Instant::now() < deadline, "SELECT+R1 saved nothing");
        r.step();
    }
    assert!(matches!(r.f.session().app().phase(), Phase::Playing { .. }));

    let traced = std::fs::read_to_string(&trace).unwrap();
    print!("{traced}");
    assert!(
        traced.lines().any(|l| l.starts_with("pico8: ")),
        "the core traced nothing to the card"
    );
}
