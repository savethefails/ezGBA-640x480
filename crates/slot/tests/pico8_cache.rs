//! Fetching a multicart's parts ahead of time, against a BBS that is a folder: what lands in
//! the cache is what PICO-8 itself keeps, `<id>.nfo` naming `<lid>.p8.png`, which it was seen to
//! load with the network gone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slot::pico8_cache::{self, Fetch};

/// The BBS: an id's revision and the cart it serves, and every address asked.
struct Bbs {
    carts: HashMap<String, (u32, PathBuf)>,
    asked: Vec<String>,
}

impl Fetch for Bbs {
    fn fetch(&mut self, url: &str, out: &Path) -> bool {
        self.asked.push(url.to_string());
        let lid = url.rsplit("lid=").next().unwrap();
        if url.contains("nfo=1") {
            let Some((rev, _)) = self.carts.get(lid) else {
                return false;
            };
            std::fs::write(out, format!("lid:{lid}-{rev}\nmid:1\ntitle:t\n")).unwrap();
            return true;
        }
        let id = lid.rsplit_once('-').unwrap().0;
        let Some((_, path)) = self.carts.get(id) else {
            return false;
        };
        std::fs::copy(path, out).is_ok()
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pico8")
        .join(name)
}

/// A launcher as the patch writes one, naming `#id`.
fn launcher(keep: &Path, id: &str) -> PathBuf {
    let side = tempfile::tempdir().unwrap();
    let path = side.path().join(format!("Games/PICO8/{id}-1.p8.png"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(fixture("multi_demo-2.p8.png"), &path).unwrap();
    slot::pico8_patch::patch(side.path(), &path).unwrap();
    let kept = keep.join(format!("launcher-{id}.p8.png"));
    std::fs::copy(&path, &kept).unwrap();
    kept
}

/// A card with a multicart naming #a and #c, and a BBS where #c is a launcher naming #x.
fn setup() -> (tempfile::TempDir, PathBuf, Bbs) {
    let d = tempfile::tempdir().unwrap();
    let games = slot::pico8_patch::games(d.path());
    std::fs::create_dir_all(&games).unwrap();
    let main = games.join("main-1.p8");
    std::fs::write(
        &main,
        "pico-8 cartridge\nversion 43\n__lua__\nparts=split\"#a,#c\"\nload(parts[1])\n",
    )
    .unwrap();
    let plain = fixture("multi_demo-2.p8.png");
    // The plain cart names #multi_demo_part; that one is a launcher naming itself, so the walk
    // also meets a cart that leads back to where it already is.
    let carts = HashMap::from([
        ("a".to_string(), (3, plain.clone())),
        ("c".to_string(), (7, launcher(d.path(), "x"))),
        ("x".to_string(), (12, plain)),
        (
            "multi_demo_part".to_string(),
            (2, launcher(d.path(), "multi_demo_part")),
        ),
    ]);
    (
        d,
        main,
        Bbs {
            carts,
            asked: Vec::new(),
        },
    )
}

#[test]
fn every_part_and_the_parts_parts_land_where_pico8_keeps_them() {
    let (d, main, mut bbs) = setup();
    assert_eq!(pico8_cache::missing(d.path(), &main), ["a", "c"]);
    let mut seen = Vec::new();
    let n = pico8_cache::download(d.path(), &main, &mut bbs, &mut |done, total| {
        seen.push((done, total))
    })
    .unwrap();
    assert_eq!(n, 4);
    let dir = pico8_cache::carts_dir(d.path());
    for (id, lid) in [
        ("a", "a-3"),
        ("c", "c-7"),
        ("x", "x-12"),
        ("multi_demo_part", "multi_demo_part-2"),
    ] {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{id}.nfo")))
                .unwrap()
                .lines()
                .next(),
            Some(format!("lid:{lid}").as_str())
        );
        assert!(dir.join(format!("{lid}.p8.png")).exists(), "{lid}");
        assert_eq!(
            pico8_cache::cached(&dir, id),
            Some(dir.join(format!("{lid}.p8.png")))
        );
    }
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert_eq!(seen.last(), Some(&(4, 4)));
    assert!(pico8_cache::missing(d.path(), &main).is_empty());

    // Again: nothing to fetch, nothing asked.
    bbs.asked.clear();
    let n = pico8_cache::download(d.path(), &main, &mut bbs, &mut |_, _| {}).unwrap();
    assert_eq!(n, 0);
    assert!(bbs.asked.is_empty(), "{:?}", bbs.asked);
}

#[test]
fn a_part_the_bbs_does_not_have_is_named_and_nothing_half_kept() {
    let (d, main, mut bbs) = setup();
    bbs.carts.remove("c");
    let err = pico8_cache::download(d.path(), &main, &mut bbs, &mut |_, _| {}).unwrap_err();
    assert_eq!(err, "c");
    let dir = pico8_cache::carts_dir(d.path());
    assert!(pico8_cache::cached(&dir, "a").is_some());
    assert!(!dir.join("c.nfo").exists());
    // It stopped at #c: what #a names is still to come.
    assert_eq!(
        pico8_cache::missing(d.path(), &main),
        ["c", "multi_demo_part"]
    );
}

#[test]
fn a_cart_that_names_no_parts_needs_nothing() {
    let d = tempfile::tempdir().unwrap();
    let cart = d.path().join("plain-1.p8");
    std::fs::write(
        &cart,
        "pico-8 cartridge\nversion 43\n__lua__\nprint(#\"hi\")\n",
    )
    .unwrap();
    assert!(pico8_cache::missing(d.path(), &cart).is_empty());
}
