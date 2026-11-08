#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct State {
    op: Cell<u32>,
    calls: Cell<u32>,
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    error: Cell<i32>,
}
unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: the SDK supplies a live key and aligned output.
    unsafe {
        assert_eq!(core::slice::from_raw_parts(key.ptr, key.len), b"core.work");
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the fixture state and SDK buffers stay live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.op.set(op);
    state.calls.set(state.calls.get() + 1);
    // SAFETY: the SDK encoded a live input slice.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let reply = state.reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: the fixture reply and the exclusive SDK output are disjoint.
    unsafe { core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len()) };
    i32::try_from(reply.len()).unwrap()
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.service_open = open;
    table.service_call = call;
    table
}
fn reply(state: &State, rows: &[Record<'_>]) {
    let mut bytes = vec![0; 4096];
    let mut used = 0;
    for row in rows {
        used += wire::record_encode_one(row, &mut bytes[used..]).unwrap();
    }
    bytes.truncate(used);
    state.reply.replace(bytes);
}
fn row(key: &'static str, token: u32, state: u32, reason: u32, due_ms: u64) -> Record<'static> {
    Record::ServiceWorkRow(wire::ServiceWorkRow {
        token,
        state,
        reason,
        due_ms,
        key,
    })
}
fn end(total: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags: 0,
        total,
        generation: 0,
        next: 0,
        cursor: None,
    })
}

