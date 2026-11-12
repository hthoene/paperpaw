#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct State {
    op: Cell<u32>,
    calls: Cell<u32>,
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    error: Cell<i32>,
}
unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: SDK provides a live key and aligned output.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(key.ptr, key.len),
            b"core.document"
        );
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
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
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len());
    }
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
fn doc_state(document: u32, state: u32, flags: u32) -> Record<'static> {
    Record::ServiceDocState(wire::ServiceDocState {
        document,
        state,
        progress_ppm: 400_000,
        status: -5,
        flags,
        conversion: 0xabcdef,
        format: "format-epub.epub",
    })
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

#[test]
fn open_sends_the_book_then_its_options_in_order() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(&state, &[doc_state(4, 1, 0b1_0110)]);
    let options = [("format-epub.css", "on"), ("reading.size", "120")];
    let book = OpenBook::new("/books/a.epub")
        .format("format-epub.epub")
        .publisher_styles()
        .color()
        .image_side(800)
        .cover_side(300)
        .css(Root::Data, "user.css")
        .options(&options);
    let st = docs.open_book(&host, &book, &mut input, &mut out).unwrap();
    assert_eq!(state.op.get(), schema::OP_OPEN);
    assert_eq!(st.id(), DocId(4));
    assert_eq!(st.preparation(), Preparation::Preparing);
    assert_eq!(
        (st.progress_ppm(), st.status(), st.conversion()),
        (400_000, -5, 0xabcdef)
    );
    assert_eq!(st.format(), "format-epub.epub");
    assert!(st.has_toc() && st.has_xpointers() && st.has_page_labels());
    assert!(!st.whole_pages() && !st.has_fonts() && !st.right_to_left() && !st.cache_made_now());
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    let mut it = rows.iter();
    assert!(matches!(
        it.next(),
        Some(Record::ServiceDocOpen(o)) if o.root == 1 && o.path == "/books/a.epub"
            && o.format == "format-epub.epub" && o.flags == 0b11 && o.image_side == 800
            && o.cover_side == 300 && o.css_root == 0 && o.css_path == "user.css"
    ));
    assert!(
        matches!(it.next(), Some(Record::ServiceDocOption(o)) if (o.key, o.value) == ("format-epub.css", "on"))
    );
    assert!(
        matches!(it.next(), Some(Record::ServiceDocOption(o)) if (o.key, o.value) == ("reading.size", "120"))
    );
    assert!(it.next().is_none());
}

#[test]
fn a_book_with_no_options_or_css_sends_one_row_and_defaults() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(&state, &[doc_state(1, 0, 0)]);
    let st = docs
        .open_book(
            &host,
            &OpenBook::new("a.txt").root(Root::Handed),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(st.preparation(), Preparation::Ready);
    let bytes = state.input.borrow();
    let rows = wire::records_decode(&bytes).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows.iter().next(),
        Some(Record::ServiceDocOpen(o)) if o.root == 5 && o.format.is_empty() && o.flags == 0
            && (o.image_side, o.cover_side, o.css_root) == (0, 0, 0) && o.css_path.is_empty()
    ));
}

#[test]
fn bad_opens_are_refused_before_the_host_is_called() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(&state, &[doc_state(1, 0, 0)]);
    let long = "p".repeat(513);
    let format = "f".repeat(129);
    let many: Vec<(&str, &str)> = (0..13).map(|_| ("a.b", "c")).collect();
    let key = "k".repeat(65);
    let value = "v".repeat(129);
    let big_options = [(key.as_str(), "v")];
    let big_values = [("k", value.as_str())];
    let cases: [(OpenBook<'_>, Error); 8] = [
        (OpenBook::new(""), Error::Invalid),
        (OpenBook::new(&long), Error::Invalid),
        (OpenBook::new("a").format(&format), Error::Invalid),
        (OpenBook::new("a").image_side(1601), Error::Invalid),
        (
            OpenBook::new("a").css(Root::Handed, "x.css"),
            Error::Invalid,
        ),
        (OpenBook::new("a").options(&many), Error::NoSpace),
        (OpenBook::new("a").options(&big_options), Error::NoSpace),
        (OpenBook::new("a").options(&big_values), Error::NoSpace),
    ];
    for (book, error) in cases {
        assert_eq!(
            docs.open_book(&host, &book, &mut input, &mut out),
            Err(error)
        );
    }
    assert_eq!(state.calls.get(), 0);
    // 12 options and 1,600 are the limits and go through.
    let twelve: Vec<(&str, &str)> = (0..12).map(|_| ("a.b", "c")).collect();
    assert!(
        docs.open_book(
            &host,
            &OpenBook::new("a").options(&twelve).image_side(1600),
            &mut input,
            &mut out
        )
        .is_ok()
    );
}

