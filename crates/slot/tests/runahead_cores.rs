//! Run-ahead against the real cores: every picture it shows is the one plain play draws a frame
//! later, and every sample it plays is the one plain play makes, in order. After slot's
//! `tests/runahead.rs`, which checks the picture on mGBA alone.
//!
//! The sound is the half that has failed. mGBA's state lost the GBA tone and wave channels'
//! frequencies, so after a load they played at their lowest pitch, and run-ahead loads every
//! frame (`cores/mgba/gba-psg-frequency-state.patch`). A load also left every channel's output
//! level, the GBA resampler and the Game Boy's part-filled sound chunk where the frames ahead had
//! put them (`psg-output-level-state.patch`, `zzz-runahead-audio-output.patch`). On snes9x, slot
//! tells it not to draw the real frame, not to make the frames ahead's sound, and to load by its
//! fast path; any of those that left something behind would show here too.
//!
//! Each core is skipped unless it is in `vendor/` (`task core`, `task core:snes9x`,
//! `task core:snes9x2005` build them). The roms are generated here.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use slot::emu::run_ahead;
use slot_retro::{ButtonMask, LibretroCore, RetroCore};

/// Presents compared, after the warm up. Long enough for the SNES rom's tone to have started
/// and run through many loops of its sample.
const PRESENTS: usize = 240;
/// Frames run before the comparison, so the roms are past their set up.
const WARMUP: usize = 60;

/// One core at a time: snes9x and snes9x2005 share their internal names, and two of them live in
/// one process at once trip over each other.
static CORES: Mutex<()> = Mutex::new(());

fn vendored(name: &str) -> Option<PathBuf> {
    let p = common::repo_root().join(format!(
        "vendor/{name}_libretro.{}",
        std::env::consts::DLL_EXTENSION
    ));
    if !p.exists() {
        eprintln!("no vendored {name} on this host, skipping");
    }
    p.exists().then_some(p)
}

/// What plain play drew and played, frame by frame, from a fresh load.
fn plain(core: &mut LibretroCore, rom: &Path, frames: usize) -> (Vec<Vec<u8>>, Vec<Vec<i16>>) {
    core.load(rom).expect("the rom would not load");
    core.set_frame_skip(false);
    let (mut pictures, mut sound) = (Vec::new(), Vec::new());
    for _ in 0..frames {
        core.run_frame(ButtonMask(0));
        pictures.push(core.video_xrgb8888().to_vec());
        sound.push(core.take_audio());
    }
    (pictures, sound)
}

fn check(name: &str, dylib: &Path, rom: &Path) {
    let _one = CORES.lock().unwrap_or_else(|e| e.into_inner());
    let mut core = LibretroCore::open(dylib).expect("the vendored core would not open");
    for ahead in [1u8, 2] {
        let (pictures, sound) = plain(&mut core, rom, WARMUP + PRESENTS + usize::from(ahead));
        assert!(
            sound[WARMUP..].iter().flatten().any(|&s| s != 0),
            "{name}: the rom made no sound, so the sound is not being compared"
        );

        core.load(rom).expect("the rom would not load");
        for want in &sound[..WARMUP] {
            core.set_frame_skip(false);
            core.run_frame(ButtonMask(0));
            assert_eq!(
                &core.take_audio(),
                want,
                "{name}: a second load is not the same game"
            );
        }
        let mut shown = Vec::new();
        for present in 0..PRESENTS {
            let frame = WARMUP + present;
            let (audio, kept) = run_ahead(&mut core, ButtonMask(0), ahead, &mut shown);
            kept.unwrap_or_else(|| panic!("{name}: run-ahead could not save and load"));
            assert!(
                audio == sound[frame],
                "{name} at {ahead} ahead: the sound of frame {frame} is not plain play's"
            );
            assert!(
                shown == pictures[frame + usize::from(ahead)],
                "{name} at {ahead} ahead: the picture shown at frame {frame} is not frame {}",
                frame + usize::from(ahead)
            );
        }
    }
}

