//! Label art and box art kept on the card already scaled, so a boot or a turn of the shelf
//! reads raw pixels back instead of decoding a PNG and resampling it every time.
//!
//! Each entry is the RGBA at the size it is drawn, behind a 32-byte header: the source's length
//! and modification time, the size it was asked to fit, and the size it came out at (box art
//! keeps its own shape, so that is the source's business, not the caller's). An entry whose
//! source has since changed, or that was fitted to a different size, is stale and is rebuilt
//! from the source. A source the decoder refused is remembered as a header with a 0 x 0 size,
//! so it is not decoded again on every boot either.
//!
//! The cache is only ever a copy: deleting `System/Cache` costs one slow boot and nothing else.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use slot_store::Cart;
use slot_ui::{contain, cover, label_size};

const DIR: &str = "System/Cache";
const HEAD: usize = 32;

/// Scaled art and the size it came out at.
pub type Art = (Vec<u8>, u32, u32);

/// `cart`'s label art at `label_size`, or `None` for a cart with no label (or one that cannot
/// be read), which then gets the generated label.
pub fn label_art(root: &Path, cart: &Cart) -> Option<Vec<u8>> {
    let (w, h) = label_size(cart);
    let scale = |src: &Path| cover(src, w, h).map(|rgba| (rgba, w, h));
    cached(
        cart.label.as_deref()?,
        &cache_path(root, "Labels", cart),
        (w, h),
        scale,
    )
    .map(|(rgba, _, _)| rgba)
}

/// `cart`'s box art, fitted inside `bound` with its own shape kept.
pub fn box_art(root: &Path, cart: &Cart, bound: (u32, u32)) -> Option<Art> {
    cached(
        cart.box_art.as_deref()?,
        &cache_path(root, "Images", cart),
        bound,
        |src| contain(src, bound.0, bound.1),
    )
}

pub fn cache_path(root: &Path, kind: &str, cart: &Cart) -> PathBuf {
    root.join(DIR)
        .join(kind)
        .join(cart.platform.dir_name())
        .join(format!("{}.rgba", cart.stem))
}

fn cached(
    src: &Path,
    entry: &Path,
    bound: (u32, u32),
    scale: impl FnOnce(&Path) -> Option<Art>,
) -> Option<Art> {
    let stamp = stamp(src)?;
    if let Some(art) = read(entry, stamp, bound) {
        return art;
    }
    let art = scale(src);
    let (w, h) = art.as_ref().map_or((0, 0), |(_, w, h)| (*w, *h));
    let body = art.as_ref().map_or(&[][..], |(rgba, _, _)| rgba.as_slice());
    let mut bytes = Vec::with_capacity(HEAD + body.len());
    bytes.extend_from_slice(&stamp.0.to_le_bytes());
    bytes.extend_from_slice(&stamp.1.to_le_bytes());
    for half in [bound.0, bound.1, w, h] {
        bytes.extend_from_slice(&half.to_le_bytes());
    }
    bytes.extend_from_slice(body);
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
fn read(entry: &Path, stamp: (u64, u64), bound: (u32, u32)) -> Option<Option<Art>> {
    let mut bytes = std::fs::read(entry).ok()?;
    if bytes.len() < HEAD {
        return None;
    }
    let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or_default());
    let half = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or_default());
    if (word(0), word(8)) != stamp || (half(16), half(20)) != bound {
        return None;
    }
    let (w, h) = (half(24), half(28));
    if w > bound.0 || h > bound.1 || bytes.len() != HEAD + (w * h * 4) as usize {
        return None;
    }
    if w == 0 || h == 0 {
        return Some(None);
    }
    bytes.drain(..HEAD);
    Some(Some((bytes, w, h)))
}
