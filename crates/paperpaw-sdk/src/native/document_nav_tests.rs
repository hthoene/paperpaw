//! The table of contents, the links and the block stream of an open document.
#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use crate::document::{Align, BlockKind, VerticalAlign};
use crate::native::declare::records_wire::Record;
use alloc::{vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_lang::{Direction, WritingMode};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct State {
    op: Cell<u32>,
    calls: Cell<u32>,
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    error: Cell<i32>,
}
unsafe extern "C" fn open(_: *mut c_void, _: PpBytes, _: u16, _: u16, out: *mut u32) -> i32 {
    // SAFETY: SDK provides an aligned output.
    unsafe { out.write(7) };
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.op.set(op);
    state.calls.set(state.calls.get() + 1);
    // SAFETY: SDK encoded a live input slice.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let reply = state.reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: the fixture reply and exclusive SDK output are disjoint.
    unsafe { core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len()) };
    i32::try_from(reply.len()).unwrap()
}
fn table() -> PpHost {
    let mut t = abi::host_table();
    t.service_open = open;
    t.service_call = call;
    t
}
fn reply(state: &State, rows: &[Record<'_>]) {
    let mut bytes = vec![0; 4096];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    state.reply.replace(bytes);
}
macro_rules! setup {
    ($table:ident, $state:ident, $host:ident, $docs:ident) => {
        let $table = table();
        let mut $state = State::default();
        let $host = Host {
            table: &$table,
            ctx: core::ptr::from_mut(&mut $state).cast(),
        };
        let $docs = Documents::open(&$host).unwrap();
    };
}
const DOC: DocId = DocId(4);

fn end(flags: u32, total: u32, generation: u32, next: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags,
        total,
        generation,
        next,
        cursor: None,
    })
}
fn toc(index: u32, level: u32, flags: u32, title: &str) -> Record<'_> {
    Record::ServiceDocToc(wire::ServiceDocToc {
        index,
        level,
        section: 2,
        offset: 10 * index,
        flags,
        title,
    })
}
fn section(index: u32, length: u32, flags: u32) -> Record<'static> {
    Record::ServiceDocSection(wire::ServiceDocSection {
        index,
        length,
        flags,
    })
}
fn link(start: u32, end: u32, kind: u32, href: &str) -> Record<'_> {
    Record::ServiceDocLink(wire::ServiceDocLink {
        start,
        end,
        kind,
        href,
    })
}
fn at(section: u32, offset: u32) -> Record<'static> {
    Record::ServiceDocAt(wire::ServiceDocAt {
        document: 4,
        section,
        offset,
        insert: 0,
        cursor: 0,
        generation: 0,
    })
}

#[test]
fn sections_read_as_lengths_page_by_cursor_and_refuse_a_bad_page() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(
        &state,
        &[
            section(0, 1_200, 0),
            section(1, u32::MAX, 1),
            section(2, 0, 0b110),
            end(0, 4, 5, 3),
        ],
    );
    let page = docs
        .sections(&host, DOC, DocCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_SECTIONS);
    let rows: Vec<_> = page.rows().collect();
    assert_eq!(page.len(), 3);
    assert_eq!(
        rows.iter()
            .map(|r| (r.index(), r.length(), r.whole_page_picture()))
            .collect::<Vec<_>>(),
        [(0, 1_200, false), (1, u32::MAX, true), (2, 0, false)]
    );
    assert_eq!(page.end().total(), 4);
    let next = page.end().next().unwrap();
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceDocPage(p))
                if (p.document, p.scope, p.cursor, p.generation) == (4, "", 0, 0)
        ));
    }
    // The cursor goes back with the end's generation; another one is stale.
    reply(&state, &[section(3, 7, 0), end(1, 4, 6, 4)]);
    assert_eq!(
        docs.sections(&host, DOC, next, &mut input, &mut out)
            .map(|p| p.len()),
        Err(Error::State)
    );
    reply(&state, &[section(3, 7, 0), end(1, 4, 5, 4)]);
    let page = docs
        .sections(&host, DOC, next, &mut input, &mut out)
        .unwrap();
    assert!(page.end().next().is_none());
    // A row that is not a section, a page that does not follow, a last page that
    // is not the last, an empty page that is not the last and unknown end flags.
    let bad: [Vec<Record<'_>>; 5] = [
        vec![toc(3, 0, 0, "x"), end(1, 4, 5, 4)],
        vec![section(3, 7, 0), end(1, 4, 5, 5)],
        vec![section(3, 7, 0), end(0, 4, 5, 4)],
        vec![end(0, 4, 5, 3)],
        vec![section(3, 7, 0), end(2, 4, 5, 4)],
    ];
    for rows in bad {
        reply(&state, &rows);
        assert_eq!(
            docs.sections(&host, DOC, next, &mut input, &mut out)
                .map(|p| p.len()),
            Err(Error::Invalid)
        );
    }
    state.error.set(abi::ERR_BUSY);
    assert_eq!(
        docs.sections(&host, DOC, next, &mut input, &mut out)
            .map(|p| p.len()),
        Err(Error::Busy)
    );
}

