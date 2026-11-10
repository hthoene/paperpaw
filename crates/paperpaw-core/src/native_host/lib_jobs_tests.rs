#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::lib_moves_journal::JOURNAL;
use paperpaw_platform::DirEntry;
use paperpaw_sdk::MemoryStorage;
use std::{cell::Cell, string::ToString, vec::Vec};

/// A card that lets `allowed` changing actions through and then fails them
/// (a power cut, as the next start sees the card); `heal` ends the cut.
struct Cut<'a> {
    card: &'a MemoryStorage,
    done: Cell<usize>,
    allowed: Cell<usize>,
}

impl<'a> Cut<'a> {
    fn after(card: &'a MemoryStorage, allowed: usize) -> Self {
        Self {
            card,
            done: Cell::new(0),
            allowed: Cell::new(allowed),
        }
    }
    fn heal(&self) {
        self.allowed.set(usize::MAX);
    }
    fn go(&self) -> Result<(), StorageError> {
        let n = self.done.get();
        self.done.set(n + 1);
        if n >= self.allowed.get() {
            return Err(StorageError::Io);
        }
        Ok(())
    }
}

impl Storage for Cut<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.go()?;
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.go()?;
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.create_dir(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.go()
    }
    fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
        self.card.entry_identity(p)
    }
}

/// A step's host: the card, what was asked of the catalog, in order.
struct Host<'a> {
    card: &'a dyn Storage,
    log: std::cell::RefCell<Vec<std::string::String>>,
    /// Pieces of kept data a book has: the step answers `More` this often.
    pieces: Cell<u32>,
    /// The status `forget` answers in a phase, and `catalogue` (`None`:
    /// none).
    forget_fails: Cell<Option<(RemovalPhase, i32)>>,
    catalogue_fails: Cell<Option<i32>>,
}

impl<'a> Host<'a> {
    fn new(card: &'a dyn Storage) -> Self {
        Self {
            card,
            log: Default::default(),
            pieces: Cell::new(0),
            forget_fails: Cell::new(None),
            catalogue_fails: Cell::new(None),
        }
    }
    fn saw(&self, what: &str) -> usize {
        self.log
            .borrow()
            .iter()
            .filter(|line| *line == what)
            .count()
    }
    fn note(&self, line: std::string::String) {
        self.log.borrow_mut().push(line);
    }
}

impl RemovalCarry for Host<'_> {
    fn forget(&self, path: &str, notes: bool, phase: RemovalPhase) -> Result<RemovalPhase, i32> {
        self.note(std::format!("forget {path} {notes} {phase:?}"));
        if let Some((fails, status)) = self.forget_fails.get()
            && fails == phase
        {
            return Err(status);
        }
        Ok(match phase {
            RemovalPhase::Details => RemovalPhase::Marks,
            _ => RemovalPhase::Done,
        })
    }
}

impl ImportCarry for Host<'_> {
    fn catalogue(&self, path: &str, importer: &str) -> Result<(), i32> {
        self.note(std::format!("catalogue {path} {importer}"));
        self.catalogue_fails.get().map_or(Ok(()), Err)
    }
}

impl JobHost for Host<'_> {
    fn storage(&self) -> &dyn Storage {
        self.card
    }
    fn remove_kept(&self, path: &str) -> Result<Progress, i32> {
        self.note(std::format!("kept {path}"));
        match self.pieces.get() {
            0 => Ok(Progress::Done),
            n => {
                self.pieces.set(n - 1);
                Ok(Progress::More)
            }
        }
    }
    fn remove_cache(&self, path: &str) -> Result<Progress, i32> {
        self.note(std::format!("cache {path}"));
        Ok(Progress::Done)
    }
    fn removed(&self, path: &str) -> Result<(), i32> {
        self.note(std::format!("removed {path}"));
        Ok(())
    }
    fn changed(&self) -> Result<(), i32> {
        self.note("changed".to_string());
        Ok(())
    }
}

const BOOKS: [&str; 3] = ["books/a.epub", "books/sub/b.txt", "books/c.fb2"];

fn card() -> MemoryStorage {
    let card = MemoryStorage::new();
    for (n, path) in BOOKS.iter().enumerate() {
        card.write(path, &[b'x'; 5][..n + 1]).unwrap();
    }
    card.write("books/a.epub.bak", b"old").unwrap();
    card.write("data/feed/new.epub", b"new book").unwrap();
    card
}

