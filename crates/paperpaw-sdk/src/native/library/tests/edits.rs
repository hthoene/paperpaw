use super::super::edits::*;
use super::*;
use crate::native::{declare::records_wire::Record, library::AnnotationKind};
use alloc::string::String;

/// Every call the host saw, and what it answers.
#[derive(Default)]
struct Log {
    calls: RefCell<Vec<(u32, Vec<u8>)>>,
    replies: RefCell<Vec<(u32, Vec<u8>)>>,
    fail_call: Cell<Option<usize>>,
    /// The status a failing call answers; 0 is `ERR_NO_SPACE`.
    status: Cell<i32>,
}
unsafe extern "C" fn open_log(_: *mut c_void, _: PpBytes, _: u16, _: u16, out: *mut u32) -> i32 {
    // SAFETY: the SDK provides an aligned output.
    unsafe { out.write(7) };
    abi::OK
}
unsafe extern "C" fn call_log(
    ctx: *mut c_void,
    _: u32,
    op: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let log = unsafe { &*ctx.cast::<Log>() };
    // SAFETY: SDK encoded a live input slice.
    let bytes = unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec();
    let index = log.calls.borrow().len();
    log.calls.borrow_mut().push((op, bytes));
    if log.fail_call.get() == Some(index) {
        return match log.status.get() {
            0 => abi::ERR_NO_SPACE,
            status => status,
        };
    }
    let replies = log.replies.borrow();
    let reply = replies
        .iter()
        .find(|(o, _)| *o == op)
        .map_or(&[][..], |(_, r)| r.as_slice());
    assert!(reply.len() <= out.cap);
    // SAFETY: the fixture reply and exclusive SDK output are disjoint.
    unsafe { core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len()) };
    i32::try_from(reply.len()).unwrap()
}
fn log_table() -> PpHost {
    let mut t = abi::host_table();
    t.service_open = open_log;
    t.service_call = call_log;
    t
}
fn bare(rows: &[Record<'_>]) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    bytes
}
fn tx_reply(id: u32) -> Vec<u8> {
    bare(
        &[Record::ServiceLibTransaction(wire::ServiceLibTransaction {
            transaction: id,
        })],
    )
}
macro_rules! setup {
    ($table:ident, $log:ident, $host:ident, $lib:ident) => {
        let $table = log_table();
        let mut $log = Log::default();
        let $host = Host {
            table: &$table,
            ctx: core::ptr::from_mut(&mut $log).cast(),
        };
        let $lib = Library::open(&$host).unwrap();
    };
}
fn book() -> BookRef<'static> {
    BookRef::path("/books/a.epub").unwrap()
}
fn sent(log: &Log, at: usize) -> (u32, Vec<Record<'static>>) {
    let (op, bytes) = log.calls.borrow()[at].clone();
    let bytes: &'static [u8] = Vec::leak(bytes);
    (op, wire::records_decode(bytes).unwrap().iter().collect())
}
fn begin(log: &Log, lib: &Library, host: &Host<'_>) -> Transaction {
    log.replies
        .borrow_mut()
        .push((schema::OP_EDIT_BEGIN, tx_reply(8)));
    let tx = lib
        .edit_begin(host, book(), 5, &mut [0; 1024], &mut [0; 4096])
        .unwrap();
    log.calls.borrow_mut().clear();
    tx
}

#[test]
fn begin_sends_the_book_and_the_generation_read_and_refuses_generation_zero() {
    setup!(table, log, host, lib);
    assert_eq!(
        lib.edit_begin(&host, book(), 0, &mut [0; 1024], &mut [0; 4096])
            .unwrap_err(),
        Error::Invalid
    );
    assert!(log.calls.borrow().is_empty());
    log.replies
        .borrow_mut()
        .push((schema::OP_EDIT_BEGIN, tx_reply(8)));
    let tx = lib
        .edit_begin(&host, book(), 5, &mut [0; 1024], &mut [0; 4096])
        .unwrap();
    assert_eq!(tx.edits(), 0);
    let (op, rows) = sent(&log, 0);
    assert_eq!(op, schema::OP_EDIT_BEGIN);
    assert!(matches!(&rows[0], Record::ServiceLibRef(r) if r.path == "/books/a.epub"));
    assert!(matches!(&rows[1], Record::ServiceLibEditHead(h) if h.annotation_generation == 5));
}

