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
fn anchor(id: u32, flags: u32, conversion: u64, start: &'static str) -> Record<'static> {
    Record::ServiceLibAnchor(wire::ServiceLibAnchor {
        id,
        flags,
        conversion,
        start,
        end: if start.is_empty() {
            ""
        } else {
            "/body/p[9]/text().4"
        },
    })
}

#[test]
fn anchors_page_with_the_annotation_generation_and_read_both_optional_parts() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    reply(
        &state,
        &[
            anchor(4, 0b11, 7, "/body/p[1]/text().0"),
            anchor(6, 0b01, 0, ""),
            anchor(9, 0b10, 0, "/body/p[2]/text().1"),
            end(0, 5, 3, 3),
        ],
    );
    let book = BookRef::path("/books/a.epub").unwrap();
    let page = lib
        .anchors(&host, book, AnnotationCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_ANCHORS);
    assert_eq!(
        (page.len(), page.end().total(), page.end().generation()),
        (3, 5, 3)
    );
    let rows: Vec<_> = page.rows().collect();
    assert_eq!(rows[0].id(), 4);
    assert_eq!(rows[0].conversion(), Some(7));
    assert_eq!(
        rows[0].xpointers(),
        Some(("/body/p[1]/text().0", "/body/p[9]/text().4"))
    );
    // A present conversion of 0 differs from none; no XPointers is None.
    assert_eq!((rows[1].conversion(), rows[1].xpointers()), (Some(0), None));
    assert_eq!(
        (rows[2].conversion(), rows[2].xpointers().is_some()),
        (None, true)
    );
    // The next page continues the same book with the same generation.
    let next = page.end().next().unwrap();
    reply(&state, &[anchor(11, 0, 0, ""), end(0, 5, 3, 4)]);
    lib.anchors(&host, book, next, &mut input, &mut out)
        .unwrap();
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    let sent: Vec<_> = rows.iter().collect();
    assert!(matches!(&sent[0], Record::ServiceLibRef(r) if r.path == "/books/a.epub"));
    assert!(
        matches!(&sent[1], Record::ServiceLibPage(p) if (p.cursor, p.annotation_generation) == (3, 3))
    );
}

#[test]
fn malformed_anchor_pages_and_stale_generations_are_refused() {
    let table = table();
    let mut state = State::default();
    let (host, lib) = host_and_lib(&table, &mut state);
    let (mut input, mut out) = ([0; 1024], [0; 4096]);
    let book = BookRef::path("/books/a.epub").unwrap();
    let start = AnnotationCursor::start();
    let bad: [Vec<Record<'static>>; 6] = [
        // An undefined flag bit, XPointer text without its bit, a conversion
        // without its bit, a repeated id, and no end.
        vec![anchor(1, 0b100, 0, ""), end(1, 1, 3, 1)],
        vec![anchor(1, 0, 0, "/body/p"), end(1, 1, 3, 1)],
        vec![anchor(1, 0, 5, ""), end(1, 1, 3, 1)],
        vec![anchor(1, 0, 0, ""), anchor(1, 0, 0, ""), end(1, 2, 3, 2)],
        vec![anchor(1, 0, 0, "")],
        vec![anchor(1, 0, 0, ""), end(1, 2, 3, 1)],
    ];
    for rows in bad {
        reply(&state, &rows);
        assert_eq!(
            lib.anchors(&host, book, start, &mut input, &mut out)
                .unwrap_err(),
            Error::Invalid,
            "{rows:?}"
        );
    }
    // A cursor from another generation is State.
    reply(&state, &[anchor(1, 0, 0, ""), end(1, 1, 4, 1)]);
    let stale = AnnotationCursor {
        offset: 0,
        generation: 3,
    };
    assert_eq!(
        lib.anchors(&host, book, stale, &mut input, &mut out)
            .unwrap_err(),
        Error::State
    );
}
