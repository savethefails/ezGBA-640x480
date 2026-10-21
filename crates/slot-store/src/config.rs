use std::io;
use std::path::Path;

/// Where the player's settings live: beside `System/` rather than in it, so an update that
/// replaces `System/` wholesale (a Mac's Replace does, for a folder) leaves them alone.
pub const CONFIG_DIR: &str = "Config";

/// The settings files a card from before `Config/` kept in `System/`.
const MOVED: [&str; 4] = [
    "slot.state",
    "selected_core.ini",
    "video_mode.ini",
    "theme.txt",
];

/// The settings that were `.ini` files and are `.txt` now, which Chrome on Windows opens rather
/// than downloading again. Renamed once, after the move out of `System/`, and never over a
/// `.txt` already there. From slot 68a2724.
const RENAMED: [(&str, &str); 4] = [
    ("Config/selected_core.ini", crate::SELECTED_CORE_FILE),
    ("Config/video_mode.ini", "Config/video_mode.txt"),
    ("Config/cart_shell.ini", crate::CART_SHELL_FILE),
    ("Labels/cart_shell.ini", crate::LABELS_SHELL_FILE),
];

/// Moves each of `MOVED` from `System/` to `Config/`, once, then gives each of `RENAMED` its new
/// name. A file already where it is going is the newer one and is never overwritten; the old
/// copy is then simply left where it was.
pub fn move_config(root: &Path) -> io::Result<()> {
    let config = root.join(CONFIG_DIR);
    std::fs::create_dir_all(&config)?;
    for name in MOVED {
        let old = root.join("System").join(name);
        let new = config.join(name);
        if old.is_file() && !new.exists() {
            std::fs::rename(&old, &new)?;
        }
    }
    for (old, new) in RENAMED {
        let old = root.join(old);
        let new = root.join(new);
        if old.is_file() && !new.exists() {
            std::fs::rename(&old, &new)?;
        }
    }
    Ok(())
}
