//! Staged annotation transactions: ops 17-21 and 26.
use super::*;
use crate::native_host::lib_service::write as w;
use paperpaw_native::host_glue::ERR_BUSY;
use wire::{
    ServiceLibEdit as Edit, ServiceLibEditHead as Head, ServiceLibEditText as EditText,
    ServiceLibReceiptAsk as ReceiptAsk, ServiceLibTransactionRef as TransactionRef,
};

type Bytes = std::vec::Vec<u8>;

fn head(generation: u32) -> Record<'static> {
    Record::ServiceLibEditHead(Head {
        annotation_generation: generation,
    })
}
fn tref(transaction: u32) -> Record<'static> {
    Record::ServiceLibTransactionRef(TransactionRef { transaction })
}
fn edit() -> Edit<'static> {
    Edit {
        action: 0,
        id: 0,
        kind: 1,
        start_section: 1,
        start_offset: 5,
        end_section: 1,
        end_offset: 9,
        flags: 0,
        percent: 10,
        conversion: 0,
        created: 1,
        updated: 1,
        client_key: "",
        text: "hello",
        note: "",
        text_bytes: 0,
        note_bytes: 0,
    }
}
fn add(fake: &Rc<Fake>, transaction: u32, edits: &[Edit<'static>]) -> Result<Bytes, i32> {
    let mut records = std::vec![tref(transaction)];
    records.extend(edits.iter().map(|e| Record::ServiceLibEdit(*e)));
    go(fake, 18, &records)
}
fn begin(fake: &Rc<Fake>) -> Result<u32, i32> {
    let reply = go(fake, 17, &[book(), head(5)])?;
    let Record::ServiceLibTransaction(t) = wire::record_decode_one(&reply).unwrap() else {
        panic!("not a transaction")
    };
    Ok(t.transaction)
}
fn part(
    fake: &Rc<Fake>,
    t: u32,
    edit: u32,
    field: u32,
    offset: u32,
    data: &str,
    last: bool,
) -> Result<Bytes, i32> {
    go(
        fake,
        19,
        &[Record::ServiceLibEditText(EditText {
            transaction: t,
            edit,
            field,
            offset,
            flags: u32::from(last),
            data,
        })],
    )
}
fn commit(fake: &Rc<Fake>, t: u32) -> Result<Bytes, i32> {
    go(fake, 20, &[tref(t)])
}
fn staged(fake: &Rc<Fake>) -> usize {
    fake.log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("stage"))
        .count()
}

#[test]
fn a_transaction_stages_edits_and_commits_them_whole() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    assert_eq!(t, 1);
    add(
        &fake,
        t,
        &[
            edit(),
            Edit {
                action: 3,
                client_key: "k1",
                ..edit()
            },
            Edit {
                action: 1,
                id: 8,
                ..edit()
            },
        ],
    )
    .unwrap();
    add(
        &fake,
        t,
        &[Edit {
            action: 2,
            id: 9,
            text: "",
            ..edit()
        }],
    )
    .unwrap();
    assert_eq!(staged(&fake), 4);
    let reply = commit(&fake, t).unwrap();
    let Record::ServiceLibCommitted(c) = wire::record_decode_one(&reply).unwrap() else {
        panic!()
    };
    assert_eq!((c.transaction, c.edits, c.annotation_generation), (1, 4, 9));
    assert_eq!(fake.last(), "commit 1 owner 7 base 5 edits 4");
    // The transaction is over, and the plugin may begin the next.
    assert_eq!(commit(&fake, t), Err(ERR_NOT_FOUND));
    assert_eq!(begin(&fake).unwrap(), 2);
}