#[test]
fn a_short_edit_is_one_call_and_a_removal_carries_only_its_id() {
    setup!(table, log, host, lib);
    let mut tx = begin(&log, &lib, &host);
    let add = AnnotationEdit::upsert("key-1", AnnotationKind::Highlight, (1, 2), (3, 4))
        .percent(40)
        .conversion(0)
        .times(10, 20)
        .text("short text")
        .note("short note");
    let remove = AnnotationEdit::remove(9).unwrap();
    lib.edit_add(
        &host,
        &mut tx,
        &[add, remove],
        &mut [0; 4096],
        &mut [0; 4096],
    )
    .unwrap();
    assert_eq!(tx.edits(), 2);
    assert_eq!(log.calls.borrow().len(), 1, "no parts for short texts");
    let (op, rows) = sent(&log, 0);
    assert_eq!(op, schema::OP_EDIT_ADD);
    assert!(matches!(&rows[0], Record::ServiceLibTransactionRef(t) if t.transaction == 8));
    let Record::ServiceLibEdit(e) = &rows[1] else {
        panic!()
    };
    assert_eq!((e.action, e.id, e.kind), (3, 0, 1));
    assert_eq!(
        (e.start_section, e.start_offset, e.end_section, e.end_offset),
        (1, 2, 3, 4)
    );
    assert_eq!(
        (e.percent, e.conversion, e.created, e.updated),
        (40, 0, 10, 20)
    );
    // Noted (bit 0) and has a conversion (bit 3); a present 0 stays present.
    assert_eq!(e.flags, 0b1001);
    assert_eq!(
        (e.client_key, e.text, e.note, e.text_bytes, e.note_bytes),
        ("key-1", "short text", "short note", 0, 0)
    );
    let Record::ServiceLibEdit(r) = &rows[2] else {
        panic!()
    };
    assert_eq!(
        (r.action, r.id, r.flags, r.text, r.client_key),
        (2, 9, 0, "", "")
    );
}

#[test]
fn long_texts_and_every_xpointer_travel_in_parts_cut_on_character_boundaries() {
    setup!(table, log, host, lib);
    let mut tx = begin(&log, &lib, &host);
    // 'é' is two bytes: 1,600 of them are 3,200 bytes, over one part of 3,072.
    let text = "é".repeat(1_600);
    let note = "n".repeat(600);
    let edit = AnnotationEdit::add(AnnotationKind::Highlight, (0, 1), (0, 9))
        .text(&text)
        .note(&note)
        .xpointers("/body/p[1]/text().0", "/body/p[2]/text().3");
    lib.edit_add(&host, &mut tx, &[edit], &mut [0; 4096], &mut [0; 4096])
        .unwrap();
    let calls = log.calls.borrow().len();
    // The edit, two parts of the text, one of the note, one per XPointer.
    assert_eq!(calls, 1 + 2 + 1 + 2);
    let (_, rows) = sent(&log, 0);
    let Record::ServiceLibEdit(e) = &rows[1] else {
        panic!()
    };
    assert_eq!(
        (e.text, e.note, e.text_bytes, e.note_bytes),
        ("", "", 3200, 600)
    );
    assert_eq!(e.flags, 1 | 1 << 4, "noted and has XPointers");
    let mut joined = String::new();
    let mut offsets = Vec::new();
    for at in 1..calls {
        let (op, rows) = sent(&log, at);
        assert_eq!(op, schema::OP_EDIT_TEXT);
        let Record::ServiceLibEditText(p) = &rows[0] else {
            panic!()
        };
        assert_eq!((p.transaction, p.edit), (8, 0));
        assert!(p.data.len() <= 3072 && !p.data.is_empty());
        offsets.push((p.field, p.offset, p.flags));
        if p.field == 0 {
            joined.push_str(p.data);
        }
    }
    assert_eq!(joined, text, "parts join to the text");
    assert_eq!(
        offsets,
        [(0, 0, 0), (0, 3072, 1), (1, 0, 1), (2, 0, 1), (3, 0, 1)]
    );
}

#[test]
fn bad_edits_and_too_many_are_refused_before_the_host_is_called() {
    setup!(table, log, host, lib);
    let mut tx = begin(&log, &lib, &host);
    let ok = AnnotationEdit::add(AnnotationKind::Bookmark, (0, 0), (0, 0));
    let long_key = "k".repeat(65);
    let long_note = "n".repeat(3073);
    let long_xp = "x".repeat(1025);
    let cases = [
        (ok.percent(101), Error::Invalid),
        (
            AnnotationEdit::upsert("", AnnotationKind::Bookmark, (0, 0), (0, 0)),
            Error::Invalid,
        ),
        (ok.client_key(&long_key), Error::Invalid),
        (ok.xpointers("", "/a"), Error::Invalid),
        (ok.xpointers("/a", &long_xp), Error::Invalid),
        (ok.note(&long_note), Error::NoSpace),
    ];
    for (edit, want) in cases {
        assert_eq!(
            lib.edit_add(&host, &mut tx, &[edit], &mut [0; 4096], &mut [0; 4096])
                .unwrap_err(),
            want
        );
    }
    assert_eq!(
        lib.edit_add(&host, &mut tx, &[ok; 4], &mut [0; 4096], &mut [0; 4096])
            .unwrap_err(),
        Error::NoSpace
    );
    assert!(AnnotationEdit::replace(0, AnnotationKind::Bookmark, (0, 0), (0, 0)).is_err());
    assert!(AnnotationEdit::remove(0).is_err());
    assert!(log.calls.borrow().is_empty());
    // A transaction stays usable after a local refusal; an empty list is nothing.
    lib.edit_add(&host, &mut tx, &[], &mut [0; 4096], &mut [0; 4096])
        .unwrap();
    lib.edit_add(&host, &mut tx, &[ok; 3], &mut [0; 4096], &mut [0; 4096])
        .unwrap();
    assert_eq!(tx.edits(), 3);
}

