use slot_gfx::{OUT_H, OUT_W};

use crate::hud::{HUD_INK, PLATE_H};
use crate::icon::{haloed, HALO_PX};
use crate::text;
use crate::CartFace;

/// Everything the HUD ever says in words. Each answers something the user just did: two confirm
/// it, two answer the link shortcut where it cannot be carried out — on a core that cannot link
/// at all, and on a cart whose link gpSP cannot carry — either of which would otherwise do
/// nothing and say nothing, and the last two are a link session ending, from whichever end ended
/// it.
///
/// Three more used to name the shelf the shoulders had just moved to. They are gone: the shelf
/// is now said by the platform's mark in the top plate's corner, which is always there rather
/// than fading after a moment — see `slot_ui::mark`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Toast {
    StateSaved,
    StateLoaded,
    NeedsGpsp,
    NoLink,
    LinkEnded,
    /// The far end of a live session ended it and sent word before going. The other side of
    /// `LinkEnded`, and a separate sentence because which device ended it is the one thing the
    /// player on this one cannot see.
    PeerEnded,
    /// The host runs another GBA BIOS, so its game cannot run here.
    BiosMismatch,
    /// The card has turned read-only or a save failed to write. Not a toast that fades but the
    /// line the HUD's alarm band carries for as long as it lasts. From slot 239769d.
    CardUnwritable,
}

impl Toast {
    pub const ALL: [Toast; 8] = [
        Toast::StateSaved,
        Toast::StateLoaded,
        Toast::NeedsGpsp,
        Toast::NoLink,
        Toast::LinkEnded,
        Toast::PeerEnded,
        Toast::BiosMismatch,
        Toast::CardUnwritable,
    ];

    /// Position in `ALL`, which is the order faces are uploaded in.
    pub fn index(self) -> usize {
        self as usize
    }

    /// Set large in the middle of the screen, on the alarm band, rather than small in the plate.
    pub fn alarm(self) -> bool {
        self == Toast::CardUnwritable
    }

    /// A second, smaller line under the first.
    pub fn detail(self) -> Option<&'static str> {
        match self {
            Toast::CardUnwritable => Some("Progress is not being saved"),
            _ => None,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Toast::StateSaved => "State Saved",
            Toast::StateLoaded => "State Loaded",
            Toast::NeedsGpsp => "Please switch to gpSP",
            Toast::NoLink => "No link support",
            Toast::LinkEnded => "Link ended",
            // Passive, and deliberately so: on this device the link was ended by somebody else,
            // and "Link was ended" reads as something that happened to you rather than
            // something you did — which is the one distinction this end cannot see for itself.
            // Impersonal too, like every other line here ("No link support", "Nobody arrived");
            // the product says "friend" nowhere, so this is not the screen to start.
            Toast::PeerEnded => "Link was ended",
            Toast::BiosMismatch => "BIOS does not match",
            Toast::CardUnwritable => "Can't write to SD card",
        }
    }
}

/// One box for every string, so which one it is never moves the line. Wide enough for the
/// longest at full size: `fit` would otherwise shrink that one line and no other.
const TOAST_W: u32 = 240;
const TOAST_H: u32 = 22;
const TOAST_PX: f32 = 16.0;
const TOAST_MIN_PX: f32 = 12.0;

const ALARM_W: u32 = 560;
const ALARM_H: u32 = 44;
const ALARM_PX: f32 = 32.0;
const DETAIL_H: u32 = 30;
const DETAIL_PX: f32 = 20.0;

/// How a toast is set: its box, its type size and where its middle lands.
struct Setting {
    w: u32,
    h: u32,
    px: f32,
    min_px: f32,
    middle: f32,
}

fn setting(toast: Toast) -> Setting {
    match toast.alarm() {
        true => Setting {
            w: ALARM_W,
            h: ALARM_H,
            px: ALARM_PX,
            min_px: ALARM_PX,
            middle: OUT_H as f32 / 2.0,
        },
        false => Setting {
            w: TOAST_W,
            h: TOAST_H,
            px: TOAST_PX,
            min_px: TOAST_MIN_PX,
            middle: PLATE_H / 2.0,
        },
    }
}

/// Where the line lands, in offscreen pixels: in the plate, or for the alarm in the middle of
/// the screen.
pub fn toast_rect(toast: Toast) -> (f32, f32, f32, f32) {
    let (w, h) = toast_box(toast);
    let (w, h) = (w as f32, h as f32);
    let middle = setting(toast).middle;
    ((OUT_W as f32 - w) / 2.0, middle - h / 2.0, w, h)
}

/// The box a toast is rastered into. Every plate toast shares one, so which one it is never
/// moves the line.
pub fn toast_box(toast: Toast) -> (u32, u32) {
    let s = setting(toast);
    (s.w + 2 * HALO_PX, s.h + detail_h(toast) + 2 * HALO_PX)
}

fn detail_h(toast: Toast) -> u32 {
    match toast.detail() {
        Some(_) => DETAIL_H,
        None => 0,
    }
}