#[test]
fn one_open_transaction_per_plugin_and_two_on_the_device() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    assert_eq!(begin(&fake), Err(ERR_FULL));
    go(&fake, 21, &[tref(t)]).unwrap();
    assert_eq!(fake.last(), "abort 1");
    assert_eq!(go(&fake, 21, &[tref(t)]), Err(ERR_NOT_FOUND));
    assert_eq!(begin(&fake).unwrap(), 2);
    // Other owners fill the device: two open, the third is refused.
    let other = fake.transactions.begin_for_test(8, 1_000);
    assert_eq!(other, 3);
    assert_eq!(fake.transactions.open_count(), 2);
    go(&fake, 21, &[tref(2)]).unwrap();
    fake.transactions.begin_for_test(9, 1_000);
    assert_eq!(begin(&fake), Err(ERR_FULL), "the device is full");
}

#[test]
fn begin_names_a_known_book_and_one_record() {
    let fake = Fake::new();
    let gone = Record::ServiceLibRef(Ref {
        path: "Books/gone.epub",
        generation: 0,
        key: None,
    });
    assert_eq!(go(&fake, 17, &[gone, head(1)]), Err(ERR_NOT_FOUND));
    assert_eq!(go(&fake, 17, &[book()]), Err(ERR_INVALID));
    assert_eq!(go(&fake, 17, &[head(1), book()]), Err(ERR_INVALID));
    assert_eq!(
        go(&fake, 17, &[book(), head(1), head(1)]).map(drop),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(fake.transactions.open_count(), 0);
}

#[test]
fn edit_add_takes_one_ref_and_up_to_three_edits() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    assert!(add(&fake, t, &[edit(); 3]).is_ok());
    assert_eq!(add(&fake, t, &[edit(); 4]).map(drop), Err(ERR_NO_SPACE));
    assert_eq!(add(&fake, t, &[]), Err(ERR_INVALID));
    assert_eq!(
        go(&fake, 18, &[Record::ServiceLibEdit(edit())]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        go(
            &fake,
            18,
            &[tref(t), tref(t), Record::ServiceLibEdit(edit())]
        ),
        Err(ERR_INVALID)
    );
    assert_eq!(
        go(&fake, 18, &[Record::ServiceLibEdit(edit()), tref(t)]),
        Err(ERR_INVALID)
    );
    assert_eq!(staged(&fake), 3, "a refused call stages nothing");
    // Foreign and unknown transactions.
    assert_eq!(add(&fake, 99, &[edit()]), Err(ERR_NOT_FOUND));
    let other = fake.transactions.begin_for_test(8, 1_000);
    assert_eq!(add(&fake, other, &[edit()]), Err(ERR_DENIED));
}

fn edit_with(change: impl Fn(&mut Edit<'static>)) -> Edit<'static> {
    let mut edit = edit();
    change(&mut edit);
    edit
}

#[test]
fn every_edit_is_checked_before_any_is_staged() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    let bad: std::vec::Vec<(&str, Edit<'static>)> = std::vec![
        ("action", edit_with(|e| e.action = 4)),
        ("flags", edit_with(|e| e.flags = 0b10)),
        ("cut flags", edit_with(|e| e.flags = 0b100)),
        ("percent", edit_with(|e| e.percent = 101)),
        ("add with an id", edit_with(|e| e.id = 3)),
        ("replace without id", edit_with(|e| e.action = 1)),
        ("remove without id", edit_with(|e| e.action = 2)),
        (
            "upsert with an id",
            edit_with(|e| {
                e.action = 3;
                e.id = 3;
                e.client_key = "k";
            })
        ),
        ("upsert without key", edit_with(|e| e.action = 3)),
        ("conversion without flag", edit_with(|e| e.conversion = 5)),
        ("backwards highlight", edit_with(|e| e.end_offset = 4)),
        ("inline text and parts", edit_with(|e| e.text_bytes = 600)),
        (
            "inline note and parts",
            edit_with(|e| {
                e.note = "n";
                e.note_bytes = 600;
            })
        ),
        (
            "remove with text",
            edit_with(|e| {
                e.action = 2;
                e.id = 1;
                e.text = "";
                e.text_bytes = 5;
            })
        ),
        (
            "remove with xpointers",
            edit_with(|e| {
                e.action = 2;
                e.id = 1;
                e.text = "";
                e.flags = 0b1_0000;
            })
        ),
    ];
    for (what, bad_edit) in bad {
        assert_eq!(
            add(&fake, t, &[edit(), bad_edit]),
            Err(ERR_INVALID),
            "{what}"
        );
    }
    assert_eq!(
        staged(&fake),
        0,
        "the good first edit of each call was not staged either"
    );
    // Good ones: a bookmark may end before it starts (only a highlight runs forward).
    assert!(
        add(
            &fake,
            t,
            &[edit_with(|e| {
                e.kind = 0;
                e.end_offset = 0;
            })]
        )
        .is_ok()
    );
    assert!(
        add(&fake, t, &[edit_with(|e| e.flags = 0b1000)]).is_ok(),
        "present and 0"
    );
    assert!(
        add(&fake, t, &[edit_with(|e| e.kind = 7)]).is_ok(),
        "other(n)"
    );
}

#[test]
fn long_texts_and_notes_come_in_parts_in_order_and_whole() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    let long = Edit {
        text: "",
        text_bytes: 6000,
        note_bytes: 4,
        flags: 0b1_0000,
        ..edit()
    };
    add(&fake, t, &[long, edit()]).unwrap();
    let chunk = "x".repeat(3000);
    // Not complete yet: the commit and the next edit_add refuse.
    assert_eq!(commit(&fake, t), Err(ERR_INVALID));
    assert_eq!(add(&fake, t, &[edit()]), Err(ERR_INVALID));
    // Gaps, overlaps, wrong fields and unknown edits are ERR_INVALID at that part.
    assert_eq!(
        part(&fake, t, 0, 0, 3000, &chunk, false),
        Err(ERR_INVALID),
        "gap"
    );
    assert_eq!(
        part(&fake, t, 5, 0, 0, "x", false),
        Err(ERR_INVALID),
        "unknown edit"
    );
    assert_eq!(
        part(&fake, t, 1, 0, 0, "x", true),
        Err(ERR_INVALID),
        "edit 1 has no parts"
    );
    assert_eq!(
        part(&fake, t, 0, 0, 0, &chunk, true),
        Err(ERR_INVALID),
        "last before the whole text"
    );
    assert!(part(&fake, t, 0, 0, 0, &chunk, false).is_ok());
    assert_eq!(
        part(&fake, t, 0, 0, 0, &chunk, false),
        Err(ERR_INVALID),
        "overlap"
    );
    assert_eq!(
        part(&fake, t, 0, 0, 3000, &chunk, false),
        Err(ERR_INVALID),
        "the whole text without last"
    );
    assert_eq!(
        part(&fake, t, 0, 0, 3000, &(chunk.clone() + "y"), true),
        Err(ERR_INVALID),
        "beyond the whole"
    );
    assert_eq!(
        part(&fake, t, 0, 0, 3000, "", true),
        Err(ERR_INVALID),
        "an empty part"
    );
    assert_eq!(
        part(&fake, t, 0, 9, 0, "x", false),
        Err(ERR_INVALID),
        "no such field"
    );
    assert!(part(&fake, t, 0, 0, 3000, &chunk, true).is_ok());
    assert_eq!(
        part(&fake, t, 0, 0, 6000, "z", true),
        Err(ERR_INVALID),
        "the text is done"
    );
    // The note, then both XPointers; each field ends with `last`.
    assert!(part(&fake, t, 0, 1, 0, "note", true).is_ok());
    assert_eq!(
        commit(&fake, t),
        Err(ERR_INVALID),
        "the XPointers are still missing"
    );
    assert!(part(&fake, t, 0, 2, 0, "/a", true).is_ok());
    assert!(part(&fake, t, 0, 3, 0, "/b", true).is_ok());
    assert!(commit(&fake, t).is_ok());
}

#[test]
fn parts_complete_a_group_and_a_commit_needs_them_all() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    let edits = [
        Edit {
            text: "",
            text_bytes: 5,
            ..edit()
        },
        Edit {
            note: "",
            note_bytes: 3,
            ..edit()
        },
        Edit {
            flags: 0b1_0000,
            ..edit()
        },
    ];
    add(&fake, t, &edits).unwrap();
    assert_eq!(commit(&fake, t), Err(ERR_INVALID));
    part(&fake, t, 1, 1, 0, "abc", true).unwrap();
    assert_eq!(commit(&fake, t), Err(ERR_INVALID));
    part(&fake, t, 0, 0, 0, "hello", true).unwrap();
    // Both XPointers of edit 2, in parts, in any order between the fields.
    part(&fake, t, 2, 3, 0, "/p[2]", false).unwrap();
    part(&fake, t, 2, 2, 0, "/p[1]", true).unwrap();
    assert_eq!(
        commit(&fake, t),
        Err(ERR_INVALID),
        "the end XPointer is not done"
    );
    part(&fake, t, 2, 3, 5, "/text()", true).unwrap();
    let reply = commit(&fake, t).unwrap();
    assert!(!reply.is_empty());
    assert_eq!(fake.last(), "commit 1 owner 7 base 5 edits 3");
}