#[test]
fn mgba_run_ahead_shows_and_plays_what_plain_play_does() {
    let Some(dylib) = vendored("mgba") else {
        return;
    };
    let rom = Path::new(env!("CARGO_TARGET_TMPDIR")).join("runahead-sound.gba");
    std::fs::write(&rom, gba_rom()).expect("write the rom");
    check("mgba", &dylib, &rom);
}

#[test]
fn mgba_game_boy_run_ahead_shows_and_plays_what_plain_play_does() {
    let Some(dylib) = vendored("mgba") else {
        return;
    };
    let rom = Path::new(env!("CARGO_TARGET_TMPDIR")).join("runahead-sound.gb");
    std::fs::write(&rom, gb_rom()).expect("write the rom");
    check("mgba (Game Boy)", &dylib, &rom);
}

#[test]
fn snes9x_run_ahead_shows_and_plays_what_plain_play_does() {
    let Some(dylib) = vendored("snes9x") else {
        return;
    };
    let rom = Path::new(env!("CARGO_TARGET_TMPDIR")).join("runahead-mode7.sfc");
    std::fs::write(&rom, snes_rom()).expect("write the rom");
    check("snes9x", &dylib, &rom);
}

#[test]
fn snes9x2005_run_ahead_shows_and_plays_what_plain_play_does() {
    let Some(dylib) = vendored("snes9x2005_plus") else {
        return;
    };
    let rom = Path::new(env!("CARGO_TARGET_TMPDIR")).join("runahead-mode7-2005.sfc");
    std::fs::write(&rom, snes_rom()).expect("write the rom");
    check("snes9x2005", &dylib, &rom);
}

/// A GBA rom that plays a tone on the first square channel and counts frames into the first
/// pixel of a mode 3 screen, so both the sound and the picture change every frame.
fn gba_rom() -> Vec<u8> {
    const CODE: [u32; 28] = [
        0xe3a00404, // mov  r0, #0x04000000
        0xe3a01c04, // mov  r1, #0x400
        0xe3811003, // orr  r1, r1, #3
        0xe5801000, // str  r1, [r0]            DISPCNT: mode 3, BG2 on
        0xe3a01080, // mov  r1, #0x80
        0xe1c018b4, // strh r1, [r0, #0x84]     SOUNDCNT_X: sound on
        0xe59f1048, // ldr  r1, =0x1177         SOUNDCNT_L: square 1 on both sides, full
        0xe1c018b0, // strh r1, [r0, #0x80]
        0xe3a01002, // mov  r1, #2
        0xe1c018b2, // strh r1, [r0, #0x82]     SOUNDCNT_H: the PSG at full
        0xe3a01b3e, // mov  r1, #0xf800
        0xe2811080, // add  r1, r1, #0x80
        0xe1c016b2, // strh r1, [r0, #0x62]     SOUND1CNT_H: volume 15, held, 50% duty
        0xe59f1030, // ldr  r1, =0x8600
        0xe1c016b4, // strh r1, [r0, #0x64]     SOUND1CNT_X: start a 256 Hz tone
        0xe3a02406, // mov  r2, #0x06000000
        0xe3a03000, // mov  r3, #0
        0xe1d040b6, // vb:  ldrh r4, [r0, #6]   VCOUNT
        0xe35400a0, //      cmp  r4, #160
        0x1afffffc, //      bne  vb
        0xe2833001, //      add  r3, r3, #1
        0xe1c230b0, //      strh r3, [r2]
        0xe1d040b6, // dr:  ldrh r4, [r0, #6]
        0xe35400a0, //      cmp  r4, #160
        0x0afffffc, //      beq  dr
        0xeafffff6, //      b    vb
        0x00001177, // the two literals the `ldr`s read
        0x00008600,
    ];
    let mut rom = vec![0u8; 0x8000];
    rom[0..4].copy_from_slice(&0xea00002eu32.to_le_bytes()); // b 0xc0
    rom[0xa0..0xac].copy_from_slice(b"SLOT SOUND\0\0");
    rom[0xac..0xb0].copy_from_slice(b"SLSN");
    rom[0xb0..0xb2].copy_from_slice(b"00");
    rom[0xb2] = 0x96; // fixed header byte, cores sniff it to identify a GBA rom
    let sum = rom[0xa0..0xbd].iter().fold(0u8, |a, b| a.wrapping_add(*b));
    rom[0xbd] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    for (i, w) in CODE.iter().enumerate() {
        let o = 0xc0 + i * 4;
        rom[o..o + 4].copy_from_slice(&w.to_le_bytes());
    }
    rom
}