/// Transparent apart from the type and the halo dilated out of it. Nothing backs a toast, so
/// the halo is the only thing keeping it legible over a bright game frame.
pub fn toast_face(toast: Toast) -> CartFace {
    let Some(font) = text::label_font() else {
        return CartFace {
            rgba: Vec::new(),
            w: 0,
            h: 0,
        };
    };
    let s = setting(toast);
    let layout = text::fit(font, toast.text(), s.w as f32, 1, s.px, s.min_px);
    let mut cov = text::coverage(s.w, s.h, &layout);
    if let Some(detail) = toast.detail() {
        let layout = text::fit(font, detail, s.w as f32, 1, DETAIL_PX, DETAIL_PX);
        cov.extend(text::coverage(s.w, DETAIL_H, &layout));
    }
    haloed(&cov, s.w, s.h + detail_h(toast), HUD_INK)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ink's own width in the box, which is what a line actually occupies once the font
    /// has uppercased it.
    fn ink_width(toast: Toast) -> u32 {
        let font = text::label_font().expect("label font");
        let s = setting(toast);
        let layout = text::fit(font, toast.text(), s.w as f32, 1, s.px, s.min_px);
        let cov = text::coverage(s.w, s.h, &layout);
        let cols: Vec<u32> = (0..s.w)
            .filter(|x| (0..s.h).any(|y| cov[(y * s.w + x) as usize] > 0))
            .collect();
        match (cols.first(), cols.last()) {
            (Some(a), Some(b)) => b - a + 1,
            _ => 0,
        }
    }

    /// The rows of a rastered face with any ink in them: where the line actually sits once it
    /// is pixels rather than a layout.
    fn ink_rows(face: &CartFace) -> (u32, u32) {
        let mut first = None;
        let mut last = 0;
        for y in 0..face.h {
            let inked = (0..face.w).any(|x| face.rgba[((y * face.w + x) * 4 + 3) as usize] > 0);
            if inked {
                first.get_or_insert(y);
                last = y;
            }
        }
        (first.unwrap_or(0), last)
    }

    /// The ink's height in a face rastered at a stated size, for holding the shipped one
    /// against the fallback it would have been given.
    fn ink_height_at(toast: Toast, px: f32) -> u32 {
        let font = text::label_font().expect("label font");
        let layout = text::fit(font, toast.text(), TOAST_W as f32, 1, px, px);
        let cov = text::coverage(TOAST_W, TOAST_H, &layout);
        let rows: Vec<u32> = (0..TOAST_H)
            .filter(|y| (0..TOAST_W).any(|x| cov[(y * TOAST_W + x) as usize] > 0))
            .collect();
        match (rows.first(), rows.last()) {
            (Some(a), Some(b)) => b - a + 1,
            _ => 0,
        }
    }

    /// The face, not the layout. What reaches the screen is a box of pixels, and a line that
    /// had to be shrunk arrives as a shorter run of ink in it — the same banner in a smaller
    /// hand, which is the failure this is here to catch.
    ///
    /// Against two references, because neither alone is enough: the 12 px fallback, which the
    /// shipped line has to stand clearly taller than, and a line already known to fit, which
    /// it has to match. Matching is to within a pixel rather than exactly, since round and
    /// pointed capitals (the S and A of STATE SAVED) overshoot the flat tops of LINK ENDED by
    /// one row at this size — a fact about the typeface, not about the size it is set at.
    #[test]
    fn the_ended_line_is_rastered_the_size_the_others_are() {
        let ended = toast_face(Toast::LinkEnded);
        let saved = toast_face(Toast::StateSaved);
        assert_eq!(
            (ended.w, ended.h),
            (saved.w, saved.h),
            "one box holds every banner"
        );
        let (top, bottom) = ink_rows(&ended);
        assert!(bottom > top, "the line rastered to nothing at all");

        let shipped = ink_height_at(Toast::LinkEnded, TOAST_PX);
        let shrunk = ink_height_at(Toast::LinkEnded, TOAST_MIN_PX);
        assert!(
            shipped > shrunk,
            "the shipped line is no taller than the {TOAST_MIN_PX} px fallback: {shipped} against {shrunk}"
        );
        let reference = ink_height_at(Toast::StateSaved, TOAST_PX);
        assert!(
            shipped.abs_diff(reference) <= 1,
            "LINK ENDED is {shipped} px of ink where STATE SAVED is {reference}"
        );
    }

    /// Every banner shares one box, and `fit` answers a line that will not fit by shrinking it
    /// — to 12 px, against the 16 every other line is set at. Nothing warns: the banner simply
    /// comes up smaller, which reads as a different banner rather than as this one saying
    /// something else. So each line is held to full size here, where adding copy that does not
    /// fit fails rather than ships.
    #[test]
    fn every_toast_is_set_at_full_size() {
        let font = text::label_font().expect("label font");
        for t in Toast::ALL {
            let s = setting(t);
            let layout = text::fit(font, t.text(), s.w as f32, 1, s.px, s.min_px);
            assert_eq!(
                layout.px,
                s.px,
                "{:?} ({:?}) was shrunk to {} px to fit {}",
                t,
                t.text(),
                layout.px,
                s.w
            );
            assert_eq!(layout.lines.len(), 1, "{t:?} wrapped onto a second line");
            let w = ink_width(t);
            assert!(w <= s.w, "{:?} is {w} px wide in a {} px box", t, s.w);
            if let Some(detail) = t.detail() {
                let layout = text::fit(font, detail, s.w as f32, 1, DETAIL_PX, DETAIL_PX);
                assert_eq!(layout.lines.len(), 1, "{t:?}'s detail wrapped");
            }
        }
    }
}
