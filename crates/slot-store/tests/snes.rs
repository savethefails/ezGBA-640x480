use slot_store::snes::{header, Region};

/// A rom of `len` bytes with a header that checks out at `at`, after `copier` bytes of copier
/// header.
fn rom(len: usize, at: usize, copier: usize, title: &str, dest: u8) -> Vec<u8> {
    let mut rom = vec![0u8; copier + len];
    let h = copier + at;
    let mut name = [b' '; 21];
    name[..title.len()].copy_from_slice(title.as_bytes());
    rom[h..h + 21].copy_from_slice(&name);
    rom[h + 0x19] = dest;
    rom[h + 0x1c..h + 0x1e].copy_from_slice(&0xbeefu16.to_le_bytes());
    rom[h + 0x1e..h + 0x20].copy_from_slice(&(0xbeefu16 ^ 0xffff).to_le_bytes());
    rom
}

fn read(bytes: Vec<u8>) -> Option<slot_store::snes::Header> {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("game.sfc");
    std::fs::write(&path, bytes).unwrap();
    header(&path)
}

#[test]
fn a_lorom_header_is_read() {
    let h = read(rom(0x10000, 0x7fc0, 0, "SUPER MARIOWORLD", 0x01)).expect("header");
    assert_eq!(h.title, "SUPER MARIOWORLD");
    assert_eq!(h.region, Region::NorthAmerica);
}

#[test]
fn a_hirom_header_is_read() {
    let h = read(rom(0x10000, 0xffc0, 0, "KILLER INSTINCT", 0x02)).expect("header");
    assert_eq!(h.title, "KILLER INSTINCT");
    assert_eq!(h.region, Region::Pal);
}

#[test]
fn a_copier_header_in_front_is_skipped() {
    let h = read(rom(0x10000, 0x7fc0, 512, "ZELDA", 0x00)).expect("header");
    assert_eq!(h.title, "ZELDA");
    assert_eq!(h.region, Region::Japan);
}

/// What a checksum and its complement are for: bytes at the header's place that do not agree
/// are not a header.
#[test]
fn a_header_whose_checksum_does_not_agree_is_none() {
    let mut bytes = rom(0x10000, 0x7fc0, 0, "BROKEN", 0x01);
    bytes[0x7fc0 + 0x1e] ^= 1;
    assert!(read(bytes).is_none());
    assert!(read(vec![0xff; 0x10000]).is_none());
}
