#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::native::declare::records_wire::{self as wire, Record};
use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::ptr;
use paperpaw_native::host_glue as abi;
#[path = "../../../tests/support/usage_host.rs"]
mod fixture;
use fixture::UsageHost;

#[test]
fn deterministic_battery_fixture_reads_scalars_names_notes_and_changing_pages() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    let usage = device_usage(&host).unwrap();
    assert_eq!(
        (
            usage.fast,
            usage.partial,
            usage.full,
            usage.pixels,
            usage.inputs,
            usage.sleeps,
            usage.panel_ms,
            usage.bluetooth_ms
        ),
        (2, 3, 4, 12500, 6, 1, 700, 900)
    );
    let mut name = String::new();
    plugin_name(&host, "battery", |s| name = s.into()).unwrap();
    assert_eq!(name, "Battery");
    work_note(&host, "battery", |at, text| {
        assert_eq!((at, text), (1234, "Sample saved"))
    })
    .unwrap();
    let mut rows = Vec::new();
    for_each_plugin_usage(&host, &mut |r| {
        rows.push((
            r.plugin.to_string(),
            r.calls,
            r.busy_us,
            r.read,
            r.written,
            r.syncs,
        ))
    })
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("core".into(), 1, 1000, 2048, 4096, 2),
            ("battery".into(), 2, 2000, 2048, 4096, 2)
        ]
    );
    assert_eq!(state.releases.get(), 5);
    assert_eq!(state.active.get(), 0);
    assert_eq!(crate::native::clock::micros(&host), Ok(123_456));
}

#[test]
fn arbitrary_short_blob_reads_are_coalesced_for_scalar_and_table_records() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    for n in 1..=fixture::PAGE0.len() {
        state.max_read.set(n);
        assert_eq!(device_usage(&host).unwrap().pixels, 12500);
        let mut rows = 0;
        let end = plugin_usage_page(&host, 0, &mut |_| rows += 1).unwrap();
        assert_eq!(
            (rows, end.next, end.total, end.generation, end.truncated),
            (1, 1, 2, 7, true)
        );
    }
}

#[test]
fn blobs_release_exactly_once_on_drop_explicit_failure_and_read_error() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    let blob = query(&host, "core.usage", 1, 0).unwrap();
    assert!(matches!(query(&host, "core.usage", 1, 0), Err(Error::Busy)));
    assert_eq!(blob.read(u64::MAX, &mut [0; 1]), Ok(0));
    assert_eq!(blob.read(0, &mut []), Ok(0));
    assert_eq!(blob.read(0, &mut [0; 4097]), Err(Error::Invalid));
    state.invalid_count.set(true);
    assert_eq!(blob.read(0, &mut [0; 4]), Err(Error::Invalid));
    state.invalid_count.set(false);
    drop(blob);
    assert_eq!(state.releases.get(), 1);
    let blob = query(&host, "core.usage", 1, 0).unwrap();
    state.release_error.set(abi::ERR_STATE);
    assert_eq!(blob.release(), Err(Error::State));
    assert_eq!(state.releases.get(), 2);
    state.release_error.set(abi::OK);
    state.read_error.set(abi::ERR_IO);
    assert_eq!(device_usage(&host), Err(Error::Io));
    assert_eq!(state.releases.get(), 3);
    state.read_error.set(abi::ERR_STATE);
    assert_eq!(device_usage(&host), Err(Error::State));
    assert_eq!(state.releases.get(), 4);
}

#[test]
fn invalid_service_keys_are_rejected_before_host_metadata_and_versions() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    state.query_error.set(abi::ERR_UNSUPPORTED);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    for key in [
        "",
        "core",
        "Core.usage",
        "core..usage",
        "core.usage/x",
        "core.plugin",
        "core.work-note",
        "core.plugin-usage/from.01",
        "core.plugin-usage/from.-1",
        "core.plugin-usage/from.4294967296",
        "core.plugin-usage/from.",
        "core.plugin/a/b",
        "core.plugin/ü",
    ] {
        assert!(
            matches!(query(&host, key, 9, 9), Err(Error::Invalid)),
            "{key}"
        );
    }
    assert!(matches!(
        query(&host, &"a".repeat(129), 9, 9),
        Err(Error::Invalid)
    ));
    assert!(state.queries.borrow().is_empty());
    assert!(matches!(
        query(&host, "core.usage", 9, 9),
        Err(Error::Unsupported)
    ));
    assert_eq!(state.queries.borrow().len(), 1);
    assert_eq!(state.releases.get(), 0);
}

fn page(records: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    let mut len = 0;
    for r in records {
        len += wire::record_encode_one(r, &mut bytes[len..]).unwrap();
    }
    bytes.truncate(len);
    bytes
}
fn end(flags: u32, total: u32, generation: u32, next: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags,
        total,
        generation,
        next,
    })
}

