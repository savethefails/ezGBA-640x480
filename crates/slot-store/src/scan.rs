use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::gba::header_title_code;
use crate::platform::Platform;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cart {
    /// Which console this cart is for, and therefore which folder under `Games/`, `Saves/`,
    /// `States/` and `Labels/` its files live in. Decided by the folder `scan` found the rom
    /// in, never by reading the rom itself.
    pub platform: Platform,
    /// Filename stem, which is the key for labels, saves and states. Not a content hash.
    pub stem: String,
    pub rom: PathBuf,
    pub label: Option<PathBuf>,
    /// A picture of this cart's box (or its title screen, or anything else), shown above the
    /// shelf while it is selected. Same lookup as `label` but under `Images`, and just as
    /// optional: most carts will not have one, and the space above the row stays empty.
    pub box_art: Option<PathBuf>,
    pub title: String,
    /// The four character header game code, empty when the rom has none. A Game Boy cart has
    /// no equivalent field, so this is always empty for `Platform::Gb` and `Platform::Gbc`.
    pub code: String,
    /// The shell the player chose for this cart in `cart_shell.txt`, or `None` for the one the
    /// built-in table picks. See `cart_shell`.
    pub shell: Option<crate::ShellChoice>,
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

/// An unmounted card or a card with no library is an empty shelf, not a boot failure — and so
/// is a platform folder that does not exist, which is the normal state of a card with no
/// Colour carts.
///
/// A platform folder that exists and cannot be read is not a boot failure either, and this is
/// the one place that has to decide that. The only caller is `App::boot`, which does
/// `scan(root).unwrap_or_default()` — so an `Err` out of here is not an error message anywhere,
/// it is every cart on the card gone from the shelf. One folder being unreadable says nothing
/// about the other two, exactly as `migrate_platforms` already argues at length for the sweeps,
/// so a folder that will not open costs the player that folder and nothing else. Same for a
/// single directory entry that will not stat: it costs that one cart.
pub fn scan(root: &Path) -> Result<Vec<Cart>, StoreError> {
    let read = |file: &str| std::fs::read_to_string(root.join(file)).unwrap_or_default();
    let shells = crate::cart_shell::layered(
        &read(crate::CART_SHELL_FILE),
        &read(crate::LABELS_SHELL_FILE),
    );
    let mut carts = Vec::new();
    for platform in Platform::ALL {
        let dir = root.join("Games").join(platform.dir_name());
        let entries = match std::fs::read_dir(&dir) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("slot: scan: {}: {e}", dir.display());
                continue;
            }
        };
        // One listing per art folder rather than a stat per cart, which on a large card was most
        // of the boot. Matched case-insensitively, as the card's FAT matches them anyway.
        let labels_dir = root.join("Labels").join(platform.dir_name());
        let labels = listing(&labels_dir);
        let images_dir = root.join("Images").join(platform.dir_name());
        let images = listing(&images_dir);
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let rom = entry.path();
            // The folder decides the platform; the extension decides whether this is a cart at
            // all. A `.gba` under `GB/` is neither, and is passed over in silence.
            if is_hidden(&rom) || !platform.accepts(&rom) || !is_file(&entry) {
                continue;
            }
            let Some(stem) = rom.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let png = format!("{stem}.png").to_lowercase();
            // The header's title and game code are not read here: opening every rom on the card
            // was the rest of the boot. `header_ids` reads them when something needs them.
            carts.push(Cart {
                platform,
                stem: stem.to_string(),
                title: String::new(),
                code: String::new(),
                label: labels.get(&png).map(|name| labels_dir.join(name)),
                box_art: images.get(&png).map(|name| images_dir.join(name)),
                shell: shells.get(&crate::cart_shell::key(stem)).copied(),
                rom,
            });
        }
    }
    carts.sort_by(|a, b| {
        (a.platform as u8, sort_key(&a.stem)).cmp(&(b.platform as u8, sort_key(&b.stem)))
    });
    Ok(carts)
}

/// A folder's entries by lowercased name, hidden ones left out. Empty for a folder that is not
/// there.
pub fn listing(dir: &Path) -> HashMap<String, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .map(|name| (name.to_lowercase(), name))
        .collect()
}

/// Off the directory entry rather than a stat, except through a symlink.
fn is_file(entry: &std::fs::DirEntry) -> bool {
    match entry.file_type() {
        Ok(t) if t.is_symlink() => entry.path().is_file(),
        Ok(t) => t.is_file(),
        Err(_) => false,
    }
}

/// A cart's header title and game code, as `(title, code)`, read from the rom the first time
/// they are asked for and kept. `scan` leaves them empty so a boot opens no rom; whatever acts
/// on a cart's identity (the shell table, the link screen, a core loading) asks here. A GBA cart
/// has both; a Game Boy cart a title and no code; a SNES cart neither, as before.
pub fn header_ids(cart: &Cart) -> (String, String) {
    if !cart.title.is_empty() || !cart.code.is_empty() {
        return (cart.title.clone(), cart.code.clone());
    }
    static IDS: OnceLock<Mutex<HashMap<PathBuf, (String, String)>>> = OnceLock::new();
    let mut ids = IDS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    ids.entry(cart.rom.clone())
        .or_insert_with(|| match cart.platform {
            Platform::Gba => header_title_code(&cart.rom).unwrap_or_default(),
            // A Game Boy cart has no GBA-style four-character game code, and the fields
            // `gba.rs` reads sit below the Game Boy header entirely.
            Platform::Gb | Platform::Gbc => (
                crate::gb::cached_header(&cart.rom)
                    .map(|h| h.title)
                    .unwrap_or_default(),
                String::new(),
            ),
            Platform::Snes => (String::new(), String::new()),
        })
        .clone()
}

/// Where a title files on the shelf: digits first, then A to Z, and case ignored.
///
/// Plain byte order put `apple` after `Zebra`, because every lowercase letter sorts above every
/// uppercase one, so a card's row depended on how its files happened to be capitalised.
///
/// The group runs ahead of the text rather than being folded into it, so that one digit-led title
/// cannot land between two letters however it is spelled, and anything led by neither, a bracket
/// or a quote, files after both rather than silently first.
pub fn sort_key(stem: &str) -> (u8, String) {
    (group_of(stem), stem.to_uppercase())
}

fn group_of(stem: &str) -> u8 {
    match stem.chars().find(|c| !c.is_whitespace()) {
        Some(c) if c.is_ascii_digit() => 0,
        Some(c) if c.is_alphabetic() => 1,
        _ => 2,
    }
}

/// The letter a title is filed under, for skipping a row a letter at a time. Every digit-led
/// title shares one bucket, and so does everything led by neither a digit nor a letter: a row of
/// thirty carts has few enough of either that giving each its own stop would be a stop that moves
/// by one, which is what the shoulder buttons are already for.
pub fn initial(stem: &str) -> char {
    match group_of(stem) {
        1 => stem
            .chars()
            .find(|c| !c.is_whitespace())
            .and_then(|c| c.to_uppercase().next())
            .unwrap_or('#'),
        _ => '#',
    }
}

/// A leading dot is card metadata rather than content, and every folder on the card is read
/// through this. macOS writes `._<name>` beside each file it copies onto a FAT volume, which
/// carries the extension of the file it shadows, so the extension alone cannot tell them
/// apart. It also sorts first, which is why the sidecar rather than the file is what a picker
/// walking the folder in order tends to land on.
pub fn is_hidden(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}