#[test]
fn a_failed_part_breaks_the_transaction_until_it_is_aborted() {
    setup!(table, log, host, lib);
    let mut tx = begin(&log, &lib, &host);
    let edit = AnnotationEdit::add(AnnotationKind::Highlight, (0, 1), (0, 9)).xpointers("/a", "/b");
    // The edit goes through, its first part fails.
    log.fail_call.set(Some(1));
    assert_eq!(
        lib.edit_add(&host, &mut tx, &[edit], &mut [0; 4096], &mut [0; 4096])
            .unwrap_err(),
        Error::NoSpace
    );
    let calls = log.calls.borrow().len();
    assert_eq!(
        lib.edit_add(&host, &mut tx, &[edit], &mut [0; 4096], &mut [0; 4096])
            .unwrap_err(),
        Error::State
    );
    assert_eq!(
        lib.edit_commit(&host, &mut tx, &mut [0; 256], &mut [0; 4096])
            .unwrap_err(),
        Error::State
    );
    assert_eq!(log.calls.borrow().len(), calls, "nothing more is sent");
    lib.edit_abort(&host, &mut tx, &mut [0; 256], &mut [0; 64])
        .unwrap();
    let (op, _) = sent(&log, calls);
    assert_eq!(op, schema::OP_EDIT_ABORT);
    // A finished transaction is State for everything.
    assert_eq!(
        lib.edit_abort(&host, &mut tx, &mut [0; 256], &mut [0; 64])
            .unwrap_err(),
        Error::State
    );
}

fn committed(transaction: u32, generation: u32) -> Vec<u8> {
    bare(&[Record::ServiceLibCommitted(wire::ServiceLibCommitted {
        transaction,
        annotation_generation: generation,
        edits: 3,
        added: 2,
    })])
}

#[test]
fn commit_reads_what_was_applied_and_checks_the_answer_belongs_to_the_transaction() {
    setup!(table, log, host, lib);
    let mut tx = begin(&log, &lib, &host);
    log.replies
        .borrow_mut()
        .push((schema::OP_EDIT_COMMIT, committed(8, 6)));
    let done = lib
        .edit_commit(&host, &mut tx, &mut [0; 256], &mut [0; 4096])
        .unwrap();
    assert_eq!(
        (
            done.transaction(),
            done.annotation_generation(),
            done.edits(),
            done.added()
        ),
        (8, 6, 3, 2)
    );
    assert_eq!(
        lib.edit_commit(&host, &mut tx, &mut [0; 256], &mut [0; 4096])
            .unwrap_err(),
        Error::State,
        "a committed transaction is finished"
    );
    for bad in [committed(9, 6), committed(8, 1), committed(8, 0)] {
        let mut tx = begin(&log, &lib, &host);
        log.replies
            .borrow_mut()
            .retain(|(op, _)| *op != schema::OP_EDIT_COMMIT);
        log.replies.borrow_mut().push((schema::OP_EDIT_COMMIT, bad));
        assert_eq!(
            lib.edit_commit(&host, &mut tx, &mut [0; 256], &mut [0; 4096])
                .unwrap_err(),
            Error::Invalid
        );
    }
}

fn receipt(edit: u32, id: u32, action: u32) -> Record<'static> {
    Record::ServiceLibReceipt(wire::ServiceLibReceipt { edit, id, action })
}

