mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::core_lock;
use slot::app::Phase;
use slot::audio::{AudioSink, StubSink};
use slot::core::open_core_for;
use slot::emu::{CoreState, EmuHandle};
use slot::session::Session;
use slot_input::{Btn, RawEvent};
use slot_retro::{ButtonMask, LibretroCore, RetroCore};
use slot_store::Core;

fn rom(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&p, common::gba_rom()).expect("write rom");
    p
}

/// The two cores are indistinguishable through the trait except by what they serialize:
/// mGBA writes out a whole machine, the mock writes a frame counter.
fn is_mgba(core: &mut dyn RetroCore, rom: &Path) -> bool {
    core.load(rom).expect("core refused the test rom");
    core.run_frame(ButtonMask::default());
    core.serialize().expect("core gave up no state").len() > 100_000
}

#[test]
fn the_vendored_core_is_preferred_over_the_mock() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    let d = common::tmp_root_with_real_carts(&[]);
    let mut core = open_core_for(d.path(), Core::Mgba, "auto", false, &[dylib]);
    assert!(
        is_mgba(core.as_mut(), &rom("preferred.gba")),
        "the mock ran with a vendored core sitting right there"
    );
}

#[test]
fn a_missing_core_falls_back_to_the_mock_rather_than_failing() {
    let d = common::tmp_root_with_real_carts(&[]);
    let mut core = open_core_for(
        d.path(),
        Core::Mgba,
        "auto",
        false,
        &[PathBuf::from("no/such/core.dylib")],
    );
    assert!(!is_mgba(core.as_mut(), &rom("missing.gba")));
}