/// Steps to the end; the number of steps.
fn run(jobs: &Jobs, host: &Host<'_>) -> usize {
    let mut steps = 0;
    while jobs.step(host).unwrap() {
        steps += 1;
        assert!(steps < 200, "never ends");
    }
    steps
}

#[test]
fn a_delete_removes_the_files_at_once_and_the_rest_in_steps() {
    let card = card();
    let jobs = Jobs::default();
    let answers = jobs.begin_delete(&card, &BOOKS, true).unwrap();
    assert_eq!(answers, [0; DELETE_MAX]);
    // The entry is on the card and the files are gone, `.bak` too.
    assert!(card.exists(JOURNAL));
    assert!(BOOKS.iter().all(|path| !card.exists(path)));
    assert!(!card.exists("books/a.epub.bak"));
    assert!(jobs.active() && jobs.pending_delete(&|p| p == "books/sub/b.txt"));
    assert!(!jobs.pending_delete(&|p| p == "books/other.txt"));
    // One job at a time, of any kind.
    assert_eq!(jobs.begin_delete(&card, &BOOKS[..1], false), Err(ERR_BUSY));
    assert_eq!(
        jobs.begin_import(&card, "data/feed/new.epub", "books/n.epub", "feed", false),
        Err(ERR_BUSY)
    );
    let host = Host::new(&card);
    host.pieces.set(2);
    run(&jobs, &host);
    assert!(!card.exists(JOURNAL) && !jobs.active());
    assert!(!jobs.pending_delete(&|p| p == "books/a.epub"));
    let log = host.log.borrow().join("|");
    // Book by book: details, marks (with the notes flag), data, event; one
    // `library_changed` at the end.
    assert!(
        log.starts_with(
            "forget books/a.epub true Details|forget books/a.epub true Marks|kept books/a.epub|kept books/a.epub|kept books/a.epub|removed books/a.epub|forget books/sub/b.txt"
        ),
        "{log}"
    );
    assert_eq!(host.saw("changed"), 1);
    assert!(log.ends_with("removed books/c.fb2|changed"), "{log}");
}

#[test]
fn a_delete_refuses_what_cannot_work_and_changes_nothing() {
    let card = card();
    let jobs = Jobs::default();
    assert_eq!(jobs.begin_delete(&card, &[], false), Err(ERR_INVALID));
    let eight = ["a"; DELETE_MAX + 1];
    assert_eq!(jobs.begin_delete(&card, &eight, false), Err(ERR_INVALID));
    assert!(!card.exists(JOURNAL) && !jobs.active());
    // A move's entry in the journal: not written over.
    card.write(JOURNAL, b"entry of another job").unwrap();
    assert!(jobs.begin_delete(&card, &BOOKS, false).is_err());
    assert!(BOOKS.iter().all(|path| card.exists(path)));
    assert_eq!(card.size(JOURNAL).unwrap(), 20, "the entry is as it was");
}

/// A book that is gone already is deleted (its data still goes); a file the
/// card refuses for good is left, with its data, and the rest goes on.
#[test]
fn a_book_that_is_gone_is_forgotten_and_one_the_card_refuses_is_left() {
    struct Refuses<'a>(&'a MemoryStorage);
    impl Storage for Refuses<'_> {
        fn read_at(&self, p: &str, a: u64, b: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(p, a, b)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.append(p, d)
        }
        fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            if p.ends_with("b.txt") {
                return Err(StorageError::Unsupported);
            }
            self.0.remove(p)
        }
        fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
            self.0.entry_identity(p)
        }
        fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
            self.0.rename(f, t)
        }
    }
    let card = card();
    card.remove(BOOKS[0]).unwrap();
    let card = Refuses(&card);
    let jobs = Jobs::default();
    let answers = jobs.begin_delete(&card, &BOOKS, false).unwrap();
    assert_eq!(answers[..3], [0, ERR_IO, 0]);
    let host = Host::new(&card);
    run(&jobs, &host);
    assert!(card.0.exists("books/sub/b.txt"), "left as it is");
    assert_eq!(host.saw("kept books/sub/b.txt"), 0);
    assert_eq!(host.saw("removed books/sub/b.txt"), 0);
    assert_eq!(host.saw("forget books/sub/b.txt false Details"), 0);
    // The others were carried.
    assert_eq!(host.saw("removed books/a.epub"), 1);
    assert_eq!(host.saw("removed books/c.fb2"), 1);
    assert!(!card.0.exists(JOURNAL));
}