#[test]
fn the_receipt_pages_the_ids_of_the_edits() {
    setup!(table, log, host, lib);
    let mut tx = begin(&log, &lib, &host);
    log.replies
        .borrow_mut()
        .push((schema::OP_EDIT_COMMIT, committed(8, 6)));
    let done = lib
        .edit_commit(&host, &mut tx, &mut [0; 256], &mut [0; 4096])
        .unwrap();
    log.calls.borrow_mut().clear();
    log.replies.borrow_mut().push((
        schema::OP_EDIT_RECEIPT,
        bare(&[receipt(0, 11, 0), receipt(1, 4, 3), end(0, 3, 6, 2)]),
    ));
    let mut out = [0; 4096];
    let page = lib
        .edit_receipt(
            &host,
            &done,
            ReceiptCursor::start(),
            &mut [0; 256],
            &mut out,
        )
        .unwrap();
    assert_eq!((page.len(), page.total()), (2, 3));
    let rows: Vec<_> = page.rows().collect();
    assert_eq!(
        (rows[0].edit(), rows[0].id(), rows[0].action()),
        (0, 11, ReceiptAction::Added)
    );
    assert_eq!(rows[1].action(), ReceiptAction::Updated);
    let (op, sent_rows) = sent(&log, 0);
    assert_eq!(op, schema::OP_EDIT_RECEIPT);
    assert!(
        matches!(&sent_rows[0], Record::ServiceLibReceiptAsk(a) if (a.transaction, a.cursor) == (8, 0))
    );
    let next = page.next().unwrap();
    log.replies.borrow_mut().clear();
    log.replies.borrow_mut().push((
        schema::OP_EDIT_RECEIPT,
        bare(&[receipt(2, 12, 7), end(1, 3, 6, 3)]),
    ));
    let mut out2 = [0; 4096];
    let page = lib
        .edit_receipt(&host, &done, next, &mut [0; 256], &mut out2)
        .unwrap();
    assert_eq!(
        (page.next(), page.rows().next().map(|r| r.action())),
        (None, Some(ReceiptAction::Other(7)))
    );
    // A page whose next is not the cursor plus its rows is Invalid.
    log.replies.borrow_mut().clear();
    log.replies.borrow_mut().push((
        schema::OP_EDIT_RECEIPT,
        bare(&[receipt(2, 12, 0), end(1, 3, 6, 9)]),
    ));
    assert_eq!(
        lib.edit_receipt(&host, &done, next, &mut [0; 256], &mut [0; 4096])
            .unwrap_err(),
        Error::Invalid
    );
    // A page of another generation than the commit's (or 0) is Invalid.
    for generation in [0, 5, 7] {
        log.replies.borrow_mut().clear();
        log.replies.borrow_mut().push((
            schema::OP_EDIT_RECEIPT,
            bare(&[receipt(2, 12, 0), end(1, 3, generation, 3)]),
        ));
        assert_eq!(
            lib.edit_receipt(&host, &done, next, &mut [0; 256], &mut [0; 4096])
                .unwrap_err(),
            Error::Invalid,
            "generation {generation}"
        );
    }
}

#[test]
fn a_local_encoding_failure_leaves_the_transaction_usable() {
    setup!(table, log, host, lib);
    let mut tx = begin(&log, &lib, &host);
    let edit = AnnotationEdit::add(AnnotationKind::Bookmark, (0, 0), (0, 0));
    // An `input` too small for the record fails before any call.
    assert!(
        lib.edit_add(&host, &mut tx, &[edit], &mut [0; 8], &mut [0; 4096])
            .is_err()
    );
    assert!(log.calls.borrow().is_empty());
    assert_eq!(tx.edits(), 0);
    lib.edit_add(&host, &mut tx, &[edit], &mut [0; 4096], &mut [0; 4096])
        .unwrap();
    assert_eq!(tx.edits(), 1);
}

#[test]
fn full_a_stale_commit_and_a_gone_receipt_are_the_hosts_answers() {
    setup!(table, log, host, lib);
    // Begin: the plugin or the device has its transactions open already.
    log.fail_call.set(Some(0));
    log.status.set(abi::ERR_FULL);
    assert_eq!(
        lib.edit_begin(&host, book(), 5, &mut [0; 1024], &mut [0; 4096])
            .unwrap_err(),
        Error::Full
    );
    log.fail_call.set(None);
    let mut tx = begin(&log, &lib, &host);
    // Commit: the generation moved. The transaction is finished whatever the
    // host answers, so abort is State and the plugin begins again.
    log.fail_call.set(Some(0));
    log.status.set(abi::ERR_STATE);
    assert_eq!(
        lib.edit_commit(&host, &mut tx, &mut [0; 256], &mut [0; 4096])
            .unwrap_err(),
        Error::State
    );
    assert_eq!(
        lib.edit_abort(&host, &mut tx, &mut [0; 256], &mut [0; 64])
            .unwrap_err(),
        Error::State
    );
    // Receipt: kept until the next begin, 10 minutes or the plugin stops.
    log.replies
        .borrow_mut()
        .push((schema::OP_EDIT_COMMIT, committed(8, 6)));
    let mut tx = begin(&log, &lib, &host);
    log.fail_call.set(None);
    let done = lib
        .edit_commit(&host, &mut tx, &mut [0; 256], &mut [0; 4096])
        .unwrap();
    log.fail_call.set(Some(log.calls.borrow().len()));
    log.status.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        lib.edit_receipt(
            &host,
            &done,
            ReceiptCursor::start(),
            &mut [0; 256],
            &mut [0; 4096]
        )
        .unwrap_err(),
        Error::NotFound
    );
}
