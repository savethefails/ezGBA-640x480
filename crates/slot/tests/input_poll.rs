use std::path::Path;

use slot::input::{faster_poll, POLL_MS};

fn node(sys: &Path, name: &str, poll: Option<&str>) -> std::path::PathBuf {
    let dir = sys.join(name).join("device");
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(p) = poll {
        std::fs::write(dir.join("poll"), p).unwrap();
    }
    Path::new("/dev/input").join(name)
}

/// The SP scans its keys every 20 ms; asked, it scans every 10.
#[test]
fn a_slowly_scanned_node_is_scanned_faster() {
    let d = tempfile::tempdir().unwrap();
    let n = node(d.path(), "event1", Some("20\n"));
    let note = faster_poll(d.path(), &n).expect("nothing done");
    assert!(note.contains("was 20 ms"), "{note}");
    let now = std::fs::read_to_string(d.path().join("event1/device/poll")).unwrap();
    assert_eq!(now.trim(), POLL_MS.to_string());
}

/// A node already this fast, faster, or not scanned at all is left alone.
#[test]
fn other_nodes_are_left_alone() {
    let d = tempfile::tempdir().unwrap();
    for (name, poll) in [
        ("event2", Some("10")),
        ("event3", Some("5")),
        ("event4", None),
    ] {
        let n = node(d.path(), name, poll);
        assert_eq!(faster_poll(d.path(), &n), None, "{name}");
    }
    assert_eq!(
        std::fs::read_to_string(d.path().join("event3/device/poll")).unwrap(),
        "5"
    );
    assert!(!d.path().join("event4/device/poll").exists());
}
