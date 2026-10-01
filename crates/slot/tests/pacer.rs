use std::time::{Duration, Instant};

use slot::pacer::{Pacer, MARGIN};

const MS: fn(u64) -> Duration = Duration::from_millis;

fn us(n: u64) -> Duration {
    Duration::from_micros(n)
}

/// Fed `frames` latches `gap` apart, each frame having taken `work`. Answers the last latch.
fn run(p: &mut Pacer, from: Instant, frames: u32, gap: Duration, work: Duration) -> Instant {
    let mut t = from;
    for _ in 0..frames {
        t += gap;
        p.swapped(work, t);
    }
    t
}

#[test]
fn nothing_to_wait_for_before_the_first_latch() {
    assert_eq!(Pacer::new().wake_at(), None);
}

/// The work starts as late as it can and still finish before the next latch, with the margin.
#[test]
fn the_work_starts_late_in_the_refresh() {
    let mut p = Pacer::new();
    let last = run(&mut p, Instant::now(), 40, us(16_800), MS(8));
    assert_eq!(p.lead(), MS(8) + MARGIN);
    let wake = p.wake_at().unwrap();
    let before_next = (last + p.period()) - wake;
    assert_eq!(before_next, MS(8) + MARGIN);
}

/// The panel's period is learnt from the latches, and a missed latch or a stall says nothing
/// about it.
#[test]
fn the_period_is_learnt_and_stalls_are_ignored() {
    let mut p = Pacer::new();
    let t = run(&mut p, Instant::now(), 200, us(16_900), MS(5));
    assert!(
        (p.period().as_micros() as i64 - 16_900).abs() < 20,
        "{:?}",
        p.period()
    );
    let before = p.period();
    run(&mut p, t, 1, us(33_800), MS(5)); // a missed latch
    run(&mut p, t + us(33_800), 1, MS(200), MS(5)); // a stall
    assert_eq!(p.period(), before);
}

/// One slow frame wakes the next half second early, and is then forgotten.
#[test]
fn a_slow_frame_is_forgotten() {
    let mut p = Pacer::new();
    let t = run(&mut p, Instant::now(), 40, us(16_800), MS(6));
    let t = run(&mut p, t, 1, us(16_800), MS(12));
    assert_eq!(p.lead(), MS(12) + MARGIN);
    run(&mut p, t, 32, us(16_800), MS(6));
    assert_eq!(p.lead(), MS(6) + MARGIN);
}

/// Work that cannot fit in a refresh starts at once, which is how the loop always ran.
#[test]
fn work_longer_than_a_refresh_starts_at_once() {
    let mut p = Pacer::new();
    let last = run(&mut p, Instant::now(), 40, us(16_800), MS(20));
    assert_eq!(p.wake_at(), Some(last));
}
