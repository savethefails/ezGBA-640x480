//! When in each refresh the device loop does its work.
//!
//! On the SP the swap returns at the display's latch, once a refresh, whenever it is called: a
//! frame submitted before the latch is on the panel at the next one, and the swap simply waits
//! until then. So the order of the loop is not what sets the delay a press sees; where in the
//! refresh the loop reads the buttons is. Read just after a latch, as it used to, the buttons
//! wait most of a refresh in a finished frame before it is shown (the latency trace measured
//! 10 to 27 ms of that). Read just before the next latch, leaving only the time the work needs,
//! they do not.
//!
//! So the loop sleeps after each latch until the work of a frame will just fit before the next:
//! read the buttons, run the emulator on them, draw, submit. How long that takes is learnt from
//! the loop itself, as the slowest of the last `WINDOW` frames, so a game whose frames cost more
//! (or run-ahead) is woken earlier, and one slow frame (a core loading, a save) is forgotten
//! half a second later instead of costing every frame after it.
//!
//! The panel's period is learnt the same way, from the latches themselves: the SP's is 16.80 ms,
//! not the 16.67 of 60 Hz, and a timer that assumed 60 Hz walked against it.

use std::time::{Duration, Instant};

/// Frames the work estimate looks back over: about half a second.
const WINDOW: usize = 32;

/// Kept between the work finishing and the latch, against a frame that runs a little long.
pub const MARGIN: Duration = Duration::from_micros(2_000);

/// What the SP's panel was measured at, until the loop has measured it itself.
const PERIOD: Duration = Duration::from_micros(16_800);

pub struct Pacer {
    period: Duration,
    last_latch: Option<Instant>,
    works: [Duration; WINDOW],
    next: usize,
    worst: Duration,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Pacer {
    pub fn new() -> Self {
        // Generous until measured: the first frames wake early rather than miss a latch.
        let start = Duration::from_millis(10);
        Pacer {
            period: PERIOD,
            last_latch: None,
            works: [start; WINDOW],
            next: 0,
            worst: start,
        }
    }

    /// The panel's period as learnt so far.
    pub fn period(&self) -> Duration {
        self.period
    }

    /// How long before a latch the work of a frame starts: the slowest recent frame and the
    /// margin.
    pub fn lead(&self) -> Duration {
        self.worst + MARGIN
    }

    /// When to start the next frame's work. `None` before the first latch, and the latch itself
    /// when the work would not fit in a refresh at all, which is the old behaviour: start at once.
    pub fn wake_at(&self) -> Option<Instant> {
        let latch = self.last_latch?;
        Some(latch + self.period.saturating_sub(self.lead()))
    }

    /// A frame was submitted after `work` of reading, running and drawing, and its swap returned
    /// at `latch`.
    pub fn swapped(&mut self, work: Duration, latch: Instant) {
        if let Some(last) = self.last_latch {
            let gap = latch.saturating_duration_since(last);
            // One refresh apart, and not a missed latch or a stall: a slow average of those is
            // the panel's period. Anything else says nothing about the panel.
            if gap > self.period.mul_f64(0.75) && gap < self.period.mul_f64(1.33) {
                self.period = self.period.mul_f64(0.95) + gap.mul_f64(0.05);
            }
        }
        self.last_latch = Some(latch);
        self.works[self.next] = work;
        self.next = (self.next + 1) % WINDOW;
        self.worst = self.works.iter().copied().max().unwrap_or(work);
    }
}
