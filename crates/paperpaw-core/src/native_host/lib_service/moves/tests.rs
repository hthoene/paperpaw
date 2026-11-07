#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The move journal and job over a fake card with injected power cuts: the
//! cases of `files.md` 7 (a cut right after the rename, a cut that leaves
//! both names, a book opened during the job, a 1,000-book folder, replaying a
//! half-done job, file-manager's cases), a job that can never stay stuck
//! (case-only renames, a new file at the old name, replacing moves), and
//! sweeps that cut after every durable write.
extern crate std;
use super::*;
use paperpaw_native::host_glue::ERR_IO;
use std::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec::Vec,
};

/// Every durable write of the host is one tick; the cut makes tick `n` fail
/// before it is applied, and every later one until `restart`.
#[derive(Default)]
struct World {
    /// Every stored directory entry by its spelling: identity, folder.
    files: RefCell<BTreeMap<String, (u64, bool)>>,
    next_id: Cell<u64>,
    catalog: RefCell<BTreeMap<String, u32>>,
    assignments: RefCell<BTreeMap<String, String>>,
    settings: RefCell<BTreeMap<String, String>>,
    entry: RefCell<Option<(String, String, bool, u64)>>,
    events: RefCell<Vec<(String, String)>>,
    ticks: Cell<usize>,
    cut: Cell<Option<usize>>,
    dead: Cell<bool>,
    /// The rename added the new names and did not remove the old ones yet.
    half: RefCell<Vec<(String, String)>>,
    /// The journal cannot be read.
    unreadable: Cell<bool>,
}

/// A path as the storage compares it: Unicode case folded, 8.3 aliases
/// resolved, segment by segment.
fn key(path: &str) -> Vec<String> {
    path.trim_matches('/')
        .split('/')
        .map(|segment| match segment.to_lowercase().as_str() {
            "books~1" => "books".to_string(),
            other => other.to_string(),
        })
        .collect()
}

impl World {
    fn tick(&self) -> Result<(), i32> {
        if self.dead.get() {
            return Err(ERR_IO);
        }
        self.ticks.set(self.ticks.get() + 1);
        if self.cut.get() == Some(self.ticks.get()) {
            self.dead.set(true);
            return Err(ERR_IO);
        }
        Ok(())
    }
    fn restart(&self) {
        self.dead.set(false);
        self.cut.set(None);
    }
    fn entry_of(&self, path: &str) -> Option<(String, u64, bool)> {
        let wanted = key(path);
        self.files
            .borrow()
            .iter()
            .find(|(stored, _)| key(stored) == wanted)
            .map(|(stored, (id, folder))| (stored.clone(), *id, *folder))
    }
    fn create(&self, path: &str, folder: bool) -> u64 {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        self.files
            .borrow_mut()
            .insert(path.to_string(), (id, folder));
        id
    }
    fn add_book(&self, path: &str, position: u32) {
        self.create(path, false);
        self.catalog.borrow_mut().insert(path.to_string(), position);
    }
    fn renamed(path: &str, from: &str, to: &str) -> String {
        format!("{to}{}", &path[from.len()..])
    }
    fn books_under(&self, folder: &str) -> Vec<String> {
        self.catalog
            .borrow()
            .keys()
            .filter(|k| self.within(k, folder) || k.as_str() == format!("{folder}.bak"))
            .cloned()
            .collect()
    }
    fn move_one(&self, old: &str, from: &str, to: &str) -> Result<(), i32> {
        self.tick()?;
        let new = Self::renamed(old, from, to);
        let mut catalog = self.catalog.borrow_mut();
        if let Some(position) = catalog.remove(old) {
            // Data at the new key (a reader wrote there) wins.
            catalog.entry(new).or_insert(position);
        }
        Ok(())
    }
}

