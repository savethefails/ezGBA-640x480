//! The frame the next boot opens on, through the real frontend: composed on the GPU and read
//! back on the edge where the screen is covered, never before it and never again after.

#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::VecDeque;

use std::time::Duration;

use common::{clocked, tmp_root_with_carts};
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface};
use slot_input::{Btn, InputSource, Millis, RawEvent};
use slot_power::SimPlatform;

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

#[test]
fn the_frame_before_a_doze_is_the_one_kept_for_the_next_boot() {
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());

    // The shelf, settled. Nothing is read while nothing covers it.
    for _ in 0..30 {
        f.compose(&mut c);
        f.advance(&mut input);
    }
    assert_eq!(
        f.take_scene(),
        None,
        "a frame was read with nothing over the screen"
    );
    f.compose(&mut c);
    // What is kept is that frame marked as a start: the pill, over the open space on the shelf.
    let mut shelf = c.read_frame();
    slot_ui::stamp_starting(&mut shelf, slot_ui::PillAt::Shelf);

    // POWER dozes: the panel goes dark. The frame before it is the shelf.
    input.0.push_back(vec![RawEvent::Down(Btn::Power)]);
    f.advance(&mut input);
    input.0.push_back(vec![RawEvent::Up(Btn::Power)]);
    f.advance(&mut input);
    f.compose(&mut c);
    let kept = f.take_scene().expect("nothing kept at the doze");
    assert_eq!(kept.len(), shelf.len());
    assert!(
        kept == shelf,
        "what was kept is not the shelf as it was last drawn"
    );
    // To look at: `SCRATCH_PNG_DIR=/tmp cargo test -p slot --test render_boot_picture`
    // writes the bootlogo.bmp this power off would leave behind.
    if let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") {
        let logo = std::path::Path::new(&dir).join("bootlogo.bmp");
        let mut bmp = vec![0u8; 54 + 640 * 480 * 3];
        bmp[0..2].copy_from_slice(b"BM");
        let len = bmp.len() as u32;
        bmp[2..6].copy_from_slice(&len.to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&640i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&480i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        std::fs::write(&logo, bmp).unwrap();
        let backup = std::path::Path::new(&dir).join("bootlogo-baseos.bmp");
        let _ = std::fs::remove_file(&backup);
        slot::boot_picture::apply(
            &logo,
            &backup,
            &slot::boot_picture::Want::Scene(kept.clone()),
        )
        .expect("paint");
        println!("wrote {}", logo.display());
    }

    // Read once, on the edge: frames composed while dozing read nothing more.
    for _ in 0..10 {
        f.compose(&mut c);
        f.advance(&mut input);
    }
    assert_eq!(
        f.take_scene(),
        None,
        "read again while the screen stayed covered"
    );
}

/// Settles a fresh shelf, covers it with `cover`, and answers the frame drawn just before the
/// cover alongside the frame that was kept.
fn kept_through(cover: fn(&mut Frontend, &mut Script)) -> Option<(Vec<u8>, Vec<u8>)> {
    let surface = HeadlessSurface::new().ok()?;
    let mut c = Compositor::new(&surface).ok()?;
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    clocked(d.path());
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    for _ in 0..30 {
        f.compose(&mut c);
        f.advance(&mut input);
    }
    f.compose(&mut c);
    let mut before = c.read_frame();
    slot_ui::stamp_starting(&mut before, slot_ui::PillAt::Shelf);
    cover(&mut f, &mut input);
    f.compose(&mut c);
    // `None` only for a machine with no GPU to compose on: a cover that read nothing fails.
    let kept = f
        .take_scene()
        .expect("nothing was kept: the screen was never covered");
    Some((before, kept))
}

#[test]
fn a_closed_lid_keeps_the_frame_before_it() {
    let Some((before, kept)) = kept_through(|f, input| {
        input.0.push_back(vec![RawEvent::Down(Btn::Lid)]);
        f.advance(input);
    }) else {
        return;
    };
    assert!(kept == before, "the lid kept some other frame");
}

/// A held POWER sends a press on the way down, which only saves, and starts the shutdown once
/// it has been held three seconds. The frame kept is the one from before the shutdown screen.
#[test]
fn a_held_power_keeps_the_frame_before_the_shutdown() {
    let Some((before, kept)) = kept_through(|f, input| {
        input.0.push_back(vec![RawEvent::Down(Btn::Power)]);
        f.advance(input);
        std::thread::sleep(Duration::from_millis(slot_input::POWER_HOLD_MS + 100));
        f.advance(input);
    }) else {
        return;
    };
    assert!(kept == before, "the hold kept some other frame");
}
