//! The latency trace, through its own public calls in the order the four threads make them. One
//! test in its own binary: the trace is process-wide.

use std::time::Duration;

#[test]
fn a_press_is_followed_to_the_panel_and_the_pace_is_summed_up() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join(slot::latency::TRACE_FILE);
    std::fs::write(&log, "").unwrap();
    slot::latency::start(d.path(), 1);

    // A press nobody plays: on the shelf it never reaches the emulator, and is given up on.
    slot::latency::read();
    std::thread::sleep(Duration::from_millis(550));

    // A press that does: each stage in turn, a few milliseconds apart.
    let step = || std::thread::sleep(Duration::from_millis(2));
    slot::latency::read();
    step();
    slot::latency::pad(0, 1);
    step();
    slot::latency::emu_started(1, 1);
    step();
    slot::latency::emu_done(Duration::from_millis(5));
    slot::latency::published();
    step();
    slot::latency::drawn();
    step();
    slot::latency::shown(4_000);

    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("start: slot"), "{text}");
    let press: Vec<&str> = text.lines().filter(|l| l.contains("press:")).collect();
    assert_eq!(press.len(), 1, "{text}");
    let p = press[0];
    assert!(
        p.contains("ahead 1") && p.contains("frame 5.0") && p.contains("swap waited 4.0"),
        "{p}"
    );
    // Measured from the second press, not the abandoned first one.
    let total: f64 = p
        .split("total ")
        .nth(1)
        .unwrap()
        .trim_end_matches(" ms")
        .parse()
        .unwrap();
    assert!((8.0..100.0).contains(&total), "total {total} in {p}");

    // A pace line after every 600 frames shown.
    for _ in 0..600 {
        slot::latency::published();
        slot::latency::drawn();
        slot::latency::shown(9_000);
    }
    let text = std::fs::read_to_string(&log).unwrap();
    let pace: Vec<&str> = text.lines().filter(|l| l.contains("pace:")).collect();
    assert_eq!(pace.len(), 1, "{text}");
    assert!(
        pace[0].contains("600 frames") && pace[0].contains("emulator frame"),
        "{}",
        pace[0]
    );
}