#[test]
fn sizes_are_capped_per_note_per_xpointer_and_per_transaction() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    // A note over 3 KiB is refused at edit_add, not at commit.
    let note = |bytes| Edit {
        note: "",
        note_bytes: bytes,
        ..edit()
    };
    assert_eq!(add(&fake, t, &[note(3073)]), Err(ERR_NO_SPACE));
    assert!(add(&fake, t, &[note(3072)]).is_ok());
    part(&fake, t, 0, 1, 0, &"n".repeat(3072), true).unwrap();
    // An XPointer is at most 1,024 bytes.
    add(
        &fake,
        t,
        &[Edit {
            flags: 0b1_0000,
            ..edit()
        }],
    )
    .unwrap();
    assert_eq!(
        part(&fake, t, 1, 2, 0, &"p".repeat(1025), true),
        Err(ERR_NO_SPACE)
    );
    assert!(part(&fake, t, 1, 2, 0, &"p".repeat(1024), true).is_ok());
    assert_eq!(
        part(&fake, t, 1, 3, 0, &"p".repeat(1024), false).map(drop),
        Ok(())
    );
    assert_eq!(part(&fake, t, 1, 3, 1024, "q", true), Err(ERR_NO_SPACE));
    assert!(part(&fake, t, 1, 3, 1024, "", true).is_err());
    // The transaction caps staged bytes at the annotation file's 1 MiB.
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    // The edit's record counts besides its text: 256 bytes leave room for it.
    let whole: u32 = (1 << 20) - 256;
    let big = Edit {
        text: "",
        text_bytes: whole,
        ..edit()
    };
    assert_eq!(
        add(
            &fake,
            t,
            &[Edit {
                text_bytes: (1 << 20) + 1,
                ..big
            }]
        ),
        Err(ERR_NO_SPACE)
    );
    assert!(add(&fake, t, &[big]).is_ok());
    let mut offset = 0;
    while offset < whole {
        let last = offset + 3072 >= whole;
        let size = if last { whole - offset } else { 3072 };
        part(&fake, t, 0, 0, offset, &"t".repeat(size as usize), last).unwrap();
        offset += 3072;
    }
    // What is left holds a few edit records, not an unbounded number: each
    // costs its record, empty or not.
    let mut more = 0;
    while add(&fake, t, &[edit()]).is_ok() {
        more += 1;
        assert!(more < 8, "the records count against the cap");
    }
    assert_eq!(add(&fake, t, &[edit()]), Err(ERR_NO_SPACE));
    assert!(commit(&fake, t).is_ok());
}

