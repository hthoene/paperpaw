use super::*;

fn reader(id: &'static str, formats: &'static str) -> Record<'static> {
    Record::ServiceDocReader(wire::ServiceDocReader {
        id,
        screen: "files.reader",
        formats,
        title: "Reader",
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
fn readers_ask_by_scope_and_cursor_and_list_their_formats() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(
        &state,
        &[
            reader("a.epub-reader", "format-epub.epub\nformat-txt.txt"),
            reader("b.any", ""),
            end(1, 2, 5, 2),
        ],
    );
    let page = docs
        .readers(
            &host,
            "format-epub.epub",
            ReaderCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_READERS);
    assert_eq!(
        (page.len(), page.end().total(), page.end().next()),
        (2, 2, None)
    );
    let rows: Vec<_> = page.rows().collect();
    assert_eq!(rows[0].id(), "a.epub-reader");
    assert_eq!(rows[0].screen(), "files.reader");
    assert_eq!(
        rows[0].formats().collect::<Vec<_>>(),
        ["format-epub.epub", "format-txt.txt"]
    );
    assert!(!rows[0].reads_every_format());
    assert!(rows[1].reads_every_format());
    assert_eq!(rows[1].formats().count(), 0);
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceDocPage(p))
            if (p.document, p.scope, p.cursor, p.generation) == (0, "format-epub.epub", 0, 0)
    ));
}

#[test]
fn a_list_continues_with_the_cursor_of_its_page_and_a_stale_one_is_state() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(
        &state,
        &[reader("a.r", ""), reader("b.r", ""), end(0, 3, 9, 2)],
    );
    let page = docs
        .readers(&host, "", ReaderCursor::start(), &mut input, &mut out)
        .unwrap();
    let next = page.end().next().unwrap();
    reply(&state, &[reader("c.r", ""), end(1, 3, 9, 3)]);
    let page = docs.readers(&host, "", next, &mut input, &mut out).unwrap();
    assert_eq!((page.len(), page.end().next()), (1, None));
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceDocPage(p)) if (p.cursor, p.generation) == (2, 9)
        ));
    }
    // The list changed under the cursor.
    reply(&state, &[reader("c.r", ""), end(1, 3, 10, 3)]);
    assert_eq!(
        docs.readers(&host, "", next, &mut input, &mut out)
            .unwrap_err(),
        Error::State
    );
}

#[test]
fn bad_scopes_and_malformed_pages_are_refused() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    let long = "s".repeat(129);
    assert_eq!(
        docs.readers(&host, &long, ReaderCursor::start(), &mut input, &mut out)
            .unwrap_err(),
        Error::Invalid
    );
    assert_eq!(state.calls.get(), 0);
    let start = ReaderCursor::start();
    let bad: [&[Record<'_>]; 6] = [
        &[],
        &[reader("a.r", "")],
        &[reader("a.r", ""), end(1, 1, 0, 1)],
        &[reader("a.r", ""), end(1, 2, 3, 1)],
        &[end(0, 1, 3, 0)],
        &[reader("a.r", ""), end(3, 1, 3, 1)],
    ];
    for rows in bad {
        reply(&state, rows);
        assert_eq!(
            docs.readers(&host, "", start, &mut input, &mut out)
                .unwrap_err(),
            Error::Invalid,
            "{rows:?}"
        );
    }
    state.error.set(abi::ERR_UNSUPPORTED);
    assert_eq!(
        docs.readers(&host, "", start, &mut input, &mut out)
            .unwrap_err(),
        Error::Unsupported
    );
}
