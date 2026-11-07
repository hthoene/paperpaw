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
    // SAFETY: SDK provides a live key and aligned output.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(key.ptr, key.len),
            b"core.library"
        );
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.op.set(op);
    state.calls.set(state.calls.get() + 1);
    // SAFETY: SDK encoded a live input slice.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let reply = state.reply.borrow();
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
fn reply(state: &State, rows: &[Record<'_>]) {
    let mut bytes = vec![0; 4096];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    state.reply.replace(bytes);
}
fn entry() -> Record<'static> {
    Record::ServiceLibEntry(wire::ServiceLibEntry {
        key: 0,
        title: "Book",
        author: "Author",
        series: "Series",
        number: "1",
        progress: 255,
        flags: 5,
        added: 7,
        badges: "Done\nFavourite",
    })
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
fn query_is_typed_borrowed_and_continues_without_losing_filters() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 2048];
    let mut out = [0; 4096];
    let query = Query::new()
        .show(Show::Unread)
        .sort(Sort::RecentlyOpened)
        .descending(true)
        .exclude_archive(true)
        .words("search")
        .unwrap()
        .author("Author")
        .unwrap();
    reply(&state, &[entry(), end(0, 2, 8, 1)]);
    let page = lib.query(&host, &query, &mut input, &mut out).unwrap();
    let row = page.rows().next().unwrap().unwrap();
    assert_eq!(
        (row.title(), row.progress(), row.finished(), row.favourite()),
        ("Book", None, true, true)
    );
    assert_eq!(row.badges().collect::<Vec<_>>(), ["Done", "Favourite"]);
    let next = page.end().next().unwrap();
    assert_eq!(page.end().total(), 2);
    let query = query.cursor(next);
    reply(&state, &[entry(), end(1, 2, 8, 2)]);
    let page = lib.query(&host, &query, &mut input, &mut out).unwrap();
    assert!(page.end().next().is_none());
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    assert!(
        matches!(rows.iter().next(),Some(Record::ServiceLibQuery(r)) if (r.show,r.sort,r.flags,r.cursor,r.generation)==(1,4,3,1,8) && r.words=="search" && r.author=="Author")
    );
}
#[test]
fn fields_require_unpaged_end_and_status_uses_two_records() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 2048];
    let mut out = [0; 4096];
    let book = BookRef::path("books/a.epub").unwrap();
    reply(
        &state,
        &[
            Record::ServiceLibField(wire::ServiceLibField {
                id: "demo.rating",
                title: "Rating",
                value: "5",
            }),
            end(0, 1, 0, 0),
        ],
    );
    let page = lib.fields(&host, book, &mut input, &mut out).unwrap();
    assert_eq!(page.rows().next().unwrap().unwrap().value(), "5");
    reply(&state, &[]);
    for finished in [false, true] {
        lib.set_finished(&host, book, finished, &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op.get(), schema::OP_SET_STATUS);
        let bytes = state.input.borrow();
        let rows = wire::records_decode(&bytes).unwrap();
        let mut rows = rows.iter();
        assert!(
            matches!(rows.next(),Some(Record::ServiceLibRef(r)) if r.path=="books/a.epub" && r.key.is_none())
        );
        assert!(
            matches!(rows.next(),Some(Record::ServiceLibStatus(r)) if r.set==u32::from(finished) && r.clear==u32::from(!finished))
        );
        assert!(rows.next().is_none());
    }
    reply(&state, &[end(0, 0, 0, 0)]);
    assert!(matches!(
        lib.set_finished(&host, book, true, &mut input, &mut out),
        Err(Error::Invalid)
    ));
    state.error.set(abi::ERR_DENIED);
    assert!(matches!(
        lib.set_finished(&host, book, true, &mut input, &mut out),
        Err(Error::Denied)
    ));
}
#[test]
fn malformed_streams_caps_generations_and_progress_are_refused() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 2048];
    let mut out = [0; 4096];
    for terminal in [
        end(1, 1, 0, 1),
        end(1, 2, 8, 1),
        end(0, 1, 8, 1),
        end(1, 1, 8, 2),
        end(3, 1, 8, 1),
    ] {
        reply(&state, &[entry(), terminal]);
        assert!(
            lib.query(&host, &Query::new(), &mut input, &mut out)
                .is_err()
        );
    }
    reply(&state, &[entry()]);
    assert!(
        lib.query(&host, &Query::new(), &mut input, &mut out)
            .is_err()
    );
    reply(&state, &[entry(), end(1, 1, 8, 1), entry()]);
    assert!(
        lib.query(&host, &Query::new(), &mut input, &mut out)
            .is_err()
    );
    reply(&state, &[entry(), end(0, 2, 8, 1)]);
    let next = lib
        .query(&host, &Query::new(), &mut input, &mut out)
        .unwrap()
        .end()
        .next()
        .unwrap();
    reply(&state, &[entry(), end(1, 2, 9, 2)]);
    assert!(matches!(
        lib.query(&host, &Query::new().cursor(next), &mut input, &mut out),
        Err(Error::State)
    ));
    let mut rows = vec![entry(); schema::PAGE_MAX_ROWS + 1];
    rows.push(end(1, 33, 8, 33));
    reply(&state, &rows);
    assert!(
        lib.query(&host, &Query::new(), &mut input, &mut out)
            .is_err()
    );
    let Record::ServiceLibEntry(mut row) = entry() else {
        panic!("fixture")
    };
    row.progress = 101;
    reply(&state, &[Record::ServiceLibEntry(row), end(1, 1, 8, 1)]);
    assert!(
        lib.query(&host, &Query::new(), &mut input, &mut out)
            .is_err()
    );
    for terminal in [
        end(1, 1, 0, 0),
        end(0, 1, 1, 0),
        end(0, 1, 0, 1),
        end(0, 2, 0, 0),
    ] {
        reply(
            &state,
            &[
                Record::ServiceLibField(wire::ServiceLibField {
                    id: "demo.x",
                    title: "X",
                    value: "1",
                }),
                terminal,
            ],
        );
        assert!(
            lib.fields(
                &host,
                BookRef::path("a.epub").unwrap(),
                &mut input,
                &mut out
            )
            .is_err()
        );
    }
}
#[test]
fn overlong_filters_and_invalid_references_never_reach_the_host() {
    assert!(Query::new().words(&"é".repeat(128)).is_ok());
    assert!(matches!(
        Query::new().words(&"é".repeat(129)),
        Err(Error::NoSpace)
    ));
    assert!(matches!(BookRef::path(""), Err(Error::Invalid)));
    assert!(matches!(BookRef::path("a\nb"), Err(Error::Invalid)));
    assert!(matches!(BookRef::catalog(0, 0), Err(Error::Invalid)));
    assert!(BookRef::catalog(0, 1).is_ok());
}
