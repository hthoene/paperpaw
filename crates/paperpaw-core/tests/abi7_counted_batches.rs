//! Counted input batches above the 8-record registration limit
//! (`docs/abi/stages-services.md` section 2.3, `ServiceOp::input_rows`):
//! `core.ui` op 16 takes up to 32 `ui_param` records in one batch. The SDK
//! encodes with `records_encode_counted`, the host decodes with
//! `host_records_visit`; nothing is ever cut.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]

use paperpaw_core::native_host::registrations::{
    REGISTRATION_MAX_RECORDS, Record, ServiceUiParam, ServiceUiTarget, host_records_visit,
    records_encode_counted, records_encode_counted_iter,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE};
use std::{cell::Cell, rc::Rc};

/// `core.ui@1` op 16: `input_rows` and `max_input` (pinned by
/// `tools/tests/test_abi_services.py`).
const HANDOVER_ROWS: usize = 32;
const HANDOVER_BYTES: usize = 2_048;

fn keys(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("k{index}")).collect()
}

fn params(keys: &[String]) -> Vec<Record<'_>> {
    keys.iter()
        .map(|key| Record::ServiceUiParam(ServiceUiParam { key, value: "v" }))
        .collect()
}

fn visit_all(bytes: &[u8], max: usize) -> (Result<usize, i32>, Vec<String>) {
    let mut seen = Vec::new();
    let result = host_records_visit(bytes, max, &mut |record| {
        let Record::ServiceUiParam(param) = record else {
            return Err(ERR_INVALID);
        };
        seen.push(param.key.to_owned());
        Ok(())
    });
    (result, seen)
}

#[test]
fn thirty_two_params_round_trip_in_one_batch_in_order() {
    const { assert!(HANDOVER_ROWS > REGISTRATION_MAX_RECORDS) };
    let keys = keys(HANDOVER_ROWS);
    let mut out = vec![0; HANDOVER_BYTES];
    let len =
        records_encode_counted(&params(&keys), HANDOVER_ROWS, HANDOVER_BYTES, &mut out).unwrap();
    let (result, seen) = visit_all(&out[..len], HANDOVER_ROWS);
    assert_eq!(result, Ok(HANDOVER_ROWS));
    assert_eq!(seen, keys);
}

#[test]
fn one_record_over_the_count_is_refused_on_both_sides_and_nothing_is_cut() {
    let keys = keys(HANDOVER_ROWS + 1);
    let mut out = vec![0xAA; HANDOVER_BYTES];
    assert_eq!(
        records_encode_counted(&params(&keys), HANDOVER_ROWS, HANDOVER_BYTES, &mut out),
        Err(ERR_NO_SPACE)
    );
    assert!(out.iter().all(|byte| *byte == 0xAA), "output untouched");
    // A plugin that frames 33 records itself: the host visits none of them.
    let len = records_encode_counted(&params(&keys), HANDOVER_ROWS + 1, HANDOVER_BYTES, &mut out)
        .unwrap();
    let (result, seen) = visit_all(&out[..len], HANDOVER_ROWS);
    assert_eq!(result, Err(ERR_NO_SPACE));
    assert!(seen.is_empty());
}

#[test]
fn byte_limit_and_output_capacity_are_checked_before_any_write() {
    let keys = keys(4);
    let records = params(&keys);
    let mut out = vec![0; HANDOVER_BYTES];
    let exact = records_encode_counted(&records, 4, HANDOVER_BYTES, &mut out).unwrap();
    assert_eq!(
        records_encode_counted(&records, 4, exact, &mut out),
        Ok(exact)
    );
    let mut short = vec![0x55; exact - 1];
    assert_eq!(
        records_encode_counted(&records, 4, exact - 1, &mut out),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(
        records_encode_counted(&records, 4, HANDOVER_BYTES, &mut short),
        Err(ERR_NO_SPACE)
    );
    assert!(short.iter().all(|byte| *byte == 0x55), "output untouched");
}

#[test]
fn a_bad_record_anywhere_visits_nothing() {
    let keys = keys(3);
    let mut records = params(&keys);
    // A host-sent row in a plugin's batch: refused before any visit.
    records.push(Record::ServiceUiTarget(ServiceUiTarget {
        target: 1,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    }));
    let mut out = vec![0; HANDOVER_BYTES];
    let len = records_encode_counted(&records, 8, HANDOVER_BYTES, &mut out).unwrap();
    let (result, seen) = visit_all(&out[..len], 8);
    assert_eq!(result, Err(ERR_INVALID));
    assert!(seen.is_empty());
    // Trailing bytes after the last record.
    let len = records_encode_counted(&params(&keys), 8, HANDOVER_BYTES, &mut out).unwrap();
    out[len] = 0;
    let (result, seen) = visit_all(&out[..=len], 8);
    assert_eq!(result, Err(ERR_INVALID));
    assert!(seen.is_empty());
}

#[test]
fn an_error_from_the_visitor_stops_the_walk() {
    let keys = keys(5);
    let mut out = vec![0; HANDOVER_BYTES];
    let len = records_encode_counted(&params(&keys), 8, HANDOVER_BYTES, &mut out).unwrap();
    let mut calls = 0;
    let result = host_records_visit(&out[..len], 8, &mut |_| {
        calls += 1;
        if calls == 2 { Err(-77) } else { Ok(()) }
    });
    assert_eq!((result, calls), (Err(-77), 2));
}

/// Params made on the fly from the keys, never collected into a slice.
fn lazy(keys: &[String]) -> impl Iterator<Item = Record<'_>> + Clone {
    keys.iter()
        .map(|key| Record::ServiceUiParam(ServiceUiParam { key, value: "v" }))
}

