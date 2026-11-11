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
        let head = out.with_extension("headers");
        let head_s = head.to_string_lossy().into_owned();
        let mut args = vec![
            "-fsSL",
            "--connect-timeout",
            "10",
            "--max-time",
            "60",
            "-o",
            &out_s,
            "-D",
            &head_s,
            "-w",
            "%{http_code}",
        ];
        if Path::new(ROOTS).exists() {
            args.extend(["--cacert", ROOTS]);
        }
        args.push(url);
        let cart = url.contains("get_cart.php");
        let mut tries = 0;
        let mut busy_waits = 0;
        loop {
            tries += 1;
            if cart {
                pace_cart();
            }
            let started = std::time::Instant::now();
            let ran = std::process::Command::new(CURL)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output();
            let ms = started.elapsed().as_millis();
            let (code, http, said) = match &ran {
                Ok(o) => (
                    o.status.code(),
                    String::from_utf8_lossy(&o.stdout).trim().to_string(),
                    String::from_utf8_lossy(&o.stderr).trim().to_string(),
                ),
                Err(e) => (None, String::new(), format!("could not run {CURL}: {e}")),
            };
            let headers = std::fs::read_to_string(&head).unwrap_or_default();
            let _ = std::fs::remove_file(&head);
            let bytes = std::fs::metadata(out).map_or(0, |m| m.len());
            log(&format!(
                "  curl {url} (try {tries}): exit {}, HTTP {}, {bytes} bytes, {ms} ms{}",
                code.map_or("none".into(), |c| c.to_string()),
                if http.is_empty() { "-" } else { &http },
                if said.is_empty() {
                    String::new()
                } else {
                    format!(": {said}")
                }
            ));
            match code {
                Some(0) => return true,
                // Too many requests: the BBS lets a burst of carts through and then refuses for
                // about a minute. Waited out, as long as it says or longer each time, and the same
                // cart asked for again.
                Some(22) if http == "429" || said.contains("429") => {
                    if busy_waits >= BUSY_WAITS.len() {
                        log("  the BBS is still refusing: giving up on this cart for now");
                        return false;
                    }
                    let asked = retry_after(&headers);
                    let wait = asked.unwrap_or(BUSY_WAITS[busy_waits]).clamp(5, 180);
                    busy_waits += 1;
                    log(&format!(
                        "  the BBS says too many requests (429){}; waiting {wait} s, then asking again",
                        asked.map_or(String::new(), |s| format!(", retry after {s} s"))
                    ));
                    wait_for_bbs(wait);
                }
                // A radio that dozed through a quiet minute can lose the first answer, as the
                // wrapper knows: a name that will not resolve or a host that will not answer is
                // asked again, but only while there is a route to ask over at all.
                Some(6 | 7) if tries < 3 && crate::pico8_net::has_route() => {
                    std::thread::sleep(std::time::Duration::from_secs(1))
                }
                Some(6 | 7) if tries < 3 => {
                    log("  no network route: not asking again");
                    return false;
                }
                _ => return false,
            }
        }
    }
}

/// How long to wait each time the BBS says too many requests, when it does not say itself.
const BUSY_WAITS: [u64; 4] = [20, 40, 60, 60];

/// The least time between one cart download and the next. The BBS lets a handful through in
/// a burst and then refuses for a minute; spacing them out keeps under that more often.
const CART_GAP_MS: u64 = 2000;

/// When the downloads may next ask the BBS, in milliseconds since the epoch: the end of a wait
/// on a refusal, which the screens show as a countdown.
static WAIT_UNTIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_CART: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Seconds left of a wait the BBS asked for, while one is running.
pub fn bbs_wait_secs() -> Option<u64> {
    let until = WAIT_UNTIL.load(std::sync::atomic::Ordering::Relaxed);
    let now = epoch_ms();
    (until > now).then(|| (until - now).div_ceil(1000))
}