#[test]
fn table_grammar_errors_missing_repeated_and_nonterminal_end_are_refused() {
    for data in [
        vec![],
        fixture::USAGE.into(),
        page(&[end(0, 0, 0, 0), end(0, 0, 0, 0)]),
        {
            let mut v = fixture::EMPTY.to_vec();
            v.push(0);
            v
        },
        page(&[end(1, 2, 0, 0)]),
        page(&[end(0, 2, 0, 1)]),
        page(&[end(0, 1, 0, 2)]),
    ] {
        let table = fixture::table();
        let mut state = UsageHost::default();
        state.responses[1].1 = data;
        let host = Host {
            table: &table,
            ctx: ptr::from_mut(&mut state).cast(),
        };
        assert_eq!(
            plugin_usage_page(&host, 0, &mut |_| {}),
            Err(Error::Invalid)
        );
        assert_eq!(state.releases.get(), 1);
    }
}

#[test]
fn empty_table_left_out_rows_and_unknown_flags_are_supported() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    state.responses[1].1 = fixture::EMPTY.into();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    for_each_plugin_usage(&host, &mut |_| panic!("empty table")).unwrap();
    state.responses[1].1 = page(&[end(2 | 128, 1, 3, 1)]);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    let end = plugin_usage_page(&host, 0, &mut |_| panic!("left-out row")).unwrap();
    assert_eq!(
        (end.next, end.total, end.left_out, end.truncated),
        (1, 1, true, false)
    );
}

#[test]
fn absent_notes_bad_scalar_tags_and_failed_release_propagate() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    assert_eq!(
        work_note(&host, "missing", |_, _| panic!()),
        Err(Error::NotFound)
    );
    state.release_error.set(abi::ERR_STATE);
    assert_eq!(device_usage(&host), Err(Error::State));
    state.release_error.set(abi::OK);
    state.responses[0].1 = fixture::NAME.into();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    assert_eq!(device_usage(&host), Err(Error::Invalid));
}

#[test]
fn rows_cross_read_buffer_boundaries_without_a_table_allocation() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    let (row, _) = wire::record_decode_prefix(fixture::PAGE0).unwrap();
    state.responses[1].1 = page(&[row, row, row, row, row, end(0, 5, 7, 5)]);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    for chunk in [1, 13, 209, 384] {
        state.max_read.set(chunk);
        let mut rows = 0;
        let end = plugin_usage_page(&host, 0, &mut |r| {
            assert_eq!(r.plugin, "core");
            rows += 1;
        })
        .unwrap();
        assert_eq!((rows, end.next), (5, 5));
    }
}

#[test]
fn changing_table_is_bounded_by_first_total_plus_one_without_restart() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    state.responses[1].1 = page(&[end(3, 2, 7, 1)]);
    state.responses[2].1 = page(&[end(3, 100, 8, 2)]);
    state.responses.push((
        "core.plugin-usage/from.2".into(),
        page(&[end(3, 100, 9, 3)]),
    ));
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    assert_eq!(
        for_each_plugin_usage(&host, &mut |_| {}),
        Err(Error::Invalid)
    );
    assert_eq!(
        &*state.queries.borrow(),
        &[
            "core.plugin-usage/from.0",
            "core.plugin-usage/from.1",
            "core.plugin-usage/from.2"
        ]
    );
    assert_eq!(state.releases.get(), 3);
}

#[test]
fn longest_note_can_arrive_one_byte_at_a_time_and_scalars_preserve_wide_values() {
    let table = fixture::table();
    let mut state = UsageHost::default();
    let note = "é".repeat(128);
    state.responses[4].1 = page(&[Record::ServiceWorkNote(wire::ServiceWorkNote {
        at_ms: u64::MAX,
        text: &note,
    })]);
    let usage = wire::ServiceUsage {
        fast: u32::MAX,
        partial: u32::MAX,
        full: u32::MAX,
        pixels: u64::MAX,
        inputs: u32::MAX,
        sleeps: u32::MAX,
        panel_ms: u64::MAX,
        bluetooth_ms: u64::MAX,
    };
    state.responses[0].1 = page(&[Record::ServiceUsage(usage)]);
    state.max_read.set(1);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut state).cast(),
    };
    work_note(&host, "battery", |at, text| {
        assert_eq!((at, text), (u64::MAX, note.as_str()))
    })
    .unwrap();
    let got = device_usage(&host).unwrap();
    assert_eq!(
        (
            got.fast,
            got.partial,
            got.full,
            got.pixels,
            got.inputs,
            got.sleeps,
            got.panel_ms,
            got.bluetooth_ms
        ),
        (
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u64::MAX,
            u32::MAX,
            u32::MAX,
            u64::MAX,
            u64::MAX
        )
    );
}
