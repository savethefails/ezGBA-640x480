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

fn pico8_shelf() -> (tempfile::TempDir, std::path::PathBuf, App) {
    let d = tmp_root_with_carts(&[]);
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
        games.join("celeste-1.p8"),
        "pico-8 cartridge\nversion 43\n__lua__\nprint(\"hi\")\n",
    )
    .unwrap();
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    a.update(0.1);
    (d, games, a)
}

fn select_start(a: &mut App) {
    a.apply(Action::GbaDown(Btn::Select));
    a.apply(Action::GbaDown(Btn::Start));
    a.apply(Action::GbaUp(Btn::Start));
    a.apply(Action::GbaUp(Btn::Select));
}

/// A BBS that is a folder: the patched cart's own id serves the cart as it is on the BBS (which
/// names #multi_demo_part), and #multi_demo_part serves a launcher that names itself.
struct Bbs {
    plain: std::path::PathBuf,
    part: std::path::PathBuf,
}

impl slot::pico8_cache::Fetch for Bbs {
    fn fetch(&mut self, url: &str, out: &Path) -> bool {
        let lid = url.rsplit("lid=").next().unwrap();
        if url.contains("nfo=1") {
            std::fs::write(out, format!("lid:{lid}-5\ntitle:t\n")).unwrap();
            return true;
        }
        let from = match lid {
            "multi_demo-5" => &self.plain,
            "multi_demo_part-5" => &self.part,
            _ => return false,
        };
        std::fs::copy(from, out).is_ok()
    }
}

fn bbs_for(a: &mut App, keep: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pico8");
    let side = keep.join("side");
    let part = side.join("Games/PICO8/multi_demo_part-1.p8.png");
    std::fs::create_dir_all(part.parent().unwrap()).unwrap();
    std::fs::copy(fixture.join("multi_demo-2.p8.png"), &part).unwrap();
    slot::pico8_patch::patch(&side, &part).unwrap();
    let plain = fixture.join("multi_demo-2.p8.png");
    a.set_offline_fetch(std::sync::Arc::new(move || {
        Box::new(Bbs {
            plain: plain.clone(),
            part: part.clone(),
        })
    }));
}

/// Frames until the download's last word is up, or a few seconds.
fn until_done(a: &mut App) -> Option<(bool, String)> {
    for _ in 0..400 {
        a.update(0.016);
        let text = a.pico8_hint_text();
        if text
            .as_ref()
            .is_some_and(|(_, t)| !t.starts_with("Downloading"))
        {
            return text;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    a.pico8_hint_text()
}

fn on_multicart(a: &mut App) {
    press(a, Btn::Right);
    a.update(0.1);
    assert_eq!(a.selected_stem(), Some("multi_demo-2.p8"));
}

#[test]
fn with_wi_fi_off_the_shelf_says_to_turn_it_on_and_select_start_does_nothing() {
    let (_d, games, mut a) = pico8_shelf();
    assert_eq!(a.selected_stem(), Some("celeste-1"));
    assert_eq!(a.pico8_hint_text(), None, "a cart that runs as it is");
    on_multicart(&mut a);
    assert_eq!(
        a.pico8_hint_text(),
        Some((false, "Needs Wi-Fi".to_string()))
    );
    select_start(&mut a);
    assert!(matches!(a.phase(), Phase::Shelf));
    assert!(matches!(
        slot::pico8_patch::status(&games.join("multi_demo-2.p8.png")),
        Status::Multicart { .. }
    ));
}

#[test]
fn select_start_patches_a_multicart_then_downloads_every_part() {
    let (d, games, mut a) = pico8_shelf();
    bbs_for(&mut a, d.path());
    a.assume_online();
    on_multicart(&mut a);
    assert_eq!(
        a.pico8_hint_text(),
        Some((true, "Patch & download".to_string()))
    );
    select_start(&mut a);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    assert_eq!(
        slot::pico8_patch::status(&games.join("multi_demo-2.p8.png")),
        Status::Patched
    );
    assert_eq!(
        until_done(&mut a),
        Some((false, "Ready offline".to_string()))
    );
    let carts = slot::pico8_cache::carts_dir(d.path());
    assert!(slot::pico8_cache::cached(&carts, "multi_demo").is_some());
    assert!(slot::pico8_cache::cached(&carts, "multi_demo_part").is_some());
    // The word goes, and a cart with everything it needs looks like any other.
    for _ in 0..40 {
        a.update(0.1);
    }
    assert_eq!(a.pico8_hint_text(), None);
    select_start(&mut a);
    assert!(matches!(a.phase(), Phase::Shelf));
}

#[test]
fn a_patched_cart_with_parts_missing_offers_the_download_alone() {
    let (d, games, mut a) = pico8_shelf();
    slot::pico8_patch::patch(d.path(), &games.join("multi_demo-2.p8.png")).unwrap();
    bbs_for(&mut a, d.path());
    a.assume_online();
    on_multicart(&mut a);
    assert_eq!(
        a.pico8_hint_text(),
        Some((true, "Download parts".to_string()))
    );
    select_start(&mut a);
    assert_eq!(
        until_done(&mut a),
        Some((false, "Ready offline".to_string()))
    );
}

#[test]
fn a_failed_download_says_so_and_can_be_tried_again() {
    let (d, games, mut a) = pico8_shelf();
    slot::pico8_patch::patch(d.path(), &games.join("multi_demo-2.p8.png")).unwrap();
    a.set_offline_fetch(std::sync::Arc::new(|| {
        Box::new(Bbs {
            plain: "/nowhere".into(),
            part: "/nowhere".into(),
        })
    }));
    a.assume_online();
    on_multicart(&mut a);
    select_start(&mut a);
    assert_eq!(
        until_done(&mut a),
        Some((false, "Download failed".to_string()))
    );
}

#[test]
fn select_start_on_a_cart_with_nothing_to_get_changes_nothing() {
    let (_d, games, mut a) = pico8_shelf();
    a.assume_online();
    select_start(&mut a);
    assert!(a.core_picker().is_none(), "the chord is not a START");
    assert_eq!(
        slot::pico8_patch::status(&games.join("celeste-1.p8")),
        Status::Fine
    );
    assert!(matches!(a.phase(), Phase::Shelf));
}
