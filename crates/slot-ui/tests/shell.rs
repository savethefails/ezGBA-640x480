use slot_store::scan;
use slot_ui::{
    cart_face, gba_shell_for, lookup_order_is_exact_then_family_then_default, mould_of, shell_for,
    table_keys, Finish, Mould, SnesShell, DEFAULT_SHELL, DMG_SHELL, DUAL_MODE_SHELL, SFC_SHELL,
    SNES_SHELL,
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

/// North American carts are the boxy shell in its mid grey; Japan's and PAL's are the rounded
/// Super Famicom shell in the lighter one. Read off the header's destination code.
#[test]
fn a_snes_carts_region_picks_its_shell() {
    let (_d, us) = snes_cart("Super Mario World", "SUPER MARIOWORLD", 0x01);
    assert_eq!(mould_of(&us), Mould::Snes(SnesShell::Boxy));
    assert_eq!(shell_for(&us), SNES_SHELL);
    for (region, what) in [(0x00, "Japan"), (0x02, "Europe")] {
        let (_d, cart) = snes_cart("Super Mario World", "SUPER MARIOWORLD", region);
        assert_eq!(mould_of(&cart), Mould::Snes(SnesShell::Rounded), "{what}");
        assert_eq!(shell_for(&cart), SFC_SHELL, "{what}");
    }
    assert!(
        SFC_SHELL
            .colour
            .iter()
            .zip(SNES_SHELL.colour)
            .all(|(a, b)| *a > b),
        "the Super Famicom's grey is the lighter one"
    );
}

#[test]
fn killer_instinct_is_black_on_both_shells() {
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

/// A dump with no header that checks out is drawn as ezGBA always drew a SNES cart.
#[test]
fn a_snes_rom_with_no_header_is_the_north_american_cart() {
    let d = tempfile::tempdir().expect("tempdir");
    let games = d.path().join("Games/SNES");
    std::fs::create_dir_all(&games).expect("games dir");
    std::fs::write(games.join("Homebrew.sfc"), vec![0xffu8; 0x8000]).expect("rom");
    let cart = &scan(d.path()).expect("scan")[0];
    assert_eq!(mould_of(cart), Mould::Snes(SnesShell::Boxy));
    assert_eq!(shell_for(cart), SNES_SHELL);
}

/// The two shells share a box on the shelf but not an outline: the rounded one's top corners
/// are swept well in from where the boxy one's are.
#[test]
fn the_snes_shells_differ_at_the_top_corners_and_share_a_size() {
    let (_d, us) = snes_cart("A", "A", 0x01);
    let (_e, jp) = snes_cart("A", "A", 0x00);
    let (us, jp) = (cart_face(&us), cart_face(&jp));
    assert_eq!((us.w, us.h), (jp.w, jp.h));
    let alpha = |f: &slot_ui::CartFace, x: u32, y: u32| f.rgba[((y * f.w + x) * 4 + 3) as usize];
    assert_eq!(alpha(&us, 6, 6), 255, "the boxy shell is square there");
    assert_eq!(alpha(&us, 1, 1), 0, "the boxy shell has its small chamfer");
    assert_eq!(alpha(&jp, 3, 3), 0, "the rounded shell is swept away there");
    assert_eq!(alpha(&jp, 2, 30), 255, "and full height below the sweep");
}

/// `cart_shell.ini` can pick either SNES shell; a Game Boy word reads as the rom's own.
#[test]
fn cart_shell_ini_picks_a_snes_mould() {
    let d = tempfile::tempdir().expect("tempdir");
    snes_rom(d.path(), "Chrono Trigger", "CHRONO TRIGGER", 0x01);
    std::fs::create_dir_all(d.path().join("Config")).expect("config");
    std::fs::write(
        d.path().join("Config/cart_shell.ini"),
        "Chrono Trigger = rounded abaaad solid\n",
    )
    .expect("ini");
    let cart = &scan(d.path()).expect("scan")[0];
    assert_eq!(mould_of(cart), Mould::Snes(SnesShell::Rounded));
    std::fs::write(
        d.path().join("Config/cart_shell.ini"),
        "Chrono Trigger = notched 102030 solid\n",
    )
    .expect("ini");
    let cart = &scan(d.path()).expect("scan")[0];
    assert_eq!(mould_of(cart), Mould::Snes(SnesShell::Boxy));
    assert_eq!(shell_for(cart).colour, [0x10, 0x20, 0x30]);
}

#[test]
fn the_flag_plastics_are_unchanged() {
    assert_eq!(gb_shell(b"TETRIS", b"", 0x00, true), DMG_SHELL);
    assert_eq!(gb_shell(b"TETRIS DX", b"", 0x80, true), DUAL_MODE_SHELL);
}