/// A cut at every changing action: after a restart on the healed card the job
/// finishes (the entry is on the card or it is not): **all** books of a
/// durable entry are gone and carried, none of a missing one is touched.
#[test]
fn a_delete_cut_at_every_action_is_finished_or_never_started() {
    let mut cuts_seen = 0;
    for allowed in 0..60 {
        let card = card();
        let cut = Cut::after(&card, allowed);
        let jobs = Jobs::default();
        let started = jobs.begin_delete(&cut, &BOOKS, true);
        let durable = card.exists(JOURNAL);
        if !durable {
            assert!(started.is_err(), "{allowed}");
            assert!(BOOKS.iter().all(|path| card.exists(path)), "{allowed}");
        }
        // The steps that follow, until the card goes.
        let first = Host::new(&cut);
        for _ in 0..50 {
            if jobs.step(&first).is_err() {
                cuts_seen += 1;
                break;
            }
        }
        // Restart: a new machine on the healed card.
        cut.heal();
        let again = Jobs::default();
        again.start(&cut).unwrap();
        let host = Host::new(&cut);
        run(&again, &host);
        assert!(!card.exists(JOURNAL) && !again.active(), "{allowed}");
        if durable {
            assert!(BOOKS.iter().all(|path| !card.exists(path)), "{allowed}");
            assert!(!card.exists("books/a.epub.bak"), "{allowed}");
            // Every book was carried, by the machine that was cut or by the
            // one that ended it (events are at least once).
            let seen = |what: &str| host.saw(what) + first.saw(what);
            assert!(seen("changed") >= 1, "{allowed}");
            for path in BOOKS {
                assert!(seen(&std::format!("removed {path}")) >= 1, "{allowed}");
                assert!(
                    seen(&std::format!("forget {path} true Marks")) >= 1,
                    "{allowed}"
                );
            }
        } else {
            assert!(BOOKS.iter().all(|path| card.exists(path)), "{allowed}");
        }
    }
    assert!(cuts_seen > 0, "the cut reached the steps");
}

/// M1: a replay (or a later book's step) never deletes a file that was put at
/// the path after the entry was written: only the file the entry names, by the
/// storage's identity, is removed.
#[test]
fn a_file_put_at_the_path_since_is_never_deleted() {
    // A cut after the entry, before any file went: a new file is put there
    // (copied back by a PC), then the start finishes the job.
    let card = card();
    let cut = Cut::after(&card, 3);
    let jobs = Jobs::default();
    let _ = jobs.begin_delete(&cut, &BOOKS, false);
    assert!(card.exists(JOURNAL), "the entry is on the card");
    card.remove(BOOKS[1]).ok();
    card.write(BOOKS[1], b"copied back").unwrap();
    card.remove("books/a.epub.bak").unwrap();
    card.write("books/a.epub.bak", b"a newer bak").unwrap();
    cut.heal();
    let again = Jobs::default();
    again.start(&cut).unwrap();
    let host = Host::new(&cut);
    run(&again, &host);
    let mut buffer = [0_u8; 16];
    let n = card.read_at(BOOKS[1], 0, &mut buffer).unwrap();
    assert_eq!(&buffer[..n], b"copied back");
    assert!(
        card.exists("books/a.epub.bak"),
        "a .bak that is not the entry's stays"
    );
    // The books that were not replaced went; the new file at the path stays,
    // and what was kept for the deleted book goes all the same.
    assert!(!card.exists(BOOKS[0]) && !card.exists(BOOKS[2]));
    assert_eq!(host.saw(&std::format!("kept {}", BOOKS[1])), 1);
    assert_eq!(host.saw(&std::format!("removed {}", BOOKS[0])), 1);
    assert!(!card.exists(JOURNAL));
}

/// Without a restart: a book's own step finds a file there that the entry
/// does not name.
#[test]
fn a_file_put_there_between_the_call_and_the_book_step_stays() {
    let card = card();
    let jobs = Jobs::default();
    jobs.begin_delete(&card, &BOOKS, false).unwrap();
    card.write(BOOKS[2], b"downloaded again").unwrap();
    let host = Host::new(&card);
    run(&jobs, &host);
    assert!(card.exists(BOOKS[2]));
    assert_eq!(host.saw(&std::format!("kept {}", BOOKS[2])), 1);
    assert!(!card.exists(JOURNAL));
}