impl MoveHost for World {
    fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32> {
        if self.unreadable.get() {
            return Err(ERR_IO);
        }
        Ok(self.entry.borrow().as_ref().is_some_and(|(f, t, r, i)| {
            visit(&Entry {
                from: f,
                to: t,
                replace: *r,
                identity: *i,
            });
            true
        }))
    }
    fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32> {
        self.tick()?;
        *self.entry.borrow_mut() = Some((
            entry.from.into(),
            entry.to.into(),
            entry.replace,
            entry.identity,
        ));
        Ok(())
    }
    fn clear_entry(&self) -> Result<(), i32> {
        self.tick()?;
        *self.entry.borrow_mut() = None;
        Ok(())
    }
    fn identity(&self, path: &str) -> Result<Option<u64>, i32> {
        Ok(self.entry_of(path).map(|(_, id, _)| id))
    }
    fn is_folder(&self, path: &str) -> Result<bool, i32> {
        Ok(self.entry_of(path).is_some_and(|(_, _, folder)| folder))
    }
    fn same_entry(&self, a: &str, b: &str) -> Result<bool, i32> {
        Ok(key(a) == key(b) && self.entry_of(a).is_some())
    }
    fn spelled_as(&self, path: &str) -> Result<bool, i32> {
        Ok(self.files.borrow().contains_key(path))
    }
    fn rename(&self, from: &str, to: &str, _replace: bool) -> Result<(), i32> {
        let moved: Vec<(String, u64, bool)> = self
            .files
            .borrow()
            .iter()
            .filter(|(f, _)| self.within(f, from))
            .map(|(f, (id, folder))| (f.clone(), *id, *folder))
            .collect();
        self.tick()?;
        let mut pairs = Vec::new();
        for (old, id, folder) in &moved {
            let new = Self::renamed(old, from, to);
            let wanted = key(&new);
            let mut files = self.files.borrow_mut();
            // What was at the new name (a replaced file) is gone; the moved
            // entry itself, under its old spelling, stays until the second write.
            files.retain(|path, (other, _)| key(path) != wanted || other == id);
            files.insert(new.clone(), (*id, *folder));
            pairs.push((old.clone(), new));
        }
        *self.half.borrow_mut() = pairs;
        self.tick()?;
        self.finish_rename();
        Ok(())
    }
    fn settle(&self) -> Result<(), i32> {
        self.tick()?;
        self.finish_rename();
        Ok(())
    }
    fn within(&self, path: &str, folder: &str) -> bool {
        let (path, folder) = (key(path), key(folder));
        path.len() >= folder.len() && path[..folder.len()] == folder[..]
    }
    fn discard_replaced(&self, from: &str, to: &str) -> Result<(), i32> {
        self.tick()?;
        // Only while the moved book's data is still at its old key.
        if self.catalog.borrow().contains_key(from) {
            self.catalog.borrow_mut().remove(to);
        }
        Ok(())
    }
    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32> {
        match phase {
            Phase::Replaced => Ok(Phase::Books(0)),
            Phase::Books(n) => {
                // Keys already spelled with the new prefix are done (a
                // case-only rename matches both spellings under `within`).
                let books: Vec<String> = self
                    .books_under(from)
                    .into_iter()
                    .filter(|k| from == to || !k.starts_with(to))
                    .collect();
                for old in books.iter().take(STEP_BOOKS) {
                    self.move_one(old, from, to)?;
                }
                Ok(if books.len() > STEP_BOOKS {
                    Phase::Books(n + 1)
                } else {
                    Phase::Assignments
                })
            }
            Phase::Assignments => {
                self.tick()?;
                let mut map = self.assignments.borrow_mut();
                let old: Vec<String> = map
                    .keys()
                    .filter(|k| self.within(k, from))
                    .cloned()
                    .collect();
                for key in old {
                    let value = map.remove(&key).unwrap();
                    map.insert(Self::renamed(&key, from, to), value);
                }
                Ok(Phase::Settings)
            }
            Phase::Settings => {
                self.tick()?;
                for value in self.settings.borrow_mut().values_mut() {
                    if self.within(value, from) {
                        *value = Self::renamed(value, from, to);
                    }
                }
                Ok(Phase::Events(0))
            }
            Phase::Events(done) => {
                let books = self.books_under(to);
                let rest: Vec<&String> =
                    books.iter().skip(done as usize).take(STEP_BOOKS).collect();
                for new in &rest {
                    let was = format!("{from}{}", &new[to.len()..]);
                    self.events.borrow_mut().push((was, (*new).clone()));
                }
                let sent = done + rest.len() as u64;
                Ok(if sent as usize >= books.len() {
                    Phase::Done
                } else {
                    Phase::Events(sent)
                })
            }
            Phase::Done => Ok(Phase::Done),
        }
    }
    fn carry_book(&self, from: &str, to: &str, book: &str) -> Result<(), i32> {
        let old = format!("{from}{}", &book[to.len()..]);
        self.move_one(&old, from, to)
    }
}

