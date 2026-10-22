mod common;

use std::time::{Duration, Instant};

use common::{session_with_platform, tmp_root_with_carts, tmp_root_with_real_carts};
use slot::app::Phase;
use slot::session::Session;
use slot_input::{Btn, Millis, RawEvent, MENU_HOLD_MS, POWER_HOLD_MS};
use slot_store::{write_slot_state, SlotState};

const FRAME_MS: Millis = 16;
const DT: f32 = 1.0 / 60.0;

/// `RETRO_RUMBLE_STRONG`, which is what the core passes.
const STRONG: u32 = 0;

/// The whole point of the interface: what the core asks for reaches the motor. Neither the
/// mock nor the test rom drives a rumble register, so this writes the cell the core's
/// callback writes.
#[test]
fn what_the_core_asks_for_reaches_the_motor() {
    // Two, because a single cart has nowhere to eject to and the slot refuses.
    let d = tmp_root_with_real_carts(&["Advance Wars", "Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    let mut now = 0;
    play(&mut s, &mut now);
    s.core_rumble()
        .expect("a seated cart has a core")
        .set(0, STRONG, u16::MAX);
    step(&mut s, &mut now);
    assert_eq!(motor.last(), u16::MAX, "the core asked and nothing moved");

    // The core is never told to stop. The phase is what takes the motor down, and it has to
    // keep it down for every frame the cart is still on its way out.
    let pressed = now;
    event(&mut s, RawEvent::Down(Btn::Menu), &mut now);
    while now < pressed + MENU_HOLD_MS + FRAME_MS {
        step(&mut s, &mut now);
    }
    assert_eq!(motor.last(), 0, "the motor outlived the cart");
    step(&mut s, &mut now);
    assert_eq!(motor.last(), 0, "the next frame turned it back on");
}

/// Rumble Off in the quick menu. The game still asks for the motor as far as the emulator
/// knows, and the motor is never told.
#[test]
fn with_rumble_off_the_motor_stays_still_whatever_the_core_asks() {
    let d = tmp_root_with_real_carts(&["Advance Wars", "Emerald"]);
    write_slot_state(
        d.path(),
        &SlotState {
            rumble: false,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    let (mut s, motor) = session_with_platform(d.path());
    let mut now = 0;
    play(&mut s, &mut now);
    let core = s.core_rumble().expect("a seated cart has a core");
    core.set(0, STRONG, u16::MAX);
    assert_eq!(
        core.strength(),
        u16::MAX,
        "the core is not asking, so this test proves nothing"
    );
    for _ in 0..5 {
        step(&mut s, &mut now);
    }
    assert_eq!(motor.last(), 0, "the motor moved with rumble off");
}

/// An unplugged cart leaving the motor on would run until the battery died.
#[test]
fn ejecting_stops_the_motor() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    s.rumble(u16::MAX);
    assert_ne!(motor.last(), 0);
    s.feed([RawEvent::Down(Btn::Menu)], 0);
    s.feed([], MENU_HOLD_MS + 1);
    assert_eq!(
        motor.last(),
        0,
        "the motor kept running after the cart came out"
    );
}

/// Same reasoning as the lid: nothing should be buzzing while the device is asleep.
#[test]
fn dozing_stops_the_motor() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    s.rumble(u16::MAX);
    s.feed([RawEvent::Down(Btn::Lid)], 0);
    assert_eq!(motor.last(), 0);
}

/// The power button dozes as well, and on a device with a stuck hall sensor it is the only
/// one of the two that gets there.
#[test]
fn the_power_button_stops_the_motor() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    s.rumble(u16::MAX);
    s.feed([RawEvent::Down(Btn::Power)], 0);
    assert_eq!(motor.last(), 0);
}

fn step(s: &mut Session, now: &mut Millis) {
    *now += FRAME_MS;
    s.feed([], *now);
    s.update(DT);
}

fn event(s: &mut Session, ev: RawEvent, now: &mut Millis) {
    *now += FRAME_MS;
    s.feed([ev], *now);
    s.update(DT);
}

/// Puts the selected cart in and waits out the load, which happens on its own thread.
fn play(s: &mut Session, now: &mut Millis) {
    event(s, RawEvent::Down(Btn::A), now);
    event(s, RawEvent::Up(Btn::A), now);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(s.app().phase(), Phase::Playing { .. }) {
        assert!(Instant::now() < deadline, "the cart never seated");
        step(s, now);
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A cart that was buzzing as the button went down must not buzz on through the shutdown:
/// `poweroff` ends the process with `exit`, and `Motor`'s own destructor never runs to put it
/// down.
#[test]
fn a_held_power_takes_the_motor_down() {
    let d = tmp_root_with_real_carts(&["Advance Wars", "Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    let mut now = 0;
    play(&mut s, &mut now);
    s.core_rumble()
        .expect("a seated cart has a core")
        .set(0, STRONG, u16::MAX);
    step(&mut s, &mut now);
    assert_eq!(
        motor.last(),
        u16::MAX,
        "the motor should be running, or this test proves nothing"
    );

    let pressed = now;
    event(&mut s, RawEvent::Down(Btn::Power), &mut now);
    while now < pressed + POWER_HOLD_MS + FRAME_MS {
        step(&mut s, &mut now);
    }
    assert!(s.app().powering_off(), "the hold did not power off");
    assert_eq!(
        motor.last(),
        0,
        "the cart kept buzzing through the shutdown"
    );
}

#[test]
fn a_short_tap_runs_the_motor_long_enough_to_feel() {
    let d = tmp_root_with_real_carts(&["Advance Wars", "Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    let mut now = 0;
    play(&mut s, &mut now);
    let core = s.core_rumble().expect("a seated cart has a core").clone();
    core.set(0, STRONG, 20_000);
    step(&mut s, &mut now);
    let start = s.app().now();
    assert_eq!(motor.last(), 20_000);

    core.set(0, STRONG, 0);
    while s.app().now() + 2 * FRAME_MS < start + slot::session::RUMBLE_MIN_MS {
        step(&mut s, &mut now);
        let held = s.app().now() - start;
        assert_eq!(
            motor.last(),
            20_000,
            "a tap stopped the motor after {held} ms"
        );
    }
    core.set(0, STRONG, 9_000);
    step(&mut s, &mut now);
    assert_eq!(motor.last(), 9_000, "the next pulse did not take over");

    core.set(0, STRONG, 0);
    step(&mut s, &mut now);
    assert_eq!(
        motor.last(),
        0,
        "the motor kept running after the game stopped asking"
    );
}

#[test]
fn a_long_pulse_stops_the_frame_the_game_stops_asking() {
    let d = tmp_root_with_real_carts(&["Advance Wars", "Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    let mut now = 0;
    play(&mut s, &mut now);
    let core = s.core_rumble().expect("a seated cart has a core").clone();
    core.set(0, STRONG, u16::MAX);
    step(&mut s, &mut now);
    let start = s.app().now();
    while s.app().now() < start + slot::session::RUMBLE_MIN_MS + FRAME_MS {
        step(&mut s, &mut now);
        assert_eq!(motor.last(), u16::MAX);
    }

    core.set(0, STRONG, 0);
    step(&mut s, &mut now);
    assert_eq!(
        motor.last(),
        0,
        "a buzz ran on past the game's own, so every Pinball hit feels too long"
    );
}

#[test]
fn a_buzz_is_cut_off_at_the_longest_the_motor_runs() {
    let d = tmp_root_with_real_carts(&["Advance Wars", "Emerald"]);
    let (mut s, motor) = session_with_platform(d.path());
    let mut now = 0;
    play(&mut s, &mut now);
    let core = s.core_rumble().expect("a seated cart has a core").clone();
    core.set(0, STRONG, u16::MAX);
    step(&mut s, &mut now);
    let start = s.app().now();
    while s.app().now() + FRAME_MS < start + slot::session::RUMBLE_MAX_MS {
        step(&mut s, &mut now);
        assert_eq!(motor.last(), u16::MAX, "the buzz was cut short");
    }
    while s.app().now() < start + 2 * slot::session::RUMBLE_MAX_MS {
        step(&mut s, &mut now);
        let held = s.app().now() - start;
        assert_eq!(
            motor.last(),
            0,
            "Pinball's GameCube-length pulse still ran {held} ms on the SP's motor"
        );
    }

    core.set(0, STRONG, 0);
    step(&mut s, &mut now);
    core.set(0, STRONG, u16::MAX);
    step(&mut s, &mut now);
    assert_eq!(motor.last(), u16::MAX, "the next hit never buzzed");
}