#[test]
fn the_hosts_refusals_pass_through_and_a_zero_id_is_malformed() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    for (status, error) in [
        (abi::ERR_FULL, Error::Full),
        (abi::ERR_DENIED, Error::Denied),
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.open_book(&host, &OpenBook::new("a"), &mut input, &mut out),
            Err(error)
        );
    }
    state.error.set(0);
    reply(&state, &[doc_state(0, 0, 0)]);
    assert_eq!(
        docs.open_book(&host, &OpenBook::new("a"), &mut input, &mut out),
        Err(Error::Invalid)
    );
    reply(&state, &[]);
    assert_eq!(
        docs.open_book(&host, &OpenBook::new("a"), &mut input, &mut out),
        Err(Error::Invalid)
    );
}

#[test]
fn state_asks_by_id_and_checks_the_answer_is_for_it() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(&state, &[doc_state(4, 2, 0)]);
    let st = docs.state(&host, DocId(4), &mut input, &mut out).unwrap();
    assert_eq!(state.op.get(), schema::OP_STATE);
    assert_eq!((st.preparation(), st.status()), (Preparation::Failed, -5));
    let bytes = state.input.borrow();
    assert!(matches!(
        wire::records_decode(&bytes).unwrap().iter().next(),
        Some(Record::ServiceDocRef(r)) if r.document == 4
    ));
    drop(bytes);
    assert_eq!(
        docs.state(&host, DocId(9), &mut input, &mut out),
        Err(Error::Invalid)
    );
    state.error.set(abi::ERR_CANCELLED);
    assert_eq!(
        docs.state(&host, DocId(4), &mut input, &mut out),
        Err(Error::Cancelled)
    );
}

#[test]
fn close_expects_an_empty_reply() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(&state, &[]);
    docs.close(&host, DocId(4), &mut input, &mut out).unwrap();
    assert_eq!(state.op.get(), schema::OP_CLOSE);
    reply(&state, &[doc_state(4, 0, 0)]);
    assert_eq!(
        docs.close(&host, DocId(4), &mut input, &mut out),
        Err(Error::Invalid)
    );
}

