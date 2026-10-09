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

/// Moves each of `MOVED` from `System/` to `Config/`, once. A file already in `Config/` is the
/// newer one and is never overwritten; the old copy is then simply left where it was.
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
    Ok(())
}
