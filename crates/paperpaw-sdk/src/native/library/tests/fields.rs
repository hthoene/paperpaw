use super::*;
fn field<'a>(id: &'a str, value: &'a str) -> Record<'a> {
    Record::ServiceLibField(wire::ServiceLibField {
        id,
        title: "Rating",
        value,
    })
}
#[test]
fn fields_use_a_separate_generation_and_preserve_full_values() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 4096];
    let book = BookRef::catalog(12, 17).unwrap();
    let value = "界".repeat(341) + "x";
    reply(
        &state,
        &[field("end-of-book.rating", &value), end(0, 2, 23, 1)],
    );
    let range = out.as_ptr_range();
    let page = lib
        .fields(&host, book, FieldCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!((page.end().total(), page.end().generation()), (2, 23));
    let row = page.rows().next().unwrap().unwrap();
    assert_eq!(
        (row.id(), row.title(), row.value()),
        ("end-of-book.rating", "Rating", value.as_str())
    );
    assert!(range.contains(&row.value().as_ptr()));
    let cursor = page.end().next().unwrap();
    reply(&state, &[field("other.field", "whole"), end(1, 2, 23, 2)]);
    let page = lib
        .fields(&host, book, cursor, &mut input, &mut out)
        .unwrap();
    assert!(page.end().next().is_none());
    assert_eq!(page.len(), 1);
    let bytes = state.input.borrow();
    let batch = wire::records_decode(&bytes).unwrap();
    assert!(
        matches!(batch.iter().next(), Some(Record::ServiceLibRef(row)) if row.key == Some(12) && row.generation == 17)
    );
    assert!(
        matches!(batch.iter().nth(1), Some(Record::ServiceLibFieldPage(row)) if row.cursor == 1 && row.generation == 23)
    );
    drop(bytes);
    reply(&state, &[field("other.field", "whole"), end(1, 2, 24, 2)]);
    assert!(matches!(
        lib.fields(&host, book, cursor, &mut input, &mut out),
        Err(Error::State)
    ));
}
#[test]
fn fields_reject_nonprogress_wrong_rows_unsorted_ids_and_invalid_totals() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 4096];
    let book = BookRef::path("a.epub").unwrap();
    let cases = [
        vec![field("p.a", "x"), end(1, 1, 0, 1)],
        vec![end(0, 1, 9, 0)],
        vec![field("p.a", "x"), end(0, 65, 9, 1)],
        vec![field("unqualified", "x"), end(1, 1, 9, 1)],
        vec![field("p.b", "x"), field("p.a", "y"), end(1, 2, 9, 2)],
        vec![field("p.a", "x"), field("p.a", "y"), end(1, 2, 9, 2)],
        vec![field("p.a", "x")],
        vec![entry(), end(1, 1, 9, 1)],
        vec![field("p.a", "x"), end(1, 1, 9, 1), field("p.b", "y")],
    ];
    for rows in cases {
        reply(&state, &rows);
        assert!(
            lib.fields(&host, book, FieldCursor::start(), &mut input, &mut out)
                .is_err()
        );
    }
    reply(&state, &[end(1, 0, 9, 0)]);
    let page = lib
        .fields(&host, book, FieldCursor::start(), &mut input, &mut out)
        .unwrap();
    assert!(page.is_empty());
    assert_eq!(page.end().total(), 0);
    state.error.set(abi::ERR_STATE);
    assert!(matches!(
        lib.fields(&host, book, FieldCursor::start(), &mut input, &mut out),
        Err(Error::State)
    ));
}
#[test]
fn field_pages_reach_all_64_fields_and_reject_page_cap_plus_one() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let lib = Library::open(&host).unwrap();
    let mut input = [0; 1024];
    let mut out = [0; 4096];
    let ids = (0..64)
        .map(|n| alloc::format!("p.field{n:02}"))
        .collect::<Vec<_>>();
    let book = BookRef::path("a.epub").unwrap();
    let mut cursor = FieldCursor::start();
    let mut count = 0;
    for (page_index, names) in ids.chunks(32).enumerate() {
        let mut rows = names
            .iter()
            .map(|id| field(id, "value"))
            .collect::<Vec<_>>();
        rows.push(end(
            u32::from(page_index == 1),
            64,
            3,
            u32::try_from((page_index + 1) * 32).unwrap(),
        ));
        reply(&state, &rows);
        let page = lib
            .fields(&host, book, cursor, &mut input, &mut out)
            .unwrap();
        count += page.rows().count();
        if let Some(next) = page.end().next() {
            cursor = next;
        }
    }
    assert_eq!(count, 64);
    let mut rows = ids[..33]
        .iter()
        .map(|id| field(id, "v"))
        .collect::<Vec<_>>();
    rows.push(end(1, 33, 3, 33));
    reply(&state, &rows);
    assert!(matches!(
        lib.fields(&host, book, FieldCursor::start(), &mut input, &mut out),
        Err(Error::Invalid)
    ));
}