/// A Game Boy rom that plays a tone on the first square channel and scrolls the background a
/// pixel every frame.
fn gb_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x40] = 0xD9; // the vblank interrupt: reti
    rom[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]); // nop; jp $0150
                                                                  // All mGBA asks of a Game Boy rom is these four bytes, the start of the header's logo, which
                                                                  // its own `GBIsROM` carries. There is no boot rom to check the rest.
    rom[0x104..0x108].copy_from_slice(&[0xCE, 0xED, 0x66, 0x66]);
    let code: [u8; 39] = [
        0x3E, 0x80, 0xE0, 0x26, // NR52: sound on
        0x3E, 0x77, 0xE0, 0x24, // NR50: both sides at full
        0x3E, 0x11, 0xE0, 0x25, // NR51: channel 1 on both sides
        0x3E, 0x80, 0xE0, 0x11, // NR11: 50% duty
        0x3E, 0xF0, 0xE0, 0x12, // NR12: volume 15, held
        0x3E, 0x00, 0xE0, 0x13, // NR13
        0x3E, 0x87, 0xE0, 0x14, // NR14: start a 512 Hz tone
        0x3E, 0x01, 0xE0, 0xFF, // IE: vblank
        0xFB, // ei
        0x76, // loop: halt
        0xF0, 0x43, 0x3C, 0xE0, 0x43, // SCX += 1
    ];
    rom[0x150..0x150 + code.len()].copy_from_slice(&code);
    rom[0x150 + code.len()..0x150 + code.len() + 2].copy_from_slice(&[0x18, 0xF8]); // jr loop
    rom[0x134..0x13E].copy_from_slice(b"RUN AHEAD\0");
    let sum = rom[0x134..0x14D]
        .iter()
        .fold(0u8, |x, &b| x.wrapping_sub(b).wrapping_sub(1));
    rom[0x14D] = sum;
    rom
}

