use std::path::PathBuf;
use std::time::{Duration, Instant};

use slot::frontend::Frontend;
use slot::input::DeviceInput;
use slot_gfx::{Compositor, FbdevSurface, Surface};
use slot_power::DevicePlatform;

/// Where BaseOS mounts the card slot has never been checked against a running device, so
/// `launch.sh` exports `SLOT_ROOT` and this is only what is left if it did not.
const CARD: &str = "/mnt/sdcard";

/// The panel is 60 Hz and EGL is asked to lock to it, but a driver that ignores the swap
/// interval would spin this loop as fast as the GPU can clear, so the frame is timed too.
const FRAME: Duration = Duration::from_micros(16_667);

pub fn run() {
    let root = std::env::var_os("SLOT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(CARD));
    let mut surface = match FbdevSurface::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("slot: {e}");
            return;
        }
    };
    let mut compositor = match Compositor::new(&surface) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("slot: {e}");
            return;
        }
    };
    let platform = DevicePlatform::new(root.clone());
    eprintln!("slot: {}", platform.report());
    platform.trace_boot();
    let mut frontend = Frontend::boot(Box::new(platform));
    frontend.upload_faces(&mut compositor);
    // Before the input opens, so the key scan rate it sets is in the trace.
    slot::latency::start(&root, slot_store::Theme::read(&root).runahead.unwrap_or(1));
    let mut input = DeviceInput::open(&root);
    let mut boot_picture_done = false;
    loop {
        let began = Instant::now();
        frontend.render(&mut compositor, surface.window_size());
        let swap = Instant::now();
        if let Err(e) = surface.swap() {
            eprintln!("slot: {e}");
            return;
        }
        slot::latency::shown(swap.elapsed().as_micros() as u64);
        frontend.advance(&mut input);
        frontend.frame_shown();
        if frontend.restarting() || frontend.powering_off() {
            // Once, before either: the shutdown screen is already on the panel and stays there
            // while this writes. Bounded, and never in the way of the power off itself.
            if !boot_picture_done {
                slot::boot_picture::on_power_off(&root, frontend.take_scene());
                boot_picture_done = true;
            }
        }
        if frontend.restarting() {
            frontend.restart();
        }
        if frontend.powering_off() {
            frontend.poweroff();
            return;
        }
        if let Some(left) = FRAME.checked_sub(began.elapsed()) {
            std::thread::sleep(left);
        }
    }
}