#[test]
fn the_toc_pages_by_cursor_and_its_rows_read_as_typed_entries() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(
        &state,
        &[
            toc(0, 0, 0, "One"),
            toc(1, 15, 0b11, "Two"),
            end(0, 3, 9, 2),
        ],
    );
    let page = docs
        .toc(&host, DOC, DocCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_TOC);
    let rows: Vec<_> = page.rows().collect();
    assert_eq!(page.len(), 2);
    assert_eq!(
        (rows[0].index(), rows[0].level(), rows[0].title()),
        (0, 0, "One")
    );
    assert_eq!(rows[0].position(), DocPosition::new(2, 0));
    assert!(!rows[0].approximate() && !rows[0].title_cut());
    assert_eq!(rows[1].level(), 15);
    assert!(rows[1].approximate() && rows[1].title_cut());
    assert_eq!(page.end().total(), 3);
    let next = page.end().next().unwrap();
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceDocPage(p)) if (p.document, p.scope, p.cursor, p.generation) == (4, "", 0, 0)
        ));
    }
    // The cursor goes back with the end's generation; another one is stale.
    reply(&state, &[toc(2, 1, 0, "Three"), end(1, 3, 8, 3)]);
    assert_eq!(
        docs.toc(&host, DOC, next, &mut input, &mut out)
            .unwrap_err(),
        Error::State
    );
    reply(&state, &[toc(2, 1, 0, "Three"), end(1, 3, 9, 3)]);
    let page = docs.toc(&host, DOC, next, &mut input, &mut out).unwrap();
    assert!(page.end().next().is_none());
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceDocPage(p)) if (p.cursor, p.generation) == (2, 9)
    ));
}

#[test]
fn a_malformed_toc_or_links_page_never_exposes_a_row() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    let pages: [Vec<Record<'_>>; 7] = [
        vec![toc(0, 16, 0, "deep"), end(1, 1, 9, 1)],
        vec![toc(0, 0, 0, "a")],
        vec![toc(0, 0, 0, "a"), end(1, 1, 0, 1)],
        vec![toc(0, 0, 0, "a"), end(1, 1, 9, 2)],
        vec![toc(0, 0, 0, "a"), end(0, 1, 9, 1)],
        vec![end(0, 3, 9, 0)],
        vec![toc(0, 0, 0, "a"), end(1, 1, 9, 1), toc(1, 0, 0, "b")],
    ];
    for rows in pages {
        reply(&state, &rows);
        assert_eq!(
            docs.toc(&host, DOC, DocCursor::start(), &mut input, &mut out)
                .map(|page| page.len())
                .unwrap_err(),
            Error::Invalid
        );
    }
    // More rows than a page holds.
    let mut rows = vec![toc(0, 0, 0, "a"); schema::PAGE_MAX_ROWS + 1];
    rows.push(end(1, 65, 9, 65));
    reply(&state, &rows);
    assert!(
        docs.toc(&host, DOC, DocCursor::start(), &mut input, &mut out)
            .is_err()
    );
    // A link that ends before it starts, and a reply that is not a link.
    reply(&state, &[link(5, 4, 0, "#a"), end(1, 1, 9, 1)]);
    assert!(
        docs.links(
            &host,
            DOC,
            DocSpan::new(0, 0..10),
            DocCursor::start(),
            &mut input,
            &mut out
        )
        .is_err()
    );
    reply(&state, &[toc(0, 0, 0, "a"), end(1, 1, 9, 1)]);
    assert!(
        docs.links(
            &host,
            DOC,
            DocSpan::new(0, 0..10),
            DocCursor::start(),
            &mut input,
            &mut out
        )
        .is_err()
    );
}

