/// Without the file on the card the trace is off: nothing is created and every call is a no-op.
#[test]
fn without_the_file_nothing_is_traced() {
    let d = tempfile::tempdir().unwrap();
    slot::latency::start(d.path(), 1);
    slot::latency::read();
    slot::latency::pad(0, 1);
    slot::latency::shown(1);
    assert!(!d.path().join(slot::latency::TRACE_FILE).exists());
}