impl World {
    /// The second directory write: the old spellings go.
    fn finish_rename(&self) {
        let pairs = core::mem::take(&mut *self.half.borrow_mut());
        let news: BTreeSet<String> = pairs.iter().map(|(_, new)| new.clone()).collect();
        for (old, _) in pairs {
            if !news.contains(&old) {
                self.files.borrow_mut().remove(&old);
            }
        }
    }
}

/// A folder `Books/old` with `n` books, a reader folder assignment and a
/// setting that names a folder in it.
fn world(n: usize) -> World {
    let w = World::default();
    w.create("Books", true);
    w.create("Books/old", true);
    for i in 0..n {
        w.add_book(&format!("Books/old/b{i:04}.epub"), 100 + i as u32);
    }
    w.assignments
        .borrow_mut()
        .insert("Books/old/sub".into(), "reader.fast".into());
    w.settings
        .borrow_mut()
        .insert("library.folder".into(), "books/OLD/sub".into());
    w
}

/// Single books: `a.epub` (position 5) and its `.bak` (6), and `b.epub`
/// (position 99, with a data folder of its own).
fn books() -> World {
    let w = World::default();
    w.create("Books", true);
    w.add_book("Books/a.epub", 5);
    w.add_book("Books/a.epub.bak", 6);
    w.add_book("Books/b.epub", 99);
    w
}

fn run(moves: &Moves, w: &World) -> usize {
    let mut steps = 0;
    while moves.step(w).unwrap() {
        steps += 1;
        assert!(steps < 10_000, "the job ends");
    }
    steps
}

/// Everything at the new place `to`, nothing at the old, entry cleared.
fn assert_moved_to(w: &World, to: &str, n: usize) {
    assert_eq!(*w.entry.borrow(), None);
    assert!(
        w.books_under("Books/old").is_empty() || to.eq_ignore_ascii_case("Books/old"),
        "{:?}",
        w.books_under("Books/old")
    );
    let books = w.books_under(to);
    assert_eq!(books.len(), n);
    for (i, key) in books.iter().enumerate() {
        assert_eq!(w.catalog.borrow()[key], 100 + i as u32, "{key}");
    }
    assert_eq!(w.assignments.borrow()[&format!("{to}/sub")], "reader.fast");
    assert_eq!(w.settings.borrow()["library.folder"], format!("{to}/sub"));
}
fn assert_moved(w: &World, n: usize) {
    assert_moved_to(w, "Books/new", n);
    assert!(
        w.files.borrow().keys().all(|f| !f.starts_with("Books/old")),
        "the old names are gone"
    );
}

/// Nothing happened.
fn assert_unmoved(w: &World, n: usize) {
    assert_eq!(*w.entry.borrow(), None);
    assert_eq!(w.books_under("Books/old").len(), n);
    assert!(w.books_under("Books/new").is_empty());
    assert_eq!(w.settings.borrow()["library.folder"], "books/OLD/sub");
}

/// The state is one of the two whole ones and no job is left.
fn whole(w: &World, moves: &Moves) -> bool {
    !moves.active() && w.entry.borrow().is_none()
}

