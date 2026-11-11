//! A page of rows in the quick menu's own look, for menus whose rows are not known until they
//! are drawn: the Wi-Fi page's saved networks and the networks a scan finds. The same ground, the
//! same full-width bar behind the row in hand, labels 32 px in from the left and values 32 px in
//! from the right, the arrows either side of a value that Left and Right change, and the legend at
//! the foot.

use crate::draw::{Draw, TexId, OUT_H, OUT_W};
use crate::plate::{centred_hints, HINT_H, LEGEND_GAP};
use crate::power_menu::{MENU_H, MENU_PAD};
use crate::quick_menu::QUICK_EDGE;
use crate::slot_chrome::{edge, opening};

pub type Face = (TexId, u32, u32);

/// One row: its label, and its value if it has one, each grey and lit.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ListRow {
    pub label: Option<[Face; 2]>,
    pub value: Option<[Face; 2]>,
    /// Left and Right change the value, so the arrows stand either side of it while it is in hand.
    pub arrows: bool,
}

/// A page as it stands this frame.
pub struct ListMenu<'a> {
    pub rows: &'a [ListRow],
    pub selected: usize,
    /// Left, then right.
    pub carets: Option<[Face; 2]>,
    /// The legend's faces with their widths, already chosen for the row in hand.
    pub legend: &'a [(TexId, u32)],
}

/// The quick menu's own row pitch at its largest, which a short page can always have.
pub const LIST_PITCH: f32 = 40.0;
/// The legend's key caps, where the quick menu has them.
const LEGEND_Y: f32 = 427.0;
const TYPE_DROP: f32 = (LIST_PITCH - MENU_H as f32) / 2.0;
const BAR_INSET: f32 = 2.0;
const CARET_GAP: f32 = 14.0;
/// The most rows a page shows at once, above the legend. A longer list scrolls with the bar.
pub const LIST_SHOWN: usize = 9;
const LIST_TOP: f32 = ((LEGEND_Y - 8.0 - LIST_PITCH * LIST_SHOWN as f32) / 2.0) as i32 as f32;

impl ListMenu<'_> {
    /// The first row on screen, so the one in hand is always shown.
    pub fn first_shown(&self) -> usize {
        let n = self.rows.len();
        if n <= LIST_SHOWN {
            return 0;
        }
        self.selected
            .saturating_sub(LIST_SHOWN / 2)
            .min(n - LIST_SHOWN)
    }

    pub fn row_top(&self, i: usize) -> f32 {
        LIST_TOP + LIST_PITCH * (i - self.first_shown()) as f32
    }

    pub fn draw(&self, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: opening(),
        });
        if self.rows.is_empty() {
            return self.draw_legend(out);
        }
        let selected = self.selected.min(self.rows.len() - 1);
        out.push(Draw::Rect {
            x: 0.0,
            y: self.row_top(selected) + BAR_INSET,
            w: OUT_W as f32,
            h: LIST_PITCH - 2.0 * BAR_INSET,
            colour: edge(),
        });
        let first = self.first_shown();
        let (right, pad) = (OUT_W as f32 - QUICK_EDGE, MENU_PAD as f32);
        for (i, row) in self.rows.iter().enumerate().skip(first).take(LIST_SHOWN) {
            let y = self.row_top(i) + TYPE_DROP;
            let lit = i == selected;
            if let Some(label) = row.label {
                let (tex, w, h) = label[1];
                push(out, tex, QUICK_EDGE - pad, y, w, h);
            }
            let Some(value) = row.value else {
                continue;
            };
            let (tex, w, h) = value[lit as usize];
            match (lit && row.arrows, self.carets) {
                (true, Some([(lt, lw, lh), (rt, rw, rh)])) => {
                    let rx = right - rw as f32;
                    push(out, rt, rx, y, rw, rh);
                    let vx = rx - CARET_GAP + pad - w as f32;
                    push(out, tex, vx, y, w, h);
                    push(out, lt, vx + pad - CARET_GAP - lw as f32, y, lw, lh);
                }
                _ => push(out, tex, right + pad - w as f32, y, w, h),
            }
        }
        self.draw_legend(out);
    }

    fn draw_legend(&self, out: &mut Vec<Draw>) {
        for (tex, w, x) in centred_hints(self.legend, LEGEND_GAP) {
            push(out, tex, x, LEGEND_Y, w, HINT_H);
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

    fn rows(n: usize) -> Vec<ListRow> {
        (0..n)
            .map(|i| ListRow {
                label: Some([(TexId::from_raw(i), 100, 40); 2]),
                value: None,
                arrows: false,
            })
            .collect()
    }

    #[test]
    fn a_long_list_scrolls_to_keep_the_row_in_hand_on_screen() {
        let r = rows(20);
        for selected in 0..20 {
            let m = ListMenu {
                rows: &r,
                selected,
                carets: None,
                legend: &[],
            };
            let top = m.row_top(selected);
            assert!(
                top >= 0.0 && top + LIST_PITCH <= LEGEND_Y,
                "row {selected} at {top}"
            );
        }
    }

    #[test]
    fn the_rows_sit_clear_of_the_legend() {
        const { assert!(LIST_TOP >= 0.0) };
        const { assert!(LIST_TOP + LIST_PITCH * LIST_SHOWN as f32 <= LEGEND_Y - 8.0) };
    }
}