#[test]
fn a_toc_entrys_target_is_its_document_position_and_refusals_are_the_status() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(&state, &[at(3, 77)]);
    assert_eq!(
        docs.toc_target(&host, DOC, 5, &mut input, &mut out)
            .unwrap(),
        DocPosition::new(3, 77)
    );
    assert_eq!(state.op.get(), schema::OP_TOC_TARGET);
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceDocIndex(i)) if (i.document, i.index) == (4, 5)
        ));
    }
    // Another document's answer, a malformed one, and the host's refusals.
    reply(
        &state,
        &[Record::ServiceDocAt(wire::ServiceDocAt {
            document: 5,
            section: 0,
            offset: 0,
            insert: 0,
            cursor: 0,
            generation: 0,
        })],
    );
    assert_eq!(
        docs.toc_target(&host, DOC, 5, &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[]);
    assert_eq!(
        docs.toc_target(&host, DOC, 5, &mut input, &mut out),
        Err(Error::Invalid)
    );
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_DENIED, Error::Denied),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.toc_target(&host, DOC, 5, &mut input, &mut out),
            Err(error)
        );
    }
}

#[test]
fn links_of_a_span_read_as_typed_links_and_a_bad_span_sends_nothing() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(
        &state,
        &[
            link(0, 4, 0, "ch2.xhtml#a"),
            link(5, 9, 1, "https://x.test/"),
            link(10, 12, 2, "#n1"),
            link(13, 14, 9, "#o"),
            end(1, 4, 6, 4),
        ],
    );
    let page = docs
        .links(
            &host,
            DOC,
            DocSpan::new(1, 0..100),
            DocCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_LINKS);
    let rows: Vec<_> = page.rows().collect();
    assert_eq!(
        rows.iter().map(Link::kind).collect::<Vec<_>>(),
        [
            LinkKind::Internal,
            LinkKind::External,
            LinkKind::Footnote,
            LinkKind::Other(9)
        ]
    );
    assert_eq!(
        (rows[0].start(), rows[0].end(), rows[0].href()),
        (0, 4, "ch2.xhtml#a")
    );
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceDocSpan(s))
            if (s.document, s.section, s.from, s.to, s.cursor, s.generation) == (4, 1, 0, 100, 0, 0)
    ));
    drop(bytes);
    let calls = state.calls.get();
    let (from, to) = (9, 3);
    assert_eq!(
        docs.links(
            &host,
            DOC,
            DocSpan::new(1, from..to),
            DocCursor::start(),
            &mut input,
            &mut out
        )
        .map(|page| page.len()),
        Err(Error::Invalid)
    );
    assert_eq!(state.calls.get(), calls);
}

#[test]
fn a_link_target_is_a_position_and_an_overlong_href_is_refused_before_the_call() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 2048], [0; 4096]);
    reply(&state, &[at(1, 2)]);
    assert_eq!(
        docs.link_target(&host, DOC, "ch2.xhtml#a", &mut input, &mut out)
            .unwrap(),
        DocPosition::new(1, 2)
    );
    assert_eq!(state.op.get(), schema::OP_LINK_TARGET);
    {
        let bytes = state.input.borrow();
        let sent = wire::records_decode(&bytes).unwrap();
        assert!(matches!(
            sent.iter().next(),
            Some(Record::ServiceDocHref(h)) if (h.document, h.href) == (4, "ch2.xhtml#a")
        ));
    }
    let calls = state.calls.get();
    let long = "h".repeat(1025);
    assert_eq!(
        docs.link_target(&host, DOC, &long, &mut input, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(state.calls.get(), calls);
    state.error.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        docs.link_target(&host, DOC, "https://x.test/", &mut input, &mut out),
        Err(Error::NotFound)
    );
}