#[test]
fn the_iterator_encoder_writes_what_the_slice_encoder_writes() {
    for count in [0, 1, REGISTRATION_MAX_RECORDS + 1, HANDOVER_ROWS] {
        let keys = keys(count);
        let mut by_slice = vec![0; HANDOVER_BYTES];
        let mut by_iter = vec![0; HANDOVER_BYTES];
        let a =
            records_encode_counted(&params(&keys), HANDOVER_ROWS, HANDOVER_BYTES, &mut by_slice);
        let b =
            records_encode_counted_iter(lazy(&keys), HANDOVER_ROWS, HANDOVER_BYTES, &mut by_iter);
        assert_eq!(a, b);
        let len = b.unwrap();
        assert_eq!(by_slice[..len], by_iter[..len]);
        assert_eq!(visit_all(&by_iter[..len], HANDOVER_ROWS).1, keys);
    }
    // The same refusals, output untouched.
    let keys = keys(HANDOVER_ROWS + 1);
    let mut out = vec![0xAA; HANDOVER_BYTES];
    assert_eq!(
        records_encode_counted_iter(lazy(&keys), HANDOVER_ROWS, HANDOVER_BYTES, &mut out),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(
        records_encode_counted_iter(lazy(&keys[..4]), 4, 8, &mut out),
        Err(ERR_NO_SPACE)
    );
    let mut short = vec![0xAA; 8];
    assert_eq!(
        records_encode_counted_iter(lazy(&keys[..4]), 4, HANDOVER_BYTES, &mut short),
        Err(ERR_NO_SPACE)
    );
    assert!(
        out.iter().chain(&short).all(|byte| *byte == 0xAA),
        "output untouched"
    );
}

/// An iterator whose third walk (the writing one) yields other records.
fn unsteady<'a>(
    keys: &'a [String],
    later: &'a str,
) -> impl Iterator<Item = Record<'a>> + Clone + 'a {
    let made = Rc::new(Cell::new(0));
    let first_two = 2 * keys.len();
    keys.iter().map(move |key| {
        made.set(made.get() + 1);
        let value = if made.get() > first_two { later } else { "v" };
        Record::ServiceUiParam(ServiceUiParam { key, value })
    })
}

#[test]
fn an_iterator_that_changes_between_walks_is_refused_and_never_writes_past_out() {
    let keys = keys(4);
    for later in ["a longer value", ""] {
        let mut out = vec![0; HANDOVER_BYTES];
        assert_eq!(
            records_encode_counted_iter(unsteady(&keys, later), 8, HANDOVER_BYTES, &mut out),
            Err(ERR_INVALID),
            "{later:?}"
        );
    }
    // Exact capacity: a longer writing walk stops at the end of `out`.
    let mut exact = vec![0; HANDOVER_BYTES];
    let len = records_encode_counted(&params(&keys), 8, HANDOVER_BYTES, &mut exact).unwrap();
    let mut out = vec![0; len];
    assert_eq!(
        records_encode_counted_iter(unsteady(&keys, "a longer value"), 8, len, &mut out),
        Err(ERR_INVALID)
    );
}

#[test]
fn an_endless_iterator_is_refused_after_one_record_over_the_count() {
    let pulled = Rc::new(Cell::new(0_usize));
    let seen = Rc::clone(&pulled);
    let endless = core::iter::repeat_with(move || {
        seen.set(seen.get() + 1);
        Record::ServiceUiParam(ServiceUiParam {
            key: "k",
            value: "v",
        })
    });
    let mut out = vec![0xAA; HANDOVER_BYTES];
    assert_eq!(
        records_encode_counted_iter(endless, HANDOVER_ROWS, HANDOVER_BYTES, &mut out),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(
        pulled.get(),
        HANDOVER_ROWS + 1,
        "the count walk stops one past the cap"
    );
    assert!(out.iter().all(|byte| *byte == 0xAA), "output untouched");
}