#[test]
fn a_move_carries_books_assignments_settings_and_events() {
    let w = world(40);
    let moves = Moves::default();
    moves.begin(&w, "Books/old", "Books/new", false).unwrap();
    assert!(moves.active());
    let steps = run(&moves, &w);
    assert!(!moves.active());
    assert_moved(&w, 40);
    assert!(steps >= 40 / STEP_BOOKS, "{steps} bounded steps");
    assert_eq!(w.events.borrow().len(), 40, "one book-moved event per book");
    assert_eq!(
        w.events.borrow()[0],
        ("Books/old/b0000.epub".into(), "Books/new/b0000.epub".into())
    );
}

#[test]
fn a_move_that_cannot_work_is_refused_before_anything_is_written() {
    let w = world(3);
    w.create("Books/taken", true);
    w.add_book("Books/a.epub", 1);
    w.add_book("Books/b.epub", 2);
    let moves = Moves::default();
    let refused = |from, to, replace| moves.begin(&w, from, to, replace);
    // An existing target without `replace`, a folder at the target (even with
    // `replace`), a folder replacing a file, a missing source, a move into
    // itself.
    assert_eq!(
        refused("Books/a.epub", "Books/b.epub", false),
        Err(ERR_STATE)
    );
    assert_eq!(refused("Books/old", "Books/taken", true), Err(ERR_STATE));
    assert_eq!(refused("Books/a.epub", "Books/taken", true), Err(ERR_STATE));
    assert_eq!(refused("Books/old", "Books/b.epub", true), Err(ERR_STATE));
    assert_eq!(refused("Books/gone", "Books/x", false), Err(ERR_NOT_FOUND));
    assert_eq!(
        refused("Books/old", "Books/old/in", false),
        Err(ERR_INVALID)
    );
    assert_eq!(w.ticks.get(), 0, "no durable write for a refusal");
    assert!(!moves.active() && w.entry.borrow().is_none());
    // A move that replaces a file is allowed.
    moves
        .begin(&w, "Books/a.epub", "Books/b.epub", true)
        .unwrap();
}

#[test]
fn a_cut_right_after_the_rename_is_finished_at_start() {
    let w = world(20);
    // Ticks: 1 entry, 2 add, 3 remove, 4.. the job; cut at the first step.
    w.cut.set(Some(4));
    let first = Moves::default();
    first.begin(&w, "Books/old", "Books/new", false).unwrap();
    assert!(first.step(&w).is_err());
    w.restart();
    assert!(
        w.identity("Books/new").unwrap().is_some() && w.identity("Books/old").unwrap().is_none()
    );
    let second = Moves::default();
    second.start(&w).unwrap();
    run(&second, &w);
    assert_moved(&w, 20);
}

#[test]
fn a_cut_that_leaves_both_names_is_repaired_before_the_entry_is_judged() {
    let w = world(20);
    w.cut.set(Some(3)); // the new names are added, the old ones not removed
    let first = Moves::default();
    assert_eq!(
        first.begin(&w, "Books/old", "Books/new", false),
        Err(ERR_IO)
    );
    w.restart();
    assert!(
        w.identity("Books/new").unwrap().is_some() && w.identity("Books/old").unwrap().is_some()
    );
    assert!(
        w.entry.borrow().is_some(),
        "the entry is kept, not dropped blind"
    );
    let second = Moves::default();
    second.start(&w).unwrap();
    assert!(
        w.identity("Books/old").unwrap().is_none(),
        "the repair settled the names"
    );
    run(&second, &w);
    assert_moved(&w, 20);
}