/// Runs `f` with an open service against a scripted host.
fn with_work<R>(f: impl FnOnce(&Work, &Host<'_>, &State) -> R) -> R {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let work = Work::open(&host).unwrap();
    f(&work, &host, &state)
}

#[test]
fn declare_encodes_every_field_of_the_spec_and_accepts_the_empty_reply() {
    with_work(|work, host, state| {
        let spec = WorkSpec::new("kosync.push", 41)
            .unwrap()
            .network()
            .after_ms(1_000)
            .within_ms(1_800_000)
            .every_ms(3_600_000)
            .at_moment(Moment::Online)
            .at_moment(Moment::Charging);
        let (mut input, mut out) = ([0; 256], [0; 64]);
        work.declare(host, &spec, &mut input, &mut out).unwrap();
        assert_eq!(state.op.get(), 1);
        let bytes = state.input.borrow().clone();
        let batch = wire::records_decode(&bytes).unwrap();
        assert_eq!(batch.len(), 1);
        let Some(Record::ServiceWorkSpec(sent)) = batch.iter().next() else {
            panic!("work spec");
        };
        assert_eq!(
            (
                sent.key,
                sent.token,
                sent.needs,
                sent.flags,
                sent.moments,
                sent.after_ms,
                sent.within_ms,
                sent.every_ms,
                sent.at_ms
            ),
            (
                "kosync.push",
                41,
                1,
                0,
                0b110,
                1_000,
                1_800_000,
                3_600_000,
                0
            )
        );
    });
}

#[test]
fn helpers_set_what_they_say() {
    with_work(|work, host, state| {
        let (mut input, mut out) = ([0; 256], [0; 64]);
        let cheap = WorkSpec::when_cheap("a", 1).unwrap();
        work.declare(host, &cheap, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow().clone();
        let batch = wire::records_decode(&bytes).unwrap();
        let Some(Record::ServiceWorkSpec(sent)) = batch.iter().next() else {
            panic!("work spec");
        };
        assert_eq!((sent.needs, sent.flags), (1, 1));
        let timer = WorkSpec::at("b", 2, 123_456).unwrap();
        work.declare(host, &timer, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow().clone();
        let batch = wire::records_decode(&bytes).unwrap();
        let Some(Record::ServiceWorkSpec(sent)) = batch.iter().next() else {
            panic!("work spec");
        };
        assert_eq!(
            (sent.at_ms, sent.needs, sent.flags, sent.after_ms),
            (123_456, 0, 0, 0)
        );
        let retry = WorkSpec::new("c", 3).unwrap().retry(2, 0);
        work.declare(host, &retry, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow().clone();
        let batch = wire::records_decode(&bytes).unwrap();
        let Some(Record::ServiceWorkSpec(sent)) = batch.iter().next() else {
            panic!("work spec");
        };
        assert_eq!(sent.after_ms, 60_000);
    });
}

#[test]
fn bad_keys_and_specs_are_refused_before_any_host_call() {
    let long = "k".repeat(KEY_MAX_BYTES + 1);
    for key in ["", long.as_str(), "a\nb", "a\u{7f}b", "tab\t"] {
        assert_eq!(
            WorkSpec::new(key, 1).unwrap_err(),
            Error::Invalid,
            "{key:?}"
        );
        assert_eq!(WorkSpec::at(key, 1, 5).unwrap_err(), Error::Invalid);
        assert_eq!(WorkSpec::when_cheap(key, 1).unwrap_err(), Error::Invalid);
    }
    assert!(WorkSpec::new(&"k".repeat(KEY_MAX_BYTES), 1).is_ok());
    // A multibyte key counts bytes, not characters.
    assert!(WorkSpec::new(&"é".repeat(KEY_MAX_BYTES / 2), 1).is_ok());
    assert!(WorkSpec::new(&"é".repeat(KEY_MAX_BYTES / 2 + 1), 1).is_err());
    let base = || WorkSpec::new("k", 1).unwrap();
    let bad = [
        WorkSpec::at("k", 1, 9).unwrap().after_ms(1),
        WorkSpec::at("k", 1, 9).unwrap().within_ms(1),
        WorkSpec::at("k", 1, 9)
            .unwrap()
            .every_ms(60_000)
            .at_moment(Moment::Wake),
        WorkSpec::at("k", 1, 9).unwrap().optional(),
        base().after_ms(SPAN_MAX_MS + 1),
        base().within_ms(SPAN_MAX_MS + 1),
        base().every_ms(SPAN_MAX_MS + 1),
        base().every_ms(EVERY_MIN_MS - 1),
        base().every_ms(1),
    ];
    with_work(|work, host, state| {
        let (mut input, mut out) = ([0; 256], [0; 64]);
        for spec in bad {
            assert_eq!(spec.check(), Err(Error::Invalid), "{spec:?}");
            assert_eq!(
                work.declare(host, &spec, &mut input, &mut out),
                Err(Error::Invalid)
            );
        }
        assert_eq!(
            work.cancel(host, "", &mut input, &mut out),
            Err(Error::Invalid)
        );
        assert_eq!(state.calls.get(), 0, "nothing reached the host");
    });
    // The edges are allowed, and an exact timer may repeat every minute or less often.
    for ok in [
        base()
            .after_ms(SPAN_MAX_MS)
            .within_ms(SPAN_MAX_MS)
            .every_ms(SPAN_MAX_MS),
        base().every_ms(EVERY_MIN_MS),
        base().every_ms(0),
        WorkSpec::at("k", 1, 9).unwrap().every_ms(EVERY_MIN_MS),
        WorkSpec::at("k", 1, 0).unwrap(),
    ] {
        assert_eq!(ok.check(), Ok(()), "{ok:?}");
    }
}

#[test]
fn cancel_names_a_key_and_the_empty_key_cancels_all() {
    with_work(|work, host, state| {
        let (mut input, mut out) = ([0; 256], [0; 64]);
        work.cancel(host, "joplin.send", &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op.get(), 2);
        let bytes = state.input.borrow().clone();
        let batch = wire::records_decode(&bytes).unwrap();
        let Some(Record::ServiceWorkKey(sent)) = batch.iter().next() else {
            panic!("work key");
        };
        assert_eq!(sent.key, "joplin.send");
        work.cancel_all(host, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow().clone();
        let batch = wire::records_decode(&bytes).unwrap();
        let Some(Record::ServiceWorkKey(sent)) = batch.iter().next() else {
            panic!("work key");
        };
        assert_eq!(sent.key, "");
        assert_eq!(state.calls.get(), 2);
    });
}

#[test]
fn host_refusals_come_back_as_their_errors_and_a_full_reply_is_refused() {
    with_work(|work, host, state| {
        let (mut input, mut out) = ([0; 256], [0; 64]);
        let spec = WorkSpec::new("a", 1).unwrap().network();
        for (status, error) in [
            (abi::ERR_DENIED, Error::Denied),
            (abi::ERR_FULL, Error::Full),
            (abi::ERR_NO_MEMORY, Error::NoMemory),
            (abi::ERR_UNSUPPORTED, Error::Unsupported),
            (abi::ERR_STATE, Error::State),
            (abi::ERR_INVALID, Error::Invalid),
        ] {
            state.error.set(status);
            assert_eq!(work.declare(host, &spec, &mut input, &mut out), Err(error));
            assert_eq!(work.cancel(host, "a", &mut input, &mut out), Err(error));
        }
        state.error.set(0);
        // A reply to a mutation is empty.
        reply(state, &[end(0)]);
        assert_eq!(
            work.declare(host, &spec, &mut input, &mut out),
            Err(Error::Invalid)
        );
        // An input buffer that cannot hold the record.
        state.reply.replace(Vec::new());
        assert_eq!(
            work.declare(host, &spec, &mut [0; 4], &mut out),
            Err(Error::NoSpace)
        );
    });
}

#[test]
fn state_reads_every_row_and_the_unpaged_end() {
    with_work(|work, host, state| {
        reply(
            state,
            &[
                row("a", 1, 0, 0, 5_000),
                row("b", 2, 1, 11, 0),
                row("c", 3, 1, 12, 9),
                row("d", 4, 1, 99, 0),
                row("e", 5, 2, 0, 0),
                end(5),
            ],
        );
        let (mut input, mut out) = ([0; 64], [0; 4096]);
        let list = work.state(host, &mut input, &mut out).unwrap();
        assert_eq!(state.op.get(), 3);
        assert!(
            state.input.borrow().iter().eq([0u8].iter()),
            "no input record"
        );
        assert_eq!((list.len(), list.is_empty()), (5, false));
        let rows: Vec<_> = list.rows().collect();
        assert_eq!(rows.len(), 5);
        let facts: Vec<_> = rows
            .iter()
            .map(|r| (r.key(), r.token(), r.phase(), r.hold(), r.due_ms()))
            .collect();
        assert_eq!(
            facts,
            vec![
                ("a", 1, Phase::Waiting, None, Some(5_000)),
                ("b", 2, Phase::Held, Some(Hold::Offline), None),
                ("c", 3, Phase::Held, Some(Hold::Saving), Some(9)),
                ("d", 4, Phase::Held, Some(Hold::Other(99)), None),
                ("e", 5, Phase::Running, None, None),
            ]
        );
        assert_eq!(list.get("c").map(|r| r.token()), Some(3));
        assert!(list.get("missing").is_none());
        assert_eq!(list.rows().len(), 5);
        // No work at all is one end.
        reply(state, &[end(0)]);
        let list = work.state(host, &mut input, &mut out).unwrap();
        assert!(list.is_empty());
        assert_eq!(list.rows().count(), 0);
    });
}

#[test]
fn a_state_reply_that_breaks_the_contract_is_refused_whole() {
    let many: Vec<_> = (0..=STATE_MAX_ROWS as u32)
        .map(|i| row("k", i, 0, 0, 0))
        .collect();
    let mut over = many.clone();
    over.push(end(STATE_MAX_ROWS as u32 + 1));
    let mut exact = many[..STATE_MAX_ROWS].to_vec();
    exact.push(end(STATE_MAX_ROWS as u32));
    let broken: [(&str, Vec<Record<'static>>); 9] = [
        ("no end", vec![row("a", 1, 0, 0, 0)]),
        ("an end that miscounts", vec![row("a", 1, 0, 0, 0), end(2)]),
        ("rows after the end", vec![end(0), row("a", 1, 0, 0, 0)]),
        (
            "a paged end",
            vec![Record::ServiceEnd(wire::ServiceEnd {
                flags: 1,
                total: 0,
                generation: 0,
                next: 0,
                cursor: None,
            })],
        ),
        (
            "an end with a generation",
            vec![Record::ServiceEnd(wire::ServiceEnd {
                flags: 0,
                total: 0,
                generation: 3,
                next: 0,
                cursor: None,
            })],
        ),
        ("an unknown state", vec![row("a", 1, 3, 0, 0), end(1)]),
        (
            "a reason on work that is not held",
            vec![row("a", 1, 0, 11, 0), end(1)],
        ),
        (
            "another record",
            vec![
                Record::ServiceWorkKey(wire::ServiceWorkKey { key: "a" }),
                end(1),
            ],
        ),
        ("one row too many", over),
    ];
    with_work(|work, host, state| {
        let (mut input, mut out) = ([0; 64], [0; 4096]);
        for (what, rows) in &broken {
            reply(state, rows);
            assert_eq!(
                work.state(host, &mut input, &mut out).unwrap_err(),
                Error::Invalid,
                "{what}"
            );
        }
        // Truncated bytes.
        reply(state, &[row("a", 1, 0, 0, 0), end(1)]);
        let cut = state.reply.borrow().len() - 2;
        state.reply.borrow_mut().truncate(cut);
        assert_eq!(
            work.state(host, &mut input, &mut out).unwrap_err(),
            Error::Invalid
        );
        // The most rows are fine.
        reply(state, &exact);
        assert_eq!(
            work.state(host, &mut input, &mut out).unwrap().len(),
            STATE_MAX_ROWS
        );
        state.error.set(abi::ERR_DENIED);
        assert_eq!(
            work.state(host, &mut input, &mut out).unwrap_err(),
            Error::Denied
        );
    });
}

#[test]
fn retry_waits_longer_each_time_up_to_six_hours_with_a_tenth_of_jitter() {
    let delays: Vec<u32> = (1..=12).map(|attempt| retry_delay_ms(attempt, 0)).collect();
    assert_eq!(
        delays,
        vec![
            30_000,
            60_000,
            120_000,
            240_000,
            480_000,
            960_000,
            1_920_000,
            3_840_000,
            7_680_000,
            15_360_000,
            RETRY_MOST_MS,
            RETRY_MOST_MS
        ]
    );
    // The first attempt, and a call with attempt 0, wait the first time.
    assert_eq!(retry_delay_ms(0, 0), RETRY_FIRST_MS);
    assert_eq!(retry_delay_ms(1, 255), RETRY_FIRST_MS + RETRY_FIRST_MS / 10);
    assert_eq!(
        retry_delay_ms(u32::MAX, 255),
        RETRY_MOST_MS + RETRY_MOST_MS / 10
    );
    assert!(retry_delay_ms(5, 128) > retry_delay_ms(5, 0));
    // Never over what `declare` takes.
    for attempt in [0, 1, 7, 30, 1_000, u32::MAX] {
        for jitter in [0, 1, 128, 255] {
            assert!(retry_delay_ms(attempt, jitter) <= SPAN_MAX_MS);
        }
    }
    assert_eq!(WorkSpec::new("a", 1).unwrap().retry(3, 0).check(), Ok(()));
}

#[test]
fn the_debug_texts_name_no_more_than_the_fields() {
    let spec = WorkSpec::new("key", 7).unwrap();
    assert!(alloc::format!("{spec:?}").contains("key"));
    with_work(|work, host, state| {
        reply(state, &[row("a", 1, 0, 0, 0), end(1)]);
        let (mut input, mut out) = ([0; 64], [0; 4096]);
        let list = work.state(host, &mut input, &mut out).unwrap();
        assert!(alloc::format!("{list:?}").contains("len: 1"));
        assert!(alloc::format!("{:?}", list.rows()).contains("remaining"));
        assert!(alloc::format!("{:?}", list.get("a").unwrap()).contains("Waiting"));
        assert!(alloc::format!("{work:?}").contains("Work"));
    });
}
