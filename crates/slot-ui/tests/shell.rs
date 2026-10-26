use slot_store::scan;
use slot_ui::{
    cart_face, gba_shell_for, lookup_order_is_exact_then_family_then_default, mould_of,
    pico8_candy_of, pico8_shell_for, shell_for, table_keys, Finish, Mould, DEFAULT_SHELL,
    DMG_SHELL, DUAL_MODE_SHELL, PICO8_CANDY, SNES_SHELL,
};
use tempfile::TempDir;

fn tmp_root() -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    for sub in ["Games", "Games/GBA", "Labels", "Saves", "States", "System"] {
        std::fs::create_dir(d.path().join(sub)).expect("create content dir");
    }
    d
}

fn write_rom_with_code(d: &TempDir, name: &str, title: &str, code: &str) {
    let mut rom = vec![0u8; 0x100];
    rom[0xa0..0xa0 + title.len()].copy_from_slice(title.as_bytes());
    rom[0xac..0xac + code.len()].copy_from_slice(code.as_bytes());
    std::fs::write(d.path().join("Games/GBA").join(name), rom).expect("write rom");
}

#[test]
fn an_unknown_game_gets_the_default_grey() {
    assert_eq!(gba_shell_for("ZZZZ").colour, DEFAULT_SHELL.colour);
    assert_eq!(gba_shell_for("").colour, DEFAULT_SHELL.colour);
    // A real, ordinary cart: Metroid Fusion, verified as AMTE. Most of the library lands
    // here and it must not be a special case.
    assert_eq!(gba_shell_for("AMTE").colour, DEFAULT_SHELL.colour);
}

#[test]
fn leafgreen_is_green_whatever_region_it_came_from() {
    for code in ["BPGE", "BPGJ", "BPGP", "BPGD"] {
        let s = gba_shell_for(code);
        assert_ne!(
            s.colour, DEFAULT_SHELL.colour,
            "{code} fell through to grey"
        );
        assert!(s.colour[1] > s.colour[0], "{code} is not green");
    }
}

/// Verified from a real header: Shrek GBA Video is MSKE. The family letter is what saves
/// this from being thirty hand transcribed rows.
#[test]
fn gba_video_carts_are_light_grey() {
    let v = gba_shell_for("MSKE");
    assert_ne!(
        v.colour, DEFAULT_SHELL.colour,
        "video fell through to the default grey"
    );
    assert!(
        v.colour.iter().all(|c| *c > 0xA0),
        "video shells are light grey, got {:?}",
        v.colour
    );
    // The whole family, not just the one title that was on hand.
    assert_eq!(gba_shell_for("MPOE").colour, v.colour);
}

/// An explicit row has to beat the family letter, or the escape hatch does not work.
#[test]
fn an_exact_entry_outranks_the_family_letter() {
    assert_eq!(gba_shell_for("MSKE").colour, gba_shell_for("MSKJ").colour);
    // Adding an exact "MSK" row must be able to override; the lookup order is what is
    // under test, so assert it directly rather than through the table.
    assert!(lookup_order_is_exact_then_family_then_default());
}

/// The Pokemon rows are the clear ones and everything else is solid. Gen 3 shipped in coloured
/// translucent shells; the table drew them solid until it was noticed, which is the kind of
/// detail the shelf exists to get right. The Game Boy Advance Video family stays solid, so this
/// also pins that the finish is per row rather than per colour.
#[test]
fn the_clear_carts_are_clear_and_the_rest_are_solid() {
    for code in table_keys() {
        let clear = code == "RZW" || ["AX", "BPE", "U3"].iter().any(|p| code.starts_with(p));
        let want = if clear {
            Finish::Translucent
        } else {
            Finish::Solid
        };
        assert_eq!(
            gba_shell_for(code).finish,
            want,
            "{code} has the wrong finish"
        );
    }
    assert_eq!(
        gba_shell_for("MSKE").finish,
        Finish::Solid,
        "the video family is not solid"
    );
    assert_eq!(
        gba_shell_for("ZZZZ").finish,
        Finish::Solid,
        "the default is not solid"
    );
}

#[test]
fn the_pokemon_shells_are_all_distinct() {
    let codes = ["AXVE", "AXPE", "BPEE", "BPRE", "BPGE"];
    let mut seen = Vec::new();
    for c in codes {
        let col = gba_shell_for(c).colour;
        assert!(
            !seen.contains(&col),
            "{c} shares a colour with another cart"
        );
        seen.push(col);
    }
}

