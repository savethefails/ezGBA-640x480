use slot_input::RawEvent::{Down, Up};
use slot_input::{
    Action::*, Btn, Btn::*, Gestures, RawEvent, MENU_HOLD_MS, POWER_HOLD_MS, SELECT_CHORD_MS,
    SELECT_TAP_MS, VOLUME_REPEAT_DELAY_MS, VOLUME_REPEAT_MS,
};

/// SELECT is the game's on the press. ezGBA has no SELECT gesture to wait for, and the chords
/// arm off the same hold behind it.
#[test]
fn select_reaches_the_game_on_the_press() {
    let mut g = Gestures::new();
    assert_eq!(g.feed(Down(Select), 0), vec![GbaDown(Select)]);
    assert!(
        g.tick(SELECT_CHORD_MS + 1).is_empty(),
        "a second press came later"
    );
    assert_eq!(g.feed(Up(Select), 2_000), vec![GbaUp(Select)]);
}

/// A chord's second key is the chord's alone, on both its edges. SELECT itself has already
/// reached the game by then, and its release follows it there.
#[test]
fn a_select_chord_keeps_the_second_key_from_the_game() {
    let mut g = Gestures::new();
    assert_eq!(g.feed(Down(Select), 0), vec![GbaDown(Select)]);
    assert_eq!(g.feed(Down(R1), 60), vec![SaveState]);
    assert!(
        g.feed(Up(R1), 100).is_empty(),
        "R1's release reached the game"
    );
    assert_eq!(g.feed(Up(Select), 120), vec![GbaUp(Select)]);
}

#[test]
fn select_chords_map_to_all_four_axes() {
    for (btn, want) in [
        (Btn::Up, BrightnessUp),
        (Btn::Down, BrightnessDown),
        (Right, BlueLightUp),
        (Left, BlueLightDown),
    ] {
        let mut g = Gestures::new();
        g.feed(RawEvent::Down(Select), 0);
        assert_eq!(g.feed(RawEvent::Down(btn), 10), vec![want]);
    }
}

#[test]
fn a_select_held_past_the_tap_minimum_lets_go_on_its_own_release() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    assert_eq!(g.feed(Up(Select), SELECT_TAP_MS + 10), vec![GbaUp(Select)]);
    assert!(g.tick(1_000).is_empty());
}

/// A tap of MENU opens the quick menu, on the release. The other two MENU gestures are
/// unchanged: a tap was the one press this button did not already mean something by.
#[test]
fn menu_single_tap_opens_the_quick_menu() {
    let mut g = Gestures::new();
    assert!(g.feed(Down(Menu), 0).is_empty(), "acted on the press");
    assert_eq!(g.feed(Up(Menu), 100), vec![QuickMenu]);
    assert!(g.tick(451).is_empty(), "fired a second time on the timer");
}

/// The hold is an eject and nothing else. A press long enough to eject is not also a tap, or
/// letting go of one would drop the quick menu over the shelf the cart just came back to.
#[test]
fn a_menu_hold_is_not_also_a_tap() {
    let mut g = Gestures::new();
    g.feed(Down(Menu), 0);
    assert_eq!(g.tick(MENU_HOLD_MS), vec![Eject]);
    assert!(g.feed(Up(Menu), MENU_HOLD_MS + 50).is_empty());
}

#[test]
fn menu_double_tap_opens_polaroids_immediately() {
    let mut g = Gestures::new();
    g.feed(Down(Menu), 0);
    g.feed(Up(Menu), 100);
    assert_eq!(g.feed(Down(Menu), 300), vec![Polaroids]);
}

#[test]
fn menu_hold_ejects_at_the_hold_time_and_not_before() {
    let mut g = Gestures::new();
    g.feed(Down(Menu), 0);
    assert!(g.tick(MENU_HOLD_MS - 1).is_empty());
    assert_eq!(g.tick(MENU_HOLD_MS), vec![Eject]);
}

#[test]
fn menu_hold_released_early_ejects_nothing() {
    let mut g = Gestures::new();
    g.feed(Down(Menu), 0);
    g.feed(Up(Menu), MENU_HOLD_MS / 2);
    assert!(g.tick(3000).is_empty());
}

