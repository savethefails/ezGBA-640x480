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

/// Booted onto a seated cart, stopped before the core is ready: the boot a powered-off game
/// comes back through.
fn resuming(root: &std::path::Path, stem: &str) -> App {
    common::clocked(root);
    slot_store::write_slot_state(
        root,
        &slot_store::SlotState {
            cart: Some(stem.into()),
            clock_set: true,
            ..Default::default()
        },
    )
    .unwrap();
    let a = App::boot(root);
    assert!(a.resuming_at_boot(), "the boot did not resume the cart");
    a
}

fn drawn(a: &App) -> Vec<slot_gfx::Draw> {
    let mut out = Vec::new();
    a.draw(&mut out);
    out
}

const STILL: usize = 4242;

fn is_still(d: &slot_gfx::Draw) -> bool {
    matches!(d, slot_gfx::Draw::Tex { tex, x, y, .. }
        if *tex == slot_gfx::TexId::from_raw(STILL) && *x == 0.0 && *y == 0.0)
}

/// The bootloader's picture stays up, alone, through the whole resume, and the game's first
/// frame replaces it at full power: no slot, no bloom.
#[test]
fn a_resume_goes_from_the_boot_picture_straight_to_the_game() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = resuming(d.path(), "Emerald");
    a.set_boot_still(slot_gfx::TexId::from_raw(STILL));
    a.set_snapshot(common::StubSnapshot::boxed());

    for _ in 0..30 {
        a.update(1.0 / 60.0);
        let out = drawn(&a);
        assert_eq!(
            out.len(),
            1,
            "something besides the picture while the core loads"
        );
        assert!(is_still(&out[0]));
    }

    // The core is ready, but has not drawn a frame yet: still the picture.
    a.on_core_ready();
    a.update(1.0 / 60.0);
    let out = drawn(&a);
    assert!(
        out.iter().any(is_still),
        "a gap between the picture and the game"
    );
    assert!(!out.iter().any(|d| matches!(d, slot_gfx::Draw::Game)));

    // Its first frame: the game, whole, on the very next frame.
    a.set_game_ready(true);
    a.update(1.0 / 60.0);
    assert!(!a.boot_still_up());
    assert_eq!(
        a.screen_power(),
        1.0,
        "the game bloomed in instead of cutting"
    );
    let out = drawn(&a);
    assert!(out.iter().any(|d| matches!(d, slot_gfx::Draw::Game)));
    assert!(!out.iter().any(is_still));
}

/// Without a picture, the resume plays as it always has, through the slot and the bloom.
#[test]
fn without_a_picture_the_resume_is_unchanged() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = resuming(d.path(), "Emerald");
    a.set_snapshot(common::StubSnapshot::boxed());
    a.on_core_ready();
    a.set_game_ready(true);
    a.update(1.0 / 60.0);
    assert!(
        a.screen_power() < 1.0,
        "the bloom was skipped with nothing to carry over"
    );
}

/// The shelf boot has no picture to carry: it is drawn as it is.
#[test]
fn a_boot_to_the_shelf_takes_no_picture() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    assert!(!a.resuming_at_boot());
    a.set_boot_still(slot_gfx::TexId::from_raw(STILL));
    assert!(!a.boot_still_up());
}

/// A resume that does not end in the game drops the picture and plays as it always did.
#[test]
fn an_eject_during_the_resume_drops_the_picture() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = resuming(d.path(), "Emerald");
    a.set_boot_still(slot_gfx::TexId::from_raw(STILL));
    a.set_snapshot(common::StubSnapshot::boxed());
    a.apply(Action::Eject);
    a.update(1.0 / 60.0);
    assert!(!a.boot_still_up());
}

use std::sync::atomic::Ordering;
use std::time::Duration;

/// The copy a power off leaves once the boot partition holds the picture.
fn picture_kept(root: &std::path::Path) {
    let path = root.join(slot::boot_picture::LAST_SCREEN);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"png").unwrap();
}

/// The level slot lights the panel to; the rig's panel starts at 0, which stands in for
/// wherever the bootloader left it.
const CARD_LEVEL: u8 = 5;