/// A three character key is short enough to collide by accident. It must not.
#[test]
fn no_two_table_entries_share_a_key() {
    let mut keys: Vec<&str> = table_keys();
    keys.sort();
    let before = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), before, "two entries claim the same key");
    assert!(
        keys.iter().all(|k| k.len() == 3 || k.len() == 4),
        "keys are a region free prefix or one region's whole code"
    );
}

#[test]
fn the_label_does_not_cover_the_whole_shell() {
    let d = tmp_root();
    write_rom_with_code(&d, "Drill Dozer.gba", "DRILL DOZER", "V49E");
    let cart = &scan(d.path()).unwrap()[0];
    let f = cart_face(cart);
    let px = |x: u32, y: u32| {
        let i = ((y * f.w + x) * 4) as usize;
        [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2]]
    };
    let shell = gba_shell_for("V49E").colour;
    assert_eq!(
        px(f.w / 2, 4),
        shell,
        "the label reaches the top edge, no shell shows"
    );
    assert_ne!(
        px(f.w / 2, f.h / 2),
        shell,
        "the label is missing from the middle"
    );
}

#[test]
fn a_region_that_shipped_other_plastic_gets_its_own() {
    let us = gba_shell_for("RZWE");
    let jp = gba_shell_for("RZWJ");
    assert_eq!(us.finish, Finish::Translucent);
    assert_eq!(jp.finish, Finish::Solid);
    assert!(jp.colour.iter().all(|c| *c > 0xd0), "Japan's is white");
    assert_ne!(us.colour, DEFAULT_SHELL.colour);
}

#[test]
fn boktai_is_clear_everywhere_and_drill_dozer_is_red() {
    for code in ["U3IE", "U3IP", "U3IJ", "U32E", "U33J"] {
        assert_eq!(gba_shell_for(code).finish, Finish::Translucent, "{code}");
    }
    let dozer = gba_shell_for("V49E");
    assert!(dozer.colour[0] > dozer.colour[2] + 40, "Drill Dozer is red");
    assert_eq!(dozer.colour, gba_shell_for("V49J").colour);
}

fn gb_shell(title: &[u8], code: &[u8], cgb: u8, japan: bool) -> slot_ui::Shell {
    let d = tempfile::tempdir().expect("tempdir");
    let games = d.path().join("Games/GB");
    std::fs::create_dir_all(&games).expect("games dir");
    let mut rom = vec![0u8; 0x150];
    rom[0x134..0x134 + title.len()].copy_from_slice(title);
    rom[0x13f..0x13f + code.len()].copy_from_slice(code);
    rom[0x143] = cgb;
    rom[0x14a] = u8::from(!japan);
    std::fs::write(games.join("Pak.gb"), rom).expect("rom");
    shell_for(&scan(d.path()).expect("scan")[0])
}

#[test]
fn red_and_blue_are_coloured_outside_japan_only() {
    let red = gb_shell(b"POKEMON RED", b"", 0x00, false);
    let blue = gb_shell(b"POKEMON BLUE", b"", 0x00, false);
    assert!(red.colour[0] > red.colour[2] + 60, "Red is red");
    assert!(blue.colour[2] > blue.colour[0] + 60, "Blue is blue");
    assert_eq!(
        gb_shell(b"POKEMON RED", b"", 0x00, true),
        DMG_SHELL,
        "Japan's Red is the grey pak"
    );
}

#[test]
fn gold_is_gold_except_in_japan() {
    let gold = gb_shell(b"POKEMON_GLD", b"AAUE", 0x80, false);
    assert_ne!(gold, DUAL_MODE_SHELL);
    assert_eq!(gold, gb_shell(b"POKEMON_GLD", b"AAUD", 0x80, false));
    assert_eq!(
        gb_shell(b"POKEMON_GLD", b"AAUJ", 0x80, true),
        DUAL_MODE_SHELL
    );
    assert_ne!(gb_shell(b"POKEMON_SLV", b"AAXE", 0x80, false), gold);
}

#[test]
fn crystal_is_aqua_under_both_its_codes() {
    let en = gb_shell(b"PM_CRYSTAL", b"BYTE", 0xc0, false);
    let jp = gb_shell(b"PM_CRYSTAL", b"BXTJ", 0xc0, true);
    assert_eq!(en, jp);
    assert_eq!(
        en.finish,
        Finish::Glitter,
        "Crystal's plastic has glitter in it"
    );
    assert!(en.colour[2] > en.colour[0], "Crystal is aqua");
}

#[test]
fn metal_gear_solid_is_black_not_clear() {
    assert_eq!(
        gb_shell(b"METALGEARGB", b"BMGE", 0xc0, false),
        DUAL_MODE_SHELL
    );
}