#[test]
fn only_the_old_name_drops_the_entry_nothing_at_all_drops_it_too() {
    // A cut after the journal, before the rename.
    let w = world(5);
    w.cut.set(Some(2));
    assert_eq!(
        Moves::default().begin(&w, "Books/old", "Books/new", false),
        Err(ERR_IO)
    );
    w.restart();
    assert!(w.entry.borrow().is_some());
    let moves = Moves::default();
    moves.start(&w).unwrap();
    assert!(!moves.active());
    assert_unmoved(&w, 5);
    // The folder was removed on a computer: nothing of it is left.
    let w = world(5);
    let id = w.identity("Books/old").unwrap().unwrap();
    *w.entry.borrow_mut() = Some(("Books/old".into(), "Books/new".into(), false, id));
    w.files
        .borrow_mut()
        .retain(|f, _| !f.starts_with("Books/old"));
    let moves = Moves::default();
    moves.start(&w).unwrap();
    assert!(!moves.active());
    assert_eq!(*w.entry.borrow(), None);
}

#[test]
fn a_new_file_at_the_old_name_after_the_rename_still_counts_as_done() {
    let w = world(6);
    w.cut.set(Some(4));
    let first = Moves::default();
    first.begin(&w, "Books/old", "Books/new", false).unwrap();
    assert!(first.step(&w).is_err());
    w.restart();
    // Someone made a new folder at the old name; it is not the moved entry.
    let fresh = w.create("Books/old", true);
    let moves = Moves::default();
    moves.start(&w).unwrap();
    assert!(moves.active(), "the job runs, it is not stuck or dropped");
    run(&moves, &w);
    assert_eq!(w.identity("Books/old").unwrap(), Some(fresh), "untouched");
    assert_eq!(w.books_under("Books/new").len(), 6);
    assert_eq!(*w.entry.borrow(), None);
}

#[test]
fn a_case_only_rename_is_decided_by_the_stored_spelling_and_never_stuck() {
    let w = world(20);
    let moves = Moves::default();
    moves.begin(&w, "Books/old", "Books/OLD", false).unwrap();
    run(&moves, &w);
    assert!(w.files.borrow().contains_key("Books/OLD"));
    assert!(!w.files.borrow().contains_key("Books/old"));
    assert_moved_to(&w, "Books/OLD", 20);
    // Every cut: the result is whole and no job is left stuck.
    let total = {
        let w = world(20);
        let moves = Moves::default();
        moves.begin(&w, "Books/old", "Books/OLD", false).unwrap();
        run(&moves, &w);
        w.ticks.get()
    };
    for cut in 1..=total {
        let w = world(20);
        w.cut.set(Some(cut));
        let moves = Moves::default();
        if moves.begin(&w, "Books/old", "Books/OLD", false).is_ok() {
            while moves.step(&w).is_ok_and(|more| more) {}
        }
        w.restart();
        let moves = Moves::default();
        moves.start(&w).unwrap();
        run(&moves, &w);
        assert!(whole(&w, &moves), "cut {cut}: a job is left");
        assert_eq!(w.files.borrow().len(), 22, "cut {cut}: one entry per name");
        let spelled_new = w.files.borrow().contains_key("Books/OLD");
        if spelled_new {
            assert_moved_to(&w, "Books/OLD", 20);
        } else {
            assert_eq!(w.books_under("Books/old").len(), 20, "cut {cut}");
        }
    }
}

#[test]
fn an_unreadable_journal_is_never_overwritten() {
    let w = world(3);
    *w.entry.borrow_mut() = Some(("Books/x".into(), "Books/y".into(), false, 1));
    w.unreadable.set(true);
    let moves = Moves::default();
    assert_eq!(moves.start(&w), Err(ERR_IO));
    assert_eq!(
        moves.begin(&w, "Books/old", "Books/new", false),
        Err(ERR_IO)
    );
    assert_eq!(w.ticks.get(), 0, "nothing was written");
    assert_eq!(
        w.entry.borrow().as_ref().unwrap().0,
        "Books/x",
        "the entry stays"
    );
    // Readable again: the old entry is judged first (nothing moved: dropped)
    // and then the new move may begin.
    w.unreadable.set(false);
    assert_eq!(moves.begin(&w, "Books/old", "Books/new", false), Ok(()));
    run(&moves, &w);
    assert_moved(&w, 3);
}