/// Dim picture, dim picture, then the game and the card's level together: the backlight is
/// left alone through the whole load and comes up on the update after the game's first frame.
#[test]
fn a_resume_keeps_the_bootloader_s_level_until_the_game_is_on_screen() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = resuming(d.path(), "Emerald");
    picture_kept(d.path());
    let (power, backlight) = common::panel(d.path(), Duration::from_secs(60));
    a.set_power(power);
    a.set_boot_still(slot_gfx::TexId::from_raw(STILL));
    a.set_snapshot(common::StubSnapshot::boxed());
    assert_eq!(
        backlight.load(Ordering::Relaxed),
        0,
        "lit before slot drew anything"
    );

    for _ in 0..30 {
        a.update(1.0 / 60.0);
        assert_eq!(
            backlight.load(Ordering::Relaxed),
            0,
            "lit while the picture was up"
        );
    }
    a.on_core_ready();
    a.update(1.0 / 60.0);
    assert_eq!(
        backlight.load(Ordering::Relaxed),
        0,
        "lit before the game had a frame"
    );

    // The game's first frame: the hand over, and the frame drawn with the game in it...
    a.set_game_ready(true);
    a.update(1.0 / 60.0);
    assert!(drawn(&a).iter().any(|d| matches!(d, slot_gfx::Draw::Game)));
    assert_eq!(
        backlight.load(Ordering::Relaxed),
        0,
        "lit before the game was presented"
    );
    // ...presented, and then the light.
    a.update(1.0 / 60.0);
    assert_eq!(backlight.load(Ordering::Relaxed), CARD_LEVEL);
    assert!(!a.backlight_held());
}

/// A shelf boot's first frame is the shelf in the picture: the light comes up as soon as
/// that frame is presented, which is the first update.
#[test]
fn a_shelf_boot_lights_on_its_first_presented_frame() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    picture_kept(d.path());
    let mut a = boot(d.path());
    let (power, backlight) = common::panel(d.path(), Duration::from_secs(60));
    a.set_power(power);
    assert_eq!(
        backlight.load(Ordering::Relaxed),
        0,
        "lit before the shelf was drawn"
    );
    a.update(1.0 / 60.0);
    assert_eq!(backlight.load(Ordering::Relaxed), CARD_LEVEL);
}

/// With no picture carried over, the panel comes up at once, as it always has.
#[test]
fn without_a_picture_the_panel_lights_at_once() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    let (power, backlight) = common::panel(d.path(), Duration::from_secs(60));
    a.set_power(power);
    assert_eq!(backlight.load(Ordering::Relaxed), CARD_LEVEL);
}

/// A resume that never reaches the game does not leave the panel dim.
#[test]
fn the_hold_gives_up_if_the_game_never_comes() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = resuming(d.path(), "Emerald");
    picture_kept(d.path());
    let (power, backlight) = common::panel(d.path(), Duration::from_secs(600));
    a.set_power(power);
    a.set_boot_still(slot_gfx::TexId::from_raw(STILL));
    a.update(1.0 / 60.0);
    assert_eq!(backlight.load(Ordering::Relaxed), 0);
    a.tick_ms(a.now() + 11_000);
    a.update(1.0 / 60.0);
    assert_eq!(backlight.load(Ordering::Relaxed), CARD_LEVEL);
    assert!(!a.boot_still_up());
}

/// The shelf comes back on the cart that was highlighted at power off, which is the one the
/// boot picture shows.
#[test]
fn the_shelf_reopens_on_the_cart_highlighted_at_power_off() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion", "Zelda"]);
    let mut a = boot(d.path());
    for _ in 0..60 {
        a.update(1.0 / 60.0);
    }
    a.apply(Action::ShelfRight);
    a.apply(Action::ShelfRight);
    let left_on = a.selected_stem().unwrap().to_string();
    assert_ne!(left_on, "Emerald", "the highlight did not move");
    a.apply(Action::PowerHold);
    a.apply(Action::GbaDown(Btn::Down));
    a.apply(Action::GbaDown(Btn::A));

    let state = slot_store::read_slot_state(d.path());
    assert_eq!(state.shelf_cart.as_deref(), Some(left_on.as_str()));
    let b = boot(d.path());
    assert_eq!(b.selected_stem(), Some(left_on.as_str()));
}
