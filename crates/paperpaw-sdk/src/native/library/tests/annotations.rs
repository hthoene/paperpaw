use super::super::annotations::AnnotationPage;
use super::*;

fn annotation() -> wire::ServiceLibAnnotation<'static> {
    wire::ServiceLibAnnotation {
        id: 9,
        kind: 3,
        start_section: 1,
        start_offset: 2,
        end_section: 3,
        end_offset: 4,
        flags: 31,
        percent: 50,
        conversion: 0,
        created: 123,
        updated: 456,
        client_key: "import-key",
        text: "text",
        note: "note",
    }
}
fn row() -> Record<'static> {
    Record::ServiceLibAnnotation(annotation())
}
#[test]
fn pages_borrow_rows_and_keep_generations_separate() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let book = BookRef::catalog(7, 91).unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 4096];
    reply(&state, &[row(), end(0, 2, 13, 1)]);
    let range = out.as_ptr_range();
    let page = lib
        .annotations(&host, book, AnnotationCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(
        (
            page.len(),
            page.is_empty(),
            page.end().total(),
            page.end().generation()
        ),
        (1, false, 2, 13)
    );
    let r = page.rows().next().unwrap().unwrap();
    assert!(range.contains(&r.text().as_ptr()));
    assert_eq!(
        (r.id(), r.kind(), r.conversion()),
        (9, AnnotationKind::Other(1), Some(0))
    );
    assert_eq!(
        (
            r.start_section(),
            r.start_offset(),
            r.end_section(),
            r.end_offset()
        ),
        (1, 2, 3, 4)
    );
    assert_eq!((r.percent(), r.created(), r.updated()), (50, 123, 456));
    assert_eq!(
        (r.client_key(), r.text(), r.note()),
        ("import-key", "text", "note")
    );
    assert!(r.noted() && r.text_cut() && r.note_cut() && r.has_xpointers());
    let next = page.end().next().unwrap();
    reply(&state, &[row(), end(1, 2, 13, 2)]);
    assert!(
        lib.annotations(&host, book, next, &mut input, &mut out)
            .unwrap()
            .end()
            .next()
            .is_none()
    );
    let bytes = state.input.borrow();
    let records = wire::records_decode(&bytes).unwrap();
    let mut records = records.iter();
    assert!(
        matches!(records.next(), Some(Record::ServiceLibRef(r)) if r.key==Some(7) && r.generation==91 && r.path.is_empty())
    );
    assert!(
        matches!(records.next(), Some(Record::ServiceLibPage(r)) if (r.cursor,r.annotation_generation)==(1,13))
    );
    drop(bytes);
    reply(&state, &[row(), end(1, 2, 14, 2)]);
    assert!(matches!(
        lib.annotations(&host, book, next, &mut input, &mut out),
        Err(Error::State)
    ));
}
#[test]
fn malformed_tails_and_rows_are_rejected_before_publishing() {
    let cursor = AnnotationCursor::start();
    for records in [
        vec![],
        vec![row()],
        vec![end(1, 0, 1, 0), row()],
        vec![row(), end(1, 1, 0, 1)],
        vec![row(), end(2, 1, 1, 1)],
        vec![row(), end(1, 2, 1, 1)],
        vec![row(), end(0, 1, 1, 1)],
        vec![end(0, 1, 1, 0)],
        vec![entry(), end(1, 1, 1, 1)],
        vec![row(), end(1, 1, 1, 2)],
    ] {
        let state = State::default();
        reply(&state, &records);
        assert!(AnnotationPage::decode(&state.reply.borrow(), cursor).is_err());
    }
    for (flags, percent) in [(32, 0), (0, 101)] {
        let state = State::default();
        let mut r = annotation();
        r.flags = flags;
        r.percent = percent;
        reply(&state, &[Record::ServiceLibAnnotation(r), end(1, 1, 1, 1)]);
        assert!(AnnotationPage::decode(&state.reply.borrow(), cursor).is_err());
    }
    let state = State::default();
    reply(&state, &[row(), end(1, 1, 1, 1)]);
    let bytes = state.reply.borrow();
    for n in 0..bytes.len() {
        assert!(AnnotationPage::decode(&bytes[..n], cursor).is_err());
    }
}
#[test]
fn page_caps_empty_and_overflow_are_checked() {
    let state = State::default();
    reply(&state, &[end(1, 0, 1, 0)]);
    assert!(
        AnnotationPage::decode(&state.reply.borrow(), AnnotationCursor::start())
            .unwrap()
            .is_empty()
    );
    let mut rows = vec![row(); 32];
    rows.push(end(1, 32, 1, 32));
    reply(&state, &rows);
    assert_eq!(
        AnnotationPage::decode(&state.reply.borrow(), AnnotationCursor::start())
            .unwrap()
            .len(),
        32
    );
    rows.insert(0, row());
    rows[33] = end(1, 33, 1, 33);
    reply(&state, &rows);
    assert!(AnnotationPage::decode(&state.reply.borrow(), AnnotationCursor::start()).is_err());
    reply(&state, &[row(), end(1, 0, 1, 0)]);
    assert!(
        AnnotationPage::decode(
            &state.reply.borrow(),
            AnnotationCursor {
                offset: u32::MAX,
                generation: 1
            }
        )
        .is_err()
    );
}
#[test]
fn text_chunks_check_identity_and_utf8_byte_offsets() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let book = BookRef::path("books/test.epub").unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 4096];
    for (kind, wire_kind) in [(AnnotationTextKind::Text, 5), (AnnotationTextKind::Note, 6)] {
        reply(
            &state,
            &[Record::ServiceLibText(wire::ServiceLibText {
                kind: wire_kind,
                index: 9,
                total: 6,
                text: "é漢",
            })],
        );
        let range = out.as_ptr_range();
        let part = lib
            .annotation_text(
                &host,
                book,
                AnnotationTextQuery::new(9, kind).from(0),
                &mut input,
                &mut out,
            )
            .unwrap();
        assert_eq!(
            (part.text(), part.total(), part.next_offset()),
            ("é漢", 6, Some(5))
        );
        assert!(range.contains(&part.text().as_ptr()));
        let records = state.input.borrow();
        assert!(
            matches!(wire::records_decode(&records).unwrap().iter().nth(1), Some(Record::ServiceLibTextAt(r)) if (r.kind,r.index,r.offset)==(wire_kind,9,0))
        );
        drop(records);
        reply(
            &state,
            &[Record::ServiceLibText(wire::ServiceLibText {
                kind: wire_kind,
                index: 9,
                total: 6,
                text: "!",
            })],
        );
        assert_eq!(
            lib.annotation_text(
                &host,
                book,
                AnnotationTextQuery::new(9, kind).from(5),
                &mut input,
                &mut out
            )
            .unwrap()
            .next_offset(),
            None
        );
    }
}
#[test]
fn text_chunks_reject_bad_identity_and_lengths() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let book = BookRef::path("books/test.epub").unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 4096];
    for (kind, index, total, text, offset) in [
        (6, 9, 1, "a", 0),
        (5, 10, 1, "a", 0),
        (5, 9, 0, "a", 0),
        (5, 9, 2, "", 0),
        (5, 9, 0, "", 1),
        (5, 9, u32::MAX, "a", u32::MAX),
    ] {
        reply(
            &state,
            &[Record::ServiceLibText(wire::ServiceLibText {
                kind,
                index,
                total,
                text,
            })],
        );
        assert!(
            lib.annotation_text(
                &host,
                book,
                AnnotationTextQuery::new(9, AnnotationTextKind::Text).from(offset),
                &mut input,
                &mut out
            )
            .is_err()
        );
    }
    reply(
        &state,
        &[Record::ServiceLibText(wire::ServiceLibText {
            kind: 5,
            index: 9,
            total: 0,
            text: "",
        })],
    );
    assert_eq!(
        lib.annotation_text(
            &host,
            book,
            AnnotationTextQuery::new(9, AnnotationTextKind::Text).from(0),
            &mut input,
            &mut out
        )
        .unwrap()
        .next_offset(),
        None
    );
}
#[test]
fn host_refusals_and_encoder_failures_propagate() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let book = BookRef::path("book.epub").unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 4096];
    assert!(
        lib.annotations(
            &host,
            book,
            AnnotationCursor::start(),
            &mut input[..1],
            &mut out
        )
        .is_err()
    );
    assert_eq!(state.calls.get(), 0);
    for (code, error) in [
        (abi::ERR_DENIED, Error::Denied),
        (abi::ERR_STATE, Error::State),
        (abi::ERR_NOT_FOUND, Error::NotFound),
    ] {
        state.error.set(code);
        assert!(
            matches!(lib.annotations(&host,book,AnnotationCursor::start(),&mut input,&mut out),Err(e) if e==error)
        );
        assert!(
            matches!(lib.annotation_text(&host,book, AnnotationTextQuery::new(9, AnnotationTextKind::Text).from(0),&mut input,&mut out),Err(e) if e==error)
        );
    }
}

mod bounds;