#[test]
fn l2_dims_and_r2_brightens_one_step_per_press() {
    let mut g = Gestures::new();
    // One shot on the press, same as the chord-driven brightness keys: nothing on
    // release, and neither reaches the game either way (mask.rs maps neither button).
    assert_eq!(g.feed(Down(L2), 0), vec![BrightnessDown]);
    assert!(g.feed(Up(L2), 50).is_empty());
    assert_eq!(g.feed(Down(R2), 1000), vec![BrightnessUp]);
    assert!(g.feed(Up(R2), 1050).is_empty());
    // Repeated presses step again each time, exactly like mashing the chord would.
    assert_eq!(g.feed(Down(R2), 2000), vec![BrightnessUp]);
    assert!(g.feed(Up(R2), 2050).is_empty());
}

/// The flush hangs off the press, because a button being held may be cut by the PMIC before
/// there is any release to see. The lock waits for the release, so that a press on its way
/// to becoming a hold does not darken the panel on the way through.
#[test]
fn power_flushes_on_the_press_and_locks_on_the_release() {
    let mut g = Gestures::new();
    assert_eq!(g.feed(Down(Btn::Power), 0), vec![PowerPress]);
    assert_eq!(g.feed(Up(Btn::Power), 80), vec![PowerTap]);
}

/// The hold arms while the button is still down and the release commits, so the shutdown
/// screen is on the panel for as long as the user holds rather than flashing past.
#[test]
fn power_held_past_the_threshold_fires_the_hold_once() {
    let mut g = Gestures::new();
    g.feed(Down(Btn::Power), 0);
    assert!(g.tick(POWER_HOLD_MS - 1).is_empty());
    assert_eq!(g.tick(POWER_HOLD_MS), vec![PowerHold]);
    assert!(g.tick(4000).is_empty(), "the hold fires once, not per tick");
    assert_eq!(g.feed(Up(Btn::Power), 4500), vec![PowerOff]);
}

/// Three seconds: long enough that a press which merely lingers is still a doze, and well short
/// of the PMIC's own six second cutoff.
#[test]
fn the_hold_is_three_seconds() {
    assert_eq!(POWER_HOLD_MS, 3000);
    let mut g = Gestures::new();
    g.feed(Down(Btn::Power), 0);
    assert!(g.tick(2000).is_empty(), "two seconds is not a hold");
    assert_eq!(g.feed(Up(Btn::Power), 2000), vec![PowerTap], "it is a doze");
}

/// And a release that never reached the threshold is a lock, never a shutdown.
#[test]
fn a_press_just_short_of_the_threshold_is_a_lock() {
    let mut g = Gestures::new();
    g.feed(Down(Btn::Power), 0);
    assert!(g.tick(POWER_HOLD_MS - 1).is_empty());
    assert_eq!(g.feed(Up(Btn::Power), POWER_HOLD_MS - 1), vec![PowerTap]);
}

/// The chord window is still generous: 120 ms was not enough time to land a chord's second key.
#[test]
fn the_chord_window_is_still_generous() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    assert_eq!(g.feed(Down(R1), 400), vec![SaveState]);
}

/// Past the window, the second key is the game's.
#[test]
fn a_key_after_the_chord_window_is_the_games() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    assert_eq!(g.feed(Down(R1), SELECT_CHORD_MS + 1), vec![GbaDown(R1)]);
}

#[test]
fn a_chord_landing_late_is_still_a_chord() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    g.tick(400);
    assert_eq!(g.feed(Down(R1), 400), vec![SaveState]);
    assert!(
        g.tick(5_000).is_empty(),
        "SELECT leaked to the game after a late chord"
    );
}

/// A chord that fired keeps the hold chording, so a held SELECT ramps brightness past the window.
#[test]
fn a_chorded_hold_keeps_chording_past_the_window() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    assert_eq!(g.feed(Down(Btn::Up), 100), vec![BrightnessUp]);
    g.feed(Up(Btn::Up), 200);
    assert_eq!(
        g.feed(Down(Btn::Up), SELECT_CHORD_MS + 300),
        vec![BrightnessUp]
    );
}

/// Down and up in one batch net out to nothing: the mask is set and cleared before the core ever
/// reads it. So a tap shorter than `SELECT_TAP_MS` keeps SELECT down until the tick lets go.
#[test]
fn a_select_tap_is_held_long_enough_for_the_core_to_see_it() {
    let mut g = Gestures::new();
    assert_eq!(g.feed(Down(Select), 0), vec![GbaDown(Select)]);
    assert!(
        g.feed(Up(Select), 10).is_empty(),
        "press and release in the same frame"
    );
    assert!(
        g.tick(SELECT_TAP_MS - 1).is_empty(),
        "released before the core could poll it"
    );
    assert_eq!(g.tick(SELECT_TAP_MS), vec![GbaUp(Select)]);
}