/// A book the storage cannot tell from another after a cut is refused; one
/// that is gone is answered and carried.
#[test]
fn a_book_without_an_identity_is_refused_and_a_gone_one_is_carried() {
    struct Blind<'a>(&'a MemoryStorage);
    impl Storage for Blind<'_> {
        fn read_at(&self, p: &str, a: u64, b: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(p, a, b)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.append(p, d)
        }
        fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            self.0.remove(p)
        }
        fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
            self.0.rename(f, t)
        }
        fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
            if p.ends_with("c.fb2") {
                return Ok(None);
            }
            self.0.entry_identity(p)
        }
    }
    let card = card();
    card.remove(BOOKS[0]).unwrap();
    let blind = Blind(&card);
    let jobs = Jobs::default();
    let answers = jobs.begin_delete(&blind, &BOOKS, false).unwrap();
    assert_eq!(answers[..3], [0, 0, ERR_UNSUPPORTED]);
    assert!(card.exists(BOOKS[2]), "refused, untouched");
    let host = Host::new(&blind);
    run(&jobs, &host);
    assert_eq!(host.saw("removed books/a.epub"), 1, "only forgotten");
    assert_eq!(host.saw("removed books/c.fb2"), 0);
    // Nothing accepted: no entry.
    let alone = Jobs::default();
    assert_eq!(
        alone.begin_delete(&blind, &BOOKS[2..], false).unwrap()[0],
        ERR_UNSUPPORTED
    );
    assert!(!card.exists(JOURNAL) && !alone.active());
}

#[test]
fn an_import_puts_the_file_in_the_library_and_catalogues_it() {
    let card = card();
    let jobs = Jobs::default();
    jobs.begin_import(
        &card,
        "data/feed/new.epub",
        "books/feeds/n.epub",
        "feed",
        false,
    )
    .unwrap();
    // The rename is done at once, the missing folder made; the entry is there.
    assert!(card.exists("books/feeds/n.epub") && !card.exists("data/feed/new.epub"));
    assert!(card.exists(JOURNAL) && jobs.active());
    assert_eq!(
        jobs.begin_import(&card, "books/c.fb2", "books/d.fb2", "feed", false),
        Err(ERR_BUSY)
    );
    let host = Host::new(&card);
    run(&jobs, &host);
    assert!(!card.exists(JOURNAL) && !jobs.active());
    assert_eq!(
        host.log.borrow().join("|"),
        "catalogue books/feeds/n.epub feed|changed"
    );
}

#[test]
fn an_import_refuses_what_it_must_and_changes_nothing() {
    let card = card();
    let jobs = Jobs::default();
    let imp = |from: &str, to: &str, replace| jobs.begin_import(&card, from, to, "feed", replace);
    assert_eq!(
        imp("data/feed/none.epub", "books/n.epub", false),
        Err(ERR_NOT_FOUND)
    );
    // A folder is no file to import; an existing book is never overwritten
    // without `replace`; a folder is never replaced.
    assert_eq!(imp("data/feed", "books/n.epub", false), Err(ERR_INVALID));
    assert_eq!(
        imp("data/feed/new.epub", "books/a.epub", false),
        Err(ERR_STATE)
    );
    assert_eq!(imp("data/feed/new.epub", "books/sub", true), Err(ERR_STATE));
    assert_eq!(
        imp("data/feed/new.epub", "data/feed/new.epub", true),
        Err(ERR_INVALID)
    );
    assert!(!card.exists(JOURNAL) && !jobs.active());
    assert!(card.exists("data/feed/new.epub"));
    let mut text = [0_u8; 3];
    card.read_at("books/a.epub", 0, &mut text).unwrap();
    assert_eq!(&text[..1], b"x");
}

#[test]
fn a_replacing_import_drops_the_caches_of_the_replaced_book() {
    let card = card();
    let jobs = Jobs::default();
    jobs.begin_import(&card, "data/feed/new.epub", "books/a.epub", "feed", true)
        .unwrap();
    let mut buffer = [0_u8; 16];
    let n = card.read_at("books/a.epub", 0, &mut buffer).unwrap();
    assert_eq!(&buffer[..n], b"new book");
    let host = Host::new(&card);
    run(&jobs, &host);
    assert_eq!(
        host.log.borrow().join("|"),
        "catalogue books/a.epub feed|cache books/a.epub|changed"
    );
}