fn head(kind: u32, flags: u32) -> Record<'static> {
    Record::ServiceDocBlock(wire::ServiceDocBlock {
        kind,
        level: 2,
        flags,
        start: 100,
        end: 160,
        align: 4,
        indent: i32::MIN,
        space: u32::MAX,
        size: 120,
        margin_start: 1,
        margin_end: 2,
        line_height: u32::MAX,
        dir: 2,
        writing_mode: 9,
        lang: "de-CH",
        drop_cap: 3,
        text_bytes: 11,
    })
}
fn text(part: &str) -> Record<'_> {
    Record::ServiceDocTextPart(wire::ServiceDocTextPart { text: part })
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one fragment walks every record kind in wire order"
)]
fn a_whole_block_in_one_fragment_reads_as_typed_records_in_order() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(
        &state,
        &[
            head(1, 0b10),
            text("Hello "),
            text("world"),
            Record::ServiceDocRun(wire::ServiceDocRun {
                start: 0,
                end: 5,
                style: 1,
            }),
            Record::ServiceDocMap(wire::ServiceDocMap {
                text: 0,
                source: 100,
            }),
            Record::ServiceDocImage(wire::ServiceDocImage {
                image: 8,
                index: 3,
                width: 40,
                height: 30,
                flags: 0b11,
            }),
            Record::ServiceDocColumns(wire::ServiceDocColumns {
                weights: &[1, 0, 2, 0],
            }),
            Record::ServiceDocCell(wire::ServiceDocCell {
                end: 6,
                span: 2,
                rows: 1,
                align: 2,
                vertical: 1,
                width: 3,
            }),
            Record::ServiceDocRuby(wire::ServiceDocRuby {
                start: 0,
                end: 5,
                kind: 7,
                size: 50,
                text: "ruby",
            }),
            Record::ServiceDocInlineLang(wire::ServiceDocInlineLang { lang: "fr" }),
            end(1, 0, 0, 160),
        ],
    );
    let fragment = docs
        .block_at(
            &host,
            DOC,
            DocPosition::new(2, 100),
            BlockCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(state.op.get(), schema::OP_BLOCK);
    assert!(fragment.is_first() && fragment.continuation().is_none());
    assert_eq!(fragment.len(), 10);
    assert!(fragment.last_in_section());
    assert_eq!(fragment.next_block(), Some(160));
    let records: Vec<_> = fragment.records().collect();
    let BlockRecord::Head(head) = records[0] else {
        panic!("no header first");
    };
    assert_eq!(head.kind(), BlockKind::Heading(2));
    assert!(head.continued() && !head.continues());
    assert_eq!((head.start(), head.end()), (100, 160));
    assert_eq!(head.align(), Align::Justify);
    assert_eq!(head.indent(), None);
    assert_eq!(
        (head.space(), head.size(), head.line_height()),
        (None, Some(120), None)
    );
    assert_eq!(head.margins(), (1, 2));
    assert_eq!(head.direction(), Direction::Rtl);
    assert_eq!(
        head.writing_mode(),
        WritingMode::HorizontalTb,
        "unknown reads as horizontal"
    );
    assert_eq!(
        (head.lang(), head.drop_cap(), head.text_bytes()),
        ("de-CH", 3, 11)
    );
    assert_eq!(records[1], BlockRecord::Text("Hello "));
    assert_eq!(records[2], BlockRecord::Text("world"));
    let BlockRecord::Run(run) = records[3] else {
        panic!()
    };
    assert_eq!((run.start(), run.end(), run.style().bits()), (0, 5, 1));
    let BlockRecord::Map(map) = records[4] else {
        panic!()
    };
    assert_eq!((map.text(), map.source()), (0, 100));
    let BlockRecord::Image(image) = records[5] else {
        panic!()
    };
    assert_eq!(
        (image.image(), image.index(), image.width(), image.height()),
        (8, 3, 40, 30)
    );
    assert!(image.colour() && image.pending());
    let BlockRecord::Columns(columns) = records[6] else {
        panic!("no columns");
    };
    assert_eq!(columns.len(), 2);
    assert_eq!(columns.weights().collect::<Vec<_>>(), [1, 2]);
    let BlockRecord::Cell(cell) = records[7] else {
        panic!()
    };
    assert_eq!(
        (cell.end(), cell.span(), cell.rows(), cell.width()),
        (6, 2, 1, 3)
    );
    assert_eq!(
        (cell.align(), cell.vertical()),
        (Align::Center, VerticalAlign::Middle)
    );
    let BlockRecord::Ruby(ruby) = records[8] else {
        panic!()
    };
    assert_eq!(ruby.kind(), crate::document::AnnotationKind::Other(4));
    assert_eq!(
        (ruby.start(), ruby.end(), ruby.size(), ruby.text()),
        (0, 5, 50, "ruby")
    );
    assert_eq!(records[9], BlockRecord::InlineLang("fr"));
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceDocAt(a))
            if (a.document, a.section, a.offset, a.insert, a.cursor, a.generation) == (4, 2, 100, 0, 0, 0)
    ));
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one block walks both fragments and every refusal"
)]
fn a_block_in_two_fragments_continues_under_its_identity_and_the_header_comes_once() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    reply(&state, &[head(0, 0), text("abc"), end(2, 0, 77, 2)]);
    let first = docs
        .block_at(
            &host,
            DOC,
            DocPosition::new(0, 0),
            BlockCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    let cursor = first.continuation().unwrap();
    assert!(!first.last_in_section() && first.next_block().is_none());
    // The second fragment: no header, the same identity, ends the block.
    reply(&state, &[text("def"), end(1, 0, 77, 9)]);
    let second = docs
        .block_at(
            &host,
            DOC,
            DocPosition::new(0, 0),
            cursor,
            &mut input,
            &mut out,
        )
        .unwrap();
    assert!(!second.is_first() && second.continuation().is_none());
    assert_eq!(second.next_block(), Some(9));
    assert!(second.last_in_section());
    let bytes = state.input.borrow();
    let sent = wire::records_decode(&bytes).unwrap();
    assert!(matches!(
        sent.iter().next(),
        Some(Record::ServiceDocAt(a)) if (a.cursor, a.generation) == (2, 77)
    ));
    drop(bytes);
    // A changed identity, on a fragment that goes on or on the last one, is State
    // like a page's: the reader starts the block again.
    for rows in [
        vec![text("x"), end(2, 0, 76, 3)],
        vec![text("x"), end(1, 0, 76, 3)],
    ] {
        reply(&state, &rows);
        assert_eq!(
            docs.block_at(
                &host,
                DOC,
                DocPosition::new(0, 0),
                cursor,
                &mut input,
                &mut out
            )
            .map(|f| f.len())
            .unwrap_err(),
            Error::State
        );
    }
    // A header in a continuation, none in a first fragment, a cursor that does not
    // follow, both end flags, an empty "more", an unknown record and trailing
    // bytes: Invalid, and the host's State is the status.
    let bad: [Vec<Record<'_>>; 7] = [
        vec![head(0, 0), end(1, 0, 0, 1)],
        vec![text("x"), end(2, 0, 77, 4)],
        vec![text("x"), end(3, 0, 77, 3)],
        vec![end(2, 0, 77, 2)],
        vec![
            Record::ServiceDocReader(wire::ServiceDocReader {
                id: "a.b",
                screen: "s",
                formats: "",
                title: "t",
            }),
            end(1, 0, 0, 3),
        ],
        vec![text("x"), end(4, 0, 0, 3)],
        vec![text("x"), end(1, 0, 0, 3), text("y")],
    ];
    for rows in bad {
        reply(&state, &rows);
        assert_eq!(
            docs.block_at(
                &host,
                DOC,
                DocPosition::new(0, 0),
                cursor,
                &mut input,
                &mut out
            )
            .map(|f| f.len())
            .unwrap_err(),
            Error::Invalid
        );
    }
    state.error.set(abi::ERR_STATE);
    assert_eq!(
        docs.block_at(
            &host,
            DOC,
            DocPosition::new(0, 0),
            cursor,
            &mut input,
            &mut out
        )
        .map(|f| f.len()),
        Err(Error::State)
    );
}

