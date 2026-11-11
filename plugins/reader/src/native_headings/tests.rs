use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use paperpaw_sdk::MemoryStorage;

/// A book of sections of blocks; a block is a heading level (0: a paragraph)
/// and a text, 10 source bytes each.
type Book = Vec<Vec<(u8, &'static str)>>;

fn reader(book: &Book) -> impl FnMut(Place) -> Result<Option<Seen>, Error> + '_ {
    move |at| {
        let Some(section) = book.get(at.section as usize) else {
            return Ok(None);
        };
        let index = (at.offset / 10) as usize;
        Ok(section.get(index).map(|(level, text)| Seen {
            heading: (*level > 0).then_some(*level),
            text: (*text).to_string(),
            start: index as u32 * 10,
            end: index as u32 * 10 + 10,
        }))
    }
}

fn sample() -> Book {
    alloc::vec![
        alloc::vec![(2, " Part  one "), (0, "text"), (3, "A\nsection")],
        alloc::vec![(0, "text"), (2, "Two"), (2, "   ")],
        alloc::vec![],
        alloc::vec![(1, "Three")],
    ]
}

fn run(scan: &mut Scan, storage: &MemoryStorage, book: &Book) -> Step {
    let mut read = reader(book);
    loop {
        match scan.step(storage, &mut read) {
            Step::More => {}
            end => return end,
        }
    }
}

#[test]
fn headings_are_found_with_their_places_and_levels_from_the_highest() {
    let storage = MemoryStorage::new();
    let book = sample();
    let mut scan = Scan::new("/b/x.epub", 100, 4).unwrap();
    assert_eq!(run(&mut scan, &storage, &book), Step::Done);
    let found: Vec<(String, u8, u32, u32)> = scan
        .entries()
        .iter()
        .map(|e| (e.title.clone(), e.level, e.at.section, e.at.offset))
        .collect();
    assert_eq!(
        found,
        [
            ("Part one".to_string(), 1, 0, 0),
            ("A section".to_string(), 2, 0, 20),
            ("Two".to_string(), 1, 1, 10),
            ("Three".to_string(), 0, 3, 0),
        ]
    );
    assert_eq!(scan.percent(), 100);
    // Done stays done.
    assert_eq!(scan.step(&storage, &mut reader(&book)), Step::Done);
}

#[test]
fn the_kept_copy_is_read_back_for_the_same_size_only() {
    let storage = MemoryStorage::new();
    let book = sample();
    let mut scan = Scan::new("/b/x.epub", 100, 4).unwrap();
    run(&mut scan, &storage, &book);
    let kept = load(&storage, "/b/x.epub", 100).unwrap().unwrap();
    assert_eq!(kept.as_slice(), scan.entries());
    assert_eq!(load(&storage, "/b/x.epub", 101).unwrap(), None);
    assert_eq!(load(&storage, "/b/other.epub", 100).unwrap(), None);
    // No temporary file stays.
    let name = file_name("/b/x.epub").unwrap();
    assert!(storage.size(&name).is_ok());
    assert!(storage.size(&[name.as_str(), ".new"].concat()).is_err());
}

#[test]
fn a_step_reads_a_few_blocks_and_the_percent_follows_the_sections() {
    let storage = MemoryStorage::new();
    let mut book: Book = Vec::new();
    for _ in 0..200 {
        book.push(alloc::vec![(0, "p"), (2, "H")]);
    }
    let mut scan = Scan::new("/b/long", 1, 200).unwrap();
    let mut read = reader(&book);
    assert_eq!(scan.step(&storage, &mut read), Step::More);
    // 60 reads: two blocks and the section's end each.
    assert_eq!(scan.entries().len(), 20);
    assert_eq!(scan.percent(), 10);
    // 500 headings are the most; this book has 200.
    while scan.step(&storage, &mut read) == Step::More {}
    assert_eq!(scan.entries().len(), 200);
}

#[test]
fn at_most_the_limit_of_headings_is_kept() {
    let storage = MemoryStorage::new();
    let book: Book = (0..MAX_HEADINGS + 50)
        .map(|_| alloc::vec![(2, "H")])
        .collect();
    let mut scan = Scan::new("/b/many", 1, book.len() as u32).unwrap();
    assert_eq!(run(&mut scan, &storage, &book), Step::Done);
    assert_eq!(scan.entries().len(), MAX_HEADINGS);
}

#[test]
fn an_unreadable_section_is_passed_and_a_busy_book_stops_the_scan() {
    let storage = MemoryStorage::new();
    let mut scan = Scan::new("/b/bad", 1, 3).unwrap();
    let mut read = |at: Place| match at.section {
        0 => Err(Error::Invalid),
        1 if at.offset == 0 => Ok(Some(Seen {
            heading: Some(1),
            text: "One".to_string(),
            start: 0,
            end: 5,
        })),
        _ => Ok(None),
    };
    assert_eq!(scan.step(&storage, &mut read), Step::Done);
    assert_eq!(scan.entries().len(), 1);
    let mut scan = Scan::new("/b/busy", 1, 3).unwrap();
    assert_eq!(
        scan.step(&storage, &mut |_| Err(Error::Busy)),
        Step::Stopped
    );
    for error in [Error::Cancelled, Error::Denied, Error::State] {
        let mut scan = Scan::new("/b/closed", 1, 3).unwrap();
        assert_eq!(scan.step(&storage, &mut |_| Err(error)), Step::Stopped);
        assert!(scan.entries().is_empty());
    }
    // Stopped stays stopped and keeps nothing.
    assert_eq!(scan.step(&storage, &mut |_| Ok(None)), Step::Stopped);
    assert_eq!(load(&storage, "/b/busy", 1).unwrap(), None);
}

#[test]
fn a_block_that_does_not_move_on_cannot_hold_the_scan() {
    let storage = MemoryStorage::new();
    let mut scan = Scan::new("/b/stuck", 1, 1).unwrap();
    let mut calls = 0;
    let mut read = |_: Place| {
        calls += 1;
        Ok(Some(Seen {
            heading: None,
            text: String::new(),
            start: 0,
            end: 0,
        }))
    };
    // Each read goes at least one byte on: a step ends after its blocks.
    assert_eq!(scan.step(&storage, &mut read), Step::More);
    assert_eq!(calls, STEP_BLOCKS);
}

#[test]
fn old_files_are_removed_beyond_the_limit() {
    let storage = MemoryStorage::new();
    let book = sample();
    for n in 0..FILES_KEPT + 2 {
        let path = ["/b/", &n.to_string()].concat();
        let mut scan = Scan::new(&path, 7, 4).unwrap();
        run(&mut scan, &storage, &book);
    }
    assert_eq!(load(&storage, "/b/0", 7).unwrap(), None);
    assert!(load(&storage, "/b/1", 7).unwrap().is_none());
    assert!(load(&storage, "/b/2", 7).unwrap().is_some());
}

#[test]
fn allocation_refused_in_turn_is_no_memory() {
    let storage = MemoryStorage::new();
    let book = sample();
    let mut scan = Scan::new("/b/x.epub", 100, 4).unwrap();
    run(&mut scan, &storage, &book);
    sweep(|| {
        Scan::new("/b/x.epub", 100, 4)?;
        load(&storage, "/b/x.epub", 100).map(|_| ())
    });
    // A refusal in a step stops the scan without keeping anything.
    let mut scan = Scan::new("/b/refused", 1, 3).unwrap();
    assert_eq!(
        scan.step(&storage, &mut |_| Err(Error::NoMemory)),
        Step::Stopped
    );
    assert_eq!(load(&storage, "/b/refused", 1).unwrap(), None);
}
