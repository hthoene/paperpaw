//! The recorder when the heap refuses: through the real native entry glue and
//! `NativeBattery`'s own token routing, each allocation the plugin makes in a
//! callback is refused in turn. An allocation that would abort (an infallible
//! `format!`, `String::from`, `push`) ends the whole test binary; a refused
//! one must leave the log whole lines and the next event working.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "invalid fixture setup fails tests"
)]
#[path = "support/fail_alloc.rs"]
mod fail_alloc;
#[path = "support/native_host.rs"]
mod host;

use paperpaw_core::native_host::{
    events::{
        encode_battery_measured, encode_resume, encode_started, encode_stopping, encode_suspend,
    },
    registrations::{Record, ServiceEnd, ServicePluginUsage},
};
use paperpaw_platform::Battery as Charge;
use paperpaw_plugin_battery::native_plugin::NativeBattery;

#[global_allocator]
static ALLOCATOR: fail_alloc::Failing = fail_alloc::Failing;

paperpaw_sdk::native_plugin!(
    NativeBattery,
    metadata = include_bytes!("../testdata/native-test-metadata.bin"),
    heap = none
);

/// The callback token of the lifecycle subscriptions (`declarations.rs`).
const RECORDER: u32 = 1;
type Encoder = fn(u64, &mut [u8]) -> Result<usize, i32>;

fn harness() -> host::Harness {
    let entry = unsafe {
        // SAFETY: the SDK export is repr(transparent) over PpEntry; this is
        // the entry pointer supplied to native loaders.
        &*std::ptr::from_ref(&PAPERPAW_ENTRY).cast::<paperpaw_native::host_glue::PpEntry>()
    };
    host::Harness::new(entry)
}

/// What the host reports at `at`: a battery, a clock and four plugins that
/// have used something (one listed twice, as a generation change does).
fn at(h: &mut host::Harness, at: u64) {
    *h.context.environment.borrow_mut() = host::environment(
        at,
        Some(Charge::new(80, false, Some(4012))),
        Some(1_791_974_460 + i64::try_from(at / 1000).unwrap()),
        true,
    );
    let mut services = h.context.services.borrow_mut();
    services.insert(
        "core.usage".into(),
        host::record(Record::ServiceUsage(host::usage(1, 384_000))),
    );
    let mut rows = Vec::new();
    let names = ["core", "battery", "fixture", "other", "fixture"];
    for plugin in names {
        rows.extend(host::record(Record::ServicePluginUsage(
            ServicePluginUsage {
                plugin,
                busy_us: 1_000,
                calls: 1,
                shown_ms: 0,
                fast: 0,
                partial: 0,
                full: 0,
                pixels: 0,
                read: 0,
                written: 0,
                syncs: 0,
                requests: 0,
                refused: 0,
                awake_ms: 0,
                awake_reason: "",
            },
        )));
    }
    rows.extend(host::record(Record::ServiceEnd(ServiceEnd {
        flags: 0,
        total: 5,
        generation: 1,
        next: 5,
    })));
    services.insert("core.plugin-usage/from.0".into(), rows);
}

fn send(h: &mut host::Harness, encode: Encoder, time: u64) -> i32 {
    at(h, time);
    let mut bytes = [0; 64];
    let len = encode(time, &mut bytes).unwrap();
    h.dispatch_token(RECORDER, &bytes[..len])
}

/// A prefix of events that must all succeed, then the event whose callback
/// is swept.
type Step = (Encoder, u64);
const SCENARIOS: &[(&[Step], Step)] = &[
    (&[], (encode_started, 0)),
    (&[(encode_started, 0)], (encode_battery_measured, 60_000)),
    (&[(encode_started, 0)], (encode_suspend, 70_000)),
    (
        &[(encode_started, 0), (encode_suspend, 70_000)],
        (encode_resume, 80_000),
    ),
    (
        &[(encode_started, 0), (encode_battery_measured, 60_000)],
        (encode_stopping, 700_000),
    ),
];

/// Every line of the log is whole, and every usage row has all its fields.
fn assert_whole(h: &host::Harness, context: &str) {
    for (name, fields) in [("log.csv", 13), ("plugins.csv", 11)] {
        let Some(text) = h.context.files.borrow().get(name).cloned() else {
            continue;
        };
        let text = String::from_utf8(text).unwrap();
        assert!(text.is_empty() || text.ends_with('\n'), "{name}: {context}");
        for line in text.lines() {
            assert_eq!(
                line.split(',').count(),
                fields,
                "{name} {line:?}: {context}"
            );
        }
    }
}

#[test]
fn every_refused_allocation_of_a_recorder_event_is_survived() {
    let mut total = 0;
    for (index, (before, (encode, time))) in SCENARIOS.iter().enumerate() {
        let mut refused = 1;
        loop {
            let mut h = harness();
            for (encode, time) in *before {
                assert_eq!(send(&mut h, *encode, *time), 0);
            }
            at(&mut h, *time);
            let mut bytes = [0; 64];
            let len = encode(*time, &mut bytes).unwrap();
            let (status, made) =
                fail_alloc::armed(refused, || h.dispatch_token(RECORDER, &bytes[..len]));
            if made < refused {
                // Every allocation of the event has been refused in turn.
                assert_eq!(status, 0, "scenario {index}: unrefused");
                break;
            }
            let context = format!("scenario {index}, allocation {refused} of the event refused");
            assert!(status <= 0, "{context}: status {status}");
            assert_whole(&h, &context);
            // The next event of the same time awake still works.
            assert_eq!(
                send(&mut h, encode_battery_measured, time + 1_000),
                0,
                "{context}"
            );
            assert_whole(&h, &context);
            h.context.assert_blobs_released();
            refused += 1;
            total += 1;
        }
    }
    // Something was swept: the plugin does allocate in a callback.
    assert!(total > 0);
}