#[test]
fn a_commit_the_host_refuses_ends_the_transaction() {
    let fake = Fake::new();
    for error in [ERR_STATE, ERR_NOT_FOUND, ERR_INVALID, ERR_IO] {
        let t = begin(&fake).unwrap();
        add(&fake, t, &[edit()]).unwrap();
        fake.commit.set(Err(error));
        assert_eq!(commit(&fake, t), Err(error));
        assert_eq!(
            fake.last(),
            std::format!("abort {t}"),
            "the staged file goes"
        );
        assert_eq!(fake.transactions.open_count(), 0);
    }
    // The edits are applied once the host says so: counts that do not add up
    // (9 edits applied of 1 staged) never turn that into a failure, or the
    // plugin would retry and add them twice. The reply carries the
    // transaction's own counts.
    fake.commit.set(Ok(w::Committed {
        annotation_generation: 1,
        edits: 9,
        added: 9,
    }));
    let t = begin(&fake).unwrap();
    add(&fake, t, &[edit()]).unwrap();
    let reply = commit(&fake, t).unwrap();
    let Record::ServiceLibCommitted(done) = wire::record_decode_one(&reply).unwrap() else {
        panic!("not a commit")
    };
    assert_eq!((done.transaction, done.edits, done.added), (t, 1, 1));
    assert_eq!(fake.transactions.open_count(), 0);
}