#[test]
fn a_replacing_move_does_not_inherit_the_replaced_files_data() {
    let w = books();
    let moves = Moves::default();
    moves
        .begin(&w, "Books/a.epub", "Books/b.epub", true)
        .unwrap();
    run(&moves, &w);
    assert_eq!(
        w.catalog.borrow().get("Books/b.epub"),
        Some(&5),
        "a's data, not b's 99"
    );
    assert_eq!(w.catalog.borrow().get("Books/b.epub.bak"), Some(&6));
    assert!(w.catalog.borrow().keys().all(|k| !k.starts_with("Books/a")));
    // A cut at every durable write: whole, or not done; never b's data under
    // the moved book, never a lost position.
    let total = w.ticks.get();
    for cut in 1..=total {
        let w = books();
        w.cut.set(Some(cut));
        let moves = Moves::default();
        if moves
            .begin(&w, "Books/a.epub", "Books/b.epub", true)
            .is_ok()
        {
            while moves.step(&w).is_ok_and(|more| more) {}
        }
        w.restart();
        let moves = Moves::default();
        moves.start(&w).unwrap();
        run(&moves, &w);
        assert!(whole(&w, &moves), "cut {cut}");
        let (a, b) = {
            let c = w.catalog.borrow();
            (
                c.get("Books/a.epub").copied(),
                c.get("Books/b.epub").copied(),
            )
        };
        match (a, b) {
            (Some(5), Some(99)) => assert!(w.files.borrow().contains_key("Books/a.epub")),
            (None, Some(5)) => assert!(!w.files.borrow().contains_key("Books/a.epub")),
            other => panic!("cut {cut}: {other:?}"),
        }
    }
}

#[test]
fn a_book_opened_during_the_job_keeps_its_position_and_what_it_writes() {
    let w = world(40);
    let moves = Moves::default();
    moves.begin(&w, "Books/old", "Books/new", false).unwrap();
    assert!(moves.step(&w).unwrap());
    // The reader opens a book the walk has not reached: carried first.
    moves.open_book(&w, "Books/new/b0039.epub").unwrap();
    assert_eq!(w.catalog.borrow()["Books/new/b0039.epub"], 139);
    // It reads on and writes a position; the walk must not overwrite it.
    w.catalog
        .borrow_mut()
        .insert("Books/new/b0039.epub".into(), 7777);
    run(&moves, &w);
    assert_eq!(w.catalog.borrow()["Books/new/b0039.epub"], 7777);
    // A book below neither name, or no job: nothing to do.
    moves.open_book(&w, "Books/new/b0001.epub").unwrap();
    Moves::default()
        .open_book(&w, "Books/new/b0001.epub")
        .unwrap();
}

