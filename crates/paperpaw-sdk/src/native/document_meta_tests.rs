use super::*;

fn meta(direction: u32, writing_mode: u32, flags: u32) -> Record<'static> {
    Record::ServiceDocMeta(wire::ServiceDocMeta {
        title: "A Title",
        language: "de-AT",
        series: "A Series",
        series_index: "2.5",
        direction,
        writing_mode,
        sections: 12,
        authors: 2,
        subjects: 3,
        description_bytes: 1500,
        flags,
    })
}
fn text(kind: u32, index: u32, total: u32, text: &'static str) -> Record<'static> {
    Record::ServiceDocText(wire::ServiceDocText {
        kind,
        index,
        total,
        text,
    })
}

#[test]
fn metadata_reads_every_field_borrowed_from_the_reply() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
    reply(&state, &[meta(1, 2, 0b11)]);
    let range = out.as_ptr_range();
    let m = docs
        .metadata(&host, DocId(4), &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_META);
    assert_eq!(
        (m.title(), m.language(), m.series(), m.series_index()),
        ("A Title", "de-AT", "A Series", "2.5")
    );
    assert!(range.contains(&m.title().as_ptr()));
    assert_eq!(
        (
            m.sections(),
            m.authors(),
            m.subjects(),
            m.description_bytes()
        ),
        (12, 2, 3, 1500)
    );
    assert!(m.right_to_left() && m.title_cut() && m.series_cut());
    assert_eq!(m.writing_mode(), 2);
    let bytes = state.input.borrow();
    assert!(matches!(
        wire::records_decode(&bytes).unwrap().iter().next(),
        Some(Record::ServiceDocRef(r)) if r.document == 4
    ));
    drop(bytes);
    reply(&state, &[meta(0, 0, 0)]);
    let m = docs
        .metadata(&host, DocId(4), &mut input, &mut out)
        .unwrap();
    assert!(!m.right_to_left() && !m.title_cut() && !m.series_cut());
    // A later writing mode reads as horizontal, never an error.
    reply(&state, &[meta(0, 9, 0)]);
    assert_eq!(
        docs.metadata(&host, DocId(4), &mut input, &mut out)
            .unwrap()
            .writing_mode(),
        0
    );
}

#[test]
fn metadata_refuses_what_the_contract_does_not_name_and_passes_host_refusals() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
    reply(&state, &[meta(2, 0, 0)]);
    assert_eq!(
        docs.metadata(&host, DocId(4), &mut input, &mut out),
        Err(Error::Invalid)
    );
    // A flag bit a newer host sets is ignored; the bits this SDK knows still read.
    reply(&state, &[meta(0, 0, 0b101)]);
    let m = docs
        .metadata(&host, DocId(4), &mut input, &mut out)
        .unwrap();
    assert!(m.title_cut() && !m.series_cut());
    reply(&state, &[]);
    assert_eq!(
        docs.metadata(&host, DocId(4), &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[meta(0, 0, 0), meta(0, 0, 0)]);
    assert_eq!(
        docs.metadata(&host, DocId(4), &mut input, &mut out),
        Err(Error::Invalid)
    );
    for (status, error) in [
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_CANCELLED, Error::Cancelled),
        (abi::ERR_NOT_FOUND, Error::NotFound),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.metadata(&host, DocId(4), &mut input, &mut out),
            Err(error)
        );
    }
}

#[test]
fn each_text_kind_is_asked_by_index_and_offset() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
    for (query, kind, index, body) in [
        (DocTextQuery::title(), 0, 0, "A Title"),
        (DocTextQuery::author(1), 1, 1, "Second Author"),
        (DocTextQuery::subject(2), 2, 2, "Fantasy"),
        (DocTextQuery::description(), 3, 0, "About the book"),
        (DocTextQuery::series(), 4, 0, "A Series"),
    ] {
        let total = u32::try_from(body.len()).unwrap();
        reply(&state, &[text(kind, index, total, body)]);
        let part = docs
            .text(&host, DocId(4), query, &mut input, &mut out)
            .unwrap();
        assert_eq!(state.op.get(), schema::OP_META_TEXT);
        assert_eq!(
            (part.text(), part.total(), part.next_offset()),
            (body, total, None)
        );
        let bytes = state.input.borrow();
        assert!(matches!(
            wire::records_decode(&bytes).unwrap().iter().next(),
            Some(Record::ServiceDocTextAt(r)) if (r.document, r.kind, r.index, r.offset) == (4, kind, index, 0)
        ));
    }
}

