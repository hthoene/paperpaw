use super::*;
use crate::native::declare::records_wire::Record;

fn host_and_lib<'a>(table: &'a PpHost, state: &mut State) -> (Host<'a>, Library) {
    let host = Host {
        table,
        ctx: core::ptr::from_mut(state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    (host, lib)
}
fn key_row() -> Record<'static> {
    Record::ServiceLibKey(wire::ServiceLibKey {
        key: "0123456789abcdef",
        fingerprint: "00112233445566778899aabbccddeeff",
        size: 1 << 33,
    })
}
fn position_row(flags: u32, xpointer: &'static str) -> Record<'static> {
    Record::ServiceLibPosition(wire::ServiceLibPosition {
        section: 3,
        offset: 40,
        insert: 2,
        progress_ppm: 250_000,
        conversion: 0,
        updated: 1_700_000_000,
        xpointer,
        flags,
    })
}
fn result(status: i32) -> Record<'static> {
    Record::ServiceLibResult(wire::ServiceLibResult {
        path: "",
        status,
        flags: 0,
    })
}

#[test]
fn book_key_borrows_the_key_fingerprint_and_size() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(&state, &[key_row()]);
    let range = out.as_ptr_range();
    let key = lib
        .book_key(
            &host,
            BookRef::path("/books/a.epub").unwrap(),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_BOOK_KEY);
    assert_eq!(
        (key.key(), key.fingerprint(), key.size()),
        (
            "0123456789abcdef",
            "00112233445566778899aabbccddeeff",
            1 << 33
        )
    );
    assert!(range.contains(&key.key().as_ptr()));
    // The request is the book reference alone.
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    assert!(
        matches!(rows.iter().next(), Some(Record::ServiceLibRef(r)) if r.path == "/books/a.epub")
    );
    assert_eq!(rows.len(), 1);
    drop(bytes);
    state.error.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        lib.book_key(&host, BookRef::catalog(1, 1).unwrap(), &mut input, &mut out),
        Err(Error::NotFound)
    );
    reply(&state, &[]);
    state.error.set(0);
    assert_eq!(
        lib.book_key(&host, BookRef::catalog(1, 1).unwrap(), &mut input, &mut out),
        Err(Error::Invalid)
    );
}

#[test]
fn position_reads_the_optional_parts_by_their_flags() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    let book = BookRef::catalog(5, 9).unwrap();
    reply(&state, &[position_row(0, "")]);
    let p = lib.position(&host, book, &mut input, &mut out).unwrap();
    assert_eq!(state.op.get(), schema::OP_POSITION);
    assert_eq!(
        (p.section, p.offset, p.insert, p.progress_ppm, p.updated),
        (3, 40, 2, 250_000, 1_700_000_000)
    );
    assert_eq!((p.conversion, p.xpointer, p.from_sync), (None, None, false));
    reply(
        &state,
        &[position_row(0b111, "/body/DocFragment[3]/p[2]/text().4")],
    );
    let p = lib.position(&host, book, &mut input, &mut out).unwrap();
    // Present and 0 differ from absent.
    assert_eq!(p.conversion, Some(0));
    assert_eq!(p.xpointer, Some("/body/DocFragment[3]/p[2]/text().4"));
    assert!(p.from_sync);
    state.error.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        lib.position(&host, book, &mut input, &mut out),
        Err(Error::NotFound)
    );
}

#[test]
fn set_position_sends_the_book_the_position_and_the_write_flags_in_order() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    let book = BookRef::path("/books/a.epub").unwrap();
    reply(&state, &[result(0)]);
    let write = PositionWrite::new(3, 40)
        .insert(2)
        .progress_ppm(1_000_000)
        .conversion(77)
        .updated(1_700_000_000)
        .xpointer("/body/p[1]")
        .sync()
        .confirmed();
    lib.set_position(&host, book, &write, &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_SET_POSITION);
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    let mut it = rows.iter();
    assert!(matches!(it.next(), Some(Record::ServiceLibRef(r)) if r.path == "/books/a.epub"));
    assert!(matches!(
        it.next(),
        Some(Record::ServiceLibPosition(p))
            if (p.section, p.offset, p.insert, p.progress_ppm, p.conversion, p.updated, p.flags) == (3, 40, 2, 1_000_000, 77, 1_700_000_000, 0b011)
                && p.xpointer == "/body/p[1]"
    ));
    assert!(matches!(it.next(), Some(Record::ServiceLibPositionSet(s)) if s.flags == 0b11));
    assert!(it.next().is_none());
    drop(bytes);
    // A local write names no flag.
    lib.set_position(&host, book, &PositionWrite::new(0, 0), &mut input, &mut out)
        .unwrap();
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    assert!(
        matches!(rows.iter().nth(1), Some(Record::ServiceLibPosition(p)) if p.flags == 0 && p.xpointer.is_empty())
    );
    assert!(matches!(rows.iter().nth(2), Some(Record::ServiceLibPositionSet(s)) if s.flags == 0));
}

#[test]
fn bad_writes_are_refused_before_the_host_is_called() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    let book = BookRef::path("/books/a.epub").unwrap();
    reply(&state, &[result(0)]);
    let long = "x".repeat(1025);
    for write in [
        PositionWrite::new(0, 0).progress_ppm(1_000_001),
        PositionWrite::new(0, 0).xpointer(&long),
        PositionWrite::new(0, 0).confirmed(),
    ] {
        assert_eq!(
            lib.set_position(&host, book, &write, &mut input, &mut out),
            Err(Error::Invalid)
        );
    }
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn a_refused_sync_write_is_state_and_other_statuses_pass_through() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    let book = BookRef::path("/books/a.epub").unwrap();
    let write = PositionWrite::new(1, 2).sync();
    for (status, error) in [
        (abi::ERR_STATE, Error::State),
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_NO_SPACE, Error::NoSpace),
    ] {
        state.error.set(status);
        assert_eq!(
            lib.set_position(&host, book, &write, &mut input, &mut out),
            Err(error)
        );
    }
    // A reply that is not one result record is malformed.
    state.error.set(0);
    reply(&state, &[]);
    assert_eq!(
        lib.set_position(&host, book, &write, &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[result(abi::ERR_STATE)]);
    assert_eq!(
        lib.set_position(&host, book, &write, &mut input, &mut out),
        Err(Error::State)
    );
}