/// A second press while the first tap's release is still owed takes it over: the pad stays
/// down, with no release and press in between for the core to miss.
#[test]
fn a_press_inside_a_taps_hold_continues_it() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    g.feed(Up(Select), 10);
    assert!(g.feed(Down(Select), 20).is_empty());
    assert!(
        g.tick(SELECT_TAP_MS + 10).is_empty(),
        "the held press was let go"
    );
    assert_eq!(g.feed(Up(Select), 500), vec![GbaUp(Select)]);
}

#[test]
fn both_volume_keys_together_mute_once() {
    let mut g = Gestures::new();
    g.feed(Down(VolUp), 0);
    let out = g.feed(Down(VolDown), 80);
    assert!(out.contains(&MuteToggle), "the pair did not mute");
    assert!(
        g.tick(400).is_empty(),
        "it kept firing while both were held"
    );
}

#[test]
fn volume_keys_far_apart_are_not_a_chord() {
    let mut g = Gestures::new();
    g.feed(Down(VolUp), 0);
    let out = g.feed(Down(VolDown), 400);
    assert!(!out.contains(&MuteToggle), "two separate presses muted");
}

/// The app rolls the chord's own two presses back, so it has to see them before it sees the
/// chord. Reversed, the second press would move the level after the mute remembered it.
#[test]
fn the_chord_arrives_behind_the_press_that_completed_it() {
    let mut g = Gestures::new();
    g.feed(Down(VolUp), 0);
    assert_eq!(g.feed(Down(VolDown), 80), vec![VolumeDown, MuteToggle]);
}

/// Unmuting is the same gesture, so a pair that never rearmed would be a one way trip.
#[test]
fn releasing_both_rearms_the_chord() {
    let mut g = Gestures::new();
    g.feed(Down(VolUp), 0);
    g.feed(Down(VolDown), 80);
    g.feed(Up(VolUp), 200);
    g.feed(Up(VolDown), 220);
    g.feed(Down(VolUp), 1_000);
    assert!(g.feed(Down(VolDown), 1_050).contains(&MuteToggle));
}

/// A held volume key ramps. One step per press would mean tapping a dozen times to cross the
/// range, and the press already records when it went down for exactly this.
#[test]
fn a_held_volume_key_repeats() {
    let mut g = Gestures::new();
    assert_eq!(g.feed(Down(VolUp), 0), vec![VolumeUp]);
    assert!(
        g.tick(VOLUME_REPEAT_DELAY_MS - 1).is_empty(),
        "it repeated before the ramp was due"
    );
    assert_eq!(g.tick(VOLUME_REPEAT_DELAY_MS), vec![VolumeUp]);
    assert!(g.tick(VOLUME_REPEAT_DELAY_MS + 1).is_empty());
    assert_eq!(
        g.tick(VOLUME_REPEAT_DELAY_MS + VOLUME_REPEAT_MS),
        vec![VolumeUp]
    );
}

#[test]
fn a_released_volume_key_stops_repeating() {
    let mut g = Gestures::new();
    g.feed(Down(VolDown), 0);
    assert_eq!(g.tick(VOLUME_REPEAT_DELAY_MS), vec![VolumeDown]);
    assert!(g.feed(Up(VolDown), VOLUME_REPEAT_DELAY_MS + 10).is_empty());
    assert!(
        g.tick(VOLUME_REPEAT_DELAY_MS * 4).is_empty(),
        "a key nobody is holding kept ramping"
    );
}

/// Both keys held is the mute chord, not two levels moving at once. The ramp would fight the
/// mute it just fired and leave the level somewhere nobody asked for.
#[test]
fn the_mute_chord_does_not_ramp() {
    let mut g = Gestures::new();
    g.feed(Down(VolUp), 0);
    assert!(g.feed(Down(VolDown), 50).contains(&MuteToggle));
    assert!(
        g.tick(VOLUME_REPEAT_DELAY_MS * 3).is_empty(),
        "the mute chord ramped the volume while it was held"
    );
}