#[test]
fn a_long_description_is_read_in_parts_cut_at_characters() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
    let head: &'static str = alloc::boxed::Box::leak("a".repeat(1021).into_boxed_str());
    reply(&state, &[text(3, 0, 1025, head)]);
    let part = docs
        .text(
            &host,
            DocId(4),
            DocTextQuery::description(),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!((part.text().len(), part.next_offset()), (1021, Some(1021)));
    reply(&state, &[text(3, 0, 1025, "😀")]);
    let part = docs
        .text(
            &host,
            DocId(4),
            DocTextQuery::description().from(1021),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!((part.text(), part.next_offset()), ("😀", None));
}

#[test]
fn a_text_that_is_not_the_one_asked_for_or_is_cut_wrongly_is_invalid() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
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
            docs.text(
                &host,
                DocId(4),
                DocTextQuery::title().from(offset),
                &mut input,
                &mut out
            ),
            Err(Error::Invalid),
            "{kind} {index} {total} {offset}"
        );
    }
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_BUSY, Error::Busy),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.text(
                &host,
                DocId(4),
                DocTextQuery::author(9),
                &mut input,
                &mut out
            ),
            Err(error)
        );
    }
    // A request that does not fit the input buffer never reaches the host.
    state.error.set(0);
    let calls = state.calls.get();
    assert!(
        docs.text(
            &host,
            DocId(4),
            DocTextQuery::title(),
            &mut [0; 4],
            &mut out
        )
        .is_err()
    );
    assert_eq!(state.calls.get(), calls);
}

#[test]
fn the_edges_of_a_part_offset_at_the_end_and_a_shortfall_of_three_or_four_bytes() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
    let ask = |query: DocTextQuery, input: &mut [u8; 64], out: &mut [u8; 4096]| {
        docs.text(&host, DocId(4), query, input, out)
            .map(|part| (part.text().len(), part.next_offset()))
    };
    // An offset at the end of the text answers an empty part and no next.
    reply(&state, &[text(3, 0, 5, "")]);
    assert_eq!(
        ask(DocTextQuery::description().from(5), &mut input, &mut out),
        Ok((0, None))
    );
    // ... and a part with text there, or an offset past the end, is refused.
    reply(&state, &[text(3, 0, 5, "a")]);
    assert_eq!(
        ask(DocTextQuery::description().from(5), &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[text(3, 0, 5, "")]);
    assert_eq!(
        ask(DocTextQuery::description().from(6), &mut input, &mut out),
        Err(Error::Invalid)
    );
    // A part may stop up to three bytes short of the cap, not four.
    let short3: &'static str = alloc::boxed::Box::leak("a".repeat(1021).into_boxed_str());
    reply(&state, &[text(3, 0, 2000, short3)]);
    assert_eq!(
        ask(DocTextQuery::description(), &mut input, &mut out),
        Ok((1021, Some(1021)))
    );
    let short4: &'static str = alloc::boxed::Box::leak("a".repeat(1020).into_boxed_str());
    reply(&state, &[text(3, 0, 2000, short4)]);
    assert_eq!(
        ask(DocTextQuery::description(), &mut input, &mut out),
        Err(Error::Invalid)
    );
    // The host's "no room" comes back as it is.
    state.error.set(abi::ERR_NO_SPACE);
    assert_eq!(
        ask(DocTextQuery::description(), &mut input, &mut out),
        Err(Error::NoSpace)
    );
}
