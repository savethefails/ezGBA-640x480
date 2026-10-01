//! The picture the bootloader shows while the device starts: the screen as it was when the
//! device last powered off.
//!
//! BaseOS draws nothing on an ordinary boot. From power on until slot's first frame the panel
//! shows `bootlogo.bmp`, which U-Boot reads from `boot-resource`, a small FAT partition on the
//! card the system booted from. Replace the pixels in that file at power off, and the next
//! boot opens on the game it was left in, or the shelf, before slot has even started.
//!
//! The same partition holds the device trees the kernel boots with, and U-Boot reads it
//! before anything can repair it. So the one write here is made as small as it can be:
//!
//! - The file is never created, renamed, truncated or grown. It is opened as it is, and only
//!   the pixel bytes inside it are written, at their own offset, at their own length. The
//!   FAT's tables and directory are never touched, and a cut during the write can only leave
//!   a picture half old and half new.
//! - Its header must already be exactly what U-Boot is known to show: 640x480, 24 bits,
//!   uncompressed, bottom up, the right length. Anything else and nothing is written.
//! - The header itself is left byte for byte as it was.
//! - BaseOS's own logo is copied onto the SD card, and that copy flushed, before its pixels
//!   are first replaced. `boot-picture off` puts it back the same way.
//! - The partition is found by its GPT name on the disk the root filesystem is on, and only
//!   on BaseOS with an unturned panel. Two candidates, none, or one already mounted, and
//!   nothing is done.
//! - Nothing here can keep the device from powering off: every failure is logged and passed
//!   over, and the whole of it has a time limit.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use slot_store::{atomic_write, BootPicture, Theme};
use slot_ui::{OUT_H, OUT_W};

/// The file U-Boot shows, at the root of `boot-resource`.
pub const LOGO: &str = "bootlogo.bmp";

/// BaseOS's own logo, kept on the card before slot first paints over it.
pub const BACKUP: &str = "System/bootlogo-baseos.bmp";

/// The partition's GPT name, which U-Boot itself goes by.
const PARTNAME: &str = "boot-resource";

/// Where the partition is mounted for the moment it takes. `/tmp` is a tmpfs on BaseOS.
const MOUNT_AT: &str = "/tmp/slot-boot-resource";

/// Longer than a mount, a 900 KB read, a 900 KB write and an fsync on any card that works.
const TIME_LIMIT: Duration = Duration::from_secs(5);

const ROW: usize = OUT_W as usize * 3;
const PIXEL_BYTES: usize = ROW * OUT_H as usize;

/// Where a BMP's pixels start, if its header is one U-Boot is known to show at this panel's
/// size: 24-bit, uncompressed, bottom up, a whole number of 4-byte rows, `len` long exactly.
/// `header` is the start of the file, at least 54 bytes of it.
pub fn pixel_offset(header: &[u8], len: u64) -> Option<u64> {
    if header.len() < 54 || &header[0..2] != b"BM" {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([header[i], header[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
    let i32_at = |i: usize| i32::from_le_bytes(header[i..i + 4].try_into().unwrap());
    let offset = u32_at(10) as u64;
    let fits = u32_at(14) >= 40 // BITMAPINFOHEADER or a later one
        && i32_at(18) == OUT_W as i32
        && i32_at(22) == OUT_H as i32 // positive: bottom up
        && u16_at(26) == 1
        && u16_at(28) == 24
        && u32_at(30) == 0 // BI_RGB
        && (54..=4096).contains(&offset)
        && len == offset + PIXEL_BYTES as u64;
    fits.then_some(offset)
}

/// A composed frame, RGBA top row first as `Compositor::read_frame` gives it, as a 24-bit
/// BMP's pixel array: BGR, bottom row first. 640 * 3 is a multiple of four, so rows need no
/// padding.
pub fn bmp_pixels(rgba: &[u8]) -> Option<Vec<u8>> {
    if rgba.len() != OUT_W as usize * OUT_H as usize * 4 {
        return None;
    }
    let mut out = Vec::with_capacity(PIXEL_BYTES);
    for row in rgba.chunks_exact(OUT_W as usize * 4).rev() {
        for px in row.chunks_exact(4) {
            out.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    Some(out)
}

/// What became of the picture.
#[derive(Debug, PartialEq, Eq)]
pub enum Painted {
    Written,
    /// The pixels were already these, so nothing was written at all.
    Unchanged,
}

/// Replaces the pixels inside `logo`, in place. Fails with `InvalidData`, having written
/// nothing, unless the file is already a picture `pixel_offset` accepts.
pub fn paint(logo: &Path, pixels: &[u8]) -> io::Result<Painted> {
    if pixels.len() != PIXEL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a whole picture",
        ));
    }
    // No create, no truncate: a file that is not there stays not there.
    let mut f = OpenOptions::new().read(true).write(true).open(logo)?;
    let len = f.metadata()?.len();
    let mut header = [0u8; 54];
    f.read_exact(&mut header)?;
    let Some(offset) = pixel_offset(&header, len) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a picture slot writes",
        ));
    };
    let mut current = vec![0u8; PIXEL_BYTES];
    f.seek(SeekFrom::Start(offset))?;
    f.read_exact(&mut current)?;
    if current == pixels {
        return Ok(Painted::Unchanged);
    }
    f.seek(SeekFrom::Start(offset))?;
    f.write_all(pixels)?;
    f.sync_all()?;
    Ok(Painted::Written)
}

/// The pixels of a picture `pixel_offset` accepts, and `None` for anything else.
pub fn read_pixels(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    let offset = pixel_offset(&bytes, bytes.len() as u64)? as usize;
    Some(bytes[offset..].to_vec())
}