fn wait_for_bbs(secs: u64) {
    WAIT_UNTIL.store(
        epoch_ms() + secs * 1000,
        std::sync::atomic::Ordering::Relaxed,
    );
    std::thread::sleep(std::time::Duration::from_secs(secs));
    WAIT_UNTIL.store(0, std::sync::atomic::Ordering::Relaxed);
}

fn pace_cart() {
    let Ok(mut last) = LAST_CART.lock() else {
        return;
    };
    if let Some(at) = *last {
        let gap = std::time::Duration::from_millis(CART_GAP_MS);
        if let Some(left) = gap.checked_sub(at.elapsed()) {
            std::thread::sleep(left);
        }
    }
    *last = Some(std::time::Instant::now());
}

/// A `Retry-After` given in seconds.
fn retry_after(headers: &str) -> Option<u64> {
    headers.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("retry-after")
            .then(|| v.trim().parse().ok())
            .flatten()
    })
}

/// A line in the download log, marked as this module's.
fn log(line: &str) {
    crate::pico8_net::note(&format!("slot: cache: {line}"));
}

/// A file's size for the log, or that it is not there.
fn size_of(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(m) => format!("{} bytes", m.len()),
        Err(_) => "missing".into(),
    }
}

/// A short, one-line look at text that came back, for the log.
fn glimpse(bytes: &[u8]) -> String {
    let text: String = String::from_utf8_lossy(&bytes[..bytes.len().min(120)])
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    text.trim().to_string()
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
///
/// Also writes `<lid>.nfo` beside it when that is missing. A cart run from the BBS hears its
/// own id with the revision on, `stat(101)` is `lemmings_p8-3`, and Lemmings loads itself back
/// by that name when it has finished installing; offline, PICO-8 finds that only through an
/// nfo named for it, and without one the load fails and the game hangs. PICO-8 itself writes
/// only `<id>.nfo`.
pub fn cached(dir: &Path, id: &str) -> Option<PathBuf> {
    let nfo = std::fs::read_to_string(dir.join(format!("{id}.nfo"))).ok()?;
    let lid = lid_of(&nfo)?;
    let cart = dir.join(format!("{lid}.p8.png"));
    if !std::fs::metadata(&cart).is_ok_and(|m| m.len() > 0) {
        return None;
    }
    keep_lid_nfo(dir, id, &lid, &nfo);
    Some(cart)
}

/// `<lid>.nfo`, the same answer under the revision's own name, when it is not there yet.
fn keep_lid_nfo(dir: &Path, id: &str, lid: &str, nfo: &str) {
    let path = dir.join(format!("{lid}.nfo"));
    if lid == id || path.exists() {
        return;
    }
    match std::fs::write(&path, nfo) {
        Ok(()) => log(&format!(
            "#{id}: wrote {lid}.nfo, for loads by its revision"
        )),
        Err(e) => log(&format!("#{id}: cannot write {lid}.nfo: {e}")),
    }
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
    let started = std::time::Instant::now();
    let dir = carts_dir(root);
    log(&format!(
        "download for {} into {}",
        cart.display(),
        dir.display()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| {
        log(&format!("cannot make {}: {e}", dir.display()));
        format!("{}: {e}", dir.display())
    })?;
    let code = read_code(cart).map_err(|e| {
        log(&format!("cannot read {}: {e}", cart.display()));
        format!("{}: {e}", cart.display())
    })?;
    let mut queue: VecDeque<String> = refs(&code).into();
    log(&format!(
        "{} names {:?} ({} bytes of code)",
        name_of(cart),
        queue,
        code.len()
    ));
    let mut seen: HashSet<String> = HashSet::new();
    let mut fetched = 0;
    while let Some(id) = queue.pop_front() {
        if seen.contains(&id) {
            continue;
        }
        if seen.len() >= MAX_CARTS {
            log(&format!(
                "stopping at {MAX_CARTS} carts, {} still queued: {:?}",
                queue.len() + 1,
                queue
            ));
            break;
        }
        seen.insert(id.clone());
        progress(seen.len() - 1, seen.len() + queue.len());
        let path = match cached(&dir, &id) {
            Some(path) => {
                log(&format!(
                    "#{id}: already kept as {} ({})",
                    name_of(&path),
                    size_of(&path)
                ));
                path
            }
            None => {
                log(&format!(
                    "#{id}: fetching ({} of {} known)",
                    seen.len(),
                    seen.len() + queue.len()
                ));
                let one = std::time::Instant::now();
                let Some(path) = fetch_one(&dir, &id, fetch) else {
                    log(&format!(
                        "#{id}: failed after {} ms; {fetched} fetched before it, {} ms in all",
                        one.elapsed().as_millis(),
                        started.elapsed().as_millis()
                    ));
                    report(root, cart);
                    return Err(id);
                };
                log(&format!(
                    "#{id}: kept as {} ({}) in {} ms",
                    name_of(&path),
                    size_of(&path),
                    one.elapsed().as_millis()
                ));
                fetched += 1;
                path
            }
        };
        match read_code(&path) {
            Ok(code) => {
                let named = refs(&code);
                let new: Vec<&String> = named
                    .iter()
                    .filter(|r| !seen.contains(*r) && !queue.contains(r))
                    .collect();
                log(&format!("#{id} names {named:?}; new to the list: {new:?}"));
                queue.extend(refs(&code).into_iter().filter(|r| !seen.contains(r)));
            }
            Err(e) => log(&format!(
                "#{id}: cannot read its code ({e}), so what it names is not known"
            )),
        }
    }
    progress(seen.len(), seen.len());
    log(&format!(
        "done: {fetched} fetched, {} carts in all, {} ms",
        seen.len(),
        started.elapsed().as_millis()
    ));
    report(root, cart);
    Ok(fetched)
}

/// One cart into the cache as PICO-8 would keep it: the nfo first, for the revision, then the
/// cart under that revision's name, and only then the nfo under the id's name, so a cart is
/// never named by an nfo before it is all there.
fn fetch_one(dir: &Path, id: &str, fetch: &mut dyn Fetch) -> Option<PathBuf> {
    // Not PICO-8's own `temp-<id>.nfo`, which a game running at the same time may be writing.
    let temp = dir.join(format!("ezgba-{id}.nfo.part"));
    let ok = fetch.fetch(&format!("{NFO_URL}{id}"), &temp);
    let nfo = std::fs::read(&temp).unwrap_or_default();
    let _ = std::fs::remove_file(&temp);
    if !ok {
        log(&format!("#{id}: the BBS did not answer for its info"));
        return None;
    }
    let nfo = String::from_utf8_lossy(&nfo).into_owned();
    log(&format!(
        "#{id}: info ({} bytes): {}",
        nfo.len(),
        glimpse(nfo.as_bytes())
    ));
    let Some(lid) = lid_of(&nfo) else {
        log(&format!(
            "#{id}: the info names no revision (no usable lid: line), so there is no cart to ask for"
        ));
        return None;
    };
    if lid != id && !lid.starts_with(&format!("{id}-")) {
        log(&format!(
            "#{id}: note: the info names {lid}, not a revision of {id}; fetching it anyway"
        ));
    }
    let cart = dir.join(format!("{lid}.p8.png"));
    let part = dir.join(format!("{lid}.p8.png.part"));
    let ok = fetch.fetch(&format!("{CART_URL}{lid}"), &part);
    let bytes = std::fs::read(&part).unwrap_or_default();
    let png = bytes.starts_with(b"\x89PNG");
    if !ok || !png {
        if !ok {
            log(&format!("#{id}: the cart {lid} did not arrive"));
        } else {
            log(&format!(
                "#{id}: what came back for {lid} is not a PNG ({} bytes): {}",
                bytes.len(),
                glimpse(&bytes)
            ));
        }
        let _ = std::fs::remove_file(&part);
        return None;
    }
    if let Err(e) = std::fs::rename(&part, &cart) {
        log(&format!(
            "#{id}: cannot move {} into place: {e}",
            part.display()
        ));
        let _ = std::fs::remove_file(&part);
        return None;
    }
    if let Err(e) = std::fs::write(dir.join(format!("{id}.nfo")), &nfo) {
        log(&format!("#{id}: cannot write {id}.nfo: {e}"));
        return None;
    }
    keep_lid_nfo(dir, id, &lid, &nfo);
    Some(cart)
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Writes to the log what `cart` needs and what the cache holds for it: every id reachable
/// from it, kept (with the revision and size) or missing, and what else is in the cache. For
/// telling what a multicart expects apart from what is on the card.
pub fn report(root: &Path, cart: &Path) {
    let dir = carts_dir(root);
    let Ok(code) = read_code(cart) else {
        log(&format!("report: cannot read {}", cart.display()));
        return;
    };
    let mut queue: VecDeque<(String, String)> = refs(&code)
        .into_iter()
        .map(|r| (r, name_of(cart)))
        .collect();
    let mut seen = HashSet::new();
    let (mut kept, mut gone) = (Vec::new(), Vec::new());
    let mut wanted_files = HashSet::new();
    while let Some((id, from)) = queue.pop_front() {
        if !seen.insert(id.clone()) || seen.len() > MAX_CARTS {
            continue;
        }
        let nfo = dir.join(format!("{id}.nfo"));
        wanted_files.insert(format!("{id}.nfo"));
        let text = std::fs::read_to_string(&nfo).ok();
        let lid = text.as_deref().and_then(lid_of);
        match (&text, &lid, cached(&dir, &id)) {
            (_, Some(lid), Some(path)) => {
                wanted_files.insert(format!("{lid}.p8.png"));
                kept.push(format!("#{id} -> {lid} ({})", size_of(&path)));
                let named = read_code(&path).map(|c| refs(&c)).unwrap_or_default();
                queue.extend(named.into_iter().map(|r| (r, id.clone())));
            }
            (Some(_), Some(lid), None) => {
                wanted_files.insert(format!("{lid}.p8.png"));
                gone.push(format!("#{id} (named by {from}): {id}.nfo says {lid}, but {lid}.p8.png is missing or empty"))
            }
            (Some(t), None, _) => gone.push(format!(
                "#{id} (named by {from}): {id}.nfo names no revision: {}",
                glimpse(t.as_bytes())
            )),
            (None, _, _) => gone.push(format!("#{id} (named by {from}): no {id}.nfo")),
        }
    }
    log(&format!(
        "report for {}: {} parts expected, {} kept, {} missing",
        name_of(cart),
        seen.len(),
        kept.len(),
        gone.len()
    ));
    for k in &kept {
        log(&format!("  kept {k}"));
    }
    for g in &gone {
        log(&format!("  MISSING {g}"));
    }
    let others: Vec<String> = std::fs::read_dir(&dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !wanted_files.contains(n))
                .collect()
        })
        .unwrap_or_default();
    let leftovers: Vec<&String> = others
        .iter()
        // Not PICO-8's own `temp-<id>.nfo`: it leaves one behind after every lookup.
        .filter(|n| n.ends_with(".part"))
        .collect();
    log(&format!(
        "  the cache holds {} other files{}",
        others.len(),
        if leftovers.is_empty() {
            String::new()
        } else {
            format!(", unfinished downloads among them: {leftovers:?}")
        }
    ));
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
    fn retry_after_is_read_in_seconds() {
        assert_eq!(
            retry_after("HTTP/1.1 429 Too Many\r\nRetry-After: 37\r\n"),
            Some(37)
        );
        assert_eq!(
            retry_after("retry-after: Wed, 21 Oct 2026 07:28:00 GMT\n"),
            None
        );
        assert_eq!(retry_after("HTTP/1.1 429\n"), None);
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