/// A cut at every changing action: the file arrived and is catalogued, or
/// nothing happened and the entry is dropped; never both and never neither.
#[test]
fn an_import_cut_at_every_action_arrives_whole_or_not_at_all() {
    for replace in [false, true] {
        for allowed in 0..40 {
            let card = card();
            let cut = Cut::after(&card, allowed);
            let jobs = Jobs::default();
            let to = if replace {
                "books/a.epub"
            } else {
                "books/feeds/n.epub"
            };
            let _ = jobs.begin_import(&cut, "data/feed/new.epub", to, "feed", replace);
            let host = Host::new(&cut);
            for _ in 0..10 {
                if jobs.step(&host).is_err() {
                    break;
                }
            }
            cut.heal();
            let again = Jobs::default();
            again.start(&cut).unwrap();
            let host = Host::new(&cut);
            run(&again, &host);
            assert!(
                !card.exists(JOURNAL) && !again.active(),
                "{replace} {allowed}"
            );
            let arrived = !card.exists("data/feed/new.epub");
            let mut buffer = [0_u8; 16];
            if arrived {
                let n = card.read_at(to, 0, &mut buffer).unwrap();
                assert_eq!(&buffer[..n], b"new book", "{replace} {allowed}");
            } else {
                // Not moved: the book that was at `to` is as it was.
                assert!(card.exists("data/feed/new.epub"));
                assert_eq!(card.exists(to), replace, "{replace} {allowed}");
                assert_eq!(host.saw("changed"), 0, "{replace} {allowed}");
            }
        }
    }
}

#[test]
fn nothing_is_made_of_a_move_entry() {
    let card = card();
    card.write(JOURNAL, b"not for this machine").unwrap();
    let jobs = Jobs::default();
    // Not whole: the move machine sets it aside; this one has no job.
    assert!(jobs.start(&card).is_err() || !jobs.active());
}

/// A card whose identity is a function of the name, as FAT's first cluster is
/// once the freed one is reused: a file put at a path after the book went has
/// the book's identity.
struct Reuse<'a>(&'a MemoryStorage);

