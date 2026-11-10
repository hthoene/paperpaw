#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use alloc::{vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};
use wire::Record;

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
        files.stat(&host, Root::UserCss, "", &mut input, &mut out),
        Err(Error::Unsupported)
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
    // Unknown flags of a step, and more removals than a call may make.
    reply(&state, &[&[progress(0, 2, 0)]]);
    assert!(matches!(
        files.remove_step(&host, Root::Data, "old", 0, &mut input, &mut out),
        Err(Error::Invalid)
    ));
    reply(&state, &[&[progress(65, 0, 3)]]);
    assert!(matches!(
        files.remove_step(&host, Root::Data, "old", 0, &mut input, &mut out),
        Err(Error::Invalid)
    ));
}

#[test]
fn a_tree_removal_that_goes_in_circles_is_given_up_on() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    // Two places taking turns with nothing removed: a changing cursor is not
    // progress. Over the idle cap after exactly cap + 1 calls.
    // (The fixture repeats its last reply, so every call is spelled out.)
    let alternating: Vec<Vec<Record<'_>>> = (0..=IDLE_CALLS)
        .map(|n| vec![progress(0, 0, 1 + u64::from(n % 2))])
        .collect();
    let rows: Vec<&[Record<'_>]> = alternating.iter().map(Vec::as_slice).collect();
    reply(&state, &rows);
    assert_eq!(
        files.remove_tree(&host, Root::Data, "old", &mut input, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(state.calls.get(), IDLE_CALLS + 1);
    // The same place again is no different.
    state.calls.set(0);
    reply(&state, &[&[progress(0, 0, 7)]]);
    assert_eq!(
        files.remove_tree(&host, Root::Data, "old", &mut input, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(state.calls.get(), IDLE_CALLS + 1);
    // Runs of the cap's length with no removal, each broken by one removal,
    // are a long tree, not a loop.
    state.calls.set(0);
    let mut rows: Vec<Vec<Record<'_>>> = Vec::new();
    for round in 0..3u64 {
        for n in 0..IDLE_CALLS {
            rows.push(vec![progress(0, 0, 1 + u64::from(n) % 2 + round)]);
        }
        rows.push(vec![progress(1, 0, 50 + round)]);
    }
    rows.push(vec![progress(2, 1, 0)]);
    let rows: Vec<&[Record<'_>]> = rows.iter().map(Vec::as_slice).collect();
    reply(&state, &rows);
    assert_eq!(
        files.remove_tree(&host, Root::Data, "old", &mut input, &mut out),
        Ok(5)
    );
    assert_eq!(state.calls.get(), 3 * (IDLE_CALLS + 1) + 1);
}

#[test]
fn replies_are_held_to_the_contract_even_with_a_big_buffer() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    let end_with = |next: u32, total: u32, generation: u32| {
        Record::ServiceEnd(wire::ServiceEnd {
            flags: 1,
            total,
            generation,
            next,
            cursor: None,
        })
    };
    let mut list = |rows: &[Record<'_>]| {
        reply(&state, &[rows]);
        files
            .list(&host, Root::Data, "", 0, &mut input, &mut out)
            .map(|p| p.len())
    };
    // The end's next, total and generation are 0 for this service.
    for (next, total, generation) in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
        assert_eq!(
            list(&[end_with(next, total, generation)]),
            Err(Error::Invalid)
        );
    }
    assert_eq!(list(&[end_with(0, 0, 0)]), Ok(0));
    // A listing never has an empty name, ".", or "..".
    for name in ["", ".", ".."] {
        assert_eq!(
            list(&[entry(name, 0, 1, None), end_with(0, 0, 0)]),
            Err(Error::Invalid),
            "{name:?}"
        );
    }
    // Stat names the root with "", and only the root; no "." either.
    reply(&state, &[&[entry("", 1, 0, None)]]);
    assert!(
        files
            .stat(&host, Root::Data, "", &mut input, &mut out)
            .unwrap()
            .is_folder()
    );
    for (path, name) in [("a", ""), ("a", "."), ("", "..")] {
        reply(&state, &[&[entry(name, 1, 0, None)]]);
        assert_eq!(
            files
                .stat(&host, Root::Data, path, &mut input, &mut out)
                .map(|e| e.size()),
            Err(Error::Invalid),
            "{path:?} {name:?}"
        );
    }
    // A reply longer than the operation's largest, in a scratch buffer that is
    // bigger than needed: the operation's own cap decides.
    let cap = files.service.max_reply(schema::OP_MAKE_DIR).unwrap();
    let mut big = vec![0u8; cap + 64];
    let at_cap = vec![0u8; cap];
    state.replies.replace(vec![at_cap]);
    let record = wire::ServiceFsPath { root: 0, path: "a" };
    let op = schema::OP_MAKE_DIR;
    assert_eq!(files.call(&host, op, record, &mut input, &mut big), Ok(cap));
    state.replies.replace(vec![vec![0u8; cap + 1]]);
    assert_eq!(
        files.call(&host, op, record, &mut input, &mut big),
        Err(Error::Invalid)
    );
}

#[test]
fn refusals_come_through_and_short_buffers_are_refused_before_a_call() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    reply(&state, &[&[]]);
    // Busy from the host is passed on untouched, for every changing op.
    state.error.set(abi::ERR_BUSY);
    assert_eq!(
        files.make_dir(&host, Root::Data, "a", &mut input, &mut out),
        Err(Error::Busy)
    );
    assert_eq!(
        files.remove_tree(&host, Root::Data, "a", &mut input, &mut out),
        Err(Error::Busy)
    );
    assert_eq!(
        files.move_entry(
            &host,
            Root::Data,
            "a",
            Root::Data,
            "b",
            false,
            &mut input,
            &mut out
        ),
        Err(Error::Busy)
    );
    state.error.set(0);
    // Reserved roots never leave, whichever call.
    let calls = state.calls.get();
    for root in [Root::UserCss, Root::DocumentCache] {
        assert_eq!(
            files
                .list(&host, root, "", 0, &mut input, &mut out)
                .map(|p| p.len()),
            Err(Error::Unsupported)
        );
        assert_eq!(
            files.make_dir(&host, root, "a", &mut input, &mut out),
            Err(Error::Unsupported)
        );
        assert_eq!(
            files
                .space(&host, root, &mut input, &mut out)
                .map(|s| s.free()),
            Err(Error::Unsupported)
        );
    }
    assert_eq!(state.calls.get(), calls);
    // The book root (3) only lists and stats; every changing op stays here.
    assert_eq!(
        files.make_dir(&host, Root::Book, "a", &mut input, &mut out),
        Err(Error::Unsupported)
    );
    assert_eq!(
        files
            .space(&host, Root::Book, &mut input, &mut out)
            .map(|s| s.free()),
        Err(Error::Unsupported)
    );
    assert_eq!(
        files.remove_step(&host, Root::Book, "a", 0, &mut input, &mut out),
        Err(Error::Unsupported)
    );
    assert_eq!(state.calls.get(), calls);
    // The bundle root (2) is read only: the host says Denied, and it comes through.
    state.error.set(abi::ERR_DENIED);
    assert_eq!(
        files.make_dir(&host, Root::Install, "a", &mut input, &mut out),
        Err(Error::Denied)
    );
    assert_eq!(
        files.remove_step(&host, Root::Install, "a", 0, &mut input, &mut out),
        Err(Error::Denied)
    );
    state.error.set(0);
    // Short buffers: no call is made, the answer is NoSpace.
    let calls = state.calls.get();
    let mut short_out = [0u8; 16];
    assert_eq!(
        files
            .list(&host, Root::Data, "", 0, &mut input, &mut short_out)
            .map(|p| p.len()),
        Err(Error::NoSpace)
    );
    let mut short_in = [0u8; 4];
    assert_eq!(
        files.make_dir(&host, Root::Data, "a", &mut short_in, &mut out),
        Err(Error::NoSpace)
    );
    assert_eq!(state.calls.get(), calls);
}

#[test]
fn the_book_folder_lists_and_stats_through_root_3_and_the_host_decides_when() {
    open_files!(state, host, files);
    let (mut input, mut out) = ([0; INPUT_BYTES], [0; OUTPUT_BYTES]);
    reply(
        &state,
        &[&[
            entry("cover.jpg", 0, 900, None),
            entry("page1.png", 0, 1200, None),
            end(1, None),
        ]],
    );
    let page = files
        .list(&host, Root::Book, "", 0, &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_LIST);
    assert_eq!(
        page.entries().map(|e| e.name()).collect::<Vec<_>>(),
        ["cover.jpg", "page1.png"]
    );
    assert_eq!(page.next(), None);
    let bytes = sent(&state);
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        batch.iter().next(),
        Some(Record::ServiceFsList(r)) if (r.root, r.path, r.cursor) == (3, "", 0)
    ));
    reply(&state, &[&[entry("page1.png", 0, 1200, None)]]);
    let e = files
        .stat(&host, Root::Book, "page1.png", &mut input, &mut out)
        .unwrap();
    assert_eq!((e.name(), e.size()), ("page1.png", 1200));
    let bytes = sent(&state);
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        batch.iter().next(),
        Some(Record::ServiceFsPath(r)) if (r.root, r.path) == (3, "page1.png")
    ));
    // Outside a folder or siblings document's format call the host says
    // Unsupported, and a book that is one file has no folder to list.
    for status in [abi::ERR_UNSUPPORTED, abi::ERR_NOT_FOUND] {
        state.error.set(status);
        assert_eq!(
            files
                .list(&host, Root::Book, "", 0, &mut input, &mut out)
                .map(|p| p.len()),
            Err(Error::from_status(status))
        );
    }
    // A long path is still refused before the call.
    state.error.set(0);
    let calls = state.calls.get();
    let long = "x".repeat(MAX_PATH + 1);
    assert_eq!(
        files
            .list(&host, Root::Book, &long, 0, &mut input, &mut out)
            .map(|p| p.len()),
        Err(Error::NoSpace)
    );
    assert_eq!(state.calls.get(), calls);
}
