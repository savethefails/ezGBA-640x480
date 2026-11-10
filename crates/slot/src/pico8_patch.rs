//! The quick menu's Patcher: makes a PICO-8 multicart that only works from the BBS run from the
//! card.
//!
//! Some multicarts fetch their other carts only when they know they came from the BBS, which they
//! ask with `stat(101)`. Started from a file, the same cart looks for its parts as files beside it
//! instead (`lem_leveldata.p8`, `snekburd_2.p8`), finds none, and stops: Lemmings says "no
//! leveldata found". The patch swaps the cart's code for a few lines that load the cart's own BBS
//! version by its id, which then installs its parts the way its author meant, and which PICO-8
//! keeps on the card and runs offline from then on. Everything else in the file, the label among
//! it, stays as it was, so the shelf shows the same cart.
//!
//! The BBS id comes from the file's name, which is how the BBS names a download:
//! `lemmings_p8-3.p8.png` is revision 3 of `lemmings_p8`. The original is kept under
//! `Saves/pico8/originals/`, and restoring puts it back.

use std::io;
use std::path::{Path, PathBuf};

/// The first line of every launcher this writes, which is how one is known again.
const MARK: &str = "-- patched by ezgba";

/// What a cart in `Games/PICO8/` is, as far as the patch goes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Status {
    /// Runs from a file as it is.
    Fine,
    /// Only finds its parts as a BBS cart, and its name gives its BBS id.
    Multicart { id: String },
    /// Only finds its parts as a BBS cart, but its name is not a BBS download's, so there is no
    /// id to load it by.
    NoId,
    /// A launcher this wrote.
    Patched,
    /// Not a cart this can read.
    Unreadable,
}

impl Status {
    /// A press of A does something: a multicart is patched and a launcher is undone.
    pub fn actionable(&self) -> bool {
        matches!(self, Status::Multicart { .. } | Status::Patched)
    }
}

/// One cart in the folder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cart {
    pub path: PathBuf,
    /// The file's name without `.p8` or `.p8.png`, as the page shows it.
    pub name: String,
    pub status: Status,
}

/// Where the carts are.
pub fn games(root: &Path) -> PathBuf {
    root.join("Games").join("PICO8")
}

/// Where the originals go.
pub fn originals(root: &Path) -> PathBuf {
    crate::pico8_net::home(root).join("originals")
}

/// Every cart in `Games/PICO8/`, in the shelf's order, with what each one is.
pub fn scan(root: &Path) -> Vec<Cart> {
    let Ok(dir) = std::fs::read_dir(games(root)) else {
        return Vec::new();
    };
    let mut carts: Vec<Cart> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && slot_store::Platform::Pico8.accepts(p))
        .map(|path| Cart {
            name: cart_name(&path),
            status: status(&path),
            path,
        })
        .collect();
    // The carts the Patcher has something to do for come first: those that need it and those
    // it has done, together, so patching or restoring one leaves it where it was.
    carts.sort_by_key(|c| (!c.status.actionable(), slot_store::sort_key(&c.name)));
    carts
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn cart_name(path: &Path) -> String {
    let name = file_name(path);
    let lower = name.to_ascii_lowercase();
    for ext in [".p8.png", ".p8"] {
        if lower.ends_with(ext) {
            return name[..name.len() - ext.len()].to_string();
        }
    }
    name
}

/// The BBS id a download's name carries: `name-3` is revision 3 of `name`. A name without the
/// revision is not taken for an id, since a cart someone renamed would load another cart.
pub fn bbs_id(name: &str) -> Option<String> {
    let (id, rev) = name.rsplit_once('-')?;
    let ok = !id.is_empty()
        && !rev.is_empty()
        && rev.bytes().all(|b| b.is_ascii_digit())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    ok.then(|| id.to_string())
}

pub fn status(path: &Path) -> Status {
    let code = match read_code(path) {
        Ok(code) => code,
        Err(e) => {
            log(&format!("{}: unreadable: {e}", file_name(path)));
            return Status::Unreadable;
        }
    };
    if code.starts_with(MARK.as_bytes()) {
        return Status::Patched;
    }
    if !asks_for_bbs(&code) {
        return Status::Fine;
    }
    match bbs_id(&cart_name(path)) {
        Some(id) => Status::Multicart { id },
        None => Status::NoId,
    }
}

/// Whether the code asks if it came from the BBS: `stat(101)`, `stat"101"`, `stat 101`.
fn asks_for_bbs(code: &[u8]) -> bool {
    let text = String::from_utf8_lossy(code).to_ascii_lowercase();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("stat") {
        let after = rest[at + 4..].trim_start_matches([' ', '(', '"', '\'', '\t']);
        if after.starts_with("101") && !after[3..].starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
        rest = &rest[at + 4..];
    }
    false
}

