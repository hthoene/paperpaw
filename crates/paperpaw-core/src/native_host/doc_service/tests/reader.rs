//! The reader hooks (`reader_opened`, `reader_closed`) and the place a book
//! to show takes (`document.md` 2.1, reason 4 `needed`).
use super::*;

/// Open one document of `owner`; `flags` are the `doc_open` flags.
fn open_as(fake: &Rc<Fake>, owner: u64, flags: u32) -> Result<u32, i32> {
    fake.owner.set(owner);
    *fake.answer.borrow_mut() = state_answer(0, 0);
    let mut open = open_record();
    open.flags = flags;
    let reply = call(fake, 1, &[Record::ServiceDocOpen(open)]);
    fake.owner.set(OWNER);
    reply.map(|bytes| state_of(&bytes).document)
}
const READING: u32 = 0b001;
const BACKGROUND: u32 = 0b101;

#[test]
fn an_open_names_the_book_to_the_host_once_and_close_releases_it() {
    let fake = Fake::new(Limits::LARGE);
    let id = opened(&fake);
    assert_eq!(*fake.readers.borrow(), [(id, 1, "books/a.epub".into())]);
    assert!(fake.released.borrow().is_empty());
    call(&fake, 18, &[doc_ref(id)]).unwrap();
    assert_eq!(*fake.released.borrow(), [id]);
}

#[test]
fn a_refused_book_opens_nothing_and_the_error_is_the_opens() {
    let fake = Fake::new(Limits::LARGE);
    for error in [ERR_FULL, ERR_BUSY] {
        fake.reader_refuses.set(Some(error));
        assert_eq!(open_as(&fake, OWNER, READING), Err(error));
        assert_eq!(fake.table.len(), 0);
        assert!(fake.calls.borrow().is_empty(), "the format is never called");
        assert!(fake.released.borrow().is_empty());
    }
}

#[test]
fn a_failed_open_releases_the_book() {
    let fake = Fake::new(Limits::LARGE);
    fake.fails.set(Some(ERR_INVALID));
    assert_eq!(open_as(&fake, OWNER, READING), Err(ERR_INVALID));
    let readers = fake.readers.borrow();
    assert_eq!(readers.len(), 1);
    assert_eq!(*fake.released.borrow(), [readers[0].0]);
}

#[test]
fn every_way_a_document_ends_releases_its_book_once() {
    // The opener stops.
    let fake = Fake::new(Limits::LARGE);
    let a = opened(&fake);
    let b = opened(&fake);
    assert_eq!(opener_stopped(&*fake, OWNER), Ok(2));
    assert_eq!(*fake.released.borrow(), [a, b]);

    // The format stops; the opener's later `close` does not release again.
    let fake = Fake::new(Limits::LARGE);
    let a = opened(&fake);
    assert_eq!(format_stopped(&*fake, FORMAT), Ok(1));
    assert_eq!(*fake.released.borrow(), [a]);
    call(&fake, 18, &[doc_ref(a)]).unwrap();
    assert_eq!(*fake.released.borrow(), [a]);

    // The host ends it (card gone: 2, book moved or removed: 3), once.
    let fake = Fake::new(Limits::LARGE);
    let a = opened(&fake);
    let b = opened(&fake);
    assert_eq!(end_by_host(&*fake, a, 2), Ok(()));
    assert_eq!(end_by_host(&*fake, b, 3), Ok(()));
    assert_eq!(end_by_host(&*fake, a, 3), Ok(()));
    assert_eq!(*fake.closed.borrow(), [(a, 2), (b, 3)]);
    assert_eq!(*fake.released.borrow(), [a, b]);
    assert_eq!(call(&fake, 2, &[doc_ref(a)]), Err(ERR_CANCELLED));
    call(&fake, 18, &[doc_ref(a)]).unwrap();
    call(&fake, 18, &[doc_ref(b)]).unwrap();
    assert_eq!(*fake.released.borrow(), [a, b]);
    assert_eq!(end_by_host(&*fake, 77, 2), Err(ERR_NOT_FOUND));
    assert_eq!(end_by_host(&*fake, a, 4), Err(ERR_INVALID));
    assert_eq!(end_by_host(&*fake, a, 0), Err(ERR_INVALID));
}

