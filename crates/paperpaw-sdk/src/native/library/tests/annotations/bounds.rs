use super::*;
use crate::native::commands::menu_alloc_tests::no_alloc;

struct Fixed<'a> {
    reply: &'a [u8],
    op: u32,
}
unsafe extern "C" fn fixed_call(
    ctx: *mut c_void,
    _: u32,
    op: u32,
    _: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: the fixed fixture and reply stay live until the SDK call returns.
    let state = unsafe { &*ctx.cast::<Fixed<'_>>() };
    assert_eq!(op, state.op);
    assert!(state.reply.len() <= out.cap);
    // SAFETY: SDK lends an exclusive buffer disjoint from the fixture's reply.
    unsafe {
        core::ptr::copy_nonoverlapping(state.reply.as_ptr(), out.ptr, state.reply.len());
    }
    i32::try_from(state.reply.len()).unwrap()
}
#[test]
fn maximum_previews_and_32_row_pages_allocate_nothing() {
    let state = State::default();
    let preview = "é".repeat(256);
    let mut annotation = annotation();
    annotation.text = &preview;
    annotation.note = &preview;
    let max_rows = vec![Record::ServiceLibAnnotation(annotation); 3];
    for (mut records, count) in [(max_rows, 3), (vec![row(); 32], 32)] {
        records.push(end(1, count, 3, count));
        reply(&state, &records);
        let reply = state.reply.borrow();
        let mut fixed = Fixed {
            reply: &reply,
            op: schema::OP_ANNOTATIONS,
        };
        let mut table = table();
        table.service_call = fixed_call;
        let host = Host {
            table: &table,
            ctx: core::ptr::from_mut(&mut fixed).cast(),
        };
        let lib = Library::open(&host).unwrap();
        let mut input = [0; 1024];
        let mut out = [0; 4096];
        let count_read = no_alloc(|| {
            let page = lib
                .annotations(
                    &host,
                    BookRef::path("books/test.epub").unwrap(),
                    AnnotationCursor::start(),
                    &mut input,
                    &mut out,
                )
                .unwrap();
            for row in page.rows() {
                assert!(row.unwrap().text().len() <= 512);
            }
            page.len()
        });
        assert_eq!(count_read, usize::try_from(count).unwrap());
    }
}
#[test]
fn full_1024_byte_text_and_note_chunks_allocate_nothing() {
    let state = State::default();
    let text = "漢".repeat(341) + "!";
    for (kind, wire_kind) in [(AnnotationTextKind::Text, 5), (AnnotationTextKind::Note, 6)] {
        reply(
            &state,
            &[Record::ServiceLibText(wire::ServiceLibText {
                kind: wire_kind,
                index: 9,
                total: 1024,
                text: &text,
            })],
        );
        let reply = state.reply.borrow();
        let mut fixed = Fixed {
            reply: &reply,
            op: schema::OP_TEXT,
        };
        let mut table = table();
        table.service_call = fixed_call;
        let host = Host {
            table: &table,
            ctx: core::ptr::from_mut(&mut fixed).cast(),
        };
        let lib = Library::open(&host).unwrap();
        let mut input = [0; 1024];
        let mut out = [0; 4096];
        no_alloc(|| {
            let part = lib
                .annotation_text(
                    &host,
                    BookRef::path("test.epub").unwrap(),
                    AnnotationTextQuery::new(9, kind),
                    &mut input,
                    &mut out,
                )
                .unwrap();
            assert_eq!(part.text(), text);
            assert_eq!(part.next_offset(), None);
        });
    }
}
#[test]
fn text_reply_truncation_trailing_records_and_wrong_types_fail() {
    let state = State::default();
    reply(
        &state,
        &[Record::ServiceLibText(wire::ServiceLibText {
            kind: 5,
            index: 9,
            total: 3,
            text: "abc",
        })],
    );
    let encoded = state.reply.borrow().clone();
    let mut cases: Vec<Vec<u8>> = (0..encoded.len()).map(|n| encoded[..n].to_vec()).collect();
    let mut trailing = encoded.clone();
    trailing.extend_from_slice(&encoded);
    cases.push(trailing);
    reply(&state, &[row()]);
    cases.push(state.reply.borrow().clone());
    for bytes in cases {
        let mut fixed = Fixed {
            reply: &bytes,
            op: schema::OP_TEXT,
        };
        let mut table = table();
        table.service_call = fixed_call;
        let host = Host {
            table: &table,
            ctx: core::ptr::from_mut(&mut fixed).cast(),
        };
        let lib = Library::open(&host).unwrap();
        let mut input = [0; 1024];
        let mut out = [0; 4096];
        assert!(matches!(
            lib.annotation_text(
                &host,
                BookRef::path("test.epub").unwrap(),
                AnnotationTextQuery::new(9, AnnotationTextKind::Text),
                &mut input,
                &mut out
            ),
            Err(Error::Invalid)
        ));
    }
}
#[test]
fn opaque_cursor_tail_and_oversized_reply_are_rejected() {
    let state = State::default();
    let terminal = wire::ServiceEnd {
        flags: 1,
        total: 0,
        generation: 1,
        next: 0,
        cursor: Some(1),
    };
    reply(&state, &[Record::ServiceEnd(terminal)]);
    assert!(AnnotationPage::decode(&state.reply.borrow(), AnnotationCursor::start()).is_err());
    let bytes = vec![0; 4097];
    let mut fixed = Fixed {
        reply: &bytes,
        op: schema::OP_ANNOTATIONS,
    };
    let mut table = table();
    table.service_call = fixed_call;
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut fixed).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 8192];
    assert!(matches!(
        lib.annotations(
            &host,
            BookRef::path("test.epub").unwrap(),
            AnnotationCursor::start(),
            &mut input,
            &mut out
        ),
        Err(Error::Invalid)
    ));
}