/// The launcher for `id`.
fn launcher(id: &str) -> String {
    format!(
        "{MARK}: runs this cart's bbs version,\n\
         -- which fetches its other carts. the original is in saves/pico8/originals.\n\
         if not load(\"#{id}\") then\n \
         cls()\n \
         print(\"this cart downloads its other\\nparts the first time it runs.\\n\\nturn on wi-fi and try again.\",4,52,7)\n \
         stop()\n\
         end\n"
    )
}

/// Swaps the cart's code for the launcher, keeping the original first. Refuses anything that
/// is not a multicart with an id, so a cart is never patched twice or patched for nothing.
pub fn patch(root: &Path, path: &Path) -> io::Result<()> {
    let name = file_name(path);
    let result = patch_inner(root, path);
    match &result {
        Ok(()) => log(&format!("{name}: patched; now {:?}", status(path))),
        Err(e) => log(&format!("{name}: patch failed: {e}")),
    }
    result
}

fn patch_inner(root: &Path, path: &Path) -> io::Result<()> {
    let name = file_name(path);
    let found = status(path);
    let Status::Multicart { id } = found else {
        log(&format!("{name}: not patched: it is {found:?}"));
        return Err(io::Error::other("not a multicart this can patch"));
    };
    let bytes = std::fs::read(path)?;
    let old_code = read_code(path).map_or(0, |c| c.len());
    let dir = originals(root);
    std::fs::create_dir_all(&dir)?;
    let keep = dir.join(&name);
    // A copy already there is the original from an earlier patch: never written over.
    if keep.exists() {
        log(&format!(
            "{name}: the original is already kept at {} ({} bytes), left as it is",
            keep.display(),
            std::fs::metadata(&keep).map_or(0, |m| m.len())
        ));
    } else {
        slot_store::atomic_write(&keep, &bytes)?;
        log(&format!(
            "{name}: original ({} bytes, {old_code} bytes of code) kept at {}",
            bytes.len(),
            keep.display()
        ));
    }
    let code = launcher(&id);
    let out = if is_png(path) {
        png_with_code(&bytes, code.as_bytes())?
    } else {
        text_with_code(&bytes, &code)?
    };
    log(&format!(
        "{name}: writing a launcher for #{id} ({} bytes of code, {} bytes of {})",
        code.len(),
        out.len(),
        if is_png(path) { "PNG" } else { "text" }
    ));
    slot_store::atomic_write(path, &out)
}

/// Puts the original back.
pub fn restore(root: &Path, path: &Path) -> io::Result<()> {
    let name = file_name(path);
    let keep = originals(root).join(&name);
    let result = (|| {
        let bytes = std::fs::read(&keep)?;
        slot_store::atomic_write(path, &bytes)?;
        std::fs::remove_file(&keep)?;
        Ok::<usize, io::Error>(bytes.len())
    })();
    match &result {
        Ok(n) => log(&format!(
            "{name}: restored from {} ({n} bytes); now {:?}",
            keep.display(),
            status(path)
        )),
        Err(e) => log(&format!(
            "{name}: restore from {} failed: {e}",
            keep.display()
        )),
    }
    result.map(|_| ())
}

/// A line in the download log, marked as this module's.
fn log(line: &str) {
    crate::pico8_net::note(&format!("slot: patch: {line}"));
}

fn is_png(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
}

/// The cart's Lua source.
pub fn read_code(path: &Path) -> io::Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    if is_png(path) {
        let mem = png_memory(&bytes)?;
        decode_code(&mem[CODE..CART])
    } else {
        let text = String::from_utf8_lossy(&bytes);
        let (_, lua, _) = lua_section(&text).ok_or_else(|| bad("no __lua__ section"))?;
        Ok(lua.as_bytes().to_vec())
    }
}

fn bad(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}

// ---- .p8 text carts ----

/// The text before the code, the code, and the text after it.
fn lua_section(text: &str) -> Option<(&str, &str, &str)> {
    let head = "__lua__\n";
    let start = text.find(head)? + head.len();
    let end = text[start..]
        .match_indices("\n__")
        .map(|(i, _)| start + i + 1)
        .find(|&i| {
            let line = text[i..].lines().next().unwrap_or("");
            line.len() > 4 && line.ends_with("__") && !line.contains(' ')
        })
        .unwrap_or(text.len());
    Some((&text[..start], &text[start..end], &text[end..]))
}

fn text_with_code(bytes: &[u8], code: &str) -> io::Result<Vec<u8>> {
    let text = String::from_utf8_lossy(bytes);
    let (before, _, after) = lua_section(&text).ok_or_else(|| bad("no __lua__ section"))?;
    Ok(format!("{before}{code}{after}").into_bytes())
}