#[test]
fn pinball_is_yellow_in_japan_only() {
    let jp = gb_shell(b"POKEPINBALL", b"VPHJ", 0x80, true);
    assert!(
        jp.colour[0] > 0xc0 && jp.colour[2] < 0x60,
        "Japan's is yellow"
    );
    assert_eq!(
        gb_shell(b"POKEPINBALL", b"VPHE", 0x80, false),
        DUAL_MODE_SHELL
    );
}

#[test]
fn an_unlisted_pak_keeps_its_flags_plastic() {
    assert_eq!(gb_shell(b"TETRIS", b"", 0x00, false), DMG_SHELL);
    assert_eq!(gb_shell(b"ZELDA", b"AZLE", 0x80, false), DUAL_MODE_SHELL);
}

/// A SNES rom with a LoROM header that checks out, `title` padded to its 21 bytes, for
/// `region`'s destination code.
fn snes_rom(dir: &std::path::Path, stem: &str, title: &str, region: u8) {
    let games = dir.join("Games/SNES");
    std::fs::create_dir_all(&games).expect("games dir");
    let mut rom = vec![0u8; 0x8000];
    let h = 0x7fc0;
    let mut name = [b' '; 21];
    name[..title.len()].copy_from_slice(title.as_bytes());
    rom[h..h + 21].copy_from_slice(&name);
    rom[h + 0x19] = region;
    rom[h + 0x1c..h + 0x1e].copy_from_slice(&0x1234u16.to_le_bytes());
    rom[h + 0x1e..h + 0x20].copy_from_slice(&(0x1234u16 ^ 0xffff).to_le_bytes());
    std::fs::write(games.join(format!("{stem}.sfc")), rom).expect("rom");
}

fn snes_cart(stem: &str, title: &str, region: u8) -> (TempDir, slot_store::Cart) {
    let d = tempfile::tempdir().expect("tempdir");
    snes_rom(d.path(), stem, title, region);
    let cart = scan(d.path()).expect("scan").remove(0);
    (d, cart)
}

/// Every SNES cart is the North American one, in its grey, whatever region the game is from.
#[test]
fn every_snes_cart_is_the_north_american_one() {
    for region in [0x00, 0x01, 0x02] {
        let (_d, cart) = snes_cart("Super Mario World", "SUPER MARIOWORLD", region);
        assert_eq!(mould_of(&cart), Mould::Snes, "{region}");
        assert_eq!(shell_for(&cart), SNES_SHELL, "{region}");
    }
}

#[test]
fn killer_instinct_is_black() {
    for region in [0x01, 0x02] {
        let (_d, cart) = snes_cart("Killer Instinct", "KILLER INSTINCT", region);
        let s = shell_for(&cart);
        assert!(
            s.colour.iter().all(|c| *c < 0x40),
            "{region}: {:?}",
            s.colour
        );
        assert_eq!(s.finish, Finish::Solid);
    }
}

/// A dump with no header that checks out is the usual grey cart.
#[test]
fn a_snes_rom_with_no_header_is_the_grey_cart() {
    let d = tempfile::tempdir().expect("tempdir");
    let games = d.path().join("Games/SNES");
    std::fs::create_dir_all(&games).expect("games dir");
    std::fs::write(games.join("Homebrew.sfc"), vec![0xffu8; 0x8000]).expect("rom");
    let cart = &scan(d.path()).expect("scan")[0];
    assert_eq!(mould_of(cart), Mould::Snes);
    assert_eq!(shell_for(cart), SNES_SHELL);
}

/// The centre column stands taller than the wings, and the label runs up to the top edge.
#[test]
fn the_snes_cart_has_its_centre_column_and_a_label_to_the_top() {
    let (_d, cart) = snes_cart("A", "A", 0x01);
    let f = cart_face(&cart);
    let alpha = |x: u32, y: u32| f.rgba[((y * f.w + x) * 4 + 3) as usize];
    assert_eq!(alpha(10, 2), 0, "the wing stands as tall as the column");
    assert_eq!(alpha(f.w / 2, 0), 255, "the column does not reach the top");
    assert_eq!(alpha(10, 10), 255, "the wing is missing");
}

/// `cart_shell.txt` still colours a SNES cart; a mould word means nothing to it.
#[test]
fn cart_shell_ini_colours_a_snes_cart() {
    let d = tempfile::tempdir().expect("tempdir");
    snes_rom(d.path(), "Chrono Trigger", "CHRONO TRIGGER", 0x01);
    std::fs::create_dir_all(d.path().join("Config")).expect("config");
    std::fs::write(
        d.path().join("Config/cart_shell.txt"),
        "Chrono Trigger = rounded 102030 solid\n",
    )
    .expect("ini");
    let cart = &scan(d.path()).expect("scan")[0];
    assert_eq!(mould_of(cart), Mould::Snes);
    assert_eq!(shell_for(cart).colour, [0x10, 0x20, 0x30]);
}

