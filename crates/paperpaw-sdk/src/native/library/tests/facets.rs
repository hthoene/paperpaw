use super::*;
fn facet<'a>(text: &'a str, count: u32, first: u32) -> Record<'a> {
    Record::ServiceLibFacet(wire::ServiceLibFacet { text, count, first })
}
fn host_for<'a>(table: &'a PpHost, state: &mut State) -> Host<'a> {
    Host {
        table,
        ctx: core::ptr::from_mut(state).cast(),
    }
}
#[test]
fn every_facet_is_asked_with_the_query_filters_and_cursor_and_pages_on() {
    let table = table();
    let mut state = State::default();
    let host = host_for(&table, &mut state);
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 1280], [0; 4096]);
    let codes = [
        (Facet::Authors, 0),
        (Facet::Series, 1),
        (Facet::Tags, 2),
        (Facet::Letters, 3),
        (Facet::Folders, 4),
    ];
    for (facet_kind, code) in codes {
        reply(
            &state,
            &[facet("Ada", 3, 0), facet("Bo", 2, 3), end(0, 3, 31, 2)],
        );
        let query = Query::new()
            .show(Show::Finished)
            .tag("sf")
            .unwrap()
            .folder("Books/Mine")
            .unwrap();
        let range = out.as_ptr_range();
        let page = lib
            .facets(&host, &query, facet_kind, &mut input, &mut out)
            .unwrap();
        assert_eq!(
            (page.len(), page.end().total(), page.end().generation()),
            (2, 3, 31)
        );
        let rows = page.rows().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(
            rows.iter()
                .map(|r| (r.text(), r.count(), r.first()))
                .collect::<Vec<_>>(),
            [("Ada", 3, 0), ("Bo", 2, 3)]
        );
        assert!(range.contains(&rows[0].text().as_ptr()));
        let next = page.end().next().unwrap();
        let sent = state.input.borrow().clone();
        let batch = wire::records_decode(&sent).unwrap();
        assert!(matches!(
            batch.iter().next(),
            Some(Record::ServiceLibQuery(q))
                if q.facet == code && q.show == 3 && q.tag == "sf" && q.folder == "Books/Mine"
                    && q.cursor == 0 && q.generation == 0
        ));
        // The next page carries the catalog generation and the offset.
        reply(&state, &[facet("Cy", 1, 5), end(1, 3, 31, 3)]);
        let page = lib
            .facets(&host, &query.cursor(next), facet_kind, &mut input, &mut out)
            .unwrap();
        assert!(page.end().next().is_none());
        let sent = state.input.borrow().clone();
        let batch = wire::records_decode(&sent).unwrap();
        assert!(matches!(
            batch.iter().next(),
            Some(Record::ServiceLibQuery(q)) if q.cursor == 2 && q.generation == 31 && q.facet == code
        ));
    }
}
#[test]
fn a_stale_catalog_generation_is_a_state_error_and_a_list_may_be_empty() {
    let table = table();
    let mut state = State::default();
    let host = host_for(&table, &mut state);
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 1280], [0; 4096]);
    reply(&state, &[facet("A", 1, 0), end(0, 2, 7, 1)]);
    let cursor = lib
        .facets(&host, &Query::new(), Facet::Authors, &mut input, &mut out)
        .unwrap()
        .end()
        .next()
        .unwrap();
    reply(&state, &[facet("B", 1, 1), end(1, 2, 8, 2)]);
    assert!(matches!(
        lib.facets(
            &host,
            &Query::new().cursor(cursor),
            Facet::Authors,
            &mut input,
            &mut out
        ),
        Err(Error::State)
    ));
    reply(&state, &[end(1, 0, 9, 0)]);
    let page = lib
        .facets(&host, &Query::new(), Facet::Tags, &mut input, &mut out)
        .unwrap();
    assert!(page.is_empty() && page.end().total() == 0);
    state.error.set(abi::ERR_STATE);
    assert!(matches!(
        lib.facets(&host, &Query::new(), Facet::Tags, &mut input, &mut out),
        Err(Error::State)
    ));
    state.error.set(abi::ERR_UNSUPPORTED);
    assert!(
        lib.facets(&host, &Query::new(), Facet::Tags, &mut input, &mut out)
            .is_err()
    );
}
#[test]
fn malformed_facet_pages_are_refused_whole() {
    let table = table();
    let mut state = State::default();
    let host = host_for(&table, &mut state);
    let lib = Library::open(&host).unwrap();
    let (mut input, mut out) = ([0; 1280], [0; 4096]);
    let cases = [
        // A value without books.
        vec![facet("A", 0, 0), end(1, 1, 5, 1)],
        // A row of another kind.
        vec![entry(), end(1, 1, 5, 1)],
        // No progress, a next beyond the total, a count that does not add up.
        vec![end(0, 1, 5, 0)],
        vec![facet("A", 1, 0), end(1, 1, 5, 2)],
        vec![facet("A", 1, 0), end(1, 3, 5, 1)],
        vec![facet("A", 1, 0), end(0, 1, 5, 1)],
        // No generation, no end, trailing rows after the end.
        vec![facet("A", 1, 0), end(1, 1, 0, 1)],
        vec![facet("A", 1, 0)],
        vec![facet("A", 1, 0), end(1, 1, 5, 1), facet("B", 1, 1)],
    ];
    for rows in cases {
        reply(&state, &rows);
        assert!(
            lib.facets(&host, &Query::new(), Facet::Series, &mut input, &mut out)
                .is_err(),
            "{rows:?}"
        );
    }
    // More rows than a page holds.
    let many = (0..33).map(|n| facet("x", 1, n)).collect::<Vec<_>>();
    let mut rows = many;
    rows.push(end(1, 33, 5, 33));
    reply(&state, &rows);
    assert!(
        lib.facets(&host, &Query::new(), Facet::Series, &mut input, &mut out)
            .is_err()
    );
}
