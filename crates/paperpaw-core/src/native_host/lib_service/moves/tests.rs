#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The move journal and job over a fake card with injected power cuts: the
//! cases of `files.md` 7 (a cut right after the rename, a cut that leaves
//! both names, a book opened during the job, a 1,000-book folder, replaying a
//! half-done job, file-manager's cases) and a sweep that cuts after every
//! durable write.
extern crate std;
use super::*;
use paperpaw_native::host_glue::ERR_IO;
use std::format;
use std::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

/// Every durable write of the host is one tick; the cut makes tick `n` fail
/// before it is applied, and every later one until `restart`.
#[derive(Default)]
struct World {
    files: RefCell<BTreeSet<String>>,
    catalog: RefCell<BTreeMap<String, u32>>,
    assignments: RefCell<BTreeMap<String, String>>,
    settings: RefCell<BTreeMap<String, String>>,
    entry: RefCell<Option<(String, String, bool)>>,
    events: RefCell<Vec<(String, String)>>,
    ticks: Cell<usize>,
    cut: Cell<Option<usize>>,
    dead: Cell<bool>,
    /// The rename added the new names and did not remove the old ones yet.
    half: Cell<bool>,
    /// Ticks since the last `mark`.
    mark: Cell<usize>,
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
    fn add_book(&self, path: &str, position: u32) {
        self.files.borrow_mut().insert(path.to_string());
        self.catalog.borrow_mut().insert(path.to_string(), position);
    }
    fn renamed(path: &str, from: &str, to: &str) -> String {
        format!("{to}{}", &path[from.len()..])
    }
    fn under(&self, path: &str, folder: &str) -> bool {
        MoveHost::within(self, path, folder)
    }
    fn books_under(&self, folder: &str) -> Vec<String> {
        self.catalog
            .borrow()
            .keys()
            .filter(|k| self.under(k, folder) || k.as_str() == format!("{folder}.bak"))
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
    fn open_entry(&self, visit: &mut dyn FnMut(&str, &str, bool)) -> Result<bool, i32> {
        Ok(self.entry.borrow().as_ref().is_some_and(|(f, t, r)| {
            visit(f, t, *r);
            true
        }))
    }
    fn write_entry(&self, from: &str, to: &str, replace: bool) -> Result<(), i32> {
        self.tick()?;
        *self.entry.borrow_mut() = Some((from.into(), to.into(), replace));
        Ok(())
    }
    fn clear_entry(&self) -> Result<(), i32> {
        self.tick()?;
        *self.entry.borrow_mut() = None;
        Ok(())
    }
    fn exists(&self, path: &str) -> Result<bool, i32> {
        Ok(self.files.borrow().contains(path))
    }
    fn rename(&self, from: &str, to: &str, _replace: bool) -> Result<(), i32> {
        let moved: Vec<String> = self
            .files
            .borrow()
            .iter()
            .filter(|f| self.under(f, from))
            .cloned()
            .collect();
        self.tick()?;
        for old in &moved {
            self.files.borrow_mut().insert(Self::renamed(old, from, to));
        }
        self.half.set(true);
        self.tick()?;
        for old in &moved {
            self.files.borrow_mut().remove(old);
        }
        self.half.set(false);
        Ok(())
    }
    fn settle(&self) -> Result<(), i32> {
        self.tick()?;
        if self.half.get() {
            let (from, _, _) = self.entry.borrow().clone().unwrap();
            self.files.borrow_mut().retain(|f| !self.under(f, &from));
            self.half.set(false);
        }
        Ok(())
    }
    fn carry(&self, from: &str, to: &str, phase: Phase) -> Result<Phase, i32> {
        match phase {
            Phase::Books(n) => {
                let books = self.books_under(from);
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
                    .filter(|k| self.under(k, from))
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
                    if self.under(value, from) {
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

/// A folder `Books/old` with `n` books, `Books` as a reader folder and a
/// setting that names it.
fn world(n: usize) -> World {
    let w = World::default();
    w.files.borrow_mut().insert("Books".into());
    w.files.borrow_mut().insert("Books/old".into());
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

fn run(moves: &Moves, w: &World) -> usize {
    let mut steps = 0;
    while moves.step(w).unwrap() {
        steps += 1;
        assert!(steps < 10_000, "the job ends");
    }
    steps
}

/// Everything at the new place, nothing at the old, entry cleared.
fn assert_moved(w: &World, n: usize) {
    assert_eq!(*w.entry.borrow(), None);
    assert!(w.books_under("Books/old").is_empty());
    let books = w.books_under("Books/new");
    assert_eq!(books.len(), n);
    for (i, key) in books.iter().enumerate() {
        assert_eq!(w.catalog.borrow()[key], 100 + i as u32, "{key}");
    }
    assert_eq!(w.assignments.borrow()["Books/new/sub"], "reader.fast");
    assert_eq!(w.settings.borrow()["library.folder"], "Books/new/sub");
    assert!(w.files.borrow().iter().all(|f| !f.starts_with("Books/old")));
}

/// Nothing happened.
fn assert_unmoved(w: &World, n: usize) {
    assert_eq!(*w.entry.borrow(), None);
    assert_eq!(w.books_under("Books/old").len(), n);
    assert!(w.books_under("Books/new").is_empty());
    assert_eq!(w.settings.borrow()["library.folder"], "books/OLD/sub");
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
fn a_cut_right_after_the_rename_is_finished_at_start() {
    let w = world(20);
    // Ticks: 1 entry, 2 add, 3 remove, 4.. the job; cut at the first step.
    w.cut.set(Some(4));
    let first = Moves::default();
    first.begin(&w, "Books/old", "Books/new", false).unwrap();
    assert!(first.step(&w).is_err());
    w.restart();
    assert!(w.exists("Books/new").unwrap() && !w.exists("Books/old").unwrap());
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
    assert!(w.exists("Books/new").unwrap() && w.exists("Books/old").unwrap());
    assert!(
        w.entry.borrow().is_some(),
        "the entry is kept, not dropped blind"
    );
    let second = Moves::default();
    second.start(&w).unwrap();
    assert!(
        !w.exists("Books/old").unwrap(),
        "the repair settled the names"
    );
    run(&second, &w);
    assert_moved(&w, 20);
}

#[test]
fn only_the_old_name_drops_the_entry_neither_drops_it_too() {
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
    // The folder was removed on a computer: neither name exists.
    let w = world(5);
    *w.entry.borrow_mut() = Some(("Books/old".into(), "Books/new".into(), false));
    w.files.borrow_mut().retain(|f| !f.starts_with("Books/old"));
    let moves = Moves::default();
    moves.start(&w).unwrap();
    assert!(!moves.active());
    assert_eq!(*w.entry.borrow(), None);
}

#[test]
fn both_names_that_the_repair_cannot_settle_keep_the_entry() {
    let w = world(3);
    *w.entry.borrow_mut() = Some(("Books/old".into(), "Books/new".into(), false));
    w.files.borrow_mut().insert("Books/new".into());
    let moves = Moves::default();
    moves.start(&w).unwrap_err();
    assert!(w.entry.borrow().is_some() && moves.active());
    // A move that replaces a file and never ran is nothing moved.
    let w = world(0);
    w.files.borrow_mut().insert("Books/a.epub".into());
    w.files.borrow_mut().insert("Books/b.epub".into());
    *w.entry.borrow_mut() = Some(("Books/a.epub".into(), "Books/b.epub".into(), true));
    let moves = Moves::default();
    moves.start(&w).unwrap();
    assert!(!moves.active() && w.entry.borrow().is_none());
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
        if moves.start(&w).is_ok() {
            run(&moves, &w);
        }
        let neither = !w.exists("Books/old").unwrap() && !w.exists("Books/new").unwrap();
        assert!(!neither, "cut {cut}: found at neither place");
        if w.exists("Books/new").unwrap() {
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
fn a_single_book_moves_with_its_bak_sibling_and_a_rename_by_case() {
    // File-manager's file case: `a.epub.bak` may be named back.
    let w = World::default();
    w.files.borrow_mut().insert("Books".into());
    w.add_book("Books/a.epub", 5);
    w.add_book("Books/a.epub.bak", 6);
    let moves = Moves::default();
    moves
        .begin(&w, "Books/a.epub", "Books/b.epub", false)
        .unwrap();
    run(&moves, &w);
    assert_eq!(w.catalog.borrow().get("Books/b.epub"), Some(&5));
    assert_eq!(w.catalog.borrow().get("Books/b.epub.bak"), Some(&6));
    assert!(w.catalog.borrow().keys().all(|k| !k.starts_with("Books/a")));
    assert_eq!(w.events.borrow().len(), 2);
    // `Books` is `books` for settings and assignments.
    assert!(MoveHost::within(&w, "BOOKS/x/y", "books/X"));
    assert!(!MoveHost::within(&w, "books/xy", "books/x"));
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