#[test]
fn a_thousand_book_folder_moves_within_one_bounded_call() {
    let w = world(1000);
    let moves = Moves::default();
    moves.begin(&w, "Books/old", "Books/new", false).unwrap();
    assert!(
        w.ticks.get() <= 3,
        "{} durable writes in the call",
        w.ticks.get()
    );
    let mut worst = 0;
    let mut steps = 0;
    loop {
        let before = w.ticks.get();
        let more = moves.step(&w).unwrap();
        worst = worst.max(w.ticks.get() - before);
        steps += 1;
        if !more {
            break;
        }
    }
    assert!(worst <= STEP_BOOKS, "a step writes {worst} times at most");
    assert!(steps <= 2 * (1000 / STEP_BOOKS + 1) + 8, "{steps} steps");
    assert_moved(&w, 1000);
    let events = w
        .events
        .borrow()
        .iter()
        .map(|e| e.1.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(events.len(), 1000);
}

#[test]
fn a_half_done_job_is_replayed_without_loss_or_duplicates() {
    let w = world(40);
    let first = Moves::default();
    first.begin(&w, "Books/old", "Books/new", false).unwrap();
    // Cut in the middle of the second batch of books.
    w.cut.set(Some(w.ticks.get() + STEP_BOOKS + 5));
    assert!(first.step(&w).unwrap());
    assert!(first.step(&w).is_err());
    w.restart();
    assert!(!w.books_under("Books/old").is_empty() && !w.books_under("Books/new").is_empty());
    let second = Moves::default();
    second.start(&w).unwrap();
    run(&second, &w);
    assert_moved(&w, 40);
}

#[test]
fn a_cut_after_every_durable_write_leaves_the_move_whole_or_not_done() {
    // Count the writes of an uncut move with three batches.
    let total = {
        let w = world(40);
        let moves = Moves::default();
        moves.begin(&w, "Books/old", "Books/new", false).unwrap();
        run(&moves, &w);
        w.ticks.get()
    };
    for cut in 1..=total {
        let w = world(40);
        w.cut.set(Some(cut));
        let moves = Moves::default();
        if moves.begin(&w, "Books/old", "Books/new", false).is_ok() {
            while moves.step(&w).is_ok_and(|more| more) {}
        }
        w.restart();
        // A new process: only the card is left.
        let moves = Moves::default();
        moves.start(&w).unwrap();
        run(&moves, &w);
        assert!(whole(&w, &moves), "cut {cut}: a job is left");
        let neither = w.identity("Books/old").unwrap().is_none()
            && w.identity("Books/new").unwrap().is_none();
        assert!(!neither, "cut {cut}: found at neither place");
        if w.identity("Books/new").unwrap().is_some() {
            assert_moved(&w, 40);
        } else {
            assert_unmoved(&w, 40);
        }
    }
}

#[test]
fn paths_at_or_below_either_name_are_busy_while_the_job_runs() {
    let w = world(3);
    let moves = Moves::default();
    assert!(!moves.busy(&w, "Books/old"));
    moves.begin(&w, "Books/old", "Books/new", false).unwrap();
    for busy in [
        "Books/old",
        "books/OLD/b0000.epub",
        "Books/new",
        "Books/new/sub/x.epub",
        // make_dir and writes at the old name, or below the new one.
        "Books/old/fresh/",
        "Books/new/fresh.epub",
        // Unicode case and 8.3 aliases name the same folders.
        "BOOKS~1/old",
        "BOOKS~1/NEW/x",
    ] {
        assert!(moves.busy(&w, busy), "{busy}");
    }
    for free in ["Books", "Books/older", "Books/news/x.epub", ""] {
        assert!(!moves.busy(&w, free), "{free}");
    }
    // One entry in the journal: the next move waits as well.
    assert_eq!(moves.begin(&w, "Books/x", "Books/y", false), Err(ERR_BUSY));
    run(&moves, &w);
    assert!(!moves.busy(&w, "Books/old"));
    moves.begin(&w, "Books/new", "Books/old", false).unwrap();
}

#[test]
fn names_match_beyond_ascii_case() {
    let w = World::default();
    assert!(w.within("BÜCHER/neu/x", "Bücher"));
    assert!(w.within("bücher", "BÜCHER/"));
    assert!(!w.within("Bücherei", "Bücher"));
}

#[test]
fn a_single_book_moves_with_its_bak_sibling_and_its_events() {
    // File-manager's file case: `a.epub.bak` may be named back.
    let w = books();
    let moves = Moves::default();
    moves
        .begin(&w, "Books/a.epub", "Books/c.epub", false)
        .unwrap();
    run(&moves, &w);
    assert_eq!(w.catalog.borrow().get("Books/c.epub"), Some(&5));
    assert_eq!(w.catalog.borrow().get("Books/c.epub.bak"), Some(&6));
    assert!(w.catalog.borrow().keys().all(|k| !k.starts_with("Books/a")));
    assert_eq!(w.events.borrow().len(), 2);
}

#[test]
fn a_journal_that_fails_leaves_no_job_and_no_change() {
    let w = world(2);
    let moves = Moves::default();
    // A card that fails the journal: no job, no change.
    w.cut.set(Some(1));
    assert_eq!(
        moves.begin(&w, "Books/old", "Books/new", false),
        Err(ERR_IO)
    );
    assert!(!moves.active());
    w.restart();
    assert_unmoved(&w, 2);
}