// ---- .p8.png carts ----
//
// A cart's 0x8000 bytes of memory (and a short trailer) are hidden in the low two bits of each
// pixel's channels, a byte a pixel, in reading order: ARGB, two bits each, A highest. The code
// is at 0x4300, compressed. The trailer at 0x8000 holds the version PICO-8 saved with and, from
// 0x8006, a SHA-1 of the 0x8000 bytes, which PICO-8 checks before it will load the cart.

const CODE: usize = 0x4300;
const CART: usize = 0x8000;
const HASH: usize = 0x8006;
/// The memory and the trailer, as many as a 160x205 label carries.
const MEMORY: usize = 0x8020;

/// The picture, as RGBA, with its size.
fn png_rgba(bytes: &[u8]) -> io::Result<(Vec<u8>, u32, u32)> {
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().map_err(io::Error::other)?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(io::Error::other)?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return Err(bad("not an RGB picture")),
    };
    if info.bit_depth != png::BitDepth::Eight || rgba.len() < MEMORY * 4 {
        return Err(bad("not a PICO-8 cart picture"));
    }
    Ok((rgba, info.width, info.height))
}

fn png_memory(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let (rgba, _, _) = png_rgba(bytes)?;
    Ok(rgba[..MEMORY * 4]
        .chunks_exact(4)
        .map(|p| ((p[3] & 3) << 6) | ((p[0] & 3) << 4) | ((p[1] & 3) << 2) | (p[2] & 3))
        .collect())
}

fn png_with_code(bytes: &[u8], code: &[u8]) -> io::Result<Vec<u8>> {
    let (mut rgba, w, h) = png_rgba(bytes)?;
    let mut mem: Vec<u8> = rgba[..MEMORY * 4]
        .chunks_exact(4)
        .map(|p| ((p[3] & 3) << 6) | ((p[0] & 3) << 4) | ((p[1] & 3) << 2) | (p[2] & 3))
        .collect();
    let packed = encode_code(code)?;
    mem[CODE..CART].fill(0);
    mem[CODE..CODE + packed.len()].copy_from_slice(&packed);
    let hash = sha1(&mem[..CART]);
    mem[HASH..HASH + 20].copy_from_slice(&hash);
    for (p, v) in rgba.chunks_exact_mut(4).zip(&mem) {
        p[0] = (p[0] & !3) | ((v >> 4) & 3);
        p[1] = (p[1] & !3) | ((v >> 2) & 3);
        p[2] = (p[2] & !3) | (v & 3);
        p[3] = (p[3] & !3) | ((v >> 6) & 3);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(io::Error::other)?;
        writer.write_image_data(&rgba).map_err(io::Error::other)?;
    }
    Ok(out)
}

/// The old format's characters, which it stores in one byte each.
const OLD_CHARS: &[u8] = b"\n 0123456789abcdefghijklmnopqrstuvwxyz!#%(){}[]<>+=/*:;.,~_";

/// Code in the old compressed format, using none of its back references: plenty for a launcher,
/// and the format every PICO-8 since 0.1 reads.
fn encode_code(code: &[u8]) -> io::Result<Vec<u8>> {
    let len = u16::try_from(code.len()).map_err(|_| bad("code too long"))?;
    let mut out = b":c:\0".to_vec();
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    for &c in code {
        match OLD_CHARS.iter().position(|&o| o == c) {
            Some(i) => out.push(i as u8 + 1),
            None => out.extend_from_slice(&[0, c]),
        }
    }
    if out.len() > CART - CODE {
        return Err(bad("code too long"));
    }
    Ok(out)
}

/// The code stored at 0x4300, in whichever of PICO-8's three forms it is.
fn decode_code(data: &[u8]) -> io::Result<Vec<u8>> {
    if data.starts_with(b"\0pxa") {
        decode_pxa(data)
    } else if data.starts_with(b":c:\0") {
        decode_old(data)
    } else {
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        Ok(data[..end].to_vec())
    }
}

fn decode_old(data: &[u8]) -> io::Result<Vec<u8>> {
    let len = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut out = Vec::with_capacity(len);
    let mut i = 8;
    while out.len() < len {
        let b = *data.get(i).ok_or_else(|| bad("code cut short"))?;
        i += 1;
        match b {
            0 => {
                out.push(*data.get(i).ok_or_else(|| bad("code cut short"))?);
                i += 1;
            }
            1..=0x3b => out.push(OLD_CHARS[b as usize - 1]),
            _ => {
                let next = *data.get(i).ok_or_else(|| bad("code cut short"))?;
                i += 1;
                let offset = (b as usize - 0x3c) * 16 + (next & 0xf) as usize;
                let n = (next >> 4) as usize + 2;
                if offset == 0 || offset > out.len() {
                    return Err(bad("bad back reference"));
                }
                for _ in 0..n {
                    out.push(out[out.len() - offset]);
                }
            }
        }
    }
    out.truncate(len);
    Ok(out)
}