#[test]
fn a_first_fragment_must_open_with_exactly_one_header_and_host_statuses_pass_through() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    for rows in [
        vec![text("x"), end(1, 0, 0, 1)],
        vec![head(0, 0), head(0, 0), end(1, 0, 0, 1)],
        vec![end(1, 0, 0, 1)],
    ] {
        reply(&state, &rows);
        assert_eq!(
            docs.block_at(
                &host,
                DOC,
                DocPosition::new(0, 0),
                BlockCursor::start(),
                &mut input,
                &mut out
            )
            .map(|f| f.len())
            .unwrap_err(),
            Error::Invalid
        );
    }
    // Odd or too many column weights, and a kind that does not fit.
    reply(
        &state,
        &[
            head(0, 0),
            Record::ServiceDocColumns(wire::ServiceDocColumns {
                weights: &[1, 0, 2],
            }),
            end(1, 0, 0, 1),
        ],
    );
    assert!(
        docs.block_at(
            &host,
            DOC,
            DocPosition::new(0, 0),
            BlockCursor::start(),
            &mut input,
            &mut out
        )
        .is_err()
    );
    reply(&state, &[head(5 + 70_000, 0), end(1, 0, 0, 1)]);
    let fragment = docs
        .block_at(
            &host,
            DOC,
            DocPosition::new(0, 0),
            BlockCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    let Some(BlockRecord::Head(h)) = fragment.records().next() else {
        panic!()
    };
    assert_eq!(h.kind(), BlockKind::Paragraph);
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_DENIED, Error::Denied),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.block_at(
                &host,
                DOC,
                DocPosition::new(9, 0),
                BlockCursor::start(),
                &mut input,
                &mut out
            )
            .map(|f| f.len()),
            Err(error)
        );
    }
}

