use slot_ui::{
    toast_face, toast_rect, Draw, Hud, HudKind, Toast, ALARM_BAND_H, OUT_H, OUT_W, PLATE_H,
};

#[test]
fn saving_and_loading_say_which_one_happened() {
    assert_eq!(Toast::StateSaved.text(), "State Saved");
    assert_eq!(Toast::StateLoaded.text(), "State Loaded");
}

/// The link shortcut on a core that cannot link says which one can, in the same banner.
#[test]
fn the_link_shortcut_on_the_wrong_core_says_to_switch() {
    assert_eq!(Toast::NeedsGpsp.text(), "Please switch to gpSP");
    let f = toast_face(Toast::NeedsGpsp);
    assert!(f.rgba.chunks(4).any(|p| p[3] > 0), "the banner is blank");
}

/// gpSP fakes named protocols and has no generic cable, so for any other cart there is no link
/// to make. The shortcut says so in the same banner rather than bringing a radio up for a game
/// that will never see a packet.
#[test]
fn a_cart_gpsp_cannot_link_says_there_is_no_link() {
    assert_eq!(Toast::NoLink.text(), "No link support");
    let f = toast_face(Toast::NoLink);
    assert!(f.rgba.chunks(4).any(|p| p[3] > 0), "the banner is blank");
}

/// Every banner the HUD can raise answers something the user just did, and none of them merely
/// describes what is already on screen. The carousel briefly had three that named the shelf it
/// had moved to; that is the plate corner's job now — see `slot_ui::mark` — and a banner saying it
/// as well would be the same fact told twice. So the list is held at seven, by name, because the
/// way a line like that comes back is one variant at a time.
#[test]
fn the_banner_says_what_happened_and_never_what_is_on_screen() {
    assert_eq!(
        Toast::ALL,
        [
            Toast::StateSaved,
            Toast::StateLoaded,
            Toast::NeedsGpsp,
            Toast::NoLink,
            Toast::LinkEnded,
            Toast::PeerEnded,
            Toast::BiosMismatch,
            Toast::GameMismatch,
            Toast::CardUnwritable,
        ],
        "a banner was added or dropped: every face is uploaded by its place in this list"
    );
    for (i, t) in Toast::ALL.iter().enumerate() {
        assert_eq!(t.index(), i, "{t:?} does not answer to its own place");
        let f = toast_face(*t);
        assert!(
            f.rgba.chunks(4).any(|p| p[3] > 0),
            "{t:?} rastered to a blank banner"
        );
    }
}

/// Every line is rastered into one box at one size, so a line too long for it would be shrunk on
/// its own and read as a different banner from the others. Measured off the pixels rather than
/// off the layout: the type is uppercased and has no descenders, so a banner set at the same
/// size covers the same rows.
///
/// To within a row, which is the typeface rather than the size. Round and pointed capitals
/// overshoot the flat ones — the S and A of STATE SAVED sit a row below the K and D of LINK
/// ENDED at 16 px — so a line built only from flat capitals is a row shorter while being set at
/// exactly the same size. A shrunk line is not a row out; it is four, which is what the bound
/// below actually catches. `toast.rs`'s own unit test holds the size itself, against both the
/// full size and the fallback.
#[test]
fn no_toast_is_shrunk_to_fit_its_box() {
    let rows = |t: Toast| {
        let f = toast_face(t);
        let inked: Vec<usize> = (0..f.h as usize)
            .filter(|y| (0..f.w as usize).any(|x| f.rgba[(y * f.w as usize + x) * 4 + 3] > 0))
            .collect();
        let first = *inked.first().expect("the banner is blank");
        let last = *inked.last().expect("the banner is blank");
        (first, last)
    };
    let (top, bottom) = rows(Toast::StateSaved);
    // The card alarm is set large on its own band, not in the plate's box.
    for t in Toast::ALL.into_iter().filter(|t| !t.alarm()) {
        let (a, b) = rows(t);
        assert!(
            a.abs_diff(top) <= 1 && b.abs_diff(bottom) <= 1,
            "{t:?} sits on rows {a}..{b} where the others sit on {top}..{bottom}, so it was shrunk to fit"
        );
    }
}

