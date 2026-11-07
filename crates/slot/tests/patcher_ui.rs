//! The quick menu's Patcher, driven through the app: it lists the card's PICO-8 carts, A patches
//! a multicart and restores a patched one, a cart that needs nothing is refused, and a resume of
//! the cart as it was is moved aside so it cannot bring the old code back.

mod common;

use std::path::Path;

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, Phase};
use slot::pico8_patch::Status;
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, Core, Platform, SlotState, StateRing};
use slot_ui::QuickRow;

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn open_patcher(a: &mut App) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::ALL.len() {
        if a.quick_menu() == Some(QuickRow::Patcher) {
            break;
        }
        press(a, Btn::Down);
    }
    assert_eq!(
        a.quick_menu(),
        Some(QuickRow::Patcher),
        "no Patcher row in {:?}",
        a.phase()
    );
    press(a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Patcher { row: 0 }),
        "A on Patcher opened {:?}",
        a.phase()
    );
}

#[test]
fn a_multicart_is_patched_and_restored_from_the_menu() {
    let d = tmp_root_with_carts(&["Emerald"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    let games = slot::pico8_patch::games(d.path());
    std::fs::create_dir_all(&games).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pico8");
    std::fs::copy(
        fixture.join("multi_demo-2.p8.png"),
        games.join("multi_demo-2.p8.png"),
    )
    .unwrap();
    std::fs::write(
        games.join("hello-1.p8"),
        "pico-8 cartridge\nversion 43\n__lua__\nprint(\"hi\")\n",
    )
    .unwrap();
    let ring = StateRing::new(d.path(), Platform::Pico8, Core::Pico8, "multi_demo-2.p8");
    ring.write_resume(b"the old code, running").unwrap();

    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    open_patcher(&mut a);
    assert_eq!(
        a.patcher_rows(),
        [
            ("multi_demo-2".to_string(), Some("Needs Patch".to_string())),
            ("hello-1".to_string(), None),
        ]
    );

    // A cart that runs as it is: nothing to do.
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert_eq!(a.patcher_carts()[1].status, Status::Fine);

    press(&mut a, Btn::Up);
    press(&mut a, Btn::A);
    assert_eq!(a.patcher_carts()[0].status, Status::Patched);
    assert_eq!(
        ring.read_resume().unwrap(),
        None,
        "the old resume would bring the old code back"
    );

    press(&mut a, Btn::A);
    assert_eq!(
        a.patcher_carts()[0].status,
        Status::Multicart {
            id: "multi_demo".into()
        }
    );

    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Patcher));
}

#[test]
fn a_card_with_no_pico8_carts_says_so() {
    // Two carts, so the boot stops on the carousel rather than seating the only one.
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .unwrap();
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    open_patcher(&mut a);
    assert!(a.patcher_carts().is_empty());
    assert_eq!(a.patcher_words(), ["No PICO-8 carts"]);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Patcher { .. }));
}
