//! Places and notes kept from earlier versions or a card that
//! cannot be read: never written over.

use super::*;

/// Where the reader kept the position of `book` before positions were
/// shared.
fn old_progress(book: &str) -> String {
    format!(
        ".paperpaw/plugins/reader/books/{:016x}/progress.ini",
        paperpaw_sdk::digest::fnv1a64(book)
    )
}

/// A position that is there but cannot be read (cut by a power cut, or a
/// card that fails to read it) is not written over by the start of the
/// book, nor by the reader's old `progress.ini`, when the book is opened
/// and closed or the device sleeps: only once the reader turns a page.
#[test]
fn a_position_that_cannot_be_read_is_not_written_over() {
    let storage = Shared::default();
    let file = paperpaw_sdk::positions::file(BOOK);
    let progress = old_progress(BOOK);
    storage
        .write(&progress, b"[progress]\nsection = 0\noffset = 40\n")
        .unwrap();
    let name: &'static str = Box::leak(file.clone().into_boxed_str());
    for (damaged, refused) in [
        (&b"[position]\nsection = 0\noff"[..], None),
        (&b"[position]\nsection = 0\noffset = 9000\n"[..], Some(name)),
    ] {
        storage.write(&file, damaged).unwrap();
        // The file is there, but reading it fails.
        storage.2.set(refused);
        let mut reading = Reading::new(storage.clone());
        reading.open();
        reading.run(core_actions::BACK);
        reading.now += 10;
        let effects = reading.kernel.suspend(Instant(reading.now));
        reading.settle(effects);
        assert_eq!(storage.0.get(&file).unwrap(), damaged, "written over");
        assert!(storage.exists(&progress));
    }
    // Once a page is turned, the place read on is kept.
    let mut reading = Reading::new(storage.clone());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    assert!(reading.kept().1 > 0);
}

/// A position the card fails to read is moved aside before the place
/// read on is kept: its text is never lost, and the reader is told once.
#[test]
fn a_position_that_cannot_be_read_is_kept_aside() {
    let storage = Shared::default();
    let file = paperpaw_sdk::positions::file(BOOK);
    let kept = b"[position]\nsection = 0\noffset = 9000\n";
    storage.write(&file, kept).unwrap();
    storage
        .2
        .set(Some(Box::leak(file.clone().into_boxed_str())));
    let (mut reading, notices) = with_notices(storage.clone());
    for _ in 0..3 {
        reading.run(core_actions::NEXT_PAGE);
    }
    reading.run(core_actions::BACK);
    let aside = format!("{file}.bad");
    let found = [&file, &aside]
        .into_iter()
        .any(|path| storage.0.get(path).is_some_and(|data| data == kept));
    assert!(found, "the position kept is lost");
    let said = notices
        .borrow()
        .iter()
        .filter(|n| n.contains("kept aside"))
        .count();
    assert_eq!(said, 1, "{:?}", notices.borrow());
}

/// A position the card fails to read once is read at the second try:
/// the book opens there.
#[test]
fn a_position_read_at_the_second_try_opens_the_book_there() {
    let storage = Shared::default();
    let file = paperpaw_sdk::positions::file(BOOK);
    storage
        .write(&file, b"[position]\nsection = 0\noffset = 9000\n")
        .unwrap();
    let mut reading = Reading::new(storage.clone());
    storage.2.set(Some(Box::leak(file.into_boxed_str())));
    // Twice: the kernel tries a small file a second time by itself.
    storage.4.set(Some(2));
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let (_, offset) = reading.kept();
    assert!(offset > 8000, "opened at {offset}");
}

/// The reader's old `progress.ini` is read where no position is kept, kept
/// as the position, and then removed: it never comes back over a newer
/// one.
#[test]
fn the_old_progress_file_is_taken_over_once() {
    let storage = Shared::default();
    let progress = old_progress(BOOK);
    storage
        .write(&progress, b"[progress]\nsection = 0\noffset = 40\n")
        .unwrap();
    let mut reading = Reading::new(storage.clone());
    reading.open();
    let kept = reading.kept();
    assert!(kept.1 > 0, "{kept:?}");
    assert!(!storage.exists(&progress), "the old file is still there");
}

/// The reader's annotations of before are moved where annotations are
/// kept now, but never over those kept there: then both stay.
#[test]
fn old_annotations_never_replace_those_kept_now() {
    let storage = Shared::default();
    let old = old_progress(BOOK).replace("progress.ini", "annotations.txt");
    let new = paperpaw_sdk::annotations::file(BOOK);
    let (before, now) = (
        b"b\t0\t0\t0\t0\t1\told\t\n".to_vec(),
        b"b\t0\t9\t0\t9\t2\t0\t0\tnow\t\n".to_vec(),
    );
    storage.write(&old, &before).unwrap();
    storage.write(&new, &now).unwrap();
    // Their annotations: the file also says which book they are of.
    let texts = |path: &str| -> Vec<String> {
        let all = paperpaw_sdk::annotations::load(&storage, path);
        all.into_iter().map(|a| a.text).collect()
    };
    let mut reading = Reading::new(storage.clone());
    reading.open();
    assert_eq!(texts(BOOK), ["now"]);
    assert_eq!(storage.0.get(&old).unwrap(), before);
    // Without annotations kept now, the old ones are moved.
    storage.0.remove(&new).unwrap();
    reading.run(core_actions::BACK);
    let mut reading = Reading::new(storage.clone());
    reading.open();
    assert_eq!(texts(BOOK), ["old"]);
    assert!(!storage.exists(&old));
}

/// A book's position and annotations say which book they are of by its
/// content, its size and KOReader's partial MD5, a text and an EPUB
/// alike; taken when the book opens, as the book is.
#[test]
fn positions_and_annotations_say_which_book_they_are_of() {
    let text = include_str!("../../../../samples/library/books/alice-in-wonderland.txt");
    let epub = include_bytes!("../../../../samples/library/books/alice-in-wonderland.epub");
    for (path, data) in [(BOOK, text.as_bytes()), (EPUB, &epub[..])] {
        let storage = Shared::default();
        let mut reading = Reading::with_book(storage.clone(), path, data);
        reading.open();
        reading.run(core_actions::NEXT_PAGE);
        reading.run(actions::TOGGLE_BOOKMARK);
        reading.run(core_actions::BACK);
        let fingerprint = paperpaw_sdk::library::Fingerprint::of(&storage, path).unwrap();
        assert_eq!(fingerprint.size, data.len() as u64);
        assert_eq!(
            paperpaw_sdk::positions::book(&storage, path).as_ref(),
            Some(&fingerprint),
            "{path}"
        );
        assert_eq!(
            paperpaw_sdk::annotations::book(&storage, path),
            Some(fingerprint),
            "{path}"
        );
    }
}

/// A page turned is not kept while it turns: a moment later, once the
/// pages rest, without the book being closed.
#[test]
fn the_place_is_kept_once_the_pages_rest() {
    let storage = Shared::default();
    let mut reading = Reading::new(storage);
    reading.open();
    let before = reading.written();
    reading.run(core_actions::NEXT_PAGE);
    assert_eq!(reading.written(), before, "written while the page turned");
    reading.now += 2_000;
    let effects = reading.kernel.tick(Instant(reading.now));
    reading.settle(effects);
    assert!(reading.written() > before, "not written at rest");
}