#[test]
fn a_toast_fades_on_the_same_curve_as_the_bar() {
    let mut h = Hud::new();
    h.toast(Toast::StateSaved, 1_000);
    assert!(h.toast_visible(2_499));
    assert!(!h.toast_visible(2_500));
}

/// Saying something twice re-shows the one banner rather than queueing a second behind it: the
/// HUD holds one line and one clock, and a second call re-stamps that clock. It matters wherever
/// a button can be worked faster than a banner fades, which is every button that raises one.
///
/// Held here rather than through the app, which is where it used to be: the three shelf banners
/// were what exercised it, and the shelf says which system it is in the plate's corner now.
#[test]
fn saying_the_same_thing_twice_re_shows_it_rather_than_stacking() {
    let mut h = Hud::new();
    h.toast(Toast::StateSaved, 1_000);
    h.toast(Toast::StateSaved, 2_400);
    // Past the first stamp's own fade and short of the second's, so the banner is up here only
    // because the second call moved the clock forward.
    assert_eq!(h.said(3_400), Some(Toast::StateSaved), "it did not re-show");
    assert_eq!(h.said(3_900), None, "it never faded");
}

/// A different banner replaces the one showing rather than waiting behind it. The same one slot,
/// read the other way round: what is on screen is always the last thing that happened.
#[test]
fn a_second_banner_replaces_the_first() {
    let mut h = Hud::new();
    h.toast(Toast::StateSaved, 1_000);
    h.toast(Toast::LinkEnded, 1_100);
    assert_eq!(h.said(1_200), Some(Toast::LinkEnded));
}

#[test]
fn a_toast_is_centred() {
    let (x, _, w, _) = toast_rect(Toast::StateSaved);
    assert_eq!(x + w / 2.0, OUT_W as f32 / 2.0);
}

/// Nothing backs the type, so the type carries its own contrast or it disappears on a white
/// game frame. Same halo the badge beside it uses.
#[test]
fn a_toast_carries_its_own_halo() {
    let f = toast_face(Toast::StateSaved);
    let dark = f
        .rgba
        .chunks(4)
        .any(|p| p[3] > 0 && p[0] < 0x40 && p[1] < 0x40 && p[2] < 0x40);
    assert!(dark, "there is nothing dark behind the type");
}

/// The toast reads against the same plate the level bar does, in the same place. It used to
/// sit below the band with no backing, which put two different treatments on one screen.
#[test]
fn a_toast_sits_in_the_plate_band_and_is_backed_by_it() {
    let mut h = Hud::new();
    h.toast(Toast::StateSaved, 0);
    let mut out = Vec::new();
    h.draw(0, &mut out);

    let plate = out
        .iter()
        .find(|d| matches!(d, Draw::Rect { w, .. } if *w == OUT_W as f32))
        .expect("the toast has nothing to be read against");
    let Draw::Rect { colour, h: ph, .. } = plate else {
        unreachable!()
    };
    assert!(colour[3] > 0.6, "the plate is too faint to give contrast");
    assert!((*ph - PLATE_H).abs() < 0.01, "the plate is not the band");

    let (_, y, _, th) = toast_rect(Toast::StateSaved);
    assert!(
        y >= 0.0 && y + th <= PLATE_H,
        "the toast at {y} is outside the band"
    );
}

/// They share one strip, so only one can have it. A toast names something that just
/// happened; a level is visible in its own effect.
#[test]
fn a_toast_takes_the_band_from_the_bar() {
    let mut h = Hud::new();
    h.show(HudKind::Volume, 50, false, 0);
    let mut bar_only = Vec::new();
    h.draw(0, &mut bar_only);
    let bars = bar_only.len();

    h.toast(Toast::StateSaved, 0);
    let mut both = Vec::new();
    h.draw(0, &mut both);
    assert!(
        both.len() < bars,
        "the bar is still drawn underneath the toast"
    );
}

