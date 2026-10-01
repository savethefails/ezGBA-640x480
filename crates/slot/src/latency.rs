//! A trace of how long a button press takes to reach the screen, stage by stage, written to the
//! card for reading afterwards.
//!
//! On when `latency-trace.log` exists at the top of the card (or `SLOT_TRACE_LATENCY` is set to
//! anything but `0`), and lines are appended to that file. The device has no environment to set
//! and its `/tmp` is gone at power off, so a file the player creates, and can take the card out
//! to read, is both the switch and the record. Off, every call here is one load of a flag.
//!
//! Two kinds of line:
//!
//! - `press`: one press, followed through the four threads it crosses. `read` is the input
//!   thread seeing it; `pad` is the button state carrying it being handed to the emulator; `emu`
//!   is the emulator starting the frame that includes it, and `frame` how long that frame took
//!   (run-ahead included); `drawn` is the display picking that frame up; `shown` is the swap that
//!   put it on the panel returning. Presses that arrive while one is still in flight are not
//!   followed: one at a time keeps every number about the same press.
//! - `pace`: every `PACE_FRAMES` displayed frames, the display's real period, how long a swap
//!   waited, what an emulator frame cost, and how long a finished frame sat before the panel
//!   showed it. That last one is the time a later read of the buttons could still have used.
//!
//! What it cannot see: the wait before the kernel reports a press (the key scan, see
//! `input::device::POLL_MS`), the frames a game itself takes to answer one, and the panel's own
//! scan out after the swap.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub const TRACE_FILE: &str = "latency-trace.log";
pub const TRACE_VAR: &str = "SLOT_TRACE_LATENCY";

/// Displayed frames per `pace` line: ten seconds at the SP's refresh.
const PACE_FRAMES: u32 = 600;

static ON: AtomicBool = AtomicBool::new(false);
static OUT: OnceLock<Mutex<File>> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();

// The press being followed: each stage's moment, in microseconds since `EPOCH` plus one, so zero
// is "not yet". Each is set at most once, and only after the one before it.
static READ: AtomicU64 = AtomicU64::new(0);
static PAD: AtomicU64 = AtomicU64::new(0);
static EMU: AtomicU64 = AtomicU64::new(0);
static FRAME_US: AtomicU64 = AtomicU64::new(0);
static PUBLISHED: AtomicU64 = AtomicU64::new(0);
static DRAWN: AtomicU64 = AtomicU64::new(0);
/// The buttons the followed press added, so the emulator frame that carries it can be told from
/// the frames before it.
static BITS: AtomicU16 = AtomicU16::new(0);
static AHEAD: AtomicU8 = AtomicU8::new(0);

// For `pace`: when the emulator last published, and when the frame the display drew was.
static LAST_PUBLISH: AtomicU64 = AtomicU64::new(0);
static DRAWN_PUBLISH: AtomicU64 = AtomicU64::new(0);
static EMU_COST: Mutex<Stat> = Mutex::new(Stat::new());
static PACE: Mutex<Pace> = Mutex::new(Pace::new());

/// Called once at boot with the card. Turns the trace on if the file is there or the variable is
/// set, and writes a line saying what this run is.
pub fn start(root: &Path, runahead: u8) {
    let path = root.join(TRACE_FILE);
    let asked = std::env::var_os(TRACE_VAR).is_some_and(|v| v != "0");
    if !asked && !path.is_file() {
        return;
    }
    let file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("slot: latency trace: {}: {e}", path.display());
            return;
        }
    };
    let _ = OUT.set(Mutex::new(file));
    EPOCH.get_or_init(Instant::now);
    ON.store(true, Ordering::Release);
    line(&format!(
        "start: slot {}, run-ahead {runahead}",
        crate::build_info::Build::current().serial()
    ));
}

fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Microseconds since the trace started, plus one so a stamp is never zero.
fn now() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_micros() as u64 + 1
}

fn line(text: &str) {
    if let Some(out) = OUT.get() {
        let mut f = out.lock().unwrap_or_else(|e| e.into_inner());
        let secs = now() as f64 / 1e6;
        let _ = writeln!(f, "{secs:9.3} {text}");
    }
}

/// A line of its own, for things worth having beside the numbers (the key scan rate, say).
pub fn note(text: &str) {
    if on() {
        line(text);
    }
}

