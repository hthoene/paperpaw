use super::*;
use alloc::string::String;

fn sent(state: &State) -> wire::ServiceDocReport<'static> {
    let bytes = state.input.borrow().clone();
    let leaked: &'static [u8] = alloc::boxed::Box::leak(bytes.into_boxed_slice());
    let Some(Record::ServiceDocReport(row)) = wire::records_decode(leaked).unwrap().iter().next()
    else {
        panic!("a report")
    };
    row
}

#[test]
fn a_report_sends_every_part_once_and_the_reply_is_empty() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 1024], [0; 64]);
    reply(&state, &[]);
    let report = Report::new(DocPosition::new(3, 4000), 250_000)
        .insert(2)
        .title("A Title")
        .chapter("One")
        .pages(12, 300)
        .chapter_left(7);
    docs.report(&host, DocId(4), &report, &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_REPORT);
    assert_eq!(state.calls.get(), 1);
    let row = sent(&state);
    assert_eq!(
        (row.document, row.section, row.offset, row.insert),
        (4, 3, 4000, 2)
    );
    assert_eq!((row.progress_ppm, row.flags), (250_000, 0));
    assert_eq!((row.title, row.chapter), ("A Title", "One"));
    assert_eq!(
        (row.page, row.pages, row.chapter_left),
        (Some(12), Some(300), Some(7))
    );
}

#[test]
fn what_is_optional_is_absent_and_finished_is_flag_bit_0() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 1024], [0; 64]);
    reply(&state, &[]);
    let report = Report::new(DocPosition::new(0, 0), 1_000_000).finished();
    docs.report(&host, DocId(4), &report, &mut input, &mut out)
        .unwrap();
    let row = sent(&state);
    assert_eq!(row.flags, 1);
    assert_eq!(
        (row.page, row.pages, row.chapter_left, row.insert),
        (None, None, None, 0)
    );
    assert_eq!((row.title, row.chapter), ("", ""));
}

#[test]
fn the_bounds_are_the_contracts_and_nothing_is_sent_for_a_refused_report() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 1024], [0; 64]);
    reply(&state, &[]);
    let at = DocPosition::new(1, 1);
    let base = Report::new(at, 10);
    // Edges that are accepted: the last page with 0 left, 1,000,000, the longest texts.
    let long = String::from_utf8(alloc::vec![b'x'; 256]).unwrap();
    for ok in [
        Report::new(at, 1_000_000),
        base.chapter("C").pages(1, 1).chapter_left(0),
        base.chapter("C").pages(10, 10).chapter_left(0),
        base.chapter("C").pages(1, 10).chapter_left(9),
        base.title(&long).chapter(&long),
    ] {
        docs.report(&host, DocId(4), &ok, &mut input, &mut out)
            .unwrap();
    }
    let calls = state.calls.get();
    // Refused before anything is sent.
    for (bad, error) in [
        (Report::new(at, 1_000_001), Error::Invalid),
        (base.pages(0, 5), Error::Invalid),
        (base.pages(6, 5), Error::Invalid),
        (base.pages(5, 0), Error::Invalid),
        (
            base.chapter("C").pages(1, 10).chapter_left(10),
            Error::Invalid,
        ),
        (
            base.chapter("C").pages(10, 10).chapter_left(1),
            Error::Invalid,
        ),
        (base.chapter("C").chapter_left(0), Error::Invalid),
        // Pages left need a chapter that is named.
        (base.pages(1, 10).chapter_left(0), Error::Invalid),
    ] {
        assert_eq!(
            docs.report(&host, DocId(4), &bad, &mut input, &mut out),
            Err(error),
            "{bad:?}"
        );
    }
    let over = String::from_utf8(alloc::vec![b'x'; 257]).unwrap();
    for bad in [base.title(&over), base.chapter(&over)] {
        assert_eq!(
            docs.report(&host, DocId(4), &bad, &mut input, &mut out),
            Err(Error::NoSpace)
        );
    }
    assert_eq!(state.calls.get(), calls, "nothing was sent");
}

#[test]
fn a_page_without_its_count_is_refused_whichever_half_is_missing() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 1024], [0; 64]);
    let mut half = Report::new(DocPosition::new(1, 1), 10);
    half.page = Some(1);
    assert_eq!(
        docs.report(&host, DocId(4), &half, &mut input, &mut out),
        Err(Error::Invalid)
    );
    half.page = None;
    half.pages = Some(1);
    assert_eq!(
        docs.report(&host, DocId(4), &half, &mut input, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn the_hosts_refusals_and_a_reply_with_rows_are_told() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 1024], [0; 64]);
    let report = Report::new(DocPosition::new(1, 1), 10);
    for (status, error) in [
        (abi::ERR_INVALID, Error::Invalid),
        (abi::ERR_DENIED, Error::Denied),
        (abi::ERR_CANCELLED, Error::Cancelled),
        (abi::ERR_UTF8, Error::Utf8),
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.report(&host, DocId(4), &report, &mut input, &mut out),
            Err(error)
        );
    }
    state.error.set(0);
    reply(&state, &[doc_state(4, 1, 0)]);
    assert_eq!(
        docs.report(&host, DocId(4), &report, &mut input, &mut out),
        Err(Error::Invalid)
    );
}
