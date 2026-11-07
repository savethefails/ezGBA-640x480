//! The Patcher against carts on a card: a multicart is found, patched into a launcher for its BBS
//! version that PICO-8 will load, and restored byte for byte.
//!
//! The fixtures are ezGBA's own: `multi_demo-2.p8`, a cart that asks `stat(101)` the way
//! Lemmings and Snekburd do, and the same cart as PICO-8 0.2.7 saved it as a `.p8.png`.

use std::path::{Path, PathBuf};

use slot::pico8_patch::{self, Status};

fn card(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let games = pico8_patch::games(d.path());
    std::fs::create_dir_all(&games).unwrap();
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pico8");
    for f in files {
        std::fs::copy(here.join(f), games.join(f)).unwrap();
    }
    (d, games)
}

fn round_trip(file: &str) {
    let (d, games) = card(&[file]);
    let cart = games.join(file);
    let original = std::fs::read(&cart).unwrap();
    let carts = pico8_patch::scan(d.path());
    assert_eq!(carts.len(), 1);
    assert_eq!(carts[0].name, "multi_demo-2");
    assert_eq!(
        carts[0].status,
        Status::Multicart {
            id: "multi_demo".into()
        }
    );

    pico8_patch::patch(d.path(), &cart).unwrap();
    assert_eq!(pico8_patch::status(&cart), Status::Patched);
    let code = String::from_utf8(pico8_patch::read_code(&cart).unwrap()).unwrap();
    assert!(code.contains("load(\"#multi_demo\")"), "{code}");
    assert_eq!(
        std::fs::read(pico8_patch::originals(d.path()).join(file)).unwrap(),
        original,
        "the original was not kept as it was"
    );
    assert!(
        pico8_patch::patch(d.path(), &cart).is_err(),
        "a patched cart was patched again"
    );

    pico8_patch::restore(d.path(), &cart).unwrap();
    assert_eq!(std::fs::read(&cart).unwrap(), original);
    assert!(!pico8_patch::originals(d.path()).join(file).exists());
}

#[test]
fn a_picture_cart_is_patched_and_restored() {
    round_trip("multi_demo-2.p8.png");
}

#[test]
fn a_text_cart_is_patched_and_restored() {
    round_trip("multi_demo-2.p8");
}

/// The label is the cart's picture on the shelf: only the low bits PICO-8 keeps its data in may
/// change.
#[test]
fn the_label_looks_the_same_after_a_patch() {
    let file = "multi_demo-2.p8.png";
    let (d, games) = card(&[file]);
    let cart = games.join(file);
    let pixels = |p: &Path| {
        let dec = png::Decoder::new(std::fs::File::open(p).unwrap());
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size()];
        r.next_frame(&mut buf).unwrap();
        buf
    };
    let before = pixels(&cart);
    pico8_patch::patch(d.path(), &cart).unwrap();
    let after = pixels(&cart);
    assert_eq!(before.len(), after.len());
    assert!(before.iter().zip(&after).all(|(a, b)| a & !3 == b & !3));
}

#[test]
fn a_renamed_multicart_has_no_id_and_is_left_alone() {
    let (d, games) = card(&["multi_demo-2.p8"]);
    let renamed = games.join("Demo.p8");
    std::fs::rename(games.join("multi_demo-2.p8"), &renamed).unwrap();
    assert_eq!(pico8_patch::status(&renamed), Status::NoId);
    assert!(pico8_patch::patch(d.path(), &renamed).is_err());
}

#[test]
fn a_cart_that_runs_from_a_file_is_fine_and_other_files_are_not_listed() {
    let (d, games) = card(&[]);
    std::fs::write(
        games.join("plain-1.p8"),
        "pico-8 cartridge\nversion 43\n__lua__\nprint(\"hi\")\n",
    )
    .unwrap();
    std::fs::write(games.join("shot.png"), b"not a cart").unwrap();
    std::fs::write(games.join("broken.p8.png"), b"not a png").unwrap();
    let carts = pico8_patch::scan(d.path());
    let got: Vec<_> = carts.iter().map(|c| (c.name.as_str(), &c.status)).collect();
    assert_eq!(
        got,
        [("broken", &Status::Unreadable), ("plain-1", &Status::Fine)]
    );
}
