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
    writes: Cell<u32>,
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
    if op == schema::OP_SET_FIELD {
        state.writes.set(state.writes.get() + 1);
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
        cursor: None,
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
fn status_uses_two_records_and_checks_the_empty_reply() {
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

#[test]
fn scalar_state_and_field_update_use_checked_schema_calls() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 2048];
    let mut out = [0; 4096];
    reply(
        &state,
        &[Record::ServiceLibState(wire::ServiceLibState {
            state: 2,
            books: 17,
            generation: 9,
            walk_ppm: 1_000_000,
        })],
    );
    let catalog = lib.state(&host, &mut input, &mut out).unwrap();
    assert_eq!(
        (catalog.phase(), catalog.books(), catalog.generation()),
        (CatalogPhase::Ready, 17, 9)
    );
    assert!(
        wire::records_decode(&state.input.borrow())
            .unwrap()
            .is_empty()
    );
    reply(&state, &[end(0, 0, 0, 0)]);
    assert!(matches!(
        lib.state(&host, &mut input, &mut out),
        Err(Error::Invalid)
    ));
    reply(&state, &[]);
    lib.set_field(
        &host,
        BookRef::catalog(0, 9).unwrap(),
        "demo.rating",
        "5",
        &mut input,
        &mut out,
    )
    .unwrap();
    assert_eq!(state.op.get(), schema::OP_SET_FIELD);
    let bytes = state.input.borrow();
    let records = wire::records_decode(&bytes).unwrap();
    let mut records = records.iter();
    assert!(
        matches!(records.next(),Some(Record::ServiceLibRef(r)) if r.path.is_empty() && r.key==Some(0) && r.generation==9)
    );
    assert!(
        matches!(records.next(),Some(Record::ServiceLibFieldSet(r)) if r.field=="demo.rating" && r.value=="5")
    );
}

fn book_row(key: u32, generation: u32) -> Record<'static> {
    Record::ServiceLibBook(wire::ServiceLibBook {
        key,
        generation,
        path: "books/a.epub",
        title: "A",
        author: "B",
        series: "C",
        number: "1",
        progress: 45,
        flags: 2,
        added: 7,
        size: 123,
        format: "core.epub",
        tags: 2,
        fields: 1,
        title_bytes: 1,
        author_bytes: 1,
        series_bytes: 1,
    })
}
#[test]
fn book_and_next_series_validate_records_and_preserve_reference_identity() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 2048];
    let mut out = [0; 4096];
    let book = BookRef::catalog(0, 9).unwrap();
    reply(&state, &[book_row(0, 9)]);
    let row = lib.book(&host, book, &mut input, &mut out).unwrap();
    assert_eq!(
        (row.path(), row.title(), row.size(), row.progress()),
        ("books/a.epub", "A", 123, Some(45))
    );
    let reference = row.reference().unwrap();
    assert_eq!((reference.0.key, reference.0.generation), (Some(0), 9));
    for bad in [book_row(1, 9), book_row(0, 10)] {
        reply(&state, &[bad]);
        assert!(matches!(
            lib.book(&host, book, &mut input, &mut out),
            Err(Error::State)
        ));
    }
    reply(&state, &[entry()]);
    assert!(matches!(
        lib.book(&host, book, &mut input, &mut out),
        Err(Error::Invalid)
    ));
    reply(&state, &[book_row(0, 9), book_row(0, 9)]);
    assert!(lib.book(&host, book, &mut input, &mut out).is_err());
    reply(&state, &[entry()]);
    assert_eq!(
        lib.next_in_series(&host, book, &mut input, &mut out)
            .unwrap()
            .title(),
        "Book"
    );
    assert_eq!(state.op.get(), schema::OP_NEXT_IN_SERIES);
    let bytes = state.input.borrow();
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(
        matches!(batch.iter().next(),Some(Record::ServiceLibRef(r)) if r.key==Some(0) && r.generation==9)
    );
    drop(bytes);
    reply(&state, &[book_row(0, 9)]);
    assert!(
        lib.next_in_series(&host, book, &mut input, &mut out)
            .is_err()
    );
    state.error.set(abi::ERR_NOT_FOUND);
    assert!(matches!(
        lib.next_in_series(&host, book, &mut input, &mut out),
        Err(Error::NotFound)
    ));
}
#[test]
fn relative_field_ids_are_refused_without_a_host_call() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 2048];
    let mut out = [0; 4096];
    assert!(matches!(
        lib.set_field(
            &host,
            BookRef::path("a.epub").unwrap(),
            "rating",
            "5",
            &mut input,
            &mut out
        ),
        Err(Error::Invalid)
    ));
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn refused_field_writes_never_commit_at_the_host_seam() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 2048];
    let mut out = [0; 4096];
    let book = BookRef::path("a.epub").unwrap();
    assert!(matches!(
        lib.set_field(&host, book, "rating", "5", &mut input, &mut out),
        Err(Error::Invalid)
    ));
    assert_eq!((state.calls.get(), state.writes.get()), (0, 0));
    state.error.set(abi::ERR_DENIED);
    assert!(matches!(
        lib.set_field(&host, book, "other.rating", "5", &mut input, &mut out),
        Err(Error::Denied)
    ));
    assert_eq!((state.calls.get(), state.writes.get()), (1, 0));
    let bytes = state.input.borrow();
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(
        matches!(batch.iter().nth(1),Some(Record::ServiceLibFieldSet(r)) if r.field=="other.rating")
    );
}

#[path = "tests/anchors.rs"]
mod anchors;
mod annotations;
#[path = "tests/book_text.rs"]
mod book_text;
#[path = "tests/cover.rs"]
mod cover;
#[path = "tests/edits.rs"]
mod edits;
#[path = "tests/facets.rs"]
mod facets;
#[path = "tests/fields.rs"]
mod fields;
#[path = "tests/files.rs"]
mod files;
#[path = "tests/position.rs"]
mod position;
#[path = "tests/provide.rs"]
mod provide;
