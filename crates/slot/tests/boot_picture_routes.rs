//! Every way the SP goes from a game or the shelf to off, and the one rule the boot picture
//! hangs on: the screen is covered from the first step of the way and stays covered until the
//! device may stop. The frontend reads the frame on the edge where the cover begins, so a
//! route that uncovered the screen part way would read a second, later frame; one that never
//! covered it would read none.

mod common;

use common::{app_playing_in, boot, tmp_root_with_carts};
use slot::app::App;
use slot_input::{Action, Btn};

/// Runs the app on until the power off may happen, asserting the screen stays covered.
fn covered_until_off(a: &mut App, ready: fn(&App) -> bool) {
    for step in 0..120 {
        assert!(a.scene_covered(), "the screen was uncovered at step {step}");
        if ready(a) {
            return;
        }
        a.tick_ms(600_000 + step * 1000);
        a.update(1.0 / 60.0);
    }
    panic!("never reached the power off");
}

#[test]
fn a_tap_dozes_and_the_doze_ends_in_a_power_off_with_the_screen_covered_throughout() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    assert!(!a.scene_covered(), "covered while playing");
    a.apply(Action::PowerTap);
    assert!(a.scene_covered(), "a doze did not cover the screen");
    a.on_doze_timeout();
    covered_until_off(&mut a, App::ready_to_power_off);
}

#[test]
fn a_closed_lid_dozes_and_powers_off_the_same_way() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::LidClose);
    assert!(a.scene_covered(), "a closed lid did not cover the screen");
    a.on_doze_timeout();
    covered_until_off(&mut a, App::ready_to_power_off);
}

#[test]
fn a_hold_then_power_off_covers_the_screen_from_the_menu_on() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::PowerHold);
    assert!(a.scene_covered(), "the power menu did not cover the screen");
    a.apply(Action::GbaDown(Btn::Down));
    a.apply(Action::GbaDown(Btn::A));
    covered_until_off(&mut a, App::ready_to_power_off);
}

#[test]
fn a_hold_then_restart_does_too() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::PowerHold);
    a.apply(Action::GbaDown(Btn::A)); // Restart is first
    covered_until_off(&mut a, App::ready_to_restart);
}

/// The shelf is a scene too: powering off from it keeps the shelf.
#[test]
fn from_the_shelf_as_well() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    for _ in 0..60 {
        a.update(1.0 / 60.0);
    }
    assert!(!a.scene_covered(), "covered on the shelf");
    a.apply(Action::PowerHold);
    a.apply(Action::GbaDown(Btn::Down));
    a.apply(Action::GbaDown(Btn::A));
    covered_until_off(&mut a, App::ready_to_power_off);
}

/// Backing out of the menu, or waking from a doze, uncovers the screen, so the next cover
/// reads the screen as it is then rather than keeping a picture from before.
#[test]
fn backing_out_or_waking_uncovers_the_screen_again() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::PowerHold);
    a.apply(Action::GbaDown(Btn::B));
    assert!(!a.scene_covered(), "still covered after B closed the menu");
    a.apply(Action::LidClose);
    a.apply(Action::LidOpen);
    assert!(!a.scene_covered(), "still covered after the lid opened");
}
