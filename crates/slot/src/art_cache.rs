//! Label and backdrop art kept on the card already scaled, so a boot or a turn of the shelf
//! reads raw pixels back instead of decoding a PNG and box-filtering it down every time.
//!
//! Each entry is the RGBA at the size it is drawn, behind a 24-byte header: the source's
//! length and modification time, then the width and height it was scaled to. An entry whose
//! source has since changed, or that was scaled for a different size, is stale and is
//! rebuilt from the source. A source the decoder refused is remembered as a header with a
//! 0 x 0 size, so it is not decoded again on every boot either.
//!
//! The cache is only ever a copy: deleting `System/Cache` costs one slow boot and nothing else.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use slot_store::Cart;
use slot_ui::{cover, label_size, OUT_H, OUT_W};

const DIR: &str = "System/Cache";
const HEAD: usize = 24;

/// `cart`'s label art at `label_size`, or `None` for a cart with no label (or one that cannot
/// be read), which then gets the generated label.
pub fn label_art(root: &Path, cart: &Cart) -> Option<Vec<u8>> {
    let (w, h) = label_size(cart);
    cached(
        cart.label.as_deref()?,
        &cache_path(root, "Labels", cart),
        w,
        h,
    )
}

/// `cart`'s backdrop, scaled to cover the whole screen.
pub fn backdrop_art(root: &Path, cart: &Cart) -> Option<Vec<u8>> {
    cached(
        cart.backdrop.as_deref()?,
        &cache_path(root, "Backdrops", cart),
        OUT_W,
        OUT_H,
    )
}

pub fn cache_path(root: &Path, kind: &str, cart: &Cart) -> PathBuf {
    root.join(DIR)
        .join(kind)
        .join(cart.platform.dir_name())
        .join(format!("{}.rgba", cart.stem))
}

fn cached(src: &Path, entry: &Path, w: u32, h: u32) -> Option<Vec<u8>> {
    let stamp = stamp(src)?;
    if let Some(art) = read(entry, stamp, w, h) {
        return art;
    }
    let art = cover(src, w, h);
    let (cw, ch) = if art.is_some() { (w, h) } else { (0, 0) };
    let mut bytes = Vec::with_capacity(HEAD + art.as_ref().map_or(0, Vec::len));
    bytes.extend_from_slice(&stamp.0.to_le_bytes());
    bytes.extend_from_slice(&stamp.1.to_le_bytes());
    bytes.extend_from_slice(&cw.to_le_bytes());
    bytes.extend_from_slice(&ch.to_le_bytes());
    bytes.extend_from_slice(art.as_deref().unwrap_or_default());
    if let Err(e) = write(entry, &bytes) {
        eprintln!("slot: art cache: {}: {e}", entry.display());
    }
    art
}

/// Through a temporary name and a rename, so a power cut mid-write leaves the old entry or
/// none, never a torn one that happens to have the right length.
fn write(entry: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = entry.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = entry.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, entry)
}

fn stamp(src: &Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(src).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Some((meta.len(), mtime))
}

/// `Some(Some(art))` for a fresh entry, `Some(None)` for a fresh record of a source the decoder
/// refused, `None` for an entry that is missing or stale.
fn read(entry: &Path, stamp: (u64, u64), w: u32, h: u32) -> Option<Option<Vec<u8>>> {
    let mut bytes = std::fs::read(entry).ok()?;
    if bytes.len() < HEAD {
        return None;
    }
    let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or_default());
    let half = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or_default());
    if (word(0), word(8)) != stamp {
        return None;
    }
    let body = match (half(16), half(20)) {
        (0, 0) => 0,
        size if size == (w, h) => (w * h * 4) as usize,
        _ => return None,
    };
    if bytes.len() != HEAD + body {
        return None;
    }
    if body == 0 {
        return Some(None);
    }
    bytes.drain(..HEAD);
    Some(Some(bytes))
}