#[test]
fn a_reply_that_cannot_be_written_stops_before_anything_is_applied() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    add(&fake, t, &[edit()]).unwrap();
    let bytes = input(20, &[tref(t)]).unwrap();
    let mut small = [0_u8; 4];
    let applied = |fake: &Rc<Fake>| fake.log.borrow().iter().any(|l| l.starts_with("commit"));
    assert_eq!(
        with(host(&fake), |frame| frame.run(
            schema_op(20),
            &bytes,
            &mut small
        )),
        Err(ERR_NO_SPACE)
    );
    assert!(!applied(&fake), "nothing was applied");
    assert_eq!(fake.transactions.open_count(), 1, "the transaction stays");
    assert!(commit(&fake, t).is_ok());
    assert!(applied(&fake));
}

#[test]
fn an_edit_counts_its_record_and_a_transaction_holds_at_most_1024() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    // 341 calls of 3 empty edits and one of 1: 1,024 edits.
    for _ in 0..341 {
        add(&fake, t, &[edit(), edit(), edit()]).unwrap();
    }
    add(&fake, t, &[edit()]).unwrap();
    assert_eq!(add(&fake, t, &[edit()]), Err(ERR_NO_SPACE));
    // The refusal staged nothing and the transaction still commits.
    assert_eq!(staged(&fake), 1024);
    assert!(commit(&fake, t).is_ok());
}

#[test]
fn a_book_with_an_open_transaction_is_busy_for_delete_and_move() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    let del = |paths: &str| {
        go_rows(
            &fake,
            10,
            &[Record::ServiceLibPaths(Paths { paths, flags: 0 })],
        )
    };
    // Only the book of the transaction is busy; other books and a gone one
    // are not (each path resolves to its own book).
    assert_eq!(
        results(&del("Books/a.epub\nBooks/b.epub\nBooks/gone.epub").unwrap()),
        [
            ("Books/a.epub".into(), ERR_BUSY, 0),
            ("Books/b.epub".into(), 0, 0),
            ("Books/gone.epub".into(), 0, 0)
        ]
    );
    let mv = |from, to| {
        go(
            &fake,
            11,
            &[Record::ServiceLibMove(Move { from, to, flags: 1 })],
        )
    };
    assert_eq!(mv("Books/a.epub", "Books/b.epub"), Err(ERR_BUSY), "source");
    assert_eq!(
        mv("Books/b.epub", "Books/a.epub"),
        Err(ERR_BUSY),
        "the replaced one too"
    );
    assert!(!fake.log.borrow().iter().any(|l| l.starts_with("move")));
    // An unrelated move goes through, and the busy book once it ends.
    assert!(mv("Books/b.epub", "Books/c.epub").is_ok());
    commit(&fake, t).unwrap();
    assert!(mv("Books/a.epub", "Books/d.epub").is_ok());
}

