//! The plugin made and driven by the scripted host: its subscription to the end
//! of a request is not submitted yet (a request to `core.net` is refused without
//! it), and an end
//! that is not of a request of its own (none is in flight) changes nothing
//! and asks the network for nothing. The calls that ask DeepL and keep the
//! key are run when the panel starts them (slice 3, `tests/acceptance_pending.rs`).
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{EventHttpDone, Record};
use paperpaw_plugin_translate::{
    NATIVE_ENTRY,
    declarations::{self, HTTP_TOKEN},
};
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use paperpaw_sdk::native::declare::Declarations;

fn done(token: u32, id: u32, status: u32, reason: u32) -> Vec<u8> {
    event_batch(&Record::EventHttpDone(EventHttpDone {
        now_ms: 0,
        id,
        token,
        status,
        reason,
        flags: 0,
        body_len: 0,
    }))
    .unwrap()
}

/// The subscription's batch, as the builders make it.
fn image_events() -> Vec<u8> {
    let mut out = Vec::new();
    let mut d = Declarations::new(|batch: &[u8]| -> Result<(), i32> {
        out.extend_from_slice(batch);
        Ok(())
    });
    declarations::events(&mut d).unwrap();
    drop(d);
    out
}

fn run(host: &ScriptedHost, batch: &[u8]) -> i32 {
    let plugin = match host.load(NATIVE_ENTRY.entry()) {
        Ok(plugin) => plugin,
        Err(status) => return status,
    };
    plugin
        .event(HTTP_TOKEN, batch)
        .map_or_else(|status| status, |done| done.status)
}

/// The subscription is in the image but not submitted: the kernel's host refuses
/// event declarations (#944) and a failed `make` keeps the plugin out. When this
/// fails because `make` submits `image::EVENTS`, say so in `PORTING-7.md`.
#[test]
fn the_end_of_a_request_is_not_subscribed_to_until_the_host_admits_it() {
    let host = ScriptedHost::new();
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    drop(plugin);
    let subscription = host
        .registrations()
        .into_iter()
        .any(|batch| batch.as_slice() == image_events());
    assert!(!subscription);
    assert!(!host.registrations().is_empty(), "the screen is submitted");
}

#[test]
fn an_end_nothing_waits_for_changes_nothing_and_asks_no_service() {
    for (token, id, status, reason) in [
        (1, 1, 200, 0),
        (1, 9, 0, 4),
        (7, 1, 456, 0),
        (1, 1, 0, 8),
        (1, 3, u32::MAX, u32::MAX),
    ] {
        let host = ScriptedHost::new();
        assert_eq!(run(&host, &done(token, id, status, reason)), 0);
        assert_eq!(host.calls(Slot::ServiceOpen), 0);
        assert_eq!(host.calls(Slot::ServiceCall), 0);
        assert_eq!(host.open_handles(), 0);
    }
}

#[test]
fn a_body_that_is_no_record_or_a_request_number_of_none_is_an_error_not_a_panic() {
    let host = ScriptedHost::new();
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let status = |batch: &[u8]| {
        plugin
            .event(HTTP_TOKEN, batch)
            .map_or_else(|s| s, |c| c.status)
    };
    assert!(status(&[1, 2, 3]) < 0);
    assert!(status(&[]) < 0);
    // The host never numbers a request 0.
    assert!(status(&done(1, 0, 200, 0)) < 0);
}

#[test]
fn no_refusal_makes_it_abort() {
    let batch = done(1, 1, 200, 0);
    let results = sweep(Slot::Registrations, -5, |host, _| run(host, &batch));
    assert_eq!(results[0].1, 0);
    assert!(results.iter().skip(1).all(|(_, status)| *status < 0));
}
