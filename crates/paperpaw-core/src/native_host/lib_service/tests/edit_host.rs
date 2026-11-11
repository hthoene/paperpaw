//! The annotation transactions (ops 17 to 21 and 26) through the service over
//! the production host and its card: a plugin's whole run, as the dispatcher
//! makes the calls.
use super::*;
use crate::native_host::lib_host::LibraryHost;
use crate::native_host::lib_reads::tests::card;
use crate::native_host::registrations::{
    ServiceLibEdit as Edit, ServiceLibEditHead as Head, ServiceLibEditText as EditText,
    ServiceLibReceiptAsk as ReceiptAsk, ServiceLibRef as Ref, ServiceLibTransactionRef as TRef,
};
use paperpaw_native::host_glue::{ERR_NO_SPACE, ERR_NOT_FOUND};

fn book() -> Record<'static> {
    Record::ServiceLibRef(Ref {
        path: "books/a.epub",
        generation: 0,
        key: None,
    })
}
fn tref(transaction: u32) -> Record<'static> {
    Record::ServiceLibTransactionRef(TRef { transaction })
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
        text: "",
        note: "",
        text_bytes: 11,
        note_bytes: 0,
    }
}
fn begin(host: &Rc<dyn LibHost>, generation: u32) -> u32 {
    let reply = call(
        host,
        17,
        &[
            book(),
            Record::ServiceLibEditHead(Head {
                annotation_generation: generation,
            }),
        ],
    )
    .unwrap();
    let Record::ServiceLibTransaction(t) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a transaction")
    };
    t.transaction
}
fn receipt(
    host: &Rc<dyn LibHost>,
    transaction: u32,
) -> Result<std::vec::Vec<(u32, u32, u32)>, i32> {
    let reply = call(
        host,
        26,
        &[Record::ServiceLibReceiptAsk(ReceiptAsk {
            transaction,
            cursor: 0,
        })],
    )?;
    let mut rows = std::vec::Vec::new();
    let mut at = 0;
    while at < reply.len() {
        let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
        at += used;
        if let Record::ServiceLibReceipt(row) = record {
            rows.push((row.edit, row.id, row.action));
        }
    }
    Ok(rows)
}

#[test]
fn a_plugin_runs_a_transaction_on_the_card_and_reads_its_receipt() {
    let card = card();
    let host: Rc<dyn LibHost> = Rc::new(LibraryHost::new(
        card.storage.clone(),
        card.env.clone(),
        None,
    ));
    let t = begin(&host, 1);
    call(&host, 18, &[tref(t), Record::ServiceLibEdit(edit())]).unwrap();
    for (offset, data, last) in [(0, "hello ", false), (6, "world", true)] {
        call(
            &host,
            19,
            &[Record::ServiceLibEditText(EditText {
                transaction: t,
                edit: 0,
                field: 0,
                offset,
                flags: u32::from(last),
                data,
            })],
        )
        .unwrap();
    }
    let reply = call(&host, 20, &[tref(t)]).unwrap();
    let Record::ServiceLibCommitted(done) = wire::record_decode_one(&reply).unwrap() else {
        panic!("committed")
    };
    assert_eq!(
        (done.annotation_generation, done.edits, done.added),
        (3, 1, 1)
    );
    assert_eq!(receipt(&host, t).unwrap(), [(0, 1, 0)]);
    // A new `edit_begin` ends the receipt of the one before.
    let second = begin(&host, 3);
    assert_ne!(second, t);
    assert_eq!(receipt(&host, t), Err(ERR_NOT_FOUND));
    // The transaction is open on the book: it cannot be deleted meanwhile
    // (the service's own check), and an abort drops what it staged.
    call(&host, 21, &[tref(second)]).unwrap();
}

#[test]
fn the_x4_holds_a_transaction_to_128_edits_and_elsewhere_it_is_1024() {
    for (small, cap) in [(true, 128_u32), (false, 1024)] {
        let card = card();
        let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
        let host: Rc<dyn LibHost> = Rc::new(if small { library.small() } else { library });
        let t = begin(&host, 1);
        // Every edit is a call of its own; the one after the cap is refused.
        for _ in 0..cap {
            let mut one = edit();
            one.text_bytes = 0;
            one.text = "x";
            call(&host, 18, &[tref(t), Record::ServiceLibEdit(one)]).unwrap();
        }
        let mut one = edit();
        one.text_bytes = 0;
        one.text = "x";
        assert_eq!(
            call(&host, 18, &[tref(t), Record::ServiceLibEdit(one)]),
            Err(ERR_NO_SPACE)
        );
    }
}