/// Sets `stage` to now, if it is unset and `before` is set.
fn stamp(stage: &AtomicU64, before: &AtomicU64) -> bool {
    before.load(Ordering::Acquire) != 0
        && stage
            .compare_exchange(0, now(), Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
}

/// How long a followed press may sit at one stage before it is given up on. A press on the shelf
/// or in a menu never reaches the emulator, and a paused game never runs it: without this the
/// first such press would hold the trace for good.
const STALE_US: u64 = 500_000;

/// The input thread saw a button go down.
pub fn read() {
    if !on() {
        return;
    }
    let t = now();
    let since = READ.load(Ordering::Acquire);
    if since != 0 && t.saturating_sub(since) > STALE_US {
        for stage in [&READ, &PAD, &EMU, &FRAME_US, &PUBLISHED, &DRAWN] {
            stage.store(0, Ordering::Release);
        }
        BITS.store(0, Ordering::Release);
    }
    let _ = READ.compare_exchange(0, t, Ordering::AcqRel, Ordering::Relaxed);
}

/// The button state handed to the emulator changed from `before` to `after`.
pub fn pad(before: u16, after: u16) {
    let added = after & !before;
    if on() && added != 0 && stamp(&PAD, &READ) {
        BITS.store(added, Ordering::Release);
    }
}

/// The emulator is starting a present's frame with buttons `mask`, `ahead` frames of run-ahead.
pub fn emu_started(mask: u16, ahead: u8) {
    if on() && mask & BITS.load(Ordering::Acquire) != 0 && stamp(&EMU, &PAD) {
        AHEAD.store(ahead, Ordering::Relaxed);
    }
}

/// The emulator finished a present's core frames, which took `cost`.
pub fn emu_done(cost: std::time::Duration) {
    if !on() {
        return;
    }
    let us = cost.as_micros() as u64;
    EMU_COST.lock().unwrap_or_else(|e| e.into_inner()).add(us);
    if EMU.load(Ordering::Acquire) != 0 {
        let _ = FRAME_US.compare_exchange(0, us, Ordering::AcqRel, Ordering::Relaxed);
    }
}

/// The emulator published a frame for the display.
pub fn published() {
    if on() {
        let t = now();
        LAST_PUBLISH.store(t, Ordering::Release);
        stamp(&PUBLISHED, &EMU);
    }
}

/// The display picked up the emulator's latest frame to draw.
pub fn drawn() {
    if on() {
        DRAWN_PUBLISH.store(LAST_PUBLISH.load(Ordering::Acquire), Ordering::Release);
        stamp(&DRAWN, &PUBLISHED);
    }
}

/// The display's swap returned after waiting `swap_us`. Ends the followed press if the frame it
/// was drawn in is the one just shown, and counts the frame for `pace`.
pub fn shown(swap_us: u64) {
    if !on() {
        return;
    }
    let t = now();
    let published = DRAWN_PUBLISH.swap(0, Ordering::AcqRel);
    let idle = (published != 0).then(|| t.saturating_sub(published));
    let pace_line = PACE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .frame(t, swap_us, idle);
    if let Some(text) = pace_line {
        let emu = std::mem::replace(
            &mut *EMU_COST.lock().unwrap_or_else(|e| e.into_inner()),
            Stat::new(),
        );
        line(&format!("{text}, emulator frame {}", emu.summary()));
    }

    if DRAWN.load(Ordering::Acquire) == 0 {
        return;
    }
    let [r, p, e, f, pb, d] =
        [&READ, &PAD, &EMU, &FRAME_US, &PUBLISHED, &DRAWN].map(|s| s.swap(0, Ordering::AcqRel));
    BITS.store(0, Ordering::Release);
    let ms = |from: u64, to: u64| to.saturating_sub(from) as f64 / 1000.0;
    line(&format!(
        "press: read>pad {:.1} pad>emu {:.1} emu>published {:.1} (frame {:.1}, ahead {}) published>drawn {:.1} drawn>shown {:.1} (swap waited {:.1}) total {:.1} ms",
        ms(r, p),
        ms(p, e),
        ms(e, pb),
        f as f64 / 1000.0,
        AHEAD.load(Ordering::Relaxed),
        ms(pb, d),
        ms(d, t),
        swap_us as f64 / 1000.0,
        ms(r, t),
    ));
}

/// A running average and worst case, in microseconds.
struct Stat {
    n: u64,
    sum: u64,
    max: u64,
}

impl Stat {
    const fn new() -> Self {
        Stat {
            n: 0,
            sum: 0,
            max: 0,
        }
    }

    fn add(&mut self, us: u64) {
        self.n += 1;
        self.sum += us;
        self.max = self.max.max(us);
    }

    fn summary(&self) -> String {
        if self.n == 0 {
            return "-".into();
        }
        format!(
            "avg {:.1} max {:.1} ms",
            self.sum as f64 / self.n as f64 / 1000.0,
            self.max as f64 / 1000.0
        )
    }
}

struct Pace {
    frames: u32,
    first: u64,
    last: u64,
    period: Stat,
    swap: Stat,
    idle: Stat,
    /// Swap returns more than one and a half periods apart: a refresh that went by with nothing
    /// new to show.
    late: u32,
}

impl Pace {
    const fn new() -> Self {
        Pace {
            frames: 0,
            first: 0,
            last: 0,
            period: Stat::new(),
            swap: Stat::new(),
            idle: Stat::new(),
            late: 0,
        }
    }

    fn frame(&mut self, t: u64, swap_us: u64, idle: Option<u64>) -> Option<String> {
        if self.last != 0 {
            let gap = t - self.last;
            self.period.add(gap);
            if gap > 25_000 {
                self.late += 1;
            }
        } else {
            self.first = t;
        }
        self.last = t;
        self.swap.add(swap_us);
        if let Some(idle) = idle {
            self.idle.add(idle);
        }
        self.frames += 1;
        if self.frames < PACE_FRAMES {
            return None;
        }
        let mean = (t - self.first) as f64 / (self.frames - 1) as f64 / 1000.0;
        let text = format!(
            "pace: {} frames, period {mean:.2} ms (max {:.1}), {} late, swap waited {}, finished frame waited {} before shown",
            self.frames,
            self.period.max as f64 / 1000.0,
            self.late,
            self.swap.summary(),
            self.idle.summary(),
        );
        *self = Pace::new();
        Some(text)
    }
}