#[test]
fn staging_that_fails_ends_the_transaction_with_the_hosts_status() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    fake.stage_fails.set(Some(ERR_NO_SPACE));
    assert_eq!(add(&fake, t, &[edit()]), Err(ERR_NO_SPACE));
    assert_eq!(fake.last(), std::format!("abort {t}"));
    assert_eq!(fake.transactions.open_count(), 0);
    fake.stage_fails.set(None);
    let t = begin(&fake).unwrap();
    add(
        &fake,
        t,
        &[Edit {
            text: "",
            text_bytes: 5,
            ..edit()
        }],
    )
    .unwrap();
    fake.stage_fails.set(Some(ERR_IO));
    assert_eq!(part(&fake, t, 0, 0, 0, "hello", true), Err(ERR_IO));
    assert_eq!(fake.transactions.open_count(), 0);
}

#[test]
fn an_idle_transaction_is_aborted_after_ten_minutes() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    fake.now.set(1_000 + 600_000);
    add(&fake, t, &[edit()]).unwrap();
    fake.now.set(1_000 + 600_000 + 600_001);
    assert_eq!(commit(&fake, t), Err(ERR_NOT_FOUND));
    assert_eq!(fake.last(), std::format!("abort {t}"));
    assert_eq!(fake.transactions.open_count(), 0);
    // Any call expires the others', so a begin finds room.
    let t = begin(&fake).unwrap();
    fake.now.set(fake.now.get() + 600_001);
    let again = begin(&fake).unwrap();
    assert_ne!(t, again);
}

#[test]
fn a_stopped_plugin_ends_its_transaction() {
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    let mut ended = std::vec::Vec::new();
    fake.transactions
        .close_owner(OWNER, |id| ended.push(id))
        .unwrap();
    assert_eq!(ended, [t]);
    assert_eq!(fake.transactions.open_count(), 0);
}

#[test]
fn receipts_page_whole_rows_for_the_committed_transaction_only() {
    let fake = Fake::new();
    *fake.receipts.borrow_mut() = (0..40).map(|i| (i, 100 + i, i % 4)).collect();
    let ask = |t, cursor| {
        go(
            &fake,
            26,
            &[Record::ServiceLibReceiptAsk(ReceiptAsk {
                transaction: t,
                cursor,
            })],
        )
    };
    assert_eq!(ask(5, 0), Err(ERR_NOT_FOUND), "not the committed one");
    assert_eq!(ask(0, 0), Err(ERR_NOT_FOUND));
    let first = ask(4, 0).unwrap();
    let rows = |bytes: &[u8]| {
        let mut at = 0;
        let mut ids = std::vec::Vec::new();
        let mut end = None;
        while at < bytes.len() {
            let (record, used) = wire::record_decode_prefix(&bytes[at..]).unwrap();
            at += used;
            match record {
                Record::ServiceLibReceipt(r) => ids.push(r.id),
                Record::ServiceEnd(e) => end = Some(e),
                _ => panic!(),
            }
        }
        (ids, end.unwrap())
    };
    let (ids, end) = rows(&first);
    assert_eq!((ids.len(), end.flags, end.total, end.next), (32, 0, 40, 32));
    let (ids, end) = rows(&ask(4, 32).unwrap());
    assert_eq!(
        (ids, end.flags, end.next),
        ((132..140).collect::<std::vec::Vec<_>>(), 1, 40)
    );
    assert!(ask(4, 0).is_ok());
    assert_eq!(go(&fake, 26, &[tref(4)]), Err(ERR_INVALID));
}

#[test]
fn an_inline_text_is_counted_once_not_twice() {
    // The framed record holds its inline text: the cap fits as many such
    // edits as their framed size allows, no fewer.
    let text: &'static str = std::boxed::Box::leak("x".repeat(512).into_boxed_str());
    let big = Edit {
        text,
        note: text,
        flags: 1,
        ..edit()
    };
    let framed = wire::record_framed_len(&Record::ServiceLibEdit(big)).unwrap();
    let fits = (1 << 20) / framed;
    assert!(fits < 1024, "the byte cap bounds it here");
    let fake = Fake::new();
    let t = begin(&fake).unwrap();
    for n in 0..fits {
        add(&fake, t, &[big]).unwrap_or_else(|e| panic!("edit {n} refused: {e}"));
    }
    assert_eq!(add(&fake, t, &[big]), Err(ERR_NO_SPACE));
}
