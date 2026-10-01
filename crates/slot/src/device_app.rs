use std::path::PathBuf;
use std::time::{Duration, Instant};

use slot::frontend::Frontend;
use slot::input::DeviceInput;
use slot_gfx::{Compositor, FbdevSurface, Surface};
use slot_power::DevicePlatform;

/// Where BaseOS mounts the card slot has never been checked against a running device, so
/// `launch.sh` exports `SLOT_ROOT` and this is only what is left if it did not.
const CARD: &str = "/mnt/sdcard";

/// The shortest a loop may take, for a driver whose swap returns at once instead of at the
/// latch: without it the loop would spin as fast as the GPU can clear.
const MIN_FRAME: Duration = Duration::from_millis(10);

/// The longest the display waits for the emulator's frame before drawing the last one again: a
/// frame that has not come by then has missed this refresh anyway.
const EMU_WAIT: Duration = Duration::from_millis(14);

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
    let mut pacer = slot::pacer::Pacer::new();
    let mut frames = 0u32;
    // One frame on the panel before the first input is read and the first update runs: a boot
    // that carries the bootloader's picture lights the panel on the first update, and has to have
    // drawn something of its own by then.
    frontend.render(&mut compositor, surface.window_size());
    if let Err(e) = surface.swap() {
        eprintln!("slot: {e}");
        return;
    }
    pacer.swapped(Duration::ZERO, Instant::now());
    loop {
        // Late: asleep through most of the refresh, so the buttons are read just before the
        // latch with only the work of this frame between them and the panel. See `pacer`.
        if let Some(at) = pacer.wake_at() {
            if let Some(left) = at.checked_duration_since(Instant::now()) {
                std::thread::sleep(left);
            }
        }
        let began = Instant::now();
        frontend.advance(&mut input);
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
        // The core runs on the buttons just read, and this frame draws what it made of them.
        let asked = Instant::now();
        let came = frontend.kick_and_wait(EMU_WAIT).is_some();
        // A frame that never came is a core out of step with the kicks for a moment (just
        // started, just unpaused) rather than a frame that costs this much, so the wait is not
        // counted as work: it would wake the next half second of frames early for nothing.
        let stalled = if came {
            Duration::ZERO
        } else {
            asked.elapsed()
        };
        frontend.render(&mut compositor, surface.window_size());
        let submit = Instant::now();
        if let Err(e) = surface.swap() {
            eprintln!("slot: {e}");
            return;
        }
        let latch = Instant::now();
        slot::latency::shown(latch.duration_since(submit).as_micros() as u64);
        pacer.swapped(submit.duration_since(began).saturating_sub(stalled), latch);
        if slot::latency::tracing() {
            frames += 1;
            if frames.is_multiple_of(600) {
                slot::latency::note(&format!(
                    "pacer: panel period {:.2} ms, work starts {:.1} ms before the latch",
                    pacer.period().as_secs_f64() * 1e3,
                    pacer.lead().as_secs_f64() * 1e3
                ));
            }
        }
        if let Some(left) = MIN_FRAME.checked_sub(began.elapsed()) {
            std::thread::sleep(left);
        }
    }
}
