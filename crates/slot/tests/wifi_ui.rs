//! The quick menu's Wi-Fi page, driven through the app with the fake radio: turning Wi-Fi on and
//! off, joining a network from a scan, the keyboard for a password, and the radio going down for
//! a doze and coming back after it.

mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, Phase};
use slot::network_page::NetPage;
use slot::wifi::{Fake, Wifi};
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, SlotState};
use slot_ui::QuickRow;
use tempfile::TempDir;

/// The worker keeps its owner in statics, so two of these must not run at once.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

struct Radio {
    running: Arc<std::sync::atomic::AtomicBool>,
    blocked: Arc<std::sync::atomic::AtomicBool>,
    log: Arc<Mutex<Vec<String>>>,
}

fn on_carousel(wifi_on: bool) -> (TempDir, App, Radio) {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            wifi_on,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    let f = Fake::demo();
    let radio = Radio {
        running: f.running.clone(),
        blocked: f.blocked.clone(),
        log: f.log.clone(),
    };
    a.set_wifi(Wifi::inline(Box::new(f)));
    (d, a, radio)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

/// A few seconds of frames, for the worker to get through what it was asked.
fn settle(a: &mut App) {
    for _ in 0..60 {
        a.update(0.1);
    }
}

fn open_wifi_page(a: &mut App) {
    a.apply(Action::QuickMenu);
    while a.quick_menu() != Some(QuickRow::Wifi) {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    assert!(
        matches!(
            a.phase(),
            Phase::Network {
                page: NetPage::Main,
                row: 0
            }
        ),
        "A on Wi-Fi opened {:?}",
        a.phase()
    );
}

fn row(a: &App) -> usize {
    match a.phase() {
        Phase::Network { row, .. } => *row,
        p => panic!("not on a Wi-Fi page: {p:?}"),
    }
}

#[test]
fn wi_fi_is_off_until_turned_on_and_the_card_keeps_it() {
    let _g = serial();
    let (d, mut a, radio) = on_carousel(false);
    settle(&mut a);
    assert!(
        !radio.running.load(Ordering::SeqCst),
        "the radio came up with Wi-Fi off"
    );
    assert_eq!(
        a.quick_value(QuickRow::Wifi),
        Some(slot_ui::QuickValue::Off)
    );
    open_wifi_page(&mut a);
    assert_eq!(
        a.network_words(),
        vec!["Wi-Fi", "Off"],
        "off is only the switch"
    );
    press(&mut a, Btn::Right);
    settle(&mut a);
    assert!(a.wifi_on());
    assert!(
        radio.running.load(Ordering::SeqCst),
        "turned on, the radio did not start"
    );
    assert!(
        read_slot_state(d.path()).wifi_on,
        "the card forgot it was on"
    );
    assert!(a.network_words().contains(&"Scan for Networks".to_string()));
    press(&mut a, Btn::A);
    settle(&mut a);
    assert!(!a.wifi_on());
    assert!(
        radio.blocked.load(Ordering::SeqCst),
        "turned off, the radio was not powered down"
    );
    assert!(!read_slot_state(d.path()).wifi_on);
}

#[test]
fn an_open_network_from_the_scan_is_joined_and_says_so() {
    let _g = serial();
    let (_d, mut a, radio) = on_carousel(true);
    open_wifi_page(&mut a);
    settle(&mut a);
    // Wi-Fi, then Scan for Networks: nothing is saved yet.
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert!(matches!(
        a.phase(),
        Phase::Network {
            page: NetPage::Scan,
            ..
        }
    ));
    settle(&mut a);
    let words = a.network_words();
    let at = words
        .iter()
        .step_by(2)
        .position(|w| w == "CoffeeShop Guest")
        .expect("the scan did not find the open network");
    for _ in 0..at {
        press(&mut a, Btn::Down);
    }
    press(&mut a, Btn::A);
    assert!(matches!(
        a.phase(),
        Phase::Network {
            page: NetPage::Main,
            row: 0
        }
    ));
    settle(&mut a);
    assert!(
        radio
            .log
            .lock()
            .unwrap()
            .iter()
            .any(|l| l == "add CoffeeShop Guest"),
        "{:?}",
        radio.log.lock().unwrap()
    );
    let words = a.network_words();
    assert_eq!(&words[..2], ["Wi-Fi", "Connected"], "{words:?}");
    assert!(words.contains(&"CoffeeShop Guest".to_string()), "{words:?}");
}

#[test]
fn a_secured_network_asks_for_its_password_and_y_gives_up() {
    let _g = serial();
    let (_d, mut a, radio) = on_carousel(true);
    open_wifi_page(&mut a);
    settle(&mut a);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    settle(&mut a);
    // The strongest is Home, which has a password.
    press(&mut a, Btn::A);
    let (title, before, after) = a
        .keyboard_words()
        .expect("no keyboard for a secured network");
    assert_eq!(title, "Password for Home");
    assert_eq!((before.as_str(), after.as_str()), ("", ""));
    press(&mut a, Btn::A);
    assert_eq!(a.keyboard_words().unwrap().1, "q");
    press(&mut a, Btn::Y);
    assert!(a.keyboard_words().is_none(), "Y left the keyboard up");
    assert!(matches!(
        a.phase(),
        Phase::Network {
            page: NetPage::Scan,
            ..
        }
    ));
    // And done joins with what was typed.
    press(&mut a, Btn::A);
    press(&mut a, Btn::A);
    press(&mut a, Btn::Start);
    assert!(a.keyboard_words().is_none());
    settle(&mut a);
    assert!(
        radio.log.lock().unwrap().iter().any(|l| l == "add Home"),
        "{:?}",
        radio.log.lock().unwrap()
    );
}

#[test]
fn b_goes_back_a_page_and_then_to_the_wi_fi_row() {
    let _g = serial();
    let (_d, mut a, _radio) = on_carousel(true);
    open_wifi_page(&mut a);
    settle(&mut a);
    press(&mut a, Btn::Down);
    let scan_row = row(&a);
    press(&mut a, Btn::A);
    press(&mut a, Btn::B);
    assert!(matches!(
        a.phase(),
        Phase::Network {
            page: NetPage::Main,
            ..
        }
    ));
    assert_eq!(row(&a), scan_row, "back from the scan on another row");
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Wifi));
}

#[test]
fn a_doze_powers_the_radio_down_and_a_wake_brings_it_back() {
    let _g = serial();
    let (_d, mut a, radio) = on_carousel(true);
    settle(&mut a);
    assert!(
        radio.running.load(Ordering::SeqCst),
        "Wi-Fi on at boot did not bring the radio up"
    );
    a.apply(Action::LidClose);
    settle(&mut a);
    assert!(
        radio.blocked.load(Ordering::SeqCst),
        "the doze left the radio on"
    );
    a.apply(Action::LidOpen);
    settle(&mut a);
    assert!(
        radio.running.load(Ordering::SeqCst),
        "the wake did not bring the radio back"
    );
    assert!(!radio.blocked.load(Ordering::SeqCst));
}
