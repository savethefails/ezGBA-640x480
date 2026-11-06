//! What PICO-8 needs to download carts from the BBS: a multicart such as pico_ball fetches its
//! other carts by id the first time each is loaded, and keeps them in `bbs/carts/` under its
//! home, `Saves/pico8`, for good.
//!
//! PICO-8 on Linux downloads by running `wget` from `PATH` (the aarch64 build has no libcurl,
//! and `use_wget 1` makes every build take that road). BaseOS's `wget` is BusyBox's, whose
//! HTTPS refuses any address unless told not to check the certificate, and PICO-8 fetches its
//! carts over HTTPS. So slot puts a `wget` of its own ahead of BusyBox's, which hands the fetch
//! to BaseOS's curl and its certificate roots.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The wrapper, as it is written out. See the script for what it understands.
pub const WGET: &str = include_str!("pico8_wget.sh");

/// Where the wrapper goes on the device. The card cannot hold an exec bit, and `/run` is a
/// tmpfs mounted without `noexec`, so a file written there can be run.
pub const BIN: &str = "/run/ezgba/bin";

/// The variable that tells the wrapper where to log, and the log's name in the card's `Logs/`.
pub const LOG_VAR: &str = "EZGBA_WGET_LOG";
pub const LOG_NAME: &str = "pico8-wget.log";

/// A line in the download log, when there is one: the device sets it, nothing else does.
pub fn note(line: &str) {
    use std::io::Write;
    let Some(path) = std::env::var_os(LOG_VAR) else {
        return;
    };
    let path = PathBuf::from(path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{} {line}", uptime());
    }
}

fn uptime() -> String {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
        .unwrap_or_else(|| "-".into())
}

/// Before PICO-8 starts: which `wget` it will find, and what its config says about using it.
pub fn note_launch(home: &Path) {
    if std::env::var_os(LOG_VAR).is_none() {
        return;
    }
    let wget = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("wget"))
                .find(|w| w.exists())
                .map_or("none".to_string(), |w| w.display().to_string())
        })
        .unwrap_or_else(|| "no PATH".into());
    let config = std::fs::read_to_string(home.join("config.txt")).unwrap_or_default();
    let line = config
        .lines()
        .find(|l| l.split_whitespace().next() == Some("use_wget"))
        .map(|l| l.split("//").next().unwrap_or(l).trim().to_string())
        .unwrap_or_else(|| "no use_wget line".into());
    note(&format!(
        "slot: PICO-8 starting: wget is {wget}; config says {line}"
    ));
}

/// Once Wi-Fi is up: whether the BBS can be reached at all, through the same wrapper PICO-8
/// uses, on a thread of its own so the frame loop never waits on the network. Its result is the
/// wrapper's own lines in the log, after what the network looks like.
pub fn check_bbs(ssid: &str) {
    if std::env::var_os(LOG_VAR).is_none() {
        return;
    }
    let ssid = ssid.to_string();
    let _ = std::thread::Builder::new()
        .name("slot-bbs-check".into())
        .spawn(move || {
            let dns = std::fs::read_to_string("/etc/resolv.conf")
                .unwrap_or_default()
                .lines()
                .filter(|l| l.starts_with("nameserver"))
                .collect::<Vec<_>>()
                .join(", ");
            let now = std::process::Command::new("date")
                .arg("-u")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            note(&format!(
                "slot: Wi-Fi connected to {ssid}; DNS: {}; clock: {now}; checking the BBS",
                if dns.is_empty() { "none" } else { &dns }
            ));
            // The two hosts PICO-8 fetches from: the BBS's lookups, and the carts themselves. A
            // refusal of the bare address (curl exit 22) still means the host was reached.
            let out = std::env::temp_dir().join("ezgba-bbs-check");
            for url in [
                "https://www.lexaloffle.com/bbs/",
                "https://carts.lexaloffle.com/",
            ] {
                let ran = std::process::Command::new("wget")
                    .arg(url)
                    .arg("-q")
                    .arg("-O")
                    .arg(&out)
                    .status();
                if let Err(e) = ran {
                    note(&format!("slot: could not run wget: {e}"));
                }
            }
            let _ = std::fs::remove_file(&out);
        });
}

/// Writes the wrapper into `dir` as `wget`, executable.
pub fn install_wget(dir: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    let path = dir.join("wget");
    std::fs::write(&path, WGET)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

/// `PATH` with `dir` first, so a `wget` in it is the one a shell finds.
pub fn path_with(dir: &Path, path: Option<OsString>) -> OsString {
    let mut out = OsString::from(dir);
    if let Some(rest) = path.filter(|p| !p.is_empty()) {
        out.push(":");
        out.push(rest);
    }
    out
}

/// PICO-8's home on the card: the save directory slot gives the core, and the folder the core
/// keeps PICO-8 in under it.
pub fn home(root: &Path) -> PathBuf {
    crate::root::saves_dir(root).join("pico8")
}

/// Makes `config.txt` in PICO-8's home say `use_wget 1`, and leaves every other line of it as it
/// was. PICO-8 reads it on starting and writes it back on quitting, keeping what it read, so a
/// file holding nothing but this line is enough: PICO-8 fills in the rest.
pub fn prefer_wget(home: &Path) -> io::Result<()> {
    let path = home.join("config.txt");
    let old = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let new = with_wget(&old);
    if new == old {
        return Ok(());
    }
    std::fs::create_dir_all(home)?;
    slot_store::atomic_write(&path, new.as_bytes())
}

const LINE: &str = "use_wget 1";

fn with_wget(config: &str) -> String {
    let mut found = false;
    let mut out: Vec<String> = config
        .lines()
        .map(|l| {
            if l.split_whitespace().next() == Some("use_wget") {
                found = true;
                // The comment PICO-8 writes after the number is kept.
                match l.split_once("//") {
                    Some((_, comment)) => format!("{LINE} //{comment}"),
                    None => LINE.to_string(),
                }
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(LINE.to_string());
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_with_no_config_gets_one_that_says_use_wget() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("Saves/pico8");
        prefer_wget(&home).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join("config.txt")).unwrap(),
            "use_wget 1\n"
        );
    }

    #[test]
    fn pico8s_own_config_keeps_every_line_and_its_comment() {
        let config = "// :: Video Settings\nwindow_size 0 0\n\
            use_wget 0 // (Linux) 1 to use wget for downloads instead of libcurl (must be installed)\n\
            root_path /x/\n";
        let out = with_wget(config);
        assert_eq!(
            out,
            "// :: Video Settings\nwindow_size 0 0\n\
            use_wget 1 // (Linux) 1 to use wget for downloads instead of libcurl (must be installed)\n\
            root_path /x/\n"
        );
        assert_eq!(with_wget(&out), out, "a second pass changed it again");
    }

    #[test]
    fn a_config_already_on_wget_is_not_rewritten() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("config.txt");
        std::fs::write(&path, "volume 256\nuse_wget 1\n").unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        prefer_wget(d.path()).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before
        );
    }

    #[test]
    fn the_wrapper_goes_first_on_path_and_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let w = install_wget(d.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&w).unwrap(), WGET);
        assert_eq!(
            std::fs::metadata(&w).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            path_with(d.path(), Some("/usr/bin:/bin".into())),
            OsString::from(format!("{}:/usr/bin:/bin", d.path().display()))
        );
        assert_eq!(path_with(d.path(), None), OsString::from(d.path()));
    }
}