/// A 32 KiB LoROM in Mode 7 with a matrix that turns a little every frame, the effect that
/// Mario Kart's track is drawn with, and a looping square wave on the sound chip's first voice,
/// uploaded through the IPL rom's own protocol.
fn snes_rom() -> Vec<u8> {
    // The sound program, at $0200 in the sound chip's memory: the sample directory (one entry),
    // one looping BRR block of a square wave, then code that sets voice 0 going and spins.
    let mut spc: Vec<u8> = vec![0x04, 0x02, 0x04, 0x02]; // source 0: start and loop at $0204
    spc.extend([0xB3, 0x77, 0x77, 0x77, 0x77, 0x99, 0x99, 0x99, 0x99]); // range 11, loop, end
    let entry = 0x0200 + spc.len() as u16;
    for (reg, value) in [
        (0x6C, 0x20), // FLG: unmuted, echo writes off
        (0x0C, 0x7F), // main volume
        (0x1C, 0x7F),
        (0x2C, 0x00), // echo volume
        (0x3C, 0x00),
        (0x4D, 0x00), // no echo
        (0x5D, 0x02), // the directory is at $0200
        (0x00, 0x7F), // voice 0: volume
        (0x01, 0x7F),
        (0x02, 0x00), // pitch $1000, the sample's own rate
        (0x03, 0x10),
        (0x04, 0x00), // source 0
        (0x05, 0x00), // gain, not ADSR
        (0x07, 0x7F), // a fixed gain
        (0x5C, 0x00), // nothing keyed off
        (0x4C, 0x01), // key voice 0 on
    ] {
        spc.extend([0x8F, reg, 0xF2, 0x8F, value, 0xF3]); // mov $F2,#reg; mov $F3,#value
    }
    spc.extend([0x2F, 0xFE]); // bra self
    const SPC_AT: u16 = 0x8800;

    let mut rom = vec![0u8; 0x8000];
    for (i, b) in rom.iter_mut().enumerate().skip(0x1000) {
        *b = ((i * 7) ^ (i >> 3) ^ (i >> 9)) as u8;
    }
    let lo = |v: u16| v as u8;
    let hi = |v: u16| (v >> 8) as u8;
    let dma = |b_reg: u8, src: u16, size: u16| {
        vec![
            0xA9,
            b_reg,
            0x8D,
            0x01,
            0x43, // lda #b_reg; sta $4301
            0xA2,
            lo(src),
            hi(src),
            0x8E,
            0x02,
            0x43, // ldx #src; stx $4302
            0x9C,
            0x04,
            0x43, // stz $4304: bank 0
            0xA2,
            lo(size),
            hi(size),
            0x8E,
            0x05,
            0x43, // ldx #size; stx $4305
            0xA9,
            0x01,
            0x8D,
            0x0B,
            0x42, // lda #1; sta $420B
        ]
    };
    let mut c: Vec<u8> = vec![
        0x78, 0x18, 0xFB, // sei; clc; xce: native mode
        0xC2, 0x10, // rep #$10: 16-bit X and Y
        0xE2, 0x20, // sep #$20: 8-bit A
        0xA9, 0x8F, 0x8D, 0x00, 0x21, // force blank
    ];
    // The upload: wait for the IPL rom's $BBAA, name the address, then each byte in turn.
    let ready = c.len();
    c.extend([0xAD, 0x40, 0x21, 0xC9, 0xAA]); // lda $2140; cmp #$AA
    c.extend([0xD0, (ready as i32 - (c.len() as i32 + 2)) as u8]); // bne ready
    c.extend([0xA2, 0x00, 0x02, 0x8E, 0x42, 0x21]); // ldx #$0200; stx $2142
    c.extend([0xA9, 0x01, 0x8D, 0x41, 0x21]); // lda #1; sta $2141
    c.extend([0xA9, 0xCC, 0x8D, 0x40, 0x21]); // lda #$CC; sta $2140
    let ack = c.len();
    c.extend([0xCD, 0x40, 0x21]); // cmp $2140
    c.extend([0xD0, (ack as i32 - (c.len() as i32 + 2)) as u8]); // bne ack
    c.extend([0xA2, 0x00, 0x00]); // ldx #0
    let byte = c.len();
    c.extend([0xBD, lo(SPC_AT), hi(SPC_AT), 0x8D, 0x41, 0x21]); // lda spc,x; sta $2141
    c.extend([0x8A, 0x8D, 0x40, 0x21]); // txa; sta $2140
    let echo = c.len();
    c.extend([0xCD, 0x40, 0x21]); // cmp $2140
    c.extend([0xD0, (echo as i32 - (c.len() as i32 + 2)) as u8]); // bne echo
    c.extend([0xE8, 0xE0, lo(spc.len() as u16), hi(spc.len() as u16)]); // inx; cpx #len
    c.extend([0xD0, (byte as i32 - (c.len() as i32 + 2)) as u8]); // bne byte
    c.extend([0xA0, lo(entry), hi(entry), 0x8C, 0x42, 0x21]); // ldy #entry; sty $2142
    c.extend([0x9C, 0x41, 0x21]); // stz $2141: run it
    c.extend([0x8A, 0x1A, 0x1A, 0x8D, 0x40, 0x21]); // txa; inc; inc; sta $2140
                                                    // Mode 7, BG1 alone, an identity matrix to start with.
    c.extend([
        0xA9, 0x07, 0x8D, 0x05, 0x21, // BGMODE 7
        0x9C, 0x1A, 0x21, // M7SEL
        0xA9, 0x01, 0x8D, 0x2C, 0x21, // TM: BG1
        0x9C, 0x1B, 0x21, 0xA9, 0x01, 0x8D, 0x1B, 0x21, // M7A $0100
        0x9C, 0x1C, 0x21, 0x9C, 0x1C, 0x21, // M7B 0
        0x9C, 0x1D, 0x21, 0x9C, 0x1D, 0x21, // M7C 0
        0x9C, 0x1E, 0x21, 0xA9, 0x01, 0x8D, 0x1E, 0x21, // M7D $0100
        0x9C, 0x00, 0x43, // DMA 0: one register, A to B
    ]);
    // The tile map in the low bytes of VRAM, the tiles in the high bytes, then the palette.
    c.extend([0x9C, 0x15, 0x21, 0xA2, 0x00, 0x00, 0x8E, 0x16, 0x21]);
    c.extend(dma(0x18, 0x9000, 0x4000));
    c.extend([
        0xA9, 0x80, 0x8D, 0x15, 0x21, 0xA2, 0x00, 0x00, 0x8E, 0x16, 0x21,
    ]);
    c.extend(dma(0x19, 0xA000, 0x4000));
    c.extend([0x9C, 0x21, 0x21]);
    c.extend(dma(0x22, 0x9000, 0x0200));
    c.extend([0xA9, 0x0F, 0x8D, 0x00, 0x21]); // screen on
    c.extend([0xA9, 0x81, 0x8D, 0x00, 0x42]); // NMI on
    c.extend([0xCB, 0x80, 0xFD]); // wai; bra
                                  // The NMI turns the matrix by the frame count.
    let nmi = 0x8000 + c.len() as u16;
    c.extend([
        0xAD, 0x10, 0x42, // acknowledge
        0xE6, 0x00, // inc $00
        0xA5, 0x00, 0x8D, 0x1B, 0x21, 0xA9, 0x01, 0x8D, 0x1B, 0x21, // M7A $01nn
        0xA5, 0x00, 0x8D, 0x1C, 0x21, 0x9C, 0x1C, 0x21, // M7B $00nn
        0xA5, 0x00, 0x49, 0xFF, 0x8D, 0x1D, 0x21, 0xA9, 0xFF, 0x8D, 0x1D, 0x21, // M7C -nn
        0xA5, 0x00, 0x8D, 0x1E, 0x21, 0xA9, 0x01, 0x8D, 0x1E, 0x21, // M7D $01nn
        0x40, // rti
    ]);
    let rti = 0x8000 + c.len() as u16;
    c.push(0x40);
    assert!(c.len() < 0x800, "the code runs into the sound program");
    rom[..c.len()].copy_from_slice(&c);
    let at = usize::from(SPC_AT - 0x8000);
    rom[at..at + spc.len()].copy_from_slice(&spc);

    rom[0x7FC0..0x7FD5].copy_from_slice(b"MODE 7 RUN AHEAD     ");
    rom[0x7FD5] = 0x20; // LoROM
    rom[0x7FD6] = 0x00;
    rom[0x7FD7] = 0x05; // 32 KiB
    rom[0x7FD8] = 0x00;
    rom[0x7FD9] = 0x01;
    rom[0x7FDA] = 0x00;
    rom[0x7FDB] = 0x00;
    let mut vec = |at: usize, to: u16| rom[at..at + 2].copy_from_slice(&to.to_le_bytes());
    for at in [0x7FE4, 0x7FE6, 0x7FE8, 0x7FEE, 0x7FF4, 0x7FF8, 0x7FFE] {
        vec(at, rti);
    }
    vec(0x7FEA, nmi);
    vec(0x7FFA, nmi);
    vec(0x7FFC, 0x8000);
    rom[0x7FDC..0x7FE0].copy_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
    let sum = rom.iter().map(|&b| b as u32).sum::<u32>() as u16;
    rom[0x7FDE..0x7FE0].copy_from_slice(&sum.to_le_bytes());
    rom[0x7FDC..0x7FDE].copy_from_slice(&(!sum).to_le_bytes());
    rom
}