/// PICO-8 0.2's format: a bit stream of literals coded through a move-to-front table, and
/// back references.
fn decode_pxa(data: &[u8]) -> io::Result<Vec<u8>> {
    let len = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut pos = 8 * 8;
    let total = data.len() * 8;
    let mut bit = || -> io::Result<u32> {
        if pos >= total {
            return Err(bad("code cut short"));
        }
        let b = (data[pos >> 3] >> (pos & 7)) & 1;
        pos += 1;
        Ok(b as u32)
    };
    let mut mtf: Vec<u8> = (0..=255).collect();
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        if bit()? == 1 {
            let mut n = 4;
            while bit()? == 1 {
                n += 1;
                if n > 8 {
                    return Err(bad("bad literal"));
                }
            }
            let mut v = 0usize;
            for k in 0..n {
                v |= (bit()? as usize) << k;
            }
            let i = v + (1 << n) - 16;
            if i >= 256 {
                return Err(bad("bad literal"));
            }
            let c = mtf.remove(i);
            mtf.insert(0, c);
            out.push(c);
        } else {
            let width = if bit()? == 1 {
                if bit()? == 1 {
                    5
                } else {
                    10
                }
            } else {
                15
            };
            let mut off = 0usize;
            for k in 0..width {
                off |= (bit()? as usize) << k;
            }
            off += 1;
            if width == 10 && off == 1 {
                loop {
                    let mut c = 0u8;
                    for k in 0..8 {
                        c |= (bit()? as u8) << k;
                    }
                    if c == 0 {
                        break;
                    }
                    out.push(c);
                }
                continue;
            }
            let mut n = 3;
            loop {
                let mut part = 0;
                for k in 0..3 {
                    part |= bit()? << k;
                }
                n += part as usize;
                if part != 7 {
                    break;
                }
            }
            if off > out.len() {
                return Err(bad("bad back reference"));
            }
            for _ in 0..n {
                out.push(out[out.len() - off]);
            }
        }
    }
    out.truncate(len);
    Ok(out)
}

/// SHA-1, for the hash PICO-8 checks in a cart's trailer.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_the_standard_vectors() {
        let hex = |b: [u8; 20]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(
            hex(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(sha1(&[b'a'; 1000])),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
    }

    #[test]
    fn a_download_name_gives_its_bbs_id_and_a_renamed_one_does_not() {
        assert_eq!(bbs_id("lemmings_p8-3").as_deref(), Some("lemmings_p8"));
        assert_eq!(bbs_id("snekburd-8").as_deref(), Some("snekburd"));
        assert_eq!(bbs_id("mot_sorcerer-13").as_deref(), Some("mot_sorcerer"));
        assert_eq!(bbs_id("Lemmings"), None);
        assert_eq!(bbs_id("lemmings"), None);
        assert_eq!(bbs_id("my cart-2"), None);
    }

    #[test]
    fn every_way_of_asking_for_stat_101_is_seen() {
        assert!(asks_for_bbs(b"if stat(101) then"));
        assert!(asks_for_bbs(b"stat\"101\" and split"));
        assert!(asks_for_bbs(b"x=stat 101"));
        assert!(asks_for_bbs(b"if STAT( 101 ) then"));
        assert!(!asks_for_bbs(b"stat(1010) stat(10) stat(46)"));
        assert!(!asks_for_bbs(b"print(\"status 101\")"));
    }

    #[test]
    fn the_old_format_reads_back_what_it_wrote() {
        let code = launcher("lemmings_p8");
        let packed = encode_code(code.as_bytes()).unwrap();
        assert_eq!(decode_old(&packed).unwrap(), code.as_bytes());
    }

    #[test]
    fn a_text_cart_keeps_everything_but_its_code() {
        let cart = "pico-8 cartridge // http://www.pico-8.com\nversion 43\n__lua__\nif stat(101) then end\n__gfx__\n0000\n__label__\n1111\n";
        let out = String::from_utf8(text_with_code(cart.as_bytes(), "x=1\n").unwrap()).unwrap();
        assert_eq!(
            out,
            "pico-8 cartridge // http://www.pico-8.com\nversion 43\n__lua__\nx=1\n__gfx__\n0000\n__label__\n1111\n"
        );
    }
}
