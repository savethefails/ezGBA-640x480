//! The on-screen keyboard, for typing a Wi-Fi password with the buttons. The keys and what the
//! buttons do with them are slot's (BrandonKowalski/slot v1.13.0 `keyboard.rs`), without its
//! numeric layout and help screen, which ezGBA has no use for; the drawing is ezGBA's own, in the
//! quick menu's ground, bar and type.
//!
//! The D-pad moves between keys and A presses one. B deletes, X types a space, SELECT is shift,
//! L1 and R1 move the cursor, START is done and Y gives up.

use slot_input::Btn;

use crate::draw::{Draw, TexId, OUT_W};
use crate::slot_chrome::{edge, opening, recess};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Special {
    Delete,
    Done,
    Shift,
    Symbols,
    Space,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Cap {
    /// The `n`th character of whichever row of characters is showing.
    Char(usize),
    Special(Special),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    Nothing,
    Changed,
    Done(String),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyboard {
    title: String,
    text: Vec<char>,
    cursor: usize,
    row: usize,
    col: usize,
    shift: bool,
    symbols: bool,
    /// Where across the keys an Up or Down is aiming, so a run of them keeps to one column
    /// through rows of different lengths.
    aim: Option<f32>,
}

use Cap::{Char as C, Special as S};
use Special::*;

const ROWS: &[&[Cap]] = &[
    &[
        C(0),
        C(1),
        C(2),
        C(3),
        C(4),
        C(5),
        C(6),
        C(7),
        C(8),
        C(9),
        S(Delete),
    ],
    &[
        C(10),
        C(11),
        C(12),
        C(13),
        C(14),
        C(15),
        C(16),
        C(17),
        C(18),
        C(19),
    ],
    &[
        C(20),
        C(21),
        C(22),
        C(23),
        C(24),
        C(25),
        C(26),
        C(27),
        C(28),
        S(Done),
    ],
    &[
        S(Shift),
        C(29),
        C(30),
        C(31),
        C(32),
        C(33),
        C(34),
        C(35),
        S(Symbols),
    ],
    &[S(Space)],
];

/// The three sets of characters the keys can show, in `Cap::Char` order.
pub const LOWER: &str = "1234567890qwertyuiopasdfghjklzxcvbnm";
pub const UPPER: &str = "1234567890QWERTYUIOPASDFGHJKLZXCVBNM";
pub const SYMBOL: &str = "!@#$%^&*()`~[]\\|{};:'\"<>?/+=_,.-\u{20ac}\u{a3}\u{a5}\u{a2}";

fn nth(s: &str, i: usize) -> char {
    s.chars().nth(i).unwrap_or(' ')
}

impl Keyboard {
    pub fn new(title: &str, text: &str) -> Keyboard {
        let text: Vec<char> = text.chars().collect();
        Keyboard {
            title: title.to_string(),
            cursor: text.len(),
            text,
            row: 1,
            col: 0,
            shift: false,
            symbols: false,
            aim: None,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// The text either side of the cursor, which is drawn as two words with the cursor between.
    pub fn split(&self) -> (String, String) {
        (
            self.text[..self.cursor].iter().collect(),
            self.text[self.cursor..].iter().collect(),
        )
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn col(&self) -> usize {
        self.col
    }

    pub fn shift(&self) -> bool {
        self.shift
    }

    pub fn symbols(&self) -> bool {
        self.symbols
    }

    pub fn rows(&self) -> &'static [&'static [Cap]] {
        ROWS
    }

    /// Which set of characters is on the keys: 0 lower, 1 upper, 2 symbols. Shift on the number
    /// row is its symbols, as on a real keyboard.
    pub fn set(&self, i: usize) -> usize {
        if self.symbols || (self.shift && i < 10) {
            2
        } else if self.shift {
            1
        } else {
            0
        }
    }

    fn value(&self, i: usize) -> char {
        nth([LOWER, UPPER, SYMBOL][self.set(i)], i)
    }

    pub fn label(&self, cap: Cap) -> String {
        match cap {
            Cap::Char(i) => self.value(i).to_string(),
            Cap::Special(s) => special_label(s, self.symbols).to_string(),
        }
    }

    fn insert(&mut self, c: char) -> Typed {
        self.text.insert(self.cursor, c);
        self.cursor += 1;
        Typed::Changed
    }

    fn backspace(&mut self) -> Typed {
        if self.cursor == 0 {
            return Typed::Nothing;
        }
        self.cursor -= 1;
        self.text.remove(self.cursor);
        Typed::Changed
    }

    fn done(&self) -> Typed {
        Typed::Done(self.text.iter().collect())
    }

    fn centre(&self, row: usize, col: usize) -> f32 {
        let [x, _, w, _] = key_rect(row, col);
        x + w / 2.0
    }

    fn nearest(&self, row: usize, x: f32) -> usize {
        (0..ROWS[row].len())
            .min_by(|a, b| {
                let off = |c: usize| (self.centre(row, c) - x).abs();
                off(*a).total_cmp(&off(*b))
            })
            .unwrap_or(0)
    }

    fn navigate(&mut self, b: Btn) {
        let n = ROWS.len();
        match b {
            Btn::Up | Btn::Down => {
                let x = self.aim.unwrap_or_else(|| self.centre(self.row, self.col));
                self.row = match b {
                    Btn::Up => (self.row + n - 1) % n,
                    _ => (self.row + 1) % n,
                };
                self.col = self.nearest(self.row, x);
                self.aim = Some(x);
            }
            Btn::Left => {
                let len = ROWS[self.row].len();
                self.col = (self.col + len - 1) % len;
                self.aim = None;
            }
            _ => {
                self.col = (self.col + 1) % ROWS[self.row].len();
                self.aim = None;
            }
        }
    }

    pub fn press(&mut self, b: Btn) -> Typed {
        match b {
            Btn::Up | Btn::Down | Btn::Left | Btn::Right => {
                self.navigate(b);
                Typed::Changed
            }
            Btn::A => match ROWS[self.row][self.col] {
                Cap::Char(i) => self.insert(self.value(i)),
                Cap::Special(Delete) => self.backspace(),
                Cap::Special(Done) => self.done(),
                Cap::Special(Shift) => {
                    self.shift = !self.shift;
                    Typed::Changed
                }
                Cap::Special(Symbols) => {
                    self.symbols = !self.symbols;
                    Typed::Changed
                }
                Cap::Special(Space) => self.insert(' '),
            },
            Btn::B => self.backspace(),
            Btn::X => self.insert(' '),
            Btn::Select => {
                self.shift = !self.shift;
                Typed::Changed
            }
            Btn::Y => Typed::Cancelled,
            Btn::Start => self.done(),
            Btn::L1 if self.cursor > 0 => {
                self.cursor -= 1;
                Typed::Changed
            }
            Btn::R1 if self.cursor < self.text.len() => {
                self.cursor += 1;
                Typed::Changed
            }
            _ => Typed::Nothing,
        }
    }
}

/// A special key's word. Symbols says where it goes back to while the symbols are up.
pub fn special_label(s: Special, symbols: bool) -> &'static str {
    match s {
        Delete => "Delete",
        Done => "Done",
        Shift => "Shift",
        Symbols if symbols => "abc",
        Symbols => "#+=",
        Space => "Space",
    }
}

/// Every word a special key can show, for the faces made at boot.
pub const SPECIAL_LABELS: [&str; 6] = ["Delete", "Done", "Shift", "#+=", "abc", "Space"];

/// How wide a special key's word may be set, inside its key.
pub fn special_room(label: &str) -> f32 {
    let special = match label {
        "Delete" => Delete,
        "Done" => Done,
        "Shift" => Shift,
        "Space" => Space,
        _ => Symbols,
    };
    units(Cap::Special(special)) * UNIT - GAP - 10.0
}

/// How wide each key is in units of a character key, row by row: the special keys are wider.
fn units(cap: Cap) -> f32 {
    match cap {
        Cap::Char(_) => 1.0,
        Cap::Special(Delete) | Cap::Special(Done) => 2.0,
        Cap::Special(Shift) | Cap::Special(Symbols) => 1.5,
        Cap::Special(Space) => 6.0,
    }
}

/// One character key's pitch across, and a row's pitch down. Twelve units is the widest row,
/// the numbers and Delete, which leaves 20 px at either edge.
const UNIT: f32 = 50.0;
const PITCH_Y: f32 = 48.0;
const GAP: f32 = 4.0;
/// The top of the first row of keys, which leaves the title and the typed text above them and
/// the legend below.
pub const KEYS_Y: f32 = 166.0;

/// Where a key is drawn, as x, y, width and height, every edge on a whole pixel. Each row is
/// centred on the screen.
pub fn key_rect(row: usize, col: usize) -> [f32; 4] {
    let caps = ROWS[row];
    let total: f32 = caps.iter().map(|c| units(*c)).sum::<f32>() * UNIT;
    let mut x = ((OUT_W as f32 - total) / 2.0).round();
    for cap in &caps[..col] {
        x += units(*cap) * UNIT;
    }
    let w = units(caps[col]) * UNIT - GAP;
    [x, KEYS_Y + row as f32 * PITCH_Y, w, PITCH_Y - GAP]
}

pub type Face = (TexId, u32, u32);

/// The faces made once at boot: a face for each character in each of the three sets, in
/// `LOWER`, `UPPER` and `SYMBOL` order, and one for each of `SPECIAL_LABELS`.
pub struct KeyboardFaces {
    pub chars: [Vec<Face>; 3],
    pub specials: Vec<Face>,
}

/// What changes as the player types, made as it changes: the title over the keys, and the text
/// either side of the cursor.
pub struct KeyboardText {
    pub title: Option<Face>,
    pub before: Option<Face>,
    pub after: Option<Face>,
}

/// The ground, the title, the field with the text and its cursor, and every key, the one in
/// hand on the bar's colour.
pub fn draw_keyboard(
    kb: &Keyboard,
    faces: &KeyboardFaces,
    text: &KeyboardText,
    out: &mut Vec<Draw>,
) {
    out.push(Draw::Rect {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: crate::draw::OUT_H as f32,
        colour: opening(),
    });
    if let Some((tex, w, h)) = text.title {
        push(out, tex, (OUT_W as f32 - w as f32) / 2.0, 30.0, w, h);
    }
    // The field: a recess the width of the keys, the text in it from the left, the cursor
    // after the text before it.
    let field = [40.0, 92.0, OUT_W as f32 - 80.0, 52.0];
    out.push(Draw::Rect {
        x: field[0],
        y: field[1],
        w: field[2],
        h: field[3],
        colour: recess(),
    });
    let mut x = field[0] + 8.0;
    let y = field[1] + (field[3] - 40.0) / 2.0;
    if let Some((tex, w, h)) = text.before {
        push(out, tex, x, y, w, h);
        x += w as f32 - 2.0 * crate::power_menu::MENU_PAD as f32;
    }
    out.push(Draw::Rect {
        x: x + crate::power_menu::MENU_PAD as f32,
        y: field[1] + 10.0,
        w: 2.0,
        h: field[3] - 20.0,
        colour: [1.0, 1.0, 1.0, 1.0],
    });
    if let Some((tex, w, h)) = text.after {
        push(out, tex, x + 4.0, y, w, h);
    }
    for (r, caps) in ROWS.iter().enumerate() {
        for (c, cap) in caps.iter().enumerate() {
            let [kx, ky, kw, kh] = key_rect(r, c);
            let lit = (r, c) == (kb.row(), kb.col());
            let lit_special = matches!(cap, Cap::Special(Shift)) && kb.shift();
            out.push(Draw::Rect {
                x: kx,
                y: ky,
                w: kw,
                h: kh,
                colour: if lit { edge() } else { recess() },
            });
            let face = match cap {
                Cap::Char(i) => faces.chars[kb.set(*i)].get(*i).copied(),
                Cap::Special(s) => {
                    let word = special_label(*s, kb.symbols());
                    SPECIAL_LABELS
                        .iter()
                        .position(|l| *l == word)
                        .and_then(|i| faces.specials.get(i).copied())
                }
            };
            if let Some((tex, w, h)) = face {
                let alpha = if lit || lit_special { 1.0 } else { 0.8 };
                out.push(Draw::Tex {
                    x: (kx + (kw - w as f32) / 2.0).round(),
                    y: (ky + (kh - h as f32) / 2.0).round(),
                    w: w as f32,
                    h: h as f32,
                    tex,
                    alpha,
                });
            }
        }
    }
}

fn push(out: &mut Vec<Draw>, tex: TexId, x: f32, y: f32, w: u32, h: u32) {
    out.push(Draw::Tex {
        x: x.round(),
        y: y.round(),
        w: w as f32,
        h: h as f32,
        tex,
        alpha: 1.0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(kb: &mut Keyboard, presses: &[Btn]) -> Vec<Typed> {
        presses.iter().map(|b| kb.press(*b)).collect()
    }

    #[test]
    fn a_starts_on_q_and_types_it() {
        let mut kb = Keyboard::new("Password", "");
        assert_eq!(kb.label(kb.rows()[kb.row()][kb.col()]), "q");
        kb.press(Btn::A);
        assert_eq!(kb.text(), "q");
    }

    #[test]
    fn shift_capitals_and_turns_the_numbers_into_their_symbols() {
        let mut kb = Keyboard::new("Password", "");
        kb.press(Btn::Select);
        kb.press(Btn::A);
        // Up from q is the 2 above it, the number row being the wider one and centred.
        kb.press(Btn::Up);
        kb.press(Btn::A);
        assert_eq!(kb.text(), "Q@");
    }

    #[test]
    fn b_deletes_x_spaces_start_finishes_and_y_gives_up() {
        let mut kb = Keyboard::new("Password", "ab");
        let out = typed(&mut kb, &[Btn::B, Btn::X, Btn::Start]);
        assert_eq!(out[2], Typed::Done("a ".into()));
        assert_eq!(kb.press(Btn::Y), Typed::Cancelled);
    }

    #[test]
    fn the_cursor_moves_and_types_where_it_is() {
        let mut kb = Keyboard::new("Password", "ac");
        kb.press(Btn::L1);
        assert_eq!(kb.split(), ("a".into(), "c".into()));
        // q on the second row, then along to w: "aw" then "c".
        kb.press(Btn::Right);
        kb.press(Btn::A);
        assert_eq!(kb.text(), "awc");
    }

    #[test]
    fn up_and_down_keep_to_one_column_through_rows_of_other_lengths() {
        let mut kb = Keyboard::new("Password", "");
        for _ in 0..4 {
            kb.press(Btn::Right);
        }
        let x = |kb: &Keyboard| {
            let [x, _, w, _] = key_rect(kb.row(), kb.col());
            x + w / 2.0
        };
        let start = x(&kb);
        kb.press(Btn::Down);
        kb.press(Btn::Down);
        kb.press(Btn::Up);
        kb.press(Btn::Up);
        assert_eq!(x(&kb), start);
    }

    #[test]
    fn every_key_is_on_the_panel_and_no_two_overlap() {
        let mut seen: Vec<[f32; 4]> = Vec::new();
        for (r, caps) in ROWS.iter().enumerate() {
            for c in 0..caps.len() {
                let k = key_rect(r, c);
                assert!(
                    k[0] >= 0.0 && k[0] + k[2] <= OUT_W as f32,
                    "{r},{c} at {k:?}"
                );
                assert!(
                    k[1] + k[3] <= crate::draw::OUT_H as f32 - 40.0,
                    "{r},{c} under the legend"
                );
                for o in &seen {
                    let apart = k[0] >= o[0] + o[2]
                        || o[0] >= k[0] + k[2]
                        || k[1] >= o[1] + o[3]
                        || o[1] >= k[1] + k[3];
                    assert!(apart, "{k:?} overlaps {o:?}");
                }
                seen.push(k);
            }
        }
    }
}
