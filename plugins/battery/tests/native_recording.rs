//! Recorder behavior through the real native entry glue and a test-only host.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "invalid fixture setup fails tests"
)]
#[path = "support/native_host.rs"]
mod host;
use paperpaw_plugin_battery::recording::Recorder;
use paperpaw_sdk::native::{Error, Host, Plugin, events::Event, reply::Reply};
#[derive(Default)]
struct Probe(Recorder);
impl Plugin for Probe {
    fn make(_: &Host<'_>) -> Result<Self, Error> {
        Ok(Self::default())
    }
    fn event(
        &mut self,
        host: &Host<'_>,
        _: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.0.event(host, event)
    }
}
fn harness() -> host::Harness {
    // SAFETY: the SDK export is repr(transparent) over PpEntry; this is the entry pointer supplied to native loaders.
    let entry = unsafe {
        &*std::ptr::from_ref(&PAPERPAW_ENTRY).cast::<paperpaw_native::host_glue::PpEntry>()
    };
    host::Harness::new(entry)
}
// Existing generated SDK-entry metadata is adequate for this in-process probe.
paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../testdata/native-test-metadata.bin"),
    heap = none
);
fn event(h: &mut host::Harness, encode: fn(u64, &mut [u8]) -> Result<usize, i32>, at: u64) {
    let mut bytes = [0; 64];
    let len = encode(at, &mut bytes).unwrap();
    assert_eq!(h.dispatch(&bytes[..len]), 0);
}
use paperpaw_core::native_host::events::{encode_started, encode_suspend};
#[test]
fn native_lifecycle_preserves_duplicate_suspend_and_releases_blobs() {
    let mut h = harness();
    event(&mut h, encode_started, 0);
    *h.context.environment.borrow_mut() = host::environment(60_000, None, None, true);
    event(&mut h, encode_suspend, 60_000);
    event(&mut h, encode_suspend, 60_000);
    let log = h.context.text(host::LOG);
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("start,"))
            .count(),
        1
    );
    assert_eq!(
        log.lines().filter(|line| line.starts_with("end,")).count(),
        2
    );
    // Start appends without syncing; each suspend flushes, as in interface 6.
    assert_eq!(h.context.syncs.get(), 2);
    h.context.assert_blobs_released();
}
#[test]
fn disabled_recorder_writes_nothing() {
    let mut h = harness();
    *h.context.environment.borrow_mut() = host::environment(0, None, None, false);
    event(&mut h, encode_started, 0);
    assert!(h.context.files.borrow().is_empty());
    assert_eq!(h.context.syncs.get(), 0);
}
#[test]
fn torn_tail_is_terminated_before_native_start() {
    let mut h = harness();
    h.context.seed(host::LOG, b"sample,60000,40");
    event(&mut h, encode_started, 0);
    assert!(
        h.context
            .text(host::LOG)
            .starts_with("sample,60000,40\nstart,")
    );
}
#[test]
fn full_log_rotates_before_native_start() {
    let mut h = harness();
    h.context.seed(host::LOG, vec![b'x'; 128 * 1024]);
    event(&mut h, encode_started, 0);
    assert_eq!(
        h.context.files.borrow().get(host::OLD).unwrap().len(),
        128 * 1024
    );
    assert!(
        h.context
            .text(host::LOG)
            .lines()
            .any(|line| line.starts_with("start,"))
    );
}