/// What the power off asks for.
pub enum Want {
    /// This frame, RGBA top row first.
    Scene(Vec<u8>),
    /// BaseOS's logo back.
    Original,
}

/// The whole of it, once the partition is reachable: `logo` is `bootlogo.bmp` on it and
/// `backup` the copy on the SD card.
///
/// The copy comes first and must be on the card before a single pixel of the original is
/// replaced. A copy that already exists is never overwritten: once the logo has been painted,
/// what is in it is no longer BaseOS's.
pub fn apply(logo: &Path, backup: &Path, want: Want) -> io::Result<Painted> {
    let pixels = match want {
        Want::Scene(rgba) => {
            let pixels = bmp_pixels(&rgba)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a whole frame"))?;
            if !backup.exists() {
                let original = std::fs::read(logo)?;
                if pixel_offset(&original, original.len() as u64).is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "not a picture slot writes",
                    ));
                }
                if let Some(dir) = backup.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                atomic_write(backup, &original)?;
            }
            pixels
        }
        Want::Original => read_pixels(backup).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no copy of the logo to put back")
        })?,
    };
    paint(logo, &pixels)
}

/// The block device `boot-resource` is on, or `None` unless there is exactly one on the disk
/// the root filesystem is on. `sys` is `/` on the device and a tree laid out like it in tests.
pub fn find_partition(sys: &Path) -> Option<String> {
    let cmdline = std::fs::read_to_string(sys.join("proc/cmdline")).ok()?;
    let root = cmdline
        .split_whitespace()
        .find_map(|w| w.strip_prefix("root=/dev/"))?;
    // mmcblk0p5 -> mmcblk0; sda5 -> sda.
    let disk = root.trim_end_matches(|c: char| c.is_ascii_digit());
    let disk =
        if disk.ends_with('p') && disk[..disk.len() - 1].ends_with(|c: char| c.is_ascii_digit()) {
            &disk[..disk.len() - 1]
        } else {
            disk
        };
    let mut found = Vec::new();
    for entry in std::fs::read_dir(sys.join("sys/class/block").join(disk)).ok()? {
        let Ok(entry) = entry else { continue };
        let Ok(uevent) = std::fs::read_to_string(entry.path().join("uevent")) else {
            continue;
        };
        let name = uevent.lines().find_map(|l| l.strip_prefix("DEVNAME="));
        let part = uevent.lines().find_map(|l| l.strip_prefix("PARTNAME="));
        if let (Some(name), Some(PARTNAME)) = (name, part) {
            found.push(name.to_string());
        }
    }
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// BaseOS, with a panel that is not mounted turned. A turned panel's logo is stored turned,
/// which a frame is not.
pub fn on_baseos(sys: &Path) -> bool {
    let Ok(release) = std::fs::read_to_string(sys.join("etc/baseos-release")) else {
        return false;
    };
    release
        .lines()
        .find_map(|l| l.strip_prefix("BASEOS_PANEL_ROTATION_CCW="))
        .map(|v| v.trim().trim_matches('"') == "0")
        .unwrap_or(true)
}

fn mounted(sys: &Path, device: &str) -> bool {
    std::fs::read_to_string(sys.join("proc/mounts"))
        .map(|m| {
            m.lines()
                .any(|l| l.split_whitespace().next() == Some(&format!("/dev/{device}")))
        })
        .unwrap_or(true) // unreadable: assume the worst
}

/// Mounts the partition, applies, and unmounts, on the device.
fn on_device(card: &Path, want: Want) -> Result<Painted, String> {
    let sys = Path::new("/");
    if !on_baseos(sys) {
        return Err("not BaseOS with an unturned panel".into());
    }
    let device = find_partition(sys).ok_or("no single boot-resource partition on the boot disk")?;
    if mounted(sys, &device) {
        return Err(format!("/dev/{device} is already mounted"));
    }
    std::fs::create_dir_all(MOUNT_AT).map_err(|e| e.to_string())?;
    let ok = Command::new("/bin/mount")
        .args([
            "-t",
            "vfat",
            "-o",
            "rw,noatime",
            &format!("/dev/{device}"),
            MOUNT_AT,
        ])
        .status()
        .map_err(|e| e.to_string())?
        .success();
    if !ok {
        return Err("mount failed".into());
    }
    let result = apply(&Path::new(MOUNT_AT).join(LOGO), &card.join(BACKUP), want);
    let _ = Command::new("/bin/umount").arg(MOUNT_AT).status();
    result.map_err(|e| e.to_string())
}

/// Called on the way to a power off or a restart, with the frame that was on screen just before
/// the shutdown began. Never fails and never waits longer than `TIME_LIMIT`.
pub fn on_power_off(card: &Path, scene: Option<Vec<u8>>) {
    let want = match (Theme::read(card).boot_picture, scene) {
        (BootPicture::Last, Some(scene)) => Want::Scene(scene),
        (BootPicture::Last, None) => return,
        // Never touched, nothing to put back: the partition is not even mounted.
        (BootPicture::Off, _) if !card.join(BACKUP).exists() => return,
        (BootPicture::Off, _) => Want::Original,
    };
    let card: PathBuf = card.to_path_buf();
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("slot-boot-picture".into())
        .spawn(move || {
            let _ = tx.send(on_device(&card, want));
        });
    if spawned.is_err() {
        eprintln!("slot: boot picture: no thread to write it from");
        return;
    }
    match rx.recv_timeout(TIME_LIMIT) {
        Ok(Ok(painted)) => eprintln!("slot: boot picture: {painted:?}"),
        Ok(Err(e)) => eprintln!("slot: boot picture: left as it was: {e}"),
        Err(_) => eprintln!("slot: boot picture: gave up waiting, powering off anyway"),
    }
}