#[test]
fn a_book_to_show_takes_the_first_background_document() {
    let fake = Fake::new(Limits::X4);
    // Two openers fill the device: 8 (not background) then 9 (background).
    let first = open_as(&fake, 8, READING).unwrap();
    let second = open_as(&fake, 9, BACKGROUND).unwrap();
    fake.shows.set(1 << 8);
    let shown = open_as(&fake, OWNER, READING).unwrap();
    // The background one went, as by `close`: images end, the format gets op
    // 4, the reader is released, its opener hears reason 4.
    assert_eq!(*fake.closed.borrow(), [(second, 4)]);
    assert_eq!(*fake.ended.borrow(), [second]);
    assert_eq!(*fake.images_ended.borrow(), [second]);
    assert_eq!(*fake.released.borrow(), [second]);
    fake.owner.set(9);
    assert_eq!(call(&fake, 2, &[doc_ref(second)]), Err(ERR_CANCELLED));
    // Its `close` succeeds and ends nothing twice.
    call(&fake, 18, &[doc_ref(second)]).unwrap();
    assert_eq!(*fake.ended.borrow(), [second]);
    assert_eq!(*fake.released.borrow(), [second]);
    call(&fake, 18, &[doc_ref(second)]).unwrap();
    assert_eq!(*fake.released.borrow(), [second]);
    fake.owner.set(OWNER);
    assert_eq!(fake.table.lease(8, first).unwrap().id, first);
    assert_eq!(fake.table.lease(OWNER, shown).unwrap().id, shown);
}

#[test]
fn without_a_background_document_the_oldest_of_a_hidden_opener_goes() {
    let fake = Fake::new(Limits::X4);
    let first = open_as(&fake, 8, READING).unwrap();
    let second = open_as(&fake, 9, READING).unwrap();
    // 8 shows a screen: 9's goes, whatever the age.
    fake.shows.set(1 << 8);
    open_as(&fake, OWNER, READING).unwrap();
    assert_eq!(*fake.closed.borrow(), [(second, 4)]);
    // 8 and the reader hold the places and both show a screen: nothing may be
    // taken for a third.
    fake.shows.set((1 << 8) | (1 << OWNER));
    assert_eq!(open_as(&fake, 11, READING), Err(ERR_FULL));
    assert_eq!(fake.table.lease(8, first).unwrap().id, first);
}

#[test]
fn the_oldest_hidden_document_goes_first() {
    let fake = Fake::new(Limits {
        per_owner: 2,
        device: 3,
    });
    let a = open_as(&fake, 8, READING).unwrap();
    let _b = open_as(&fake, 9, READING).unwrap();
    let _c = open_as(&fake, 10, READING).unwrap();
    open_as(&fake, OWNER, READING).unwrap();
    assert_eq!(*fake.closed.borrow(), [(a, 4)]);
}

#[test]
fn a_background_open_never_takes_a_place_and_the_openers_own_cap_holds() {
    let fake = Fake::new(Limits::X4);
    open_as(&fake, 8, READING).unwrap();
    open_as(&fake, 9, READING).unwrap();
    assert_eq!(open_as(&fake, OWNER, BACKGROUND), Err(ERR_FULL));
    assert!(fake.closed.borrow().is_empty());
    // The opener's own cap (one on the X4) is not made room for by taking
    // another's document.
    let fake = Fake::new(Limits::X4);
    open_as(&fake, OWNER, READING).unwrap();
    assert_eq!(open_as(&fake, OWNER, READING), Err(ERR_FULL));
    assert!(fake.closed.borrow().is_empty());
}

#[test]
fn a_refused_book_after_a_taken_place_keeps_the_place_free() {
    let fake = Fake::new(Limits::X4);
    open_as(&fake, 8, READING).unwrap();
    let second = open_as(&fake, 9, BACKGROUND).unwrap();
    // The in-use table is full even after the taken document released: the
    // open fails, the taken document stays ended.
    fake.reader_refuses.set(Some(ERR_FULL));
    assert_eq!(open_as(&fake, OWNER, READING), Err(ERR_FULL));
    assert_eq!(*fake.closed.borrow(), [(second, 4)]);
    assert_eq!(fake.table.len(), 1);
}