/// A rom the core will not take has to reach the app as `Failed`. Anything else leaves the
/// insert animation waiting on a core that is never coming.
#[test]
fn a_rom_the_real_core_refuses_reports_failed() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    let d = common::tmp_root_with_carts(&["Broken"]);
    let emu = EmuHandle::spawn(
        open_core_for(d.path(), Core::Mgba, "auto", false, &[dylib]),
        d.path().join("Games/GBA/Broken.gba"),
        StubSink::new().ring(),
        None,
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while emu.state() == CoreState::Loading {
        assert!(Instant::now() < deadline, "the core never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(emu.state(), CoreState::Failed);
}

/// And the refusal has to reach the slot. The mock takes anything, so this is the one path
/// that only exists once the real core is the default.
#[test]
fn a_cart_the_real_core_refuses_comes_back_out_of_the_slot() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        return;
    };
    std::env::set_var("SLOT_CORE", dylib);
    let d = common::tmp_root_with_carts(&["Broken"]);
    common::clocked(d.path());
    let mut s = Session::boot(d.path().to_path_buf());
    s.feed([RawEvent::Down(Btn::A)], 16);
    s.update(1.0 / 60.0);
    assert!(matches!(s.app().phase(), Phase::Inserting { .. }));

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut now = 16;
    while !matches!(s.app().phase(), Phase::Shelf) {
        assert!(
            Instant::now() < deadline,
            "the refused cart never came back out"
        );
        now += 16;
        s.feed([], now);
        s.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Selects bank 1 of a 128 KB flash chip and then programs one byte a frame, 512 of them, so a
/// state loaded on any of those frames lands in the middle of the game writing its second bank.
/// The game code is Super Mario Advance 4's, which mGBA's override list gives a 128 KB flash
/// chip from the start, as it does every commercial game with one (Pokemon among them).
fn flash_bank_rom() -> PathBuf {
    const CODE: [u32; 44] = [
        0xe3a00301, //        mov  r0, #0x04000000
        0xe3a0140e, //        mov  r1, #0x0E000000      flash
        0xe2812c55, //        add  r2, r1, #0x5500
        0xe2822055, //        add  r2, r2, #0x55        0x0E005555
        0xe2813c2a, //        add  r3, r1, #0x2A00
        0xe28330aa, //        add  r3, r3, #0xAA        0x0E002AAA
        0xe3a0900a, //        mov  r9, #10
        0xeb00001c, // wait:  bl   vsync               ten frames first
        0xe2599001, //        subs r9, r9, #1
        0x1afffffc, //        bne  wait
        0xeb000014, //        bl   unlock
        0xe3a040b0, //        mov  r4, #0xB0
        0xe5c24000, //        strb r4, [r2]            bank select
        0xe3a04001, //        mov  r4, #1
        0xe5c14000, //        strb r4, [r1]            bank 1
        0xe3a05000, //        mov  r5, #0
        0xe0656185, // next:  rsb  r6, r5, r5, lsl #3
        0xe2866003, //        add  r6, r6, #3
        0xe20660ff, //        and  r6, r6, #0xFF       byte i is i * 7 + 3
        0xeb00000b, //        bl   unlock
        0xe3a040a0, //        mov  r4, #0xA0
        0xe5c24000, //        strb r4, [r2]            program
        0xe7c16005, //        strb r6, [r1, r5]
        0xe7d14005, // poll:  ldrb r4, [r1, r5]
        0xe1540006, //        cmp  r4, r6
        0x1afffffc, //        bne  poll                until the chip has it
        0xeb000009, //        bl   vsync
        0xe2855001, //        add  r5, r5, #1
        0xe3550c02, //        cmp  r5, #512
        0x1afffff1, //        bne  next
        0xeb000005, // idle:  bl   vsync
        0xeafffffd, //        b    idle
        0xe3a040aa, // unlock: mov r4, #0xAA
        0xe5c24000, //        strb r4, [r2]
        0xe3a04055, //        mov  r4, #0x55
        0xe5c34000, //        strb r4, [r3]
        0xe12fff1e, //        bx   lr
        0xe1d040b6, // vsync: ldrh r4, [r0, #6]        VCOUNT
        0xe35400a0, //        cmp  r4, #160
        0x2afffffc, //        bhs  vsync
        0xe1d040b6, // 1:     ldrh r4, [r0, #6]
        0xe35400a0, //        cmp  r4, #160
        0x3afffffc, //        blo  1b
        0xe12fff1e, //        bx   lr
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[0..4].copy_from_slice(&0xea00002eu32.to_le_bytes()); // b 0xc0
    rom[0xa0..0xac].copy_from_slice(b"FLASH BANK\0\0");
    rom[0xac..0xb0].copy_from_slice(b"AX4E");
    rom[0xb0..0xb2].copy_from_slice(b"01");
    rom[0xb2] = 0x96;
    let sum = rom[0xa0..0xbd].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    rom[0xbd] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    for (i, w) in CODE.iter().enumerate() {
        let o = 0xc0 + i * 4;
        rom[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join("flash-bank.gba");
    std::fs::write(&p, rom).expect("write flash bank rom");
    p
}

/// mGBA dropped which bank of a 128 KB flash chip was selected whenever a state was loaded, so
/// the writes after it went into bank 0. Run-ahead loads a state every frame, and it is on by
/// default, so a game saving to its second bank (Pokemon, Super Mario Advance 4) had its save
/// scrambled. `cores/mgba/flash-bank-survives-mask.patch`, from slot b0fe199, keeps the bank.
///
/// Measured before the patch: 511 of the 512 bytes went to bank 0 with a load every frame, and
/// the same with run-ahead at 1 and 2 frames. A game mGBA does not know, whose flash it detects
/// on first use, still never finishes its save under run-ahead, patch or not.
#[test]
fn on_mgba_loading_a_state_while_the_game_writes_its_second_flash_bank_keeps_the_save_whole() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        eprintln!("no vendored mGBA core, skipping");
        return;
    };
    let rom = flash_bank_rom();
    // `None` plays straight through, `Some(None)` loads a state every frame, `Some(Some(n))`
    // runs `n` frames ahead.
    let save = |how: Option<Option<u8>>| {
        let mut core =
            LibretroCore::open(&dylib).expect("vendored core is present but would not open");
        core.load(&rom).expect("mGBA refused the rom");
        let mut picture = Vec::new();
        for _ in 0..700 {
            match how {
                None => core.run_frame(ButtonMask::default()),
                Some(None) => {
                    core.run_frame(ButtonMask::default());
                    let state = core.serialize().expect("mGBA would not save");
                    core.unserialize(&state)
                        .expect("mGBA would not load its own state");
                }
                Some(Some(ahead)) => {
                    let (_, shown) =
                        slot::emu::run_ahead(&mut core, ButtonMask::default(), ahead, &mut picture);
                    assert!(shown.is_some(), "mGBA refused run-ahead");
                }
            }
            let _ = core.take_audio();
        }
        core.save_ram().expect("mGBA gave no save")
    };
    let played = save(None);
    let written = (0..512)
        .filter(|&i| played[0x10000 + i] == (i * 7 + 3) as u8)
        .count();
    assert_eq!(
        written, 512,
        "the rom never wrote its second bank, so this test proves nothing"
    );
    for (how, what) in [
        (Some(None), "loading a state every frame"),
        (Some(Some(1)), "running one frame ahead"),
        (Some(Some(2)), "running two frames ahead"),
    ] {
        let saved = save(how);
        let wrong = saved.iter().zip(&played).filter(|(a, b)| a != b).count();
        assert_eq!(
            wrong, 0,
            "{what} left {wrong} bytes of the save different from playing straight through"
        );
    }
}

/// Waits ten frames and writes one byte of SRAM. Its game code is one mGBA has no override for,
/// as Mario Kart: Super Circuit's is not, so until that write mGBA carries the whole 128 KB save
/// buffer in a state, and after it the 32 KB of SRAM it has taken the cart for.
fn sram_rom() -> PathBuf {
    const CODE: [u32; 17] = [
        0xe3a00301, //        mov  r0, #0x04000000
        0xe3a0140e, //        mov  r1, #0x0E000000      the save
        0xe3a0900a, //        mov  r9, #10
        0xeb000005, // wait:  bl   vsync
        0xe2599001, //        subs r9, r9, #1
        0x1afffffc, //        bne  wait
        0xe3a0405a, //        mov  r4, #0x5A
        0xe5c14000, //        strb r4, [r1]            one byte of SRAM
        0xeb000000, // idle:  bl   vsync
        0xeafffffd, //        b    idle
        0xe1d040b6, // vsync: ldrh r4, [r0, #6]        VCOUNT
        0xe35400a0, //        cmp  r4, #160
        0x2afffffc, //        bhs  vsync
        0xe1d040b6, // 1:     ldrh r4, [r0, #6]
        0xe35400a0, //        cmp  r4, #160
        0x3afffffc, //        blo  1b
        0xe12fff1e, //        bx   lr
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[0..4].copy_from_slice(&0xea00002eu32.to_le_bytes()); // b 0xc0
    rom[0xa0..0xac].copy_from_slice(b"SRAM SHRINK\0");
    rom[0xac..0xb0].copy_from_slice(b"ZSRS");
    rom[0xb0..0xb2].copy_from_slice(b"01");
    rom[0xb2] = 0x96;
    let sum = rom[0xa0..0xbd].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    rom[0xbd] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    for (i, w) in CODE.iter().enumerate() {
        let o = 0xc0 + i * 4;
        rom[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }
    let p = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sram-shrink.gba");
    std::fs::write(&p, rom).expect("write sram rom");
    p
}

/// A state that shrinks after its size was learned: here a save chip mGBA detected on first use. `serialize` is exact whatever came before, because a padded
/// state is refused by mGBA's link loader, which a host's pair is sent to; only run-ahead's own
/// save, which comes back into this same core, may carry the padding.
#[test]
fn on_mgba_a_save_after_the_state_shrank_is_exact() {
    let _g = core_lock();
    let Some(dylib) = common::vendored_core() else {
        eprintln!("no vendored mGBA core, skipping");
        return;
    };
    let rom = sram_rom();
    let mut core = LibretroCore::open(&dylib).expect("vendored core is present but would not open");
    core.load(&rom).expect("mGBA refused the rom");
    for _ in 0..2 {
        core.run_frame(ButtonMask::default());
    }
    let learned = core.serialize_runahead().expect("mGBA would not save");
    for _ in 0..30 {
        core.run_frame(ButtonMask::default());
    }
    // Run-ahead's save still reuses the size it learned, and pads to it.
    let padded = core.serialize_runahead().expect("mGBA would not save");
    let exact = core.serialize().expect("mGBA would not save");
    assert!(
        exact.len() < learned.len(),
        "a save after the state shrank was {} bytes, the {} it had been: padded, or the save \
         chip never shrank it",
        exact.len(),
        learned.len()
    );
    assert_eq!(padded.len(), learned.len());
    assert_eq!(
        &padded[..exact.len()],
        &exact[..],
        "run-ahead's save is not the same state with zeros after it"
    );
    assert_eq!(
        exact,
        core.serialize().expect("mGBA would not save"),
        "two saves of the same frame differed"
    );
}
