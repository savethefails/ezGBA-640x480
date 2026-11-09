//! A PICO-8 multicart's other carts, fetched ahead of time so the game plays with Wi-Fi off.
//!
//! A multicart names its parts by BBS id, `load("#lemcavea")`, and PICO-8 fetches each one the
//! first time it is loaded, keeping two files per cart in `bbs/carts/` under its home: `<id>.nfo`,
//! the BBS's answer naming the current revision (`lid:lemcavea-14`), and `<lid>.p8.png`, the cart.
//! With both there, a load with no network falls back to them. This writes the same two files,
//! fetched from the same addresses PICO-8 asks, for every id a cart names, then for every id
//! those carts name in turn.
//!
//! The ids come from the cart's string literals: `"#lemleveldata,#lemcavea"`, `"#snekburd_extra"`.
//! A part named some other way (built up from pieces at run time) is not found here, and PICO-8
//! still fetches it itself the first time it is wanted with Wi-Fi on.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::pico8_patch::read_code;

/// Where PICO-8 asks, word for word, with the version the device's build sends.
const NFO_URL: &str = "http://www.lexaloffle.com/bbs/cpost_lister3.php?nfo=1&version=000207r&lid=";
const CART_URL: &str = "http://www.lexaloffle.com/bbs/get_cart.php?cat=7&play_src=2&lid=";

/// The most carts one multicart may pull in. Far past any real one (Lemmings is twelve), and
/// there so a cart that names hundreds cannot fill the card.
pub const MAX_CARTS: usize = 64;

/// PICO-8's download cache on the card.
pub fn carts_dir(root: &Path) -> PathBuf {
    crate::pico8_net::home(root).join("bbs/carts")
}

/// Fetches `url` into `out`, `true` when it arrived.
pub trait Fetch {
    fn fetch(&mut self, url: &str, out: &Path) -> bool;
}

/// BaseOS's curl, checked against the system's roots, as PICO-8's own downloads are.
pub struct Curl;

impl Fetch for Curl {
    fn fetch(&mut self, url: &str, out: &Path) -> bool {
        const CURL: &str = "/usr/bin/curl";
        const ROOTS: &str = "/etc/ssl/certs/ca-certificates.crt";
        let out_s = out.to_string_lossy().into_owned();
        let mut args = vec![
            "-fsSL",
            "--connect-timeout",
            "15",
            "--max-time",
            "120",
            "-o",
            &out_s,
        ];
        if Path::new(ROOTS).exists() {
            args.extend(["--cacert", ROOTS]);
        }
        args.push(url);
        // A radio that dozed through a quiet minute can lose the first answer, as the wrapper
        // knows: a name that will not resolve or a host that will not answer is asked again.
        for attempt in 1..=3 {
            let code = std::process::Command::new(CURL)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok()
                .and_then(|s| s.code());
            crate::pico8_net::note(&format!("slot: offline copy: {url}: curl exit {code:?}"));
            match code {
                Some(0) => return true,
                Some(6 | 7) if attempt < 3 => std::thread::sleep(std::time::Duration::from_secs(1)),
                _ => return false,
            }
        }
        false
    }
}

