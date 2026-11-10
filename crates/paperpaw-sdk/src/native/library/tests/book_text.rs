use super::*;

fn text(kind: u32, index: u32, total: u32, text: &'static str) -> Record<'static> {
    Record::ServiceLibText(wire::ServiceLibText {
        kind,
        index,
        total,
        text,
    })
}

#[test]
fn favourite_sets_and_clears_only_bit_one() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 2048], [0; 4096]);
    let book = BookRef::path("books/a.epub").unwrap();
    reply(&state, &[]);
    for favourite in [true, false, true] {
        lib.set_favourite(&host, book, favourite, &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op.get(), schema::OP_SET_STATUS);
        let bytes = state.input.borrow();
        let rows = wire::records_decode(&bytes).unwrap();
        let mut rows = rows.iter();
        assert!(matches!(rows.next(), Some(Record::ServiceLibRef(_))));
        assert!(
            matches!(rows.next(), Some(Record::ServiceLibStatus(r)) if (r.set, r.clear) == if favourite { (2, 0) } else { (0, 2) })
        );
        assert!(rows.next().is_none());
    }
    // Finished still uses bit 0 only.
    lib.set_finished(&host, book, true, &mut input, &mut out)
        .unwrap();
    let bytes = state.input.borrow();
    assert!(
        matches!(wire::records_decode(&bytes).unwrap().iter().nth(1), Some(Record::ServiceLibStatus(r)) if (r.set, r.clear) == (1, 0))
    );
    drop(bytes);
    // A reply that is not empty, and the host's refusals, come back.
    reply(&state, &[end(0, 0, 0, 0)]);
    assert_eq!(
        lib.set_favourite(&host, book, true, &mut input, &mut out),
        Err(Error::Invalid)
    );
    for (status, error) in [
        (abi::ERR_DENIED, Error::Denied),
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_STATE, Error::State),
    ] {
        state.error.set(status);
        assert_eq!(
            lib.set_favourite(&host, book, true, &mut input, &mut out),
            Err(error)
        );
    }
}

#[test]
fn book_text_asks_each_kind_and_reads_the_whole_by_offsets() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    let book = BookRef::path("books/a.epub").unwrap();
    for (query, kind, index, body) in [
        (BookTextQuery::title(), 0, 0, "A title"),
        (BookTextQuery::author(), 1, 0, "An author"),
        (BookTextQuery::series(), 2, 0, "A series"),
        (BookTextQuery::tag(3), 3, 3, "fiction"),
        (BookTextQuery::field(1), 4, 1, "calibre.rating=5"),
    ] {
        let total = u32::try_from(body.len()).unwrap();
        reply(&state, &[text(kind, index, total, body)]);
        let range = out.as_ptr_range();
        let part = lib
            .book_text(&host, book, query, &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op.get(), schema::OP_TEXT);
        assert_eq!(
            (part.text(), part.total(), part.next_offset()),
            (body, total, None)
        );
        assert!(range.contains(&part.text().as_ptr()));
        let bytes = state.input.borrow();
        let rows = wire::records_decode(&bytes).unwrap();
        assert!(matches!(rows.iter().next(), Some(Record::ServiceLibRef(_))));
        assert!(
            matches!(rows.iter().nth(1), Some(Record::ServiceLibTextAt(r)) if (r.kind, r.index, r.offset) == (kind, index, 0))
        );
    }
}

#[test]
fn a_long_text_is_read_in_parts_and_a_cut_at_a_character_goes_on_at_the_cut() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    let book = BookRef::path("books/a.epub").unwrap();
    // 1,025 bytes: a part stops three bytes short of the cap at a character.
    let head: &'static str = alloc::boxed::Box::leak(("a".repeat(1021) + "😀").into_boxed_str());
    reply(&state, &[text(0, 0, 1025, &head[..1021])]);
    let part = lib
        .book_text(&host, book, BookTextQuery::title(), &mut input, &mut out)
        .unwrap();
    assert_eq!((part.text().len(), part.next_offset()), (1021, Some(1021)));
    reply(&state, &[text(0, 0, 1025, "😀")]);
    let part = lib
        .book_text(
            &host,
            book,
            BookTextQuery::title().from(1021),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!((part.text(), part.next_offset()), ("😀", None));
    let bytes = state.input.borrow();
    assert!(
        matches!(wire::records_decode(&bytes).unwrap().iter().nth(1), Some(Record::ServiceLibTextAt(r)) if r.offset == 1021)
    );
}

