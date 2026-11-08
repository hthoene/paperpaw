//! The scripted `core.registry`: pages with a cursor and a generation,
//! single ops, and the sweep that proves nothing is held after a refusal.
#![allow(clippy::unwrap_used, reason = "fixture failures fail the test")]
mod common;
use common::{call, entry};
use paperpaw_core::native_host::registrations::{
    RawRecord, Record, ServiceEnd, ServiceRegChoice, ServiceRegQuery, records_encode,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_SPACE, ERR_STATE};
use paperpaw_scripted_host::{ScriptedHost, Slot, row, sweep_released};

fn query(cursor: u32, generation: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 256];
    let len = records_encode(
        &[Record::ServiceRegQuery(ServiceRegQuery {
            scope: "",
            cursor,
            generation,
            flags: 0,
        })],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(len);
    call("core.registry", 3, &bytes)
}

fn rows(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|index| {
            let value = format!("v{index}");
            row(&Record::ServiceRegChoice(ServiceRegChoice {
                value: &value,
                title: "t",
            }))
            .unwrap()
        })
        .collect()
}

/// The end record of a reply stream (the last frame).
fn end_of(reply: &[u8]) -> ServiceEnd {
    let mut at = 0;
    let mut last = None;
    while at < reply.len() {
        let (raw, used) = RawRecord::prefix(&reply[at..]).unwrap();
        last = Some(raw);
        at += used;
    }
    last.unwrap().decode::<ServiceEnd>().unwrap()
}

#[test]
fn a_list_is_paged_by_cursor_with_a_stable_generation() {
    let host = ScriptedHost::new();
    let registry = host.registry();
    registry.set_rows(3, rows(5));
    registry.page_limits(2, 4096);
    let plugin = host.load(&entry()).unwrap();
    let first = plugin.event(0, &query(0, 0), 4096).unwrap();
    let end = end_of(&first.out);
    assert_eq!(
        (end.total, end.next, end.generation, end.flags),
        (5, 2, 1, 0)
    );
    let second = plugin.event(0, &query(2, 1), 4096).unwrap();
    assert_eq!(end_of(&second.out).next, 4);
    let last = plugin.event(0, &query(4, 1), 4096).unwrap();
    let end = end_of(&last.out);
    assert_eq!((end.next, end.flags), (5, 1));
    assert_eq!(registry.pages(), 3);
    // A cursor past the end is invalid; a cursor without a generation is a state error.
    assert_eq!(
        plugin.event(0, &query(6, 1), 4096).unwrap().status,
        ERR_INVALID
    );
    assert_eq!(
        plugin.event(0, &query(2, 0), 4096).unwrap().status,
        ERR_STATE
    );
}

#[test]
fn a_generation_change_between_pages_is_a_state_error() {
    let host = ScriptedHost::new();
    let registry = host.registry();
    registry.set_rows(3, rows(5));
    registry.page_limits(2, 4096);
    let plugin = host.load(&entry()).unwrap();
    assert!(plugin.event(0, &query(0, 0), 4096).unwrap().status > 0);
    registry.set_generation(2);
    assert_eq!(
        plugin.event(0, &query(2, 1), 4096).unwrap().status,
        ERR_STATE
    );
    // Starting over reads the new generation.
    let again = plugin.event(0, &query(0, 0), 4096).unwrap();
    assert_eq!(end_of(&again.out).generation, 2);
}

#[test]
fn rows_stay_whole_and_a_row_that_never_fits_is_no_space() {
    let host = ScriptedHost::new();
    let registry = host.registry();
    registry.set_rows(3, rows(5));
    let plugin = host.load(&entry()).unwrap();
    // Room for the end record and two rows only: pages of two, rows never cut.
    let one_row = rows(1)[0].len();
    let end_len = end_of_len();
    registry.page_limits(32, 2 * one_row + end_len);
    let first = plugin.event(0, &query(0, 0), 4096).unwrap();
    assert_eq!(end_of(&first.out).next, 2);
    registry.page_limits(32, end_len);
    assert_eq!(
        plugin.event(0, &query(0, 0), 4096).unwrap().status,
        ERR_NO_SPACE
    );
}

fn end_of_len() -> usize {
    row(&Record::ServiceEnd(ServiceEnd {
        flags: 0,
        total: 5,
        generation: 1,
        next: 2,
        cursor: None,
    }))
    .unwrap()
    .len()
}

#[test]
fn single_ops_answer_from_a_closure_and_the_sweep_finds_nothing_held() {
    let runs = sweep_released(Slot::ServiceCall, ERR_BUSY, |host, refused| {
        let registry = host.registry();
        registry.on(2, |_| Ok(b"value".to_vec()));
        let plugin = host.load(&entry()).unwrap();
        let status = plugin
            .event(0, &call("core.registry", 2, b""), 16)
            .unwrap()
            .status;
        assert_eq!(status, if refused.is_some() { ERR_BUSY } else { 5 });
        status
    });
    // One clean run and one per service call the plugin made.
    assert_eq!(runs.len(), 2);
}

#[test]
#[should_panic(expected = "still held")]
fn the_sweep_fails_a_path_that_leaves_a_handle_open() {
    sweep_released(Slot::ServiceCall, ERR_BUSY, |host, _| {
        let registry = host.registry();
        registry.on(2, |_| Ok(Vec::new()));
        let plugin = host.load(&entry()).unwrap();
        let _ = plugin.event(0, &call("core.registry", 2, b""), 16);
        // The plugin is never torn down: its service handle stays open.
        std::mem::forget(plugin);
    });
}