#[test]
fn the_flag_plastics_are_unchanged() {
    assert_eq!(gb_shell(b"TETRIS", b"", 0x00, true), DMG_SHELL);
    assert_eq!(gb_shell(b"TETRIS DX", b"", 0x80, true), DUAL_MODE_SHELL);
}

/// PICO-8's red, blue, green, yellow, light grey and white.
const P8_RED: [u8; 3] = [0xff, 0x00, 0x4d];
const P8_BLUE: [u8; 3] = [0x29, 0xad, 0xff];
const P8_GREEN: [u8; 3] = [0x00, 0xe4, 0x36];
const P8_YELLOW: [u8; 3] = [0xff, 0xec, 0x27];
const P8_GREY: [u8; 3] = [0xc2, 0xc3, 0xc7];
const P8_WHITE: [u8; 3] = [0xff, 0xf1, 0xe8];

fn write_png(path: &std::path::Path, w: u32, h: u32, colour: impl Fn(u32, u32) -> [u8; 3]) {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend(colour(x, y));
            rgba.push(0xff);
        }
    }
    let f = std::fs::File::create(path).expect("png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&rgba).unwrap();
}

/// The shell of a `.p8.png` cart named `stem` whose 128 x 128 picture is `picture`, the rest of
/// the cart PICO-8's dark grey.
fn pico8_shell(stem: &str, picture: impl Fn(u32, u32) -> [u8; 3]) -> slot_ui::Shell {
    let d = tempfile::tempdir().expect("tempdir");
    let games = d.path().join("Games/PICO8");
    std::fs::create_dir_all(&games).expect("games dir");
    write_png(&games.join(format!("{stem}.png")), 160, 205, |x, y| {
        if (16..144).contains(&x) && (24..152).contains(&y) {
            picture(x - 16, y - 24)
        } else {
            [0x5f, 0x57, 0x4f]
        }
    });
    shell_for(&scan(d.path()).expect("scan")[0])
}

fn candy(colour: [u8; 3]) -> usize {
    PICO8_CANDY
        .iter()
        .position(|c| *c == colour)
        .expect("a candy")
}

/// A red car in the middle of a blue sky over green grass is a red cart: the sky and the grass
/// fill the picture's edges, so they are what is behind the game, not what it is about.
#[test]
fn a_pico8_cart_takes_its_shell_from_the_subject_of_its_picture() {
    let shell = pico8_shell("racer.p8", |x, y| {
        if (40..88).contains(&x) && (48..96).contains(&y) {
            P8_RED
        } else if y < 80 {
            P8_BLUE
        } else {
            P8_GREEN
        }
    });
    assert_eq!(shell.colour, PICO8_CANDY[6], "red");
    assert_eq!(shell.finish, Finish::Translucent);
}

/// A picture of one colour edge to edge is still that colour's.
#[test]
fn a_pico8_picture_of_one_colour_gives_that_colour() {
    let shell = pico8_shell("meadow.p8", |_, _| P8_GREEN);
    assert_eq!(candy(shell.colour), 5, "green");
}

/// Black, the greys and white lend a shell nothing, so the name picks as it did before.
#[test]
fn a_pico8_picture_with_no_colour_falls_back_to_the_name() {
    let shell = pico8_shell("mono.p8", |x, _| if x < 64 { P8_GREY } else { P8_WHITE });
    assert_eq!(shell, pico8_shell_for("mono.p8"));
}

/// A picture in `Labels/PICO8/` is the cart's label, so it is what the shell is taken from too,
/// whatever its size.
#[test]
fn a_pico8_label_picture_decides_the_shell() {
    let d = tempfile::tempdir().expect("tempdir");
    let games = d.path().join("Games/PICO8");
    let labels = d.path().join("Labels/PICO8");
    std::fs::create_dir_all(&games).expect("games dir");
    std::fs::create_dir_all(&labels).expect("labels dir");
    std::fs::write(games.join("sun.p8"), "pico-8 cartridge\n").expect("cart");
    write_png(&labels.join("sun.png"), 300, 200, |_, _| P8_YELLOW);
    let cart = &scan(d.path()).expect("scan")[0];
    assert!(cart.label.is_some(), "the label is found");
    assert_eq!(candy(shell_for(cart).colour), 7, "yellow");
}

#[test]
fn pico8_candy_of_a_short_buffer_is_none() {
    assert_eq!(pico8_candy_of(&[0; 16]), None);
}