type Reader = fn(&Documents, &Host<'_>, &mut [u8], &mut [u8]);
type Case<'a> = (Vec<Record<'a>>, Reader);

/// Reads everything a reply exposes, so a bad byte that got through is a panic here.
fn read_block(docs: &Documents, host: &Host<'_>, input: &mut [u8], out: &mut [u8]) {
    let at = DocPosition::new(2, 100);
    if let Ok(fragment) = docs.block_at(host, DOC, at, BlockCursor::start(), input, out) {
        for record in fragment.records() {
            match record {
                BlockRecord::Head(head) => {
                    let _ = (head.kind(), head.start(), head.end(), head.text_bytes());
                }
                BlockRecord::Columns(columns) => {
                    assert_eq!(columns.weights().count(), columns.len());
                }
                _ => {}
            }
        }
        let _ = (fragment.continuation(), fragment.next_block());
    }
}
fn read_sections(docs: &Documents, host: &Host<'_>, input: &mut [u8], out: &mut [u8]) {
    if let Ok(page) = docs.sections(host, DOC, DocCursor::start(), input, out) {
        for row in page.rows() {
            let _ = (row.index(), row.length(), row.whole_page_picture());
        }
        let _ = page.end();
    }
}
fn read_toc(docs: &Documents, host: &Host<'_>, input: &mut [u8], out: &mut [u8]) {
    if let Ok(page) = docs.toc(host, DOC, DocCursor::start(), input, out) {
        for row in page.rows() {
            let _ = (row.index(), row.level(), row.title(), row.position());
        }
        let _ = page.end();
    }
}

#[test]
fn every_truncation_and_byte_change_of_a_valid_reply_is_an_error_or_a_valid_view() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 256], [0; 4096]);
    let sets: [Case<'_>; 3] = [
        (
            vec![
                head(1, 0b10),
                text("Hello"),
                Record::ServiceDocRun(wire::ServiceDocRun {
                    start: 0,
                    end: 5,
                    style: 1,
                }),
                Record::ServiceDocColumns(wire::ServiceDocColumns {
                    weights: &[1, 0, 2, 0],
                }),
                Record::ServiceDocRuby(wire::ServiceDocRuby {
                    start: 0,
                    end: 5,
                    kind: 7,
                    size: 50,
                    text: "ruby",
                }),
                Record::ServiceDocInlineLang(wire::ServiceDocInlineLang { lang: "fr" }),
                end(1, 0, 0, 160),
            ],
            read_block,
        ),
        (
            vec![toc(0, 0, 0, "One"), toc(1, 1, 1, "Two"), end(1, 2, 8, 2)],
            read_toc,
        ),
        (
            vec![section(0, 900, 0), section(1, 70, 1), end(1, 2, 8, 2)],
            read_sections,
        ),
    ];
    for (rows, read) in sets {
        reply(&state, &rows);
        let valid = state.reply.borrow().clone();
        read(&docs, &host, &mut input, &mut out);
        for cut in 0..valid.len() {
            state.reply.replace(valid[..cut].to_vec());
            read(&docs, &host, &mut input, &mut out);
        }
        for at in 0..valid.len() {
            for change in [0x00, 0x01, 0x7f, 0x80, 0xff] {
                let mut bytes = valid.clone();
                bytes[at] ^= change;
                state.reply.replace(bytes);
                read(&docs, &host, &mut input, &mut out);
            }
        }
    }
}
