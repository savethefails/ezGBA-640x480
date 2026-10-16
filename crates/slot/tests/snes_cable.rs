//! A SNES link through the real emulator worker: snes9x2005 as the host, this test as the joiner,
//! speaking the cable protocol. A SNES link is two pads on one console, so what has to hold is
//! that the joiner's buttons arrive on the host's second port, frame for frame.
//!
//! Skipped unless `vendor/snes9x2005_plus_libretro.so` is there (`task core:snes9x2005` builds
//! it). The ROM is generated here: a few dozen bytes of 65816 that read both pads every frame.

mod common;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slot::audio::Ring;
use slot::cable::{self, Cable};
use slot::emu::{CoreState, EmuHandle, Speed};
use slot_retro::{ButtonMask, LibretroCore, LinkChannel};

fn vendored() -> Option<PathBuf> {
    let p = common::repo_root().join(format!(
        "vendor/snes9x2005_plus_libretro.{}",
        std::env::consts::DLL_EXTENSION
    ));
    if !p.exists() {
        eprintln!("no vendored snes9x2005 core on this host, skipping");
    }
    p.exists().then_some(p)
}

/// A 32 KiB LoROM that reads both pads after every vblank (auto-joypad read) and folds them into
/// a running value it shows as the backdrop colour. Player 2's buttons change the picture.
fn two_pad_rom() -> Vec<u8> {
    let mut rom = vec![0xFFu8; 0x8000];
    let mut c: Vec<u8> = vec![
        0x78, 0x18, 0xFB, // sei; clc; xce: native mode
        0xE2, 0x30, // sep #$30: 8-bit registers
        0xA9, 0x8F, 0x8D, 0x00, 0x21, // force blank
        0x9C, 0x00, 0x00, 0x9C, 0x01, 0x00, // stz $0000; stz $0001
        0xA9, 0x0F, 0x8D, 0x00, 0x21, // screen on
        0xA9, 0x81, 0x8D, 0x00, 0x42, // NMI and auto-joypad on
    ];
    let top = c.len();
    c.push(0xCB); // wai
    let wait = c.len();
    c.extend([0xAD, 0x12, 0x42, 0x29, 0x01]); // lda $4212; and #1: auto-read busy?
    c.extend([0xD0, (wait as i32 - (c.len() as i32 + 2)) as u8]); // bne wait
    c.extend([0xAD, 0x00, 0x00, 0x0A, 0x0A, 0x6D, 0x00, 0x00, 0x69, 0x01]); // v0 = v0*5+1
    c.extend([0x4D, 0x18, 0x42, 0x4D, 0x19, 0x42, 0x8D, 0x00, 0x00]); // ^= pad 1
    c.extend([0xAD, 0x01, 0x00, 0x2A, 0x4D, 0x1A, 0x42, 0x4D, 0x1B, 0x42]); // v1 = rol v1 ^ pad 2
    c.extend([0x6D, 0x00, 0x00, 0x8D, 0x01, 0x00]); // v1 += v0
    c.extend([0x9C, 0x21, 0x21]); // colour 0
    c.extend([0xAD, 0x00, 0x00, 0x8D, 0x22, 0x21]);
    c.extend([0xAD, 0x01, 0x00, 0x29, 0x7F, 0x8D, 0x22, 0x21]);
    c.extend([0x80, (top as i32 - (c.len() as i32 + 2)) as u8]); // bra top
    let nmi = 0x8000 + c.len() as u16;
    c.extend([0xAD, 0x10, 0x42, 0x40]); // acknowledge; rti
    let rti = 0x8000 + c.len() as u16;
    c.push(0x40);
    rom[..c.len()].copy_from_slice(&c);
    rom[0x7FC0..0x7FD5].copy_from_slice(b"TWO PAD CABLE TEST   ");
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

/// Both directions of one in-memory wire.
#[derive(Default)]
struct Wire {
    to_test: Vec<Vec<u8>>,
    to_host: Vec<Vec<u8>>,
}

struct HostEnd(Arc<Mutex<Wire>>);

impl LinkChannel for HostEnd {
    fn send(&mut self, _flags: i32, buf: &[u8]) {
        self.0.lock().unwrap().to_test.push(buf.to_vec());
    }
    fn try_recv(&mut self) -> Option<Vec<u8>> {
        let mut w = self.0.lock().unwrap();
        (!w.to_host.is_empty()).then(|| w.to_host.remove(0))
    }
}

/// Runs a session of `frames` joiner frames with player 2 holding `p2(frame)`, and answers the
/// hash of the host's picture once it has run every frame it was given the buttons for.
fn session(dylib: &Path, rom: &Path, frames: u64, p2: impl Fn(u64) -> u16) -> u64 {
    let core = LibretroCore::open(dylib).expect("the vendored core would not open");
    let emu = EmuHandle::spawn(
        Box::new(core),
        rom.to_path_buf(),
        Arc::new(Ring::new(1 << 14)),
        None,
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while emu.state() == CoreState::Loading {
        assert!(Instant::now() < deadline, "the core never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(emu.state(), CoreState::Ready);
    emu.set_speed(Speed::Normal);

    let wire = Arc::new(Mutex::new(Wire::default()));
    emu.begin_cable(0, Box::new(HostEnd(wire.clone())));
    let mut c = Cable::new(1);
    let want = frames + cable::DELAY;
    let deadline = Instant::now() + Duration::from_secs(20);
    while emu.linked_frames() < want {
        assert!(
            Instant::now() < deadline,
            "the host ran {} of {want} frames",
            emu.linked_frames()
        );
        let inbound: Vec<Vec<u8>> = std::mem::take(&mut wire.lock().unwrap().to_test);
        for p in inbound {
            c.accept(&p);
        }
        // The host's state arrives first; this joiner has no core to restore it into, and needs
        // none: it only plays the second pad.
        if c.take_state().is_some() {
            c.prime();
            wire.lock()
                .unwrap()
                .to_host
                .push(cable::ready_packet().to_vec());
        }
        if c.primed() && c.frame() < frames {
            let p = c.sample(ButtonMask(p2(c.frame())));
            wire.lock().unwrap().to_host.push(p.to_vec());
            if c.ready().is_some() {
                c.advance();
            }
        }
        std::thread::sleep(Duration::from_micros(500));
    }
    // It cannot run past the last frame it has both pads for: the next one waits on this end.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        emu.linked_frames(),
        want,
        "the host ran a frame it had no pad 2 for"
    );
    let picture = emu.latest_frame().expect("no frame");
    let mut h = DefaultHasher::new();
    picture.hash(&mut h);
    drop(picture);
    emu.end_link();
    h.finish()
}

#[test]
fn the_joiners_buttons_reach_the_hosts_second_port_frame_for_frame() {
    let _g = common::core_lock();
    let Some(dylib) = vendored() else { return };
    let dir = tempfile::tempdir().unwrap();
    let rom = dir.path().join("two-pad.sfc");
    std::fs::write(&rom, two_pad_rom()).unwrap();

    let idle = session(&dylib, &rom, 90, |_| 0);
    let again = session(&dylib, &rom, 90, |_| 0);
    let pressed = session(&dylib, &rom, 90, |f| if f % 7 < 3 { 0x0080 } else { 0 });
    assert_eq!(idle, again, "the same buttons gave two different machines");
    assert_ne!(idle, pressed, "player 2's buttons never reached the game");
}

/// snes9x2005 as the joiner, taking in a host's machine that is somewhere else. A SNES state is
/// the core's structs copied out raw, so bytes 4..8, where mGBA keeps its BIOS checksum, are a
/// CPU register here: read as a BIOS, every SNES host looked like another one and no joiner ever
/// restored.
#[test]
fn a_snes_joiner_restores_a_host_somewhere_else() {
    use slot_retro::RetroCore;
    let _g = common::core_lock();
    let Some(dylib) = vendored() else { return };
    let dir = tempfile::tempdir().unwrap();
    let rom = dir.path().join("two-pad.sfc");
    std::fs::write(&rom, two_pad_rom()).unwrap();

    let host_state = {
        let mut core = LibretroCore::open(&dylib).expect("the vendored core would not open");
        core.load(&rom).expect("the ROM would not load");
        for f in 0..37u16 {
            core.run_frame(ButtonMask(f));
        }
        core.serialize().expect("the core would not serialize")
    };

    let core = LibretroCore::open(&dylib).expect("the vendored core would not open");
    let emu = EmuHandle::spawn(
        Box::new(core),
        rom.clone(),
        Arc::new(Ring::new(1 << 14)),
        None,
        None,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while emu.state() == CoreState::Loading {
        assert!(Instant::now() < deadline, "the core never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
    emu.set_speed(Speed::Normal);
    let wire = Arc::new(Mutex::new(Wire::default()));
    wire.lock().unwrap().to_host = cable::state_packets(&host_state);
    emu.begin_cable(1, Box::new(HostEnd(wire.clone())));

    let deadline = Instant::now() + Duration::from_secs(5);
    while !wire
        .lock()
        .unwrap()
        .to_test
        .contains(&cable::ready_packet().to_vec())
    {
        assert!(
            !emu.bios_mismatch(),
            "a SNES host was refused as another BIOS"
        );
        assert!(
            Instant::now() < deadline,
            "the joiner never said it was ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    emu.end_link();
}