#[test]
fn xpointer_and_locate_convert_a_position_both_ways() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    reply(
        &state,
        &[Record::ServiceDocXpointer(wire::ServiceDocXpointer {
            document: 4,
            text: "/body/DocFragment[3]/p[2]/text().4",
        })],
    );
    let range = out.as_ptr_range();
    let x = docs
        .xpointer(
            &host,
            DocId(4),
            DocPosition::new(2, 40),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(x, "/body/DocFragment[3]/p[2]/text().4");
    assert!(range.contains(&x.as_ptr()));
    assert_eq!(state.op.get(), schema::OP_XPOINTER);
    {
        let bytes = state.input.borrow();
        assert!(matches!(
            wire::records_decode(&bytes).unwrap().iter().next(),
            Some(Record::ServiceDocAt(a)) if (a.document, a.section, a.offset, a.insert, a.cursor, a.generation) == (4, 2, 40, 0, 0, 0)
        ));
    }
    reply(
        &state,
        &[Record::ServiceDocAt(wire::ServiceDocAt {
            document: 4,
            section: 2,
            offset: 40,
            insert: 0,
            cursor: 0,
            generation: 0,
        })],
    );
    let at = docs
        .locate(
            &host,
            DocId(4),
            "/body/DocFragment[3]/p[2]/text().4",
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(at, DocPosition::new(2, 40));
    assert_eq!(state.op.get(), schema::OP_LOCATE);
    let bytes = state.input.borrow();
    assert!(matches!(
        wire::records_decode(&bytes).unwrap().iter().next(),
        Some(Record::ServiceDocXpointer(x)) if x.document == 4 && x.text == "/body/DocFragment[3]/p[2]/text().4"
    ));
}

#[test]
fn xpointer_and_locate_refuse_bad_input_and_foreign_answers() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 4096], [0; 4096]);
    assert_eq!(
        docs.locate(&host, DocId(4), &"x".repeat(1025), &mut input, &mut out),
        Err(Error::Invalid)
    );
    assert_eq!(state.calls.get(), 0);
    // Not found where the format has none.
    state.error.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        docs.xpointer(
            &host,
            DocId(4),
            DocPosition::new(0, 0),
            &mut input,
            &mut out
        ),
        Err(Error::NotFound)
    );
    assert_eq!(
        docs.locate(&host, DocId(4), "/body", &mut input, &mut out),
        Err(Error::NotFound)
    );
    // An answer for another document is malformed.
    state.error.set(0);
    reply(
        &state,
        &[Record::ServiceDocXpointer(wire::ServiceDocXpointer {
            document: 9,
            text: "/a",
        })],
    );
    assert_eq!(
        docs.xpointer(
            &host,
            DocId(4),
            DocPosition::new(0, 0),
            &mut input,
            &mut out
        ),
        Err(Error::Invalid)
    );
    reply(
        &state,
        &[Record::ServiceDocAt(wire::ServiceDocAt {
            document: 9,
            section: 1,
            offset: 1,
            insert: 0,
            cursor: 0,
            generation: 0,
        })],
    );
    assert_eq!(
        docs.locate(&host, DocId(4), "/a", &mut input, &mut out),
        Err(Error::Invalid)
    );
}

#[path = "document_meta_tests.rs"]
mod meta;
#[path = "document_readers_tests.rs"]
mod readers;

#[path = "document_formats_tests.rs"]
mod formats_tests;
#[path = "document_report_tests.rs"]
mod report_tests;

#[test]
fn ask_image_asks_for_one_picture_by_its_index() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
    let row = |index| {
        Record::ServiceDocImage(wire::ServiceDocImage {
            image: 7,
            index,
            width: 200,
            height: 100,
            flags: 0,
        })
    };
    reply(&state, &[row(3)]);
    let image = docs
        .ask_image(&host, DocId(4), 3, 0, &mut input, &mut out)
        .unwrap();
    assert_eq!(
        (image.image(), image.index(), image.width(), image.height()),
        (7, 3, 200, 100)
    );
    assert_eq!(state.op.get(), schema::OP_IMAGE);
    {
        let bytes = state.input.borrow();
        assert!(matches!(
            wire::records_decode(&bytes).unwrap().iter().next(),
            Some(Record::ServiceDocImageAsk(a)) if (a.document, a.index, a.side) == (4, 3, 0)
        ));
    }
    // Another picture's answer is no answer.
    reply(&state, &[row(5)]);
    assert_eq!(
        docs.ask_image(&host, DocId(4), 3, 0, &mut input, &mut out),
        Err(Error::Invalid)
    );
}

#[test]
fn ask_image_passes_refusals_through_and_wants_exactly_one_picture() {
    setup!(table, state, host, docs);
    let (mut input, mut out) = ([0; 64], [0; 4096]);
    for (status, error) in [
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_INVALID, Error::Invalid),
    ] {
        state.error.set(status);
        assert_eq!(
            docs.ask_image(&host, DocId(4), 3, 0, &mut input, &mut out),
            Err(error)
        );
    }
    state.error.set(0);
    let row = Record::ServiceDocImage(wire::ServiceDocImage {
        image: 7,
        index: 3,
        width: 200,
        height: 100,
        flags: 0,
    });
    // No picture, or two: malformed.
    for rows in [&[][..], &[row, row][..]] {
        reply(&state, rows);
        assert_eq!(
            docs.ask_image(&host, DocId(4), 3, 0, &mut input, &mut out),
            Err(Error::Invalid)
        );
    }
}