impl Storage for Reuse<'_> {
    fn read_at(&self, p: &str, a: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(p, a, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.0.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.0.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.0.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.0.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.0.create_dir(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
    fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
        // The journal's own files keep the card's identities; the books' are
        // by name.
        if p.starts_with("books/") && self.0.exists(p) {
            return Ok(Some(100 + p.len() as u64));
        }
        self.0.entry_identity(p)
    }
}

/// R1 (Senior): after the files went, the entry says so. A cut after that, a
/// file put at the path that has the book's identity (FAT reuses clusters),
/// and a restart: the new file stays.
#[test]
fn a_book_that_is_removed_is_never_looked_at_again() {
    let memory = card();
    let card = Reuse(&memory);
    let jobs = Jobs::default();
    jobs.begin_delete(&card, &BOOKS[..2], false).unwrap();
    assert!(!memory.exists(BOOKS[0]) && !memory.exists(BOOKS[1]));
    // Cut here: the entry is on the card and says both are done with.
    drop(jobs);
    assert!(matches!(
        Journal(&card).read_job().unwrap(),
        Some(JobRecord::Delete { removed: 0b11, .. })
    ));
    memory.write(BOOKS[0], b"copied back").unwrap();
    assert_eq!(
        card.entry_identity(BOOKS[0]).unwrap(),
        Some(100 + BOOKS[0].len() as u64),
        "the same identity as the book had"
    );
    let again = Jobs::default();
    again.start(&card).unwrap();
    let host = Host::new(&card);
    run(&again, &host);
    let mut buffer = [0_u8; 16];
    let n = memory.read_at(BOOKS[0], 0, &mut buffer).unwrap();
    assert_eq!(&buffer[..n], b"copied back");
    // What was kept for the deleted book went all the same.
    assert_eq!(host.saw(&std::format!("kept {}", BOOKS[0])), 1);
    assert!(!memory.exists(JOURNAL));
}

/// A book the card refused at the call and removes at a step is marked then.
#[test]
fn a_book_removed_by_a_step_is_marked_too() {
    let memory = card();
    let cut = Cut::after(&memory, 3);
    let jobs = Jobs::default();
    let _ = jobs.begin_delete(&cut, &BOOKS[..1], false);
    cut.heal();
    let again = Jobs::default();
    again.start(&cut).unwrap();
    let host = Host::new(&cut);
    assert!(again.step(&host).unwrap());
    assert!(!memory.exists(BOOKS[0]));
    assert!(matches!(
        Journal(&cut).read_job().unwrap(),
        Some(JobRecord::Delete { removed: 1, .. })
    ));
    run(&again, &host);
    assert!(!memory.exists(JOURNAL));
}

/// A part the card refuses for good (a damaged list) is final: the delete
/// finishes, the rest of the book goes, the part is asked once and left; a
/// fault that may pass is repeated.
#[test]
fn a_damaged_part_is_final_and_never_repeated() {
    for phase in [RemovalPhase::Details, RemovalPhase::Marks] {
        let card = card();
        card.write("data/favourites.txt", b"\xff damaged").unwrap();
        let jobs = Jobs::default();
        jobs.begin_delete(&card, &BOOKS[..2], false).unwrap();
        let host = Host::new(&card);
        host.forget_fails.set(Some((phase, ERR_INVALID)));
        run(&jobs, &host);
        // Asked once per book, no more; the other phase and the rest ran.
        assert_eq!(
            host.saw(&std::format!("forget {} false {phase:?}", BOOKS[0])),
            1
        );
        assert_eq!(
            host.saw(&std::format!("forget {} false {phase:?}", BOOKS[1])),
            1
        );
        for book in &BOOKS[..2] {
            assert_eq!(host.saw(&std::format!("kept {book}")), 1, "{phase:?}");
            assert_eq!(host.saw(&std::format!("removed {book}")), 1, "{phase:?}");
        }
        assert_eq!(host.saw("changed"), 1);
        assert_eq!(
            card.read_at("data/favourites.txt", 0, &mut [0; 16])
                .unwrap(),
            9,
            "the list is untouched"
        );
        assert!(!card.exists(JOURNAL) && !jobs.active(), "{phase:?}");
    }
    // A fault that may pass: the step fails each time, the job stays and the
    // entry with it; when the card heals the job finishes.
    let card = card();
    let jobs = Jobs::default();
    jobs.begin_delete(&card, &BOOKS[..1], false).unwrap();
    let host = Host::new(&card);
    host.forget_fails.set(Some((RemovalPhase::Details, ERR_IO)));
    assert_eq!(jobs.step(&host), Ok(true));
    for _ in 0..4 {
        assert_eq!(jobs.step(&host), Err(ERR_IO));
    }
    assert!(jobs.active() && card.exists(JOURNAL));
    host.forget_fails.set(None);
    run(&jobs, &host);
    assert!(!card.exists(JOURNAL));
}

/// An import's note the card refuses for good is left; the book is in.
#[test]
fn a_damaged_importer_note_is_final() {
    let card = card();
    let jobs = Jobs::default();
    jobs.begin_import(
        &card,
        "data/feed/new.epub",
        "books/feeds/n.epub",
        "feed",
        false,
    )
    .unwrap();
    let host = Host::new(&card);
    host.catalogue_fails.set(Some(ERR_INVALID));
    run(&jobs, &host);
    assert_eq!(host.saw("catalogue books/feeds/n.epub feed"), 1);
    assert_eq!(host.saw("changed"), 1);
    assert!(card.exists("books/feeds/n.epub") && !card.exists(JOURNAL));
}

/// `carry_status` and `is_final` agree with the storage steps' `permanent`.
#[test]
fn the_catalog_statuses_agree_with_what_is_permanent() {
    for error in [
        StorageError::Io,
        StorageError::CARD_GONE,
        StorageError::InvalidData,
        StorageError::InvalidPath,
        StorageError::Unsupported,
        StorageError::TooLarge,
        StorageError::Full,
        StorageError::NotFound,
        StorageError::OutOfMemory,
    ] {
        assert_eq!(
            is_final(carry_status(error)),
            book_data_steps::permanent(error),
            "{error:?}"
        );
    }
    assert!(
        !is_final(ERR_NO_MEMORY)
            && !is_final(ERR_BUSY)
            && !is_final(ERR_IO)
            && !is_final(ERR_STATE)
    );
}
