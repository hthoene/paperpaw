//! Fixtures for the future Battery facade, built only from merged contracts.
//! These do not execute an interface-7 Battery plugin or qualify live dispatch.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]

#[path = "support/abi7.rs"]
mod fixture;

use paperpaw_core::native_host::declarations::validate_batch;
use paperpaw_sdk::native::events::{self, Event};

#[test]
fn borrowed_settings_match_the_existing_golden_without_registrar_conversion() {
    let mut framed = Vec::new();
    fixture::settings(|batch| {
        let records = paperpaw_core::native_host::registrations::records_decode(batch).unwrap();
        validate_batch("battery", &records).unwrap();
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(batch);
        Ok(())
    })
    .unwrap();
    assert_eq!(framed, include_bytes!("../testdata/declarations.bin.v1"));
}

#[test]
fn settings_submission_failure_reaches_construction() {
    let mut calls = 0;
    let result = fixture::settings(|_| {
        calls += 1;
        Err(-4)
    });
    assert_eq!(
        result,
        Err(paperpaw_sdk::native::declare::DeclareError::Sink(-4))
    );
    assert_eq!(calls, 1);
}

#[test]
fn lifecycle_fixtures_keep_all_five_edges_distinct() {
    for now_ms in [0, 60_000, u64::MAX] {
        let fixtures = fixture::lifecycle(now_ms).unwrap();
        assert_eq!(fixtures.len(), 5);
        for fixture in fixtures {
            let (name, now) = match events::decode(&fixture.bytes).unwrap() {
                Event::Started { now_ms } => ("started", now_ms),
                Event::Resume { now_ms } => ("resume", now_ms),
                Event::Suspend { now_ms } => ("suspend", now_ms),
                Event::Stopping { now_ms } => ("stopping", now_ms),
                Event::BatteryMeasured { now_ms } => ("battery-measured", now_ms),
                other => panic!("unexpected event: {other:?}"),
            };
            assert_eq!((name, now), (fixture.name, now_ms));
        }
    }
}

#[test]
fn page_turn_fixtures_preserve_unknown_and_known_counters() {
    for (page, pages) in [
        (None, None),
        (Some(1), None),
        (None, Some(300)),
        (Some(12), Some(300)),
    ] {
        let bytes = fixture::page_turn(60_000, page, pages).unwrap();
        let Event::PageTurned(event) = events::decode(&bytes).unwrap() else {
            panic!("expected a page-turn event");
        };
        assert_eq!((event.page, event.pages), (page, pages));
        assert_eq!(event.now_ms, 60_000);
        assert_eq!(event.path, "books/fixture.txt");
        let start = bytes.as_ptr() as usize;
        let path_start = event.path.as_ptr() as usize;
        assert!(path_start >= start && path_start + event.path.len() <= start + bytes.len());
    }
}
