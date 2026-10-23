//! The link's own lines, appended to `Logs/slot-link.log` on the card as well as stderr. Stderr
//! goes to `/tmp/slot.log`, which a reboot clears, and a link that hangs is exactly the case a
//! player reboots out of before anything can copy it off. `slotlink.sh` traces the radio into
//! the same file, so one log reads the whole attempt in order: radio, socket, state, session.

use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const LINK_LOG_NAME: &str = "slot-link.log";

/// Past this the log starts again, the old one kept beside it as `.old`: links are a handful of
/// lines each, so this is many sessions, and a card is never filled by a log nobody reads.
const MAX_BYTES: u64 = 256 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();
/// One writer at a time, so the emulation thread and the link thread never interleave a line.
static WRITE: Mutex<()> = Mutex::new(());

/// Where the lines go from here on. Unset, as on the host, they go to stderr alone.
pub fn install(path: &Path) {
    let _ = PATH.set(path.to_path_buf());
}

pub fn path() -> Option<&'static Path> {
    PATH.get().map(PathBuf::as_path)
}

/// One line to stderr and, when installed, to the card. Synced per line, because the line
/// before a hang is the one worth having and a hang is followed by a held power button. These
/// come once per step of a link starting, never per frame, so the sync costs nothing that shows.
pub fn line(args: fmt::Arguments) {
    eprintln!("{args}");
    let Some(path) = path() else {
        return;
    };
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {args}", uptime());
        let _ = f.sync_data();
    }
}

/// Seconds since boot, as `slotlink.sh` stamps its own lines, so the two sort together.
fn uptime() -> String {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "-".into())
}

/// `eprintln!`, kept on the card too. See [`line`].
#[macro_export]
macro_rules! link_log {
    ($($arg:tt)*) => {
        $crate::link_log::line(format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_log_starts_again_and_keeps_the_last_one() {
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("Logs").join(LINK_LOG_NAME);
        std::fs::create_dir_all(log.parent().unwrap()).unwrap();
        std::fs::write(&log, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        // `PATH` is process-wide and set once, so this drives the rotation the way `line` does.
        let _ = PATH.set(log.clone());
        if path() != Some(log.as_path()) {
            return;
        }
        line(format_args!("slot: cable: sending 1 bytes of state"));
        let got = std::fs::read_to_string(&log).unwrap();
        assert!(
            got.ends_with(" slot: cable: sending 1 bytes of state\n"),
            "{got:?}"
        );
        assert_eq!(
            std::fs::metadata(log.with_extension("log.old"))
                .unwrap()
                .len(),
            MAX_BYTES + 1
        );
    }
}