/// The BBS ids named in a cart's string literals, in the order they first appear.
pub fn refs(code: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in strings(code) {
        let mut i = 0;
        while let Some(off) = s[i..].iter().position(|&b| b == b'#') {
            let at = i + off;
            i = at + 1;
            let starts = at == 0 || matches!(s[at - 1], b',' | b' ' | b';' | b'\n' | b'\t');
            if !starts {
                continue;
            }
            let end = s[i..]
                .iter()
                .position(|&b| !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
                .map_or(s.len(), |n| i + n);
            let mut id_end = end;
            // An id may name its revision, `#lemcavea-14`.
            if end < s.len() && s[end] == b'-' {
                let digits = s[end + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if digits > 0 {
                    id_end = end + 1 + digits;
                }
            }
            if end > i {
                let id = String::from_utf8_lossy(&s[i..id_end]).into_owned();
                if !out.contains(&id) {
                    out.push(id);
                }
            }
            i = id_end.max(i);
        }
    }
    out
}

/// Every string literal in Lua source, comments skipped.
fn strings(code: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = code.len();
    // `[[`, `[=[`, ... at `i`: the level and where the contents start.
    let long_open = |i: usize| -> Option<(usize, usize)> {
        if code.get(i) != Some(&b'[') {
            return None;
        }
        let eq = code[i + 1..].iter().take_while(|&&b| b == b'=').count();
        (code.get(i + 1 + eq) == Some(&b'[')).then_some((eq, i + 2 + eq))
    };
    let long_close = |from: usize, eq: usize| -> (usize, usize) {
        let mut j = from;
        while j < n {
            if code[j] == b']' {
                let e = code[j + 1..].iter().take_while(|&&b| b == b'=').count();
                if e == eq && code.get(j + 1 + e) == Some(&b']') {
                    return (j, j + 2 + e);
                }
            }
            j += 1;
        }
        (n, n)
    };
    while i < n {
        let b = code[i];
        if b == b'-' && code.get(i + 1) == Some(&b'-') {
            if let Some((eq, start)) = long_open(i + 2) {
                i = long_close(start, eq).1;
            } else {
                i = code[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(n, |p| i + p + 1);
            }
        } else if b == b'"' || b == b'\'' {
            let start = i + 1;
            let mut j = start;
            while j < n && code[j] != b && code[j] != b'\n' {
                j += if code[j] == b'\\' { 2 } else { 1 };
            }
            out.push(&code[start..j.min(n)]);
            i = j + 1;
        } else if let Some((eq, start)) = long_open(i) {
            let (end, next) = long_close(start, eq);
            out.push(&code[start..end]);
            i = next;
        } else {
            i += 1;
        }
    }
    out
}

/// The cart PICO-8 keeps for `id`, when both its files are there.
pub fn cached(dir: &Path, id: &str) -> Option<PathBuf> {
    let nfo = std::fs::read_to_string(dir.join(format!("{id}.nfo"))).ok()?;
    let lid = lid_of(&nfo)?;
    let cart = dir.join(format!("{lid}.p8.png"));
    std::fs::metadata(&cart)
        .is_ok_and(|m| m.len() > 0)
        .then_some(cart)
}

/// The revision an nfo names, `lid:lemcavea-14`.
fn lid_of(nfo: &str) -> Option<String> {
    let lid = nfo.lines().find_map(|l| l.strip_prefix("lid:"))?.trim();
    let ok = !lid.is_empty()
        && lid
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    ok.then(|| lid.to_string())
}

/// The ids `cart` needs that the cache does not have yet. Only those reachable through carts
/// already kept: a missing cart's own parts are not known until it is fetched.
pub fn missing(root: &Path, cart: &Path) -> Vec<String> {
    let dir = carts_dir(root);
    let Ok(code) = read_code(cart) else {
        return Vec::new();
    };
    let mut queue: VecDeque<String> = refs(&code).into();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) || seen.len() > MAX_CARTS {
            continue;
        }
        match cached(&dir, &id) {
            Some(path) => queue.extend(read_code(&path).map(|c| refs(&c)).unwrap_or_default()),
            None => out.push(id),
        }
    }
    out
}

/// Fetches every cart `cart` needs and does not have, and the carts those need. `progress` is
/// told how many are done of how many are known so far, which grows as carts name more.
/// Returns how many were fetched, or the id that could not be.
pub fn download(
    root: &Path,
    cart: &Path,
    fetch: &mut dyn Fetch,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<usize, String> {
    let dir = carts_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let code = read_code(cart).map_err(|e| format!("{}: {e}", cart.display()))?;
    let mut queue: VecDeque<String> = refs(&code).into();
    let mut seen: HashSet<String> = HashSet::new();
    let mut fetched = 0;
    while let Some(id) = queue.pop_front() {
        if seen.contains(&id) {
            continue;
        }
        if seen.len() >= MAX_CARTS {
            break;
        }
        seen.insert(id.clone());
        progress(seen.len() - 1, seen.len() + queue.len());
        let path = match cached(&dir, &id) {
            Some(path) => path,
            None => {
                let path = fetch_one(&dir, &id, fetch).ok_or_else(|| id.clone())?;
                fetched += 1;
                path
            }
        };
        if let Ok(code) = read_code(&path) {
            queue.extend(refs(&code).into_iter().filter(|r| !seen.contains(r)));
        }
    }
    progress(seen.len(), seen.len());
    Ok(fetched)
}

/// One cart into the cache as PICO-8 would keep it: the nfo first, for the revision, then the
/// cart under that revision's name, and only then the nfo under the id's name, so a cart is
/// never named by an nfo before it is all there.
fn fetch_one(dir: &Path, id: &str, fetch: &mut dyn Fetch) -> Option<PathBuf> {
    // Not PICO-8's own `temp-<id>.nfo`, which a game running at the same time may be writing.
    let temp = dir.join(format!("ezgba-{id}.nfo.part"));
    let ok = fetch.fetch(&format!("{NFO_URL}{id}"), &temp);
    let nfo = std::fs::read_to_string(&temp).unwrap_or_default();
    let _ = std::fs::remove_file(&temp);
    if !ok {
        return None;
    }
    let lid = lid_of(&nfo)?;
    let cart = dir.join(format!("{lid}.p8.png"));
    let part = dir.join(format!("{lid}.p8.png.part"));
    let ok = fetch.fetch(&format!("{CART_URL}{lid}"), &part);
    let png = std::fs::read(&part).is_ok_and(|b| b.starts_with(b"\x89PNG"));
    if !ok || !png || std::fs::rename(&part, &cart).is_err() {
        let _ = std::fs::remove_file(&part);
        return None;
    }
    std::fs::write(dir.join(format!("{id}.nfo")), nfo).ok()?;
    Some(cart)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_come_from_strings_and_not_from_the_length_operator() {
        let code = br##"
local ids = "#lemleveldata,#lemcavea,#" .. stat(101)
-- load("#commented_out")
load(stat"101" and "#snekburd_extra" or "snekburd_extra.p8")
x = #pos + #tbl
s = [[#long_one]]
t = 'a #not_at_start_x#y'
load("#rev_named-12")
"##;
        assert_eq!(
            refs(code),
            [
                "lemleveldata",
                "lemcavea",
                "snekburd_extra",
                "long_one",
                "not_at_start_x",
                "rev_named-12"
            ]
        );
    }

    #[test]
    fn an_nfo_names_its_revision() {
        assert_eq!(
            lid_of("lid:lemcavea-14\nmid:1\ntitle:x\n").as_deref(),
            Some("lemcavea-14")
        );
        assert_eq!(lid_of("title:x\n"), None);
        assert_eq!(lid_of("lid:../evil\n"), None);
    }
}