/// A second press after the ramp starts from the top again, rather than inheriting the pace
/// of the press before it.
#[test]
fn each_press_starts_its_own_ramp() {
    let mut g = Gestures::new();
    g.feed(Down(VolUp), 0);
    g.tick(VOLUME_REPEAT_DELAY_MS);
    g.feed(Up(VolUp), VOLUME_REPEAT_DELAY_MS + 5);
    assert_eq!(g.feed(Down(VolUp), 5_000), vec![VolumeUp]);
    assert!(
        g.tick(5_000 + VOLUME_REPEAT_DELAY_MS - 1).is_empty(),
        "the new press repeated early"
    );
    assert_eq!(g.tick(5_000 + VOLUME_REPEAT_DELAY_MS), vec![VolumeUp]);
}

/// X on its own is the game's X. A chord key is only a chord while SELECT is down.
#[test]
fn x_without_select_is_still_the_games_x() {
    let mut g = Gestures::new();
    assert_eq!(g.feed(Down(X), 0), vec![GbaDown(Btn::X)]);
    assert_eq!(g.feed(Up(X), 40), vec![GbaUp(Btn::X)]);
}

/// SELECT+MENU, which is what opens the in-game menu. One gesture, not two: it fires on the
/// MENU press and the release says nothing at all.
#[test]
fn select_and_menu_open_the_in_game_menu() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    assert_eq!(g.feed(Down(Menu), 50), vec![GameMenu]);
    assert!(
        g.feed(Up(Menu), 150).is_empty(),
        "the release landed as a second gesture on top of the menu"
    );
    assert_eq!(g.feed(Up(Select), 200), vec![GbaUp(Select)]);
}

/// The chorded press must not also arm the eject hold, or reading the menu with the buttons
/// still down would eject the cart out from under it.
#[test]
fn a_chorded_menu_never_arms_the_eject_hold() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    assert_eq!(g.feed(Down(Menu), 50), vec![GameMenu]);
    assert!(
        g.tick(50 + MENU_HOLD_MS + 1).is_empty(),
        "holding the menu open ejected the cart"
    );
}

/// The whole test is the ordering inside `menu_down`. Behind the double tap check, a
/// SELECT+MENU that follows a recent MENU tap opens the switcher instead of the menu.
#[test]
fn a_chord_after_a_recent_menu_tap_is_still_the_menu() {
    let mut g = Gestures::new();
    g.feed(Down(Menu), 0);
    assert_eq!(g.feed(Up(Menu), 100), vec![QuickMenu]);
    g.feed(Down(Select), 150);
    assert_eq!(
        g.feed(Down(Menu), 200),
        vec![GameMenu],
        "a recent tap turned the chord into the switcher"
    );
}

/// The difference between a chord and a trap. Past the chord window, MENU under a held SELECT
/// is an ordinary MENU.
#[test]
fn menu_under_a_select_held_past_the_window_is_not_the_menu() {
    let mut g = Gestures::new();
    g.feed(Down(Select), 0);
    assert!(
        g.feed(Down(Menu), 700).is_empty(),
        "a SELECT held past the window still chorded"
    );
    assert_eq!(g.feed(Up(Menu), 800), vec![QuickMenu]);
}

/// The chord clears both of MENU's own windows, so the press after it has to start its own.
///
/// It has to begin with a real tap, or the half of that which matters is invisible: the tap
/// leaves a double tap window open, the chord lands inside it, and a chord that does not
/// close that window leaves the very next MENU tap opening the switcher instead of the quick
/// menu — a press whose meaning depends on a chord two presses ago.
#[test]
fn the_menu_button_still_works_after_a_chord() {
    let mut g = Gestures::new();
    g.feed(Down(Menu), 0);
    assert_eq!(g.feed(Up(Menu), 100), vec![QuickMenu]);
    g.feed(Down(Select), 150);
    assert_eq!(g.feed(Down(Menu), 200), vec![GameMenu]);
    assert!(g.feed(Up(Menu), 250).is_empty());
    assert_eq!(g.feed(Up(Select), 260), vec![GbaUp(Select)]);
    assert!(
        g.feed(Down(Menu), 400).is_empty(),
        "the tap before the chord was still standing as half of a double tap"
    );
    assert_eq!(
        g.feed(Up(Menu), 450),
        vec![QuickMenu],
        "the chord left the menu button dead"
    );
}