// The card alarm, from slot 239769d.
fn band(h: &Hud, now: u64) -> Option<(f32, f32, [f32; 4])> {
    let mut out = Vec::new();
    h.draw(now, &mut out);
    out.iter().rev().find_map(|d| match d {
        Draw::Rect {
            y, w, h, colour, ..
        } if *w == OUT_W as f32 && *h == ALARM_BAND_H => Some((*y, *h, *colour)),
        _ => None,
    })
}

#[test]
fn the_card_alarm_stays_up_until_it_is_cleared() {
    let mut h = Hud::new();
    h.set_alarm(true, 1_000);
    assert!(
        band(&h, 3_600_000).is_some(),
        "the alarm went away by itself"
    );
    h.set_alarm(false, 3_600_001);
    let mut out = Vec::new();
    h.draw(3_600_002, &mut out);
    assert!(out.is_empty(), "the alarm outlived a write that landed");
}

#[test]
fn the_card_alarm_is_a_red_band_across_the_middle_that_pulses() {
    let mut h = Hud::new();
    h.set_alarm(true, 0);
    let (y, bh, bright) = band(&h, 0).expect("no alarm band");
    let middle = OUT_H as f32 / 2.0;
    assert!(
        (y + bh / 2.0 - middle).abs() < 0.5,
        "the band is not centred"
    );
    assert!(
        bright[0] > 2.0 * bright[1] && bright[0] > 2.0 * bright[2],
        "{bright:?} is not red"
    );
    let (_, _, dim) = band(&h, 250).unwrap();
    assert!(
        dim[0] < bright[0] - 0.2,
        "the band did not pulse: {bright:?} then {dim:?}"
    );
}

#[test]
fn a_second_failure_does_not_restart_the_pulse() {
    let mut h = Hud::new();
    h.set_alarm(true, 0);
    h.set_alarm(true, 250);
    let mut fresh = Hud::new();
    fresh.set_alarm(true, 0);
    assert_eq!(band(&h, 500), band(&fresh, 500));
}

#[test]
fn the_card_alarm_text_is_large_and_inside_its_band() {
    let alarm = toast_face(Toast::CardUnwritable);
    let toast = toast_face(Toast::StateSaved);
    assert!(alarm.h > toast.h && alarm.w > toast.w);
    let (x, y, w, th) = toast_rect(Toast::CardUnwritable);
    let top = (OUT_H as f32 - ALARM_BAND_H) / 2.0;
    assert!(
        y >= top && y + th <= top + ALARM_BAND_H,
        "the alarm text overhangs its band"
    );
    assert_eq!(x + w / 2.0, OUT_W as f32 / 2.0);
}

#[test]
fn the_card_alarm_is_drawn_over_the_volume_bar() {
    let mut h = Hud::new();
    h.set_alarm(true, 0);
    h.show(HudKind::Volume, 50, false, 1_000);
    let mut out = Vec::new();
    h.draw(1_100, &mut out);
    let alarm = out
        .iter()
        .position(|d| matches!(d, Draw::Rect { h, .. } if *h == ALARM_BAND_H))
        .expect("the bar hid the alarm");
    assert!(alarm > 0, "the bar was not drawn under the alarm");
    assert!(
        out[alarm + 1..]
            .iter()
            .all(|d| matches!(d, Draw::Tex { .. })),
        "something was drawn over the alarm band"
    );
}

#[test]
fn the_card_alarm_says_what_failed_and_what_it_costs() {
    assert_eq!(Toast::CardUnwritable.text(), "Can't write to SD card");
    assert_eq!(
        Toast::CardUnwritable.detail(),
        Some("Progress is not being saved")
    );
    assert_eq!(Toast::StateSaved.detail(), None);
}

#[test]
fn the_card_alarm_inks_both_of_its_lines() {
    let f = toast_face(Toast::CardUnwritable);
    let inked = |rows: std::ops::Range<u32>| {
        rows.into_iter()
            .any(|y| (0..f.w).any(|x| f.rgba[((y * f.w + x) * 4 + 3) as usize] > 0x80))
    };
    let half = f.h / 2;
    assert!(inked(0..half), "the first line is blank");
    assert!(inked(half + 8..f.h), "the second line is blank");
}
