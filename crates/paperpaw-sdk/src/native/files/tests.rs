#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
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
    /// Replies in order; the last is repeated.
    replies: RefCell<Vec<Vec<u8>>>,
    error: Cell<i32>,
}

unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: SDK provides a live key and aligned output.
    unsafe {
        assert_eq!(core::slice::from_raw_parts(key.ptr, key.len), b"core.files");
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}

unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.op.set(op);
    let n = state.calls.get();
    state.calls.set(n + 1);
    // SAFETY: SDK encoded a live input slice.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let replies = state.replies.borrow();
    let reply = replies.get(n as usize).or(replies.last()).unwrap();
    assert!(reply.len() <= out.cap);
    // SAFETY: the fixture reply and exclusive SDK output are disjoint.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len());
    }
    i32::try_from(reply.len()).unwrap()
}

fn table() -> PpHost {
    let mut t = abi::host_table();
    t.service_open = open;
    t.service_call = call;
    t
}

fn encode(rows: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    bytes
}

fn reply(state: &State, rows: &[&[Record<'_>]]) {
    state
        .replies
        .replace(rows.iter().map(|r| encode(r)).collect());
}

fn entry(name: &'static str, flags: u32, size: u64, modified: Option<u64>) -> Record<'static> {
    Record::ServiceFsEntry(wire::ServiceFsEntry {
        name,
        flags,
        size,
        modified,
    })
}

fn end(flags: u32, cursor: Option<u64>) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags,
        total: 0,
        generation: 0,
        next: 0,
        cursor,
    })
}

fn progress(removed: u32, flags: u32, next: u64) -> Record<'static> {
    Record::ServiceFsProgress(wire::ServiceFsProgress {
        removed,
        flags,
        next,
    })
}

macro_rules! open_files {
    ($state:ident, $host:ident, $files:ident) => {
        let table = table();
        let mut $state = State::default();
        let $host = Host {
            table: &table,
            ctx: core::ptr::from_mut(&mut $state).cast(),
        };
        let $files = Files::open(&$host).unwrap();
    };
}

fn sent(state: &State) -> Vec<u8> {
    state.input.borrow().clone()
}

#[test]
fn a_folder_is_listed_page_by_page_with_its_cursor() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    reply(
        &state,
        &[&[
            entry("books", 1, 0, Some(1_700_000_000)),
            entry("a.epub", 0, 1234, None),
            entry("very long name", 2, 7, None),
            end(0, Some(99)),
        ]],
    );
    let page = files
        .list(&host, Root::Files, "shelf", 0, &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_LIST);
    let rows: Vec<Entry<'_>> = page.entries().collect();
    assert_eq!(page.len(), 3);
    assert_eq!(rows[0].name(), "books");
    assert!(rows[0].is_folder() && !rows[0].unaddressable());
    assert_eq!(rows[0].modified(), Some(1_700_000_000));
    assert_eq!((rows[1].size(), rows[1].modified()), (1234, None));
    assert!(rows[2].unaddressable());
    assert_eq!(page.next(), Some(99));
    {
        let bytes = sent(&state);
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceFsList(r)) if (r.root, r.path, r.cursor, r.flags) == (1, "shelf", 0, 0)
        ));
    }
    reply(&state, &[&[entry("last", 0, 1, None), end(1, None)]]);
    let page = files
        .list(&host, Root::Data, "", 99, &mut input, &mut out)
        .unwrap();
    assert_eq!(page.next(), None);
    assert_eq!(page.entries().count(), 1);
    let bytes = sent(&state);
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceFsList(r)) if (r.root, r.cursor) == (0, 99)
    ));
}

#[test]
fn a_page_that_cannot_go_on_is_refused() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    let mut list = |rows: &[Record<'_>], cursor| {
        reply(&state, &[rows]);
        files
            .list(&host, Root::Data, "", cursor, &mut input, &mut out)
            .map(|p| p.len())
    };
    // Not the last, and no place to go on from.
    assert!(matches!(
        list(&[entry("a", 0, 1, None), end(0, None)], 0),
        Err(Error::Invalid)
    ));
    // Empty, and the same place again: it would repeat for ever.
    assert!(matches!(list(&[end(0, Some(5))], 5), Err(Error::Invalid)));
    assert_eq!(list(&[end(0, Some(6))], 5).unwrap(), 0);
    // Unknown flags of the page or an entry, a name with a slash, a folder with a size.
    assert!(matches!(list(&[end(2, None)], 0), Err(Error::Invalid)));
    assert!(matches!(
        list(&[entry("a", 4, 0, None), end(1, None)], 0),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        list(&[entry("a/b", 0, 0, None), end(1, None)], 0),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        list(&[entry("d", 1, 9, None), end(1, None)], 0),
        Err(Error::Invalid)
    ));
    // An entry in place of the end.
    assert!(matches!(
        list(&[entry("a", 0, 1, None)], 0),
        Err(Error::Invalid)
    ));
    // More rows than a page holds.
    let many: Vec<Record<'_>> = (0..=PAGE_MAX_ROWS)
        .map(|_| entry("a", 0, 1, None))
        .chain([end(1, None)])
        .collect();
    assert!(matches!(list(&many, 0), Err(Error::Invalid)));
}