#[test]
fn book_text_refuses_a_reply_that_is_not_the_text_asked_for() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    let book = BookRef::path("books/a.epub").unwrap();
    for (kind, index, total, body, offset) in [
        (1, 0, 1, "a", 0),
        (0, 1, 1, "a", 0),
        (0, 0, 2, "a", 0),
        (0, 0, 1, "a", 2),
        (0, 0, 5000, "a", 0),
        (0, 0, 0, "a", 0),
    ] {
        reply(&state, &[text(kind, index, total, body)]);
        assert_eq!(
            lib.book_text(
                &host,
                book,
                BookTextQuery::title().from(offset),
                &mut input,
                &mut out
            ),
            Err(Error::Invalid),
            "{kind} {index} {total} {offset}"
        );
    }
    // Two records, none and a record of another kind are malformed.
    reply(&state, &[text(0, 0, 1, "a"), text(0, 0, 1, "a")]);
    assert_eq!(
        lib.book_text(&host, book, BookTextQuery::title(), &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[]);
    assert_eq!(
        lib.book_text(&host, book, BookTextQuery::title(), &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[end(0, 0, 0, 0)]);
    assert_eq!(
        lib.book_text(&host, book, BookTextQuery::title(), &mut input, &mut out),
        Err(Error::Invalid)
    );
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_STATE, Error::State),
        (abi::ERR_DENIED, Error::Denied),
    ] {
        state.error.set(status);
        assert_eq!(
            lib.book_text(&host, book, BookTextQuery::tag(9), &mut input, &mut out),
            Err(error)
        );
    }
    // A request that does not fit the input buffer is refused before the host is called.
    state.error.set(0);
    let calls = state.calls.get();
    assert!(
        lib.book_text(&host, book, BookTextQuery::title(), &mut [0; 4], &mut out)
            .is_err()
    );
    assert_eq!(state.calls.get(), calls);
}

#[test]
fn the_edges_of_a_part_offset_at_the_end_and_a_shortfall_of_three_or_four_bytes() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    let book = BookRef::path("books/a.epub").unwrap();
    // An offset at the end of the text answers an empty part and no next.
    reply(&state, &[text(0, 0, 5, "")]);
    let part = lib
        .book_text(
            &host,
            book,
            BookTextQuery::title().from(5),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!((part.text(), part.next_offset()), ("", None));
    // ... and a part with text there, or an offset past the end, is refused.
    for (body, offset) in [("a", 5), ("", 6)] {
        reply(&state, &[text(0, 0, 5, body)]);
        assert_eq!(
            lib.book_text(
                &host,
                book,
                BookTextQuery::title().from(offset),
                &mut input,
                &mut out
            ),
            Err(Error::Invalid)
        );
    }
    // A part may stop up to three bytes short of the cap, not four.
    for (len, ok) in [(1021, true), (1020, false)] {
        let body: &'static str = alloc::boxed::Box::leak("a".repeat(len).into_boxed_str());
        reply(&state, &[text(0, 0, 2000, body)]);
        let got = lib.book_text(&host, book, BookTextQuery::title(), &mut input, &mut out);
        assert_eq!(got.is_ok(), ok, "{len}");
        if !ok {
            assert_eq!(got, Err(Error::Invalid));
        }
    }
    // The host's "no room" comes back as it is.
    state.error.set(abi::ERR_NO_SPACE);
    assert_eq!(
        lib.book_text(&host, book, BookTextQuery::title(), &mut input, &mut out),
        Err(Error::NoSpace)
    );
}
