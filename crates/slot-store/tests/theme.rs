use slot_store::{Aspect, BootPicture, LcdGrid, Scaling, Theme};

/// The card is edited on a desktop by hand. Every way that can go wrong has to leave a device
/// that still boots and a slot that is still visible.
#[test]
fn a_broken_line_leaves_that_colour_alone_and_the_rest_applies() {
    let t = Theme::parse(
        "housing #102030\n\
         recess  not-a-colour\n\
         openin  #ffffff\n\
         edge\n\
         opening #010203\n",
    );
    let d = Theme::default();
    assert_eq!(t.housing, [0x10, 0x20, 0x30], "a good line was dropped");
    assert_eq!(
        t.opening,
        [0x01, 0x02, 0x03],
        "a line after a bad one was lost"
    );
    assert_eq!(t.recess, d.recess, "a malformed value was taken anyway");
    assert_eq!(t.edge, d.edge, "a line with no value was taken anyway");
}

/// `#` opens a comment only at the start of a line, because it is also how a colour is
/// written. Reading it both ways silently drops every colour in the file.
#[test]
fn a_leading_hash_is_a_comment_and_a_value_hash_is_not() {
    let t = Theme::parse("# housing #ffffff\nhousing #102030\n");
    assert_eq!(t.housing, [0x10, 0x20, 0x30]);
}

#[test]
fn a_colour_reads_with_or_without_its_hash() {
    assert_eq!(
        Theme::parse("edge #0a0b0c").edge,
        Theme::parse("edge 0a0b0c").edge
    );
}

/// Trailing junk means the line was meant as something else. Taking the first two words of it
/// turns a typo into a colour nobody chose.
#[test]
fn a_line_with_more_than_a_name_and_a_value_is_ignored() {
    assert_eq!(
        Theme::parse("housing #102030 #405060").housing,
        Theme::default().housing
    );
}

/// A card with no theme is the common case, not an error.
#[test]
fn a_missing_file_is_the_default_theme() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(Theme::read(d.path()), Theme::default());
}

#[test]
fn menu_off_is_read_and_the_menu_is_on_by_default() {
    assert!(Theme::default().menu);
    let t = Theme::parse("menu off\nscrim #F7E7CE\n");
    assert!(!t.menu);
    assert_eq!(t.scrim, [0xf7, 0xe7, 0xce]);
    assert!(Theme::parse("menu maybe").menu);
}

/// 4:3 fills the panel and is what a card with no `picture` line gets; 3:2 is asked for by
/// name. Anything else leaves the default, as a misspelt colour does.
#[test]
fn the_picture_is_four_three_unless_the_card_asks_for_three_two() {
    assert_eq!(Theme::parse("").picture, Aspect::FourThree);
    assert_eq!(Theme::parse("picture 3:2").picture, Aspect::ThreeTwo);
    assert_eq!(Theme::parse("PICTURE 3:2").picture, Aspect::ThreeTwo);
    assert_eq!(
        Theme::parse("picture 3:2\npicture 4:3").picture,
        Aspect::FourThree
    );
    assert_eq!(Theme::parse("picture 16:9").picture, Aspect::FourThree);
    assert_eq!(
        Theme::parse("picture 3:2 please").picture,
        Aspect::FourThree
    );
}

/// Pixel AA at 1.0 unless the card says otherwise; a sharpness outside 0.0 to 2.0, or one that
/// is not a number, leaves the default.
#[test]
fn the_scaler_is_pixel_aa_at_one_unless_the_card_says_otherwise() {
    let t = Theme::parse("");
    assert_eq!((t.scaler, t.sharpness), (Scaling::PixelAa, 1.0));
    assert_eq!(
        Theme::parse("scaler shimmerless").scaler,
        Scaling::Shimmerless
    );
    assert_eq!(Theme::parse("sharpness 1.5").sharpness, 1.5);
    assert_eq!(Theme::parse("sharpness 3").sharpness, 1.0);
    assert_eq!(Theme::parse("sharpness sharp").sharpness, 1.0);
}