#[test]
fn stat_and_space_are_typed() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    reply(&state, &[&[entry("b.epub", 0, 55, Some(42))]]);
    let e = files
        .stat(&host, Root::Files, "x/b.epub", &mut input, &mut out)
        .unwrap();
    assert_eq!((e.name(), e.size(), e.modified()), ("b.epub", 55, Some(42)));
    assert_eq!(state.op.get(), schema::OP_STAT);
    reply(
        &state,
        &[&[Record::ServiceFsSpace(wire::ServiceFsSpace {
            flags: 1 | 4,
            free: 1000,
            total: 5000,
        })]],
    );
    let s = files
        .space(&host, Root::Files, &mut input, &mut out)
        .unwrap();
    assert_eq!(
        (s.free(), s.total(), s.ignores_case()),
        (Some(1000), None, true)
    );
    reply(
        &state,
        &[&[Record::ServiceFsSpace(wire::ServiceFsSpace {
            flags: 8,
            free: 0,
            total: 0,
        })]],
    );
    assert!(matches!(
        files.space(&host, Root::Data, &mut input, &mut out),
        Err(Error::Invalid)
    ));
    // Roots the service does not know are refused before the call.
    let calls = state.calls.get();
    assert!(matches!(
        files.stat(&host, Root::Book, "", &mut input, &mut out),
        Err(Error::Invalid)
    ));
    assert_eq!(state.calls.get(), calls);
}

#[test]
fn changes_check_their_empty_reply_and_carry_their_flags() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    reply(&state, &[&[]]);
    files
        .make_dir(&host, Root::Data, "a/b", &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_MAKE_DIR);
    files
        .move_entry(
            &host,
            Root::Data,
            "x.tmp",
            Root::Files,
            "books/x.epub",
            true,
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_MOVE);
    {
        let bytes = sent(&state);
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceFsMove(r))
                if (r.from_root, r.from, r.to_root, r.to, r.flags) == (0, "x.tmp", 1, "books/x.epub", 1)
        ));
    }
    files
        .move_entry(
            &host,
            Root::Data,
            "a",
            Root::Data,
            "b",
            false,
            &mut input,
            &mut out,
        )
        .unwrap();
    let bytes = sent(&state);
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(sent.iter().next(), Some(Record::ServiceFsMove(r)) if r.flags == 0));
    // A reply with something in it is not the host's.
    reply(&state, &[&[end(1, None)]]);
    assert!(matches!(
        files.make_dir(&host, Root::Data, "a", &mut input, &mut out),
        Err(Error::Invalid)
    ));
    // The host's refusals come through; a path over the cap never leaves.
    reply(&state, &[&[]]);
    state.error.set(abi::ERR_DENIED);
    assert!(matches!(
        files.make_dir(&host, Root::Install, "a", &mut input, &mut out),
        Err(Error::Denied)
    ));
    state.error.set(0);
    let long = "a".repeat(MAX_PATH + 1);
    let calls = state.calls.get();
    assert!(matches!(
        files.make_dir(&host, Root::Data, &long, &mut input, &mut out),
        Err(Error::NoSpace)
    ));
    assert_eq!(state.calls.get(), calls);
}

#[test]
fn a_tree_is_removed_step_by_step_until_done() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    reply(
        &state,
        &[
            &[progress(64, 0, 11)],
            &[progress(0, 0, 12)],
            &[progress(5, 1, 0)],
        ],
    );
    let total = files
        .remove_tree(&host, Root::Data, "old", &mut input, &mut out)
        .unwrap();
    assert_eq!(total, 69);
    assert_eq!(state.calls.get(), 3);
    assert_eq!(state.op.get(), schema::OP_REMOVE_TREE);
    // The last step went on from the place before.
    let bytes = sent(&state);
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceFsList(r)) if (r.path, r.cursor) == ("old", 12)
    ));
    // A host that never advances is given up on.
    let before = state.calls.get();
    reply(&state, &[&[progress(0, 0, 7)]]);
    assert!(matches!(
        files.remove_tree(&host, Root::Data, "old", &mut input, &mut out),
        Err(Error::Invalid)
    ));
    assert!(state.calls.get() - before <= STALLED_STEPS + 3);
    // Unknown flags of a step.
    reply(&state, &[&[progress(0, 2, 0)]]);
    assert!(matches!(
        files.remove_step(&host, Root::Data, "old", 0, &mut input, &mut out),
        Err(Error::Invalid)
    ));
}
