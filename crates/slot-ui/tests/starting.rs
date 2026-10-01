use slot_gfx::{OUT_H, OUT_W};
use slot_ui::{stamp_starting, PillAt};

fn frame(rgb: [u8; 3]) -> Vec<u8> {
    [rgb[0], rgb[1], rgb[2], 255]
        .iter()
        .copied()
        .cycle()
        .take((OUT_W * OUT_H * 4) as usize)
        .collect()
}

fn changed_rows(before: &[u8], after: &[u8]) -> (usize, usize) {
    let row = OUT_W as usize * 4;
    let rows: Vec<usize> = (0..OUT_H as usize)
        .filter(|y| before[y * row..(y + 1) * row] != after[y * row..(y + 1) * row])
        .collect();
    (*rows.first().unwrap(), *rows.last().unwrap())
}

/// A small mark, and only there: low over a game, above the carts on the shelf, and the
/// rest of the picture untouched.
#[test]
fn the_pill_sits_where_it_is_asked_and_nowhere_else() {
    let plain = frame([200, 120, 60]);
    let mut game = plain.clone();
    stamp_starting(&mut game, PillAt::Game);
    let (top, bottom) = changed_rows(&plain, &game);
    assert!(top > 400 && bottom < 470, "game pill rows {top}..{bottom}");

    let mut shelf = plain.clone();
    stamp_starting(&mut shelf, PillAt::Shelf);
    let (top, bottom) = changed_rows(&plain, &shelf);
    assert!(top > 110 && bottom < 190, "shelf pill rows {top}..{bottom}");

    // Narrow: well inside the panel's width, centred.
    let row = OUT_W as usize * 4;
    let y = 150;
    let xs: Vec<usize> = (0..OUT_W as usize)
        .filter(|x| {
            shelf[y * row + x * 4..y * row + x * 4 + 4]
                != plain[y * row + x * 4..y * row + x * 4 + 4]
        })
        .collect();
    let (l, r) = (*xs.first().unwrap(), *xs.last().unwrap());
    assert!(r - l < 260, "pill {l}..{r} is too wide");
    assert!(
        (l + r) / 2 >= 315 && (l + r) / 2 <= 325,
        "pill {l}..{r} is off centre"
    );
}

/// The capsule is dark and the word is light, on any picture: there is ink in it.
#[test]
fn the_word_is_written_on_the_capsule() {
    let mut f = frame([255, 255, 255]);
    stamp_starting(&mut f, PillAt::Shelf);
    let row = OUT_W as usize * 4;
    let line: Vec<[u8; 3]> = (0..OUT_W as usize)
        .map(|x| {
            [
                f[150 * row + x * 4],
                f[150 * row + x * 4 + 1],
                f[150 * row + x * 4 + 2],
            ]
        })
        .collect();
    assert!(line.iter().any(|p| p[0] < 60), "no dark capsule");
    assert!(
        line.iter().any(|p| p[0] > 200 && p[0] < 250),
        "no light type on the capsule"
    );
    // Opaque in, opaque out.
    assert!(f.chunks_exact(4).all(|p| p[3] == 255));
}

/// A frame of the wrong size is left alone rather than written past its end.
#[test]
fn a_frame_of_another_size_is_left_alone() {
    let mut small = vec![7u8; 100];
    stamp_starting(&mut small, PillAt::Game);
    assert!(small.iter().all(|b| *b == 7));
}
