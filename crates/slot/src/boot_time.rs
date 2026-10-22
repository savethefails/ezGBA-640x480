//! How long the boot and a game's launch take, step by step, in milliseconds since the first
//! mark: `slot: boot: ...` and `slot: launch: ...` lines in the log, which is how the SP's own
//! numbers are read. From slot 8a52c5d and 4b5b4d6.

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn mark(phase: &str) {
    eprintln!("slot: boot: {phase} at {:.1} ms", ms());
}

/// Milliseconds since the first `mark` or `ms`, which `device_app` makes the start of `main`.
pub fn ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1e3
}