/// No grid unless the card asks for one.
#[test]
fn the_grid_is_off_unless_the_card_asks_for_it() {
    assert_eq!(Theme::parse("").grid, LcdGrid::Off);
    assert_eq!(Theme::parse("grid on").grid, LcdGrid::On);
    assert_eq!(Theme::parse("grid strict").grid, LcdGrid::Strict);
    assert_eq!(Theme::parse("grid lcd").grid, LcdGrid::Lcd);
    assert_eq!(Theme::parse("grid on\ngrid off").grid, LcdGrid::Off);
    assert_eq!(Theme::parse("grid lots").grid, LcdGrid::Off);
}

/// The grid's depth is the card's to set, within what still draws a line and no darker than
/// black; anything else leaves the grid's own default.
#[test]
fn grid_depth_is_read_as_a_percentage() {
    assert_eq!(Theme::parse("").grid_depth, None);
    assert_eq!(Theme::parse("grid-depth 60").grid_depth, Some(60.0));
    assert_eq!(Theme::parse("grid-depth 32.5").grid_depth, Some(32.5));
    assert_eq!(Theme::parse("grid-depth 2").grid_depth, None);
    assert_eq!(Theme::parse("grid-depth 150").grid_depth, None);
    assert_eq!(Theme::parse("grid-depth dark").grid_depth, None);
}

/// The settings menu writes one line and leaves the rest of the file as the user wrote it: a
/// line already setting it is rewritten in place, later repeats of it go, comments that mention
/// it stay, and a card with no file gets one.
#[test]
fn write_theme_setting_changes_one_line_and_keeps_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("Config/theme.txt");
    let read = || std::fs::read_to_string(&path).unwrap();

    slot_store::write_theme_setting(d.path(), "grid", "lcd").unwrap();
    assert_eq!(read(), "grid lcd\n");

    std::fs::write(
        &path,
        "# grid on looks nice\nscrim #F7E7CE\nGRID on\npicture 3:2\ngrid strict\n",
    )
    .unwrap();
    slot_store::write_theme_setting(d.path(), "grid", "off").unwrap();
    assert_eq!(
        read(),
        "# grid on looks nice\nscrim #F7E7CE\ngrid off\npicture 3:2\n"
    );
    slot_store::write_theme_setting(d.path(), "grid-depth", "70").unwrap();
    assert_eq!(
        read(),
        "# grid on looks nice\nscrim #F7E7CE\ngrid off\npicture 3:2\ngrid-depth 70\n"
    );
    let theme = Theme::read(d.path());
    assert_eq!(theme.grid, LcdGrid::Off);
    assert_eq!(theme.grid_depth, Some(70.0));
    assert_eq!(theme.scrim, [0xF7, 0xE7, 0xCE]);
}

/// Run-ahead is 0, 1 or 2 frames, or `off`; anything else leaves the default.
#[test]
fn runahead_is_read_as_a_frame_count() {
    assert_eq!(Theme::parse("").runahead, None);
    assert_eq!(Theme::parse("runahead 0").runahead, Some(0));
    assert_eq!(Theme::parse("runahead off").runahead, Some(0));
    assert_eq!(Theme::parse("runahead 2").runahead, Some(2));
    assert_eq!(Theme::parse("runahead 3").runahead, None);
    assert_eq!(Theme::parse("runahead lots").runahead, None);
}

#[test]
fn the_boot_picture_is_the_last_screen_unless_turned_off() {
    let read = |text: &str| Theme::parse(text).boot_picture;
    assert_eq!(read(""), BootPicture::Last);
    assert_eq!(read("boot-picture off\n"), BootPicture::Off);
    assert_eq!(read("boot-picture OFF\n"), BootPicture::Off);
    assert_eq!(read("boot-picture last\n"), BootPicture::Last);
    assert_eq!(
        read("boot-picture never\n"),
        BootPicture::Last,
        "a typo is the default"
    );
}
