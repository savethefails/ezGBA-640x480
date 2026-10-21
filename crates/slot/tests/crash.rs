use std::os::unix::process::ExitStatusExt;
use std::process::Command;

const CHILD: &str = "SLOT_CRASH_CHILD";
const LOG: &str = "SLOT_CRASH_LOG";

#[test]
fn a_crash_says_where_it_happened_and_still_dies_by_its_signal() {
    if std::env::var_os(CHILD).is_some() {
        let log = std::env::var_os(LOG).map(std::path::PathBuf::from);
        slot::crash::install(log.as_deref());
        std::thread::Builder::new()
            .name("slot-emu".into())
            .spawn(|| unsafe { std::ptr::read_volatile(8 as *const u8) })
            .unwrap()
            .join()
            .ok();
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let card_log = d.path().join("slot-crash.log");
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "a_crash_says_where_it_happened_and_still_dies_by_its_signal",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(LOG, &card_log)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let line = log
        .lines()
        .find(|l| l.starts_with("slot: crash: "))
        .unwrap_or_else(|| panic!("no crash line in the log:\n{log}"));
    assert!(
        line.contains("SIGSEGV") || line.contains("SIGBUS"),
        "{line}"
    );
    assert!(line.contains("thread slot-emu"), "{line}");
    assert!(line.contains("addr 0x8 "), "{line}");
    let exe = std::env::current_exe().unwrap();
    let name = exe.file_name().unwrap().to_string_lossy();
    assert!(
        line.contains(&*name),
        "the crash does not name the code it was in: {line}"
    );
    assert!(
        matches!(out.status.signal(), Some(11) | Some(10)),
        "the process did not die by its signal, so no core is kept: {:?}",
        out.status
    );
    // ezGBA's stderr goes to /tmp, which a reboot clears: the same line has to be on the card.
    let kept = std::fs::read_to_string(&card_log).expect("nothing was written to the card log");
    assert_eq!(
        kept.trim_end(),
        line,
        "the card log does not hold the crash line"
    );
}
