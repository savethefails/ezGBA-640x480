//! The SNES internal header's title: what the shell table keys on, for the few carts that did
//! not come in the usual grey.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Where the header sits in a LoROM and a HiROM image (ExHiROM's is past 4 MB and rare enough
/// to read as unknown).
const HEADER_AT: [u64; 2] = [0x7fc0, 0xffc0];
/// A copier header, 512 bytes some dumps carry in front of the rom.
const COPIER: u64 = 512;
const HEADER_LEN: usize = 0x20;
const TITLE_LEN: usize = 21;
const COMPLEMENT_OFF: usize = 0x1c;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub title: String,
}

/// The header, or `None` when no candidate place holds one whose checksum and complement agree.
/// That check is what tells a header from whatever bytes a LoROM has at the HiROM offset.
pub fn header(rom: &Path) -> Option<Header> {
    let mut f = File::open(rom).ok()?;
    let len = f.metadata().ok()?.len();
    let skip = if len % 1024 == COPIER { COPIER } else { 0 };
    HEADER_AT.iter().find_map(|at| {
        let mut buf = [0u8; HEADER_LEN];
        f.seek(SeekFrom::Start(skip + at)).ok()?;
        f.read_exact(&mut buf).ok()?;
        parse(&buf)
    })
}

pub fn parse(bytes: &[u8]) -> Option<Header> {
    let bytes = bytes.get(..HEADER_LEN)?;
    let word = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    if word(COMPLEMENT_OFF) ^ word(COMPLEMENT_OFF + 2) != 0xffff {
        return None;
    }
    let title = &bytes[..TITLE_LEN];
    if !title.iter().all(|b| (0x20..0x7f).contains(b)) {
        return None;
    }
    Some(Header {
        title: String::from_utf8_lossy(title).trim().to_string(),
    })
}
