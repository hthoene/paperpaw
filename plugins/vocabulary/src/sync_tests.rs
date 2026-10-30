use super::*;
use crate::cards::{self, Card, Problem};
use crate::fsrs::{Fx, Grade};
use paperpaw_sdk::MemoryStorage;

pub(crate) const FOLDER: &str = "Flashcards";

pub(crate) fn put(storage: &MemoryStorage, name: &str, text: &str) {
    storage
        .write(&alloc::format!("{FOLDER}/{name}"), text.as_bytes())
        .unwrap();
}

/// The job run through, `link` the answer to a question.
pub(crate) fn run(storage: &MemoryStorage, force: bool, link: bool) -> Vec<FileReport> {
    let mode = if force { Mode::All } else { Mode::Manual };
    let mut job = Job::new(storage, FOLDER, mode);
    for _ in 0..10_000 {
        match job.step(storage) {
            Step::Working => {}
            Step::Ask(_) => job.answer(storage, link),
            Step::Done => return job.reports,
        }
    }
    panic!("the job does not end");
}

pub(crate) fn deck_id(storage: &MemoryStorage, name: &str) -> u32 {
    cards::decks(storage)
        .unwrap()
        .into_iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("no deck {name}"))
        .id
}

pub(crate) fn cards_of(storage: &MemoryStorage, deck: u32) -> Vec<Card> {
    cards::page(storage, deck, 0, 100_000).0
}

pub(crate) fn fronts(cards: &[Card]) -> Vec<String> {
    cards.iter().map(|c| c.front.clone()).collect()
}

/// A card answered Good: its learning state is no longer new.
pub(crate) fn learn(storage: &MemoryStorage, deck: u32, front: &str) {
    let card = cards_of(storage, deck)
        .into_iter()
        .find(|c| c.front == front)
        .unwrap();
    let after = card
        .state
        .answered(Grade::Good, 100 * 86_400, false, Fx::hundredths(90));
    cards::record(storage, deck, 0, &after).unwrap();
    cards::fold(storage).unwrap();
}

#[test]
fn a_file_makes_a_deck_named_like_it_and_is_not_read_again() {
    let storage = MemoryStorage::new();
    put(
        &storage,
        "Spanish.tsv",
        "casa\thouse\ngato\tcat\tEl gato duerme.\n",
    );
    put(&storage, "_ignored.tsv", "x\ty\n");
    put(&storage, "notes.pdf", "x");
    let reports = run(&storage, false, true);
    assert_eq!(reports.len(), 1);
    assert_eq!((reports[0].added, reports[0].outcome), (2, Outcome::Merged));
    let deck = deck_id(&storage, "Spanish");
    let cards = cards_of(&storage, deck);
    assert_eq!(fronts(&cards), ["casa", "gato"]);
    assert_eq!(cards[1].example, "El gato duerme.");
    // Listed again: nothing differs, nothing is read.
    assert_eq!(
        pending(&storage, FOLDER),
        Pending {
            folder: true,
            files: 1,
            todo: 0,
            failed: 0,
        }
    );
    assert!(Job::new(&storage, FOLDER, Mode::Manual).is_empty());
    // Scanned anew: its content is as it was.
    let again = run(&storage, true, true);
    assert_eq!(again[0].outcome, Outcome::Same);
    assert_eq!(cards_of(&storage, deck).len(), 2);
}

#[test]
fn lines_added_changed_and_removed_keep_what_was_learnt() {
    let storage = MemoryStorage::new();
    put(&storage, "Words.csv", "front,back\na,1\nb,2\nc,3\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "Words");
    learn(&storage, deck, "b");
    // b's back changed, c's line gone, d new, a as it was.
    put(&storage, "Words.csv", "front,back\na,1\nb,two\nd,4\n");
    let reports = run(&storage, false, true);
    let report = &reports[0];
    assert_eq!(
        (report.added, report.changed, report.same),
        (1, 1, 1),
        "{report:?}"
    );
    let cards = cards_of(&storage, deck);
    // Nothing is deleted by a line that is gone.
    assert_eq!(fronts(&cards), ["a", "b", "c", "d"]);
    let b = &cards[1];
    assert_eq!(b.back, "two");
    assert!(!b.state.is_new(), "b keeps what was learnt");
    assert!(cards[3].state.is_new());
    // The due counts follow (three new cards, one learnt).
    assert_eq!(cards::sizes(&storage).get(&deck), Some(&4));
}

#[test]
fn a_changed_front_without_an_id_is_a_new_card() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    learn(&storage, deck, "gato");
    put(&storage, "W.tsv", "gatito\tcat\n");
    run(&storage, false, true);
    let cards = cards_of(&storage, deck);
    assert_eq!(fronts(&cards), ["gato", "gatito"]);
    assert!(!cards[0].state.is_new());
    assert!(cards[1].state.is_new());
}

#[test]
fn an_id_column_keeps_what_was_learnt_through_a_changed_front() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    let id = cards_of(&storage, deck)[0].state.id;
    learn(&storage, deck, "gato");
    put(
        &storage,
        "W.tsv",
        &alloc::format!(
            "#paperpaw:deck {deck}\nid\tfront\tback\n{id}\tgatito\tcat\n\tperro\tdog\n"
        ),
    );
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].added, 0, "{:?}", reports[0]);
    let cards = cards_of(&storage, deck);
    // The front changed in place; perro, with no id, matched by its front.
    assert_eq!(fronts(&cards), ["gatito", "perro"]);
    assert!(!cards[0].state.is_new());
    // And a second look changes nothing.
    put(
        &storage,
        "W.tsv",
        &alloc::format!(
            "#paperpaw:deck {deck}\nid\tfront\tback\n{id}\tgatito\tcat\n\tperro\tdog\n\n"
        ),
    );
    let reports = run(&storage, true, true);
    assert_eq!(cards_of(&storage, deck).len(), 2, "{reports:?}");
}

#[test]
fn equal_fronts_go_by_their_order() {
    let storage = MemoryStorage::new();
    put(&storage, "H.tsv", "bank\tseat\nbank\tbuilding\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "H");
    put(
        &storage,
        "H.tsv",
        "bank\tseat\nbank\tbuilding\nbank\tshore\n",
    );
    let reports = run(&storage, false, true);
    assert_eq!((reports[0].added, reports[0].changed), (1, 0));
    let backs: Vec<String> = cards_of(&storage, deck)
        .into_iter()
        .map(|c| c.back)
        .collect();
    assert_eq!(backs, ["seat", "building", "shore"]);
}

#[test]
fn a_deck_of_the_name_that_is_no_file_s_is_asked_about() {
    for link in [true, false] {
        let storage = MemoryStorage::new();
        let own = cards::add_deck(&storage, "spanish").unwrap();
        cards::add(
            &storage,
            own,
            [Texts {
                front: "casa".into(),
                back: "house".into(),
                example: String::new(),
            }],
        )
        .unwrap();
        put(&storage, "Spanish.tsv", "casa\thouse\ngato\tcat\n");
        let mut job = Job::new(&storage, FOLDER, Mode::Manual);
        let mut asked = false;
        loop {
            match job.step(&storage) {
                Step::Working => {}
                Step::Ask(q) => {
                    asked = true;
                    assert_eq!(
                        (q.file.as_str(), q.deck.as_str()),
                        ("Spanish.tsv", "Spanish")
                    );
                    job.answer(&storage, link);
                }
                Step::Done => break,
            }
        }
        assert!(asked);
        if link {
            // Merged into the deck: casa matched, gato added.
            assert_eq!(fronts(&cards_of(&storage, own)), ["casa", "gato"]);
            assert_eq!(cards::decks(&storage).unwrap().len(), 1);
        } else {
            assert_eq!(cards_of(&storage, own).len(), 1);
            let other = deck_id(&storage, "Spanish (2)");
            assert_eq!(fronts(&cards_of(&storage, other)), ["casa", "gato"]);
        }
    }
}

#[test]
fn a_renamed_file_keeps_its_deck_and_its_name_follows() {
    let storage = MemoryStorage::new();
    put(&storage, "Old.tsv", "a\t1\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "Old");
    storage.remove("Flashcards/Old.tsv").unwrap();
    put(&storage, "New.tsv", "a\t1\n");
    run(&storage, false, true);
    assert_eq!(cards::decks(&storage).unwrap().len(), 1);
    assert_eq!(deck_id(&storage, "New"), deck);
    // The old file is gone from the ledger, the new one is in it.
    let names: Vec<String> = sources(&storage)
        .unwrap()
        .sources
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, ["New.tsv"]);
}

#[test]
fn a_deleted_file_leaves_its_deck_as_it_is() {
    let storage = MemoryStorage::new();
    put(&storage, "Gone.tsv", "a\t1\nb\t2\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "Gone");
    storage.remove("Flashcards/Gone.tsv").unwrap();
    assert!(run(&storage, true, true).is_empty());
    assert_eq!(cards_of(&storage, deck).len(), 2);
}

#[test]
fn a_big_deck_is_merged_in_slices_without_a_card_twice() {
    let storage = MemoryStorage::new();
    let n = 4000;
    let file = |back: &str, from: usize, to: usize| -> String {
        (from..to)
            .map(|i| alloc::format!("word{i}\t{back}{i}\n"))
            .collect()
    };
    put(&storage, "Big.tsv", &file("m", 0, n));
    run(&storage, false, true);
    let deck = deck_id(&storage, "Big");
    learn(&storage, deck, "word123");
    // Every fifth back changed, 300 new lines at the end.
    let mut changed = String::new();
    for i in 0..n {
        let back = if i % 5 == 0 { "z" } else { "m" };
        changed.push_str(&alloc::format!("word{i}\t{back}{i}\n"));
    }
    changed.push_str(&file("n", n, n + 300));
    put(&storage, "Big.tsv", &changed);
    let reports = run(&storage, false, true);
    let r = &reports[0];
    assert_eq!(
        (r.added, r.changed, r.same),
        (300, n / 5, n - n / 5),
        "{r:?}"
    );
    let cards = cards_of(&storage, deck);
    assert_eq!(cards.len(), n + 300);
    let mut seen: Vec<&str> = cards.iter().map(|c| c.front.as_str()).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), n + 300);
    let word0 = cards.iter().find(|c| c.front == "word0").unwrap();
    assert_eq!(word0.back, "z0");
    let word123 = cards.iter().find(|c| c.front == "word123").unwrap();
    assert!(!word123.state.is_new());
    // Done again: nothing to do.
    let again = run(&storage, true, true);
    assert_eq!(again[0].outcome, Outcome::Same);
}

#[test]
fn a_file_that_is_no_cards_makes_no_deck_and_tells_why() {
    let storage = MemoryStorage::new();
    storage
        .write("Flashcards/Junk.tsv", b"\tonly a back\n\tmore\n")
        .unwrap();
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].outcome, Outcome::NoCards);
    assert!(reports[0].skipped > 0 && !reports[0].problems.is_empty());
    assert!(cards::decks(&storage).unwrap().is_empty());
    // A file with a header and nothing else is an empty deck.
    put(&storage, "Empty.tsv", "front\tback\n");
    run(&storage, false, true);
    assert!(cards_of(&storage, deck_id(&storage, "Empty")).is_empty());
}

#[test]
fn too_many_decks_leaves_the_files_over_unread() {
    let storage = MemoryStorage::new();
    for i in 0..cards::MAX_DECKS + 2 {
        put(&storage, &alloc::format!("d{i:02}.tsv"), "a\tb\n");
    }
    let reports = run(&storage, false, true);
    let refused = reports
        .iter()
        .filter(|r| r.outcome == Outcome::TooManyDecks)
        .count();
    assert_eq!(cards::decks(&storage).unwrap().len(), cards::MAX_DECKS);
    assert_eq!(
        refused,
        2.min(reports.len()),
        "{:?}",
        reports.iter().map(|r| r.outcome).collect::<Vec<_>>()
    );
}

#[test]
fn the_folder_is_listed_for_decks_only() {
    let storage = MemoryStorage::new();
    assert_eq!(pending(&storage, FOLDER), Pending::default());
    make_folder(&storage, FOLDER, "how to");
    assert!(storage.exists("Flashcards/_Read me.txt"));
    assert_eq!(
        pending(&storage, FOLDER),
        Pending {
            folder: true,
            files: 0,
            todo: 0,
            failed: 0,
        }
    );
    put(&storage, "A.TSV", "a\tb\n");
    put(&storage, ".hidden.csv", "a\tb\n");
    storage.create_dir("Flashcards/sub").unwrap();
    put(&storage, "sub/B.tsv", "a\tb\n");
    let (_, found) = files(&storage, FOLDER);
    assert_eq!(
        found.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["A.TSV"]
    );
    assert_eq!(deck_name("A.TSV"), "A");
    assert_eq!(deck_name("My list.v2.csv"), "My list.v2");
    assert_eq!(folder_of(" /Cards/ "), "Cards");
    assert_eq!(folder_of(""), DEFAULT_FOLDER);
}

#[test]
fn keys_ignore_case_and_spaces_but_not_letters() {
    assert_eq!(front_key("  Der  Hund "), front_key("der hund"));
    assert_ne!(front_key("hund"), front_key("hunde"));
    assert_ne!(front_key("a b"), front_key("ab"));
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(48))]

    /// Any bytes as a file: no panic, and the same merge done again adds
    /// and changes nothing.
    #[test]
    fn any_file_merges_without_a_panic_and_twice_changes_nothing(
        file in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1500),
        words in proptest::collection::vec("[a-z]{1,4}", 0..40),
    ) {
        let storage = MemoryStorage::new();
        storage.write("Flashcards/Any.tsv", &file).unwrap();
        let _ = run(&storage, false, true);
        // A file of words with equal fronts and a header-like start.
        let text: String = words.iter().map(|w| alloc::format!("{w}\t{w}x\n")).collect();
        storage.write("Flashcards/Words.csv", text.as_bytes()).unwrap();
        let _ = run(&storage, false, true);
        let before: Vec<usize> = cards::decks(&storage).unwrap().iter().map(|d| cards_of(&storage, d.id).len()).collect();
        let again = run(&storage, true, true);
        let after: Vec<usize> = cards::decks(&storage).unwrap().iter().map(|d| cards_of(&storage, d.id).len()).collect();
        proptest::prop_assert_eq!(before, after);
        for report in again {
            proptest::prop_assert!(report.added == 0 && report.changed == 0, "{:?}", report);
        }
    }
}

#[test]
fn problems_are_kept_to_the_first_twenty() {
    let storage = MemoryStorage::new();
    let mut text = String::from("ok\tfine\n");
    for _ in 0..50 {
        text.push_str("\tno front\n");
    }
    put(&storage, "P.tsv", &text);
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].skipped, 50);
    assert_eq!(reports[0].problems.len(), 20);
    assert_eq!(reports[0].problems[0], (2, Fault::NoFront));
    let _: Option<Problem> = None;
}

// ---- The senior review of PR #32 -----------------------------------------

/// Counts what one thread allocates, for the tests of the heap a merge takes.
pub(crate) mod heap {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct Counting;

    std::thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
        static LIVE: Cell<isize> = const { Cell::new(0) };
        static PEAK: Cell<isize> = const { Cell::new(0) };
        static BIGGEST: Cell<usize> = const { Cell::new(0) };
    }

    fn note(size: usize, grow: bool) {
        let _ = ON.try_with(|on| {
            if !on.get() {
                return;
            }
            let _ = LIVE.try_with(|live| {
                let now = live.get()
                    + if grow {
                        size as isize
                    } else {
                        -(size as isize)
                    };
                live.set(now);
                let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
            });
            if grow {
                let _ = BIGGEST.try_with(|big| big.set(big.get().max(size)));
            }
        });
    }

    // SAFETY: only counts, and passes every call to the system allocator.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(layout) };
            if !p.is_null() {
                note(layout.size(), true);
            }
            p
        }
        unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
            unsafe { System.dealloc(p, layout) };
            note(layout.size(), false);
        }
        unsafe fn realloc(&self, p: *mut u8, layout: Layout, new: usize) -> *mut u8 {
            let q = unsafe { System.realloc(p, layout, new) };
            if !q.is_null() {
                note(layout.size(), false);
                note(new, true);
            }
            q
        }
    }

    /// Runs `work` without counting (what the storage does inside is not the
    /// plugin's heap).
    pub fn paused<T>(work: impl FnOnce() -> T) -> T {
        let was = ON.with(Cell::get);
        ON.with(|on| on.set(false));
        let out = work();
        ON.with(|on| on.set(was));
        out
    }

    /// The largest single allocation and the most held at once (above what
    /// was held at the start) while `work` runs on this thread.
    pub fn measure<T>(work: impl FnOnce() -> T) -> (T, usize, usize) {
        ON.with(|on| on.set(true));
        LIVE.with(|v| v.set(0));
        PEAK.with(|v| v.set(0));
        BIGGEST.with(|v| v.set(0));
        let out = work();
        ON.with(|on| on.set(false));
        (
            out,
            BIGGEST.with(Cell::get),
            usize::try_from(PEAK.with(Cell::get)).unwrap_or(0),
        )
    }
}

#[global_allocator]
static COUNTING: heap::Counting = heap::Counting;

/// The job run through, with how many steps it took.
pub(crate) fn run_counting(storage: &MemoryStorage, mode: Mode) -> (Vec<FileReport>, usize) {
    let mut job = Job::new(storage, FOLDER, mode);
    for steps in 1..100_000 {
        match job.step(storage) {
            Step::Working => {}
            Step::Ask(_) => job.answer(storage, true),
            Step::Done => return (job.reports, steps),
        }
    }
    panic!("the job does not end");
}

pub(crate) fn ledger_text(storage: &MemoryStorage) -> Vec<u8> {
    paperpaw_sdk::read_file(storage, &ledger_path(), 1 << 20).unwrap_or_default()
}

#[test]
fn an_id_column_of_a_spreadsheet_is_no_card_number() {
    // The reviewer's case: ids that are not the reader's own numbers.
    let storage = MemoryStorage::new();
    put(
        &storage,
        "W.tsv",
        "id\tfront\tback\n2\tcat\tKatze\n1\tdog\tHund\n",
    );
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    learn(&storage, deck, "cat");
    put(
        &storage,
        "W.tsv",
        "id\tfront\tback\n2\tcat\tKatze\n1\tdog\tder Hund\n",
    );
    let reports = run(&storage, false, true);
    assert_eq!(
        (reports[0].added, reports[0].changed),
        (0, 1),
        "{reports:?}"
    );
    let cards = cards_of(&storage, deck);
    let by = |front: &str| cards.iter().find(|c| c.front == front).unwrap();
    assert!(!by("cat").state.is_new(), "cat kept its progress");
    assert_eq!(by("cat").back, "Katze");
    assert!(by("dog").state.is_new());
    assert_eq!(by("dog").back, "der Hund");
}

#[test]
fn a_deck_of_equal_fronts_is_told_and_takes_little_heap() {
    let storage = MemoryStorage::new();
    let text: String = (0..5000)
        .map(|i| alloc::format!("same\tback {i}\n"))
        .collect();
    put(&storage, "Same.tsv", &text);
    run(&storage, false, true);
    let deck = deck_id(&storage, "Same");
    assert_eq!(cards_of(&storage, deck).len(), 5000);
    // Changed, so that it is merged: 5,000 cards in one slice.
    let text: String = (0..5000)
        .map(|i| alloc::format!("same\tback {}\n", i + 1))
        .collect();
    put(&storage, "Same.tsv", &text);
    let ((reports, _), biggest, peak) = heap::measure(|| run_counting(&storage, Mode::Manual));
    assert_eq!(reports[0].outcome, Outcome::Crowded);
    assert!(biggest <= 64 * 1024, "one allocation of {biggest} bytes");
    assert!(peak <= 96 * 1024, "{peak} bytes at once");
    assert_eq!(cards_of(&storage, deck).len(), 5000);
}

#[test]
fn a_file_over_the_limits_is_refused_unread() {
    let storage = MemoryStorage::new();
    // Over 4 MiB: not even hashed.
    let big = b"a\tb\n".repeat((MAX_FILE as usize + 1) / 4 + 1);
    storage.write("Flashcards/Huge.tsv", &big).unwrap();
    let ((reports, steps), _, _) = heap::measure(|| run_counting(&storage, Mode::Manual));
    assert_eq!(reports[0].outcome, Outcome::TooLarge);
    assert!(steps <= 3, "{steps} steps");
    assert!(cards::decks(&storage).unwrap().is_empty());
    // More lines than a deck holds.
    let lines: String = (0..=cards::MAX_CARDS)
        .map(|i| alloc::format!("w{i}\tb\n"))
        .collect();
    put(&storage, "Lines.tsv", &lines);
    let reports = run(&storage, false, true);
    let lines_report = reports.iter().find(|r| r.file == "Lines.tsv").unwrap();
    assert_eq!(lines_report.outcome, Outcome::TooLarge);
    assert!(cards::decks(&storage).unwrap().is_empty());
}

#[test]
fn a_big_file_is_hashed_and_counted_in_steps() {
    let storage = MemoryStorage::new();
    let text: String = (0..18_000)
        .map(|i| alloc::format!("word{i:05}\tback back back\n"))
        .collect();
    assert!(text.len() > 4 * STEP as usize);
    put(&storage, "Steps.tsv", &text);
    let (reports, steps) = run_counting(&storage, Mode::Manual);
    assert_eq!(reports[0].outcome, Outcome::Merged);
    assert_eq!(reports[0].added, 18_000);
    // The hashing and the counting each took more than a few steps.
    assert!(steps >= 8, "{steps} steps");
}

#[test]
fn a_ledger_that_cannot_be_read_is_not_written_over() {
    let storage = MemoryStorage::new();
    put(&storage, "A.tsv", "a\tb\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "A");
    // A newer version of the ledger, as an older reader meets it.
    let newer = alloc::format!("paperpaw-sources 9\nA.tsv\t{deck}\t4\t0\tff\n");
    storage.write(&ledger_path(), newer.as_bytes()).unwrap();
    put(&storage, "B.tsv", "c\td\n");
    let reports = run(&storage, false, true);
    assert!(
        reports.iter().all(|r| r.outcome == Outcome::Unreadable),
        "{reports:?}"
    );
    assert_eq!(ledger_text(&storage), newer.as_bytes());
    assert_eq!(cards::decks(&storage).unwrap().len(), 1, "no deck made");
    // Too big for the ledger: the same.
    let mut huge = String::from("paperpaw-sources 1\n");
    while huge.len() <= LEDGER_MAX {
        huge.push_str(&alloc::format!("{}\t{deck}\t4\t0\tff\n", "x".repeat(200)));
    }
    storage.write(&ledger_path(), huge.as_bytes()).unwrap();
    let reports = run(&storage, false, true);
    assert!(reports.iter().all(|r| r.outcome == Outcome::Unreadable));
    assert_eq!(ledger_text(&storage).len(), huge.len());
}

#[test]
fn a_line_of_the_ledger_not_understood_is_kept_and_a_dead_link_goes() {
    let storage = MemoryStorage::new();
    put(&storage, "A.tsv", "a\tb\n");
    run(&storage, false, true);
    let mut text = String::from_utf8(ledger_text(&storage)).unwrap();
    text.push_str("from a later reader\twith\tmore\n");
    text.push_str("Dead.tsv\t999\t4\t0\tff\n");
    storage.write(&ledger_path(), text.as_bytes()).unwrap();
    put(&storage, "A.tsv", "a\tb\nc\td\n");
    run(&storage, false, true);
    let after = String::from_utf8(ledger_text(&storage)).unwrap();
    assert!(after.contains("from a later reader\twith\tmore"), "{after}");
    assert!(!after.contains("Dead.tsv"), "{after}");
    assert!(after.contains("A.tsv"));
}

#[test]
fn a_front_over_the_limit_is_not_added_again_at_every_update() {
    let storage = MemoryStorage::new();
    let front = "long ".repeat(200);
    put(&storage, "L.tsv", &alloc::format!("{front}\tone\n"));
    run(&storage, false, true);
    let deck = deck_id(&storage, "L");
    put(&storage, "L.tsv", &alloc::format!("{front}\tsecond\n"));
    let reports = run(&storage, false, true);
    assert_eq!(
        (reports[0].added, reports[0].changed),
        (0, 1),
        "{reports:?}"
    );
    assert_eq!(cards_of(&storage, deck).len(), 1);
}

#[test]
fn a_file_that_failed_is_told_and_not_tried_at_every_opening() {
    let storage = MemoryStorage::new();
    put(&storage, "Bad.tsv", "\tno front\n\tnone\n");
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].outcome, Outcome::NoCards);
    let seen = pending(&storage, FOLDER);
    assert_eq!((seen.todo, seen.failed), (0, 1));
    // Opened by itself: not tried. Chosen by hand: tried and told.
    assert!(Job::new(&storage, FOLDER, Mode::Auto).is_empty());
    assert!(!Job::new(&storage, FOLDER, Mode::Manual).is_empty());
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].outcome, Outcome::NoCards);
    // Mended: it is taken at once.
    put(&storage, "Bad.tsv", "good\tfine\n");
    assert!(!Job::new(&storage, FOLDER, Mode::Auto).is_empty());
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].outcome, Outcome::Merged);
    assert_eq!(pending(&storage, FOLDER).failed, 0);
}

#[test]
fn utf16_is_told_and_windows_1252_is_read() {
    let storage = MemoryStorage::new();
    storage
        .write("Flashcards/Sheet.txt", b"\xff\xfew\0o\0r\0d\0\t\0b\0\n\0")
        .unwrap();
    storage
        .write("Flashcards/German.csv", b"Front;Back\nM\xfcll;rubbish\n")
        .unwrap();
    let reports = run(&storage, false, true);
    let of = |file: &str| reports.iter().find(|r| r.file == file).unwrap();
    assert_eq!(of("Sheet.txt").outcome, Outcome::Utf16);
    assert_eq!(of("German.csv").outcome, Outcome::Merged);
    let deck = deck_id(&storage, "German");
    assert_eq!(fronts(&cards_of(&storage, deck)), ["Müll"]);
    assert!(
        cards::decks(&storage)
            .unwrap()
            .iter()
            .all(|d| d.name != "Sheet")
    );
}

#[test]
fn two_files_of_one_name_make_two_decks() {
    let storage = MemoryStorage::new();
    put(&storage, "Spanish.csv", "casa,house\n");
    put(&storage, "Spanish.tsv", "gato\tcat\n");
    let reports = run(&storage, false, true);
    assert_eq!(reports.len(), 2);
    assert_eq!(
        fronts(&cards_of(&storage, deck_id(&storage, "Spanish"))),
        ["casa"]
    );
    assert_eq!(
        fronts(&cards_of(&storage, deck_id(&storage, "Spanish (tsv)"))),
        ["gato"]
    );
    // Nothing to ask or add the next time.
    assert!(Job::new(&storage, FOLDER, Mode::Manual).is_empty());
    let again = run(&storage, true, true);
    assert!(
        again.iter().all(|r| r.outcome == Outcome::Same),
        "{again:?}"
    );
    assert_eq!(cards::decks(&storage).unwrap().len(), 2);
}

#[test]
fn the_folder_cannot_be_the_plugin_s_own_data_or_a_way_up() {
    assert_eq!(folder_of(" Words/ "), "Words");
    assert_eq!(folder_of(""), DEFAULT_FOLDER);
    assert_eq!(folder_of("../elsewhere"), DEFAULT_FOLDER);
    assert_eq!(folder_of("a/../b"), DEFAULT_FOLDER);
    let own = cards::path("");
    assert_eq!(folder_of(&own), DEFAULT_FOLDER);
    assert_eq!(folder_of(own.trim_end_matches('/')), DEFAULT_FOLDER);
    assert_eq!(folder_of(&alloc::format!("{own}sub")), DEFAULT_FOLDER);
}

/// A storage that can fail, and whose own allocations are not counted.
pub(crate) struct Wrapped<'a> {
    pub inner: &'a MemoryStorage,
    /// Appends that go through before the rest fail (`usize::MAX`: none fail).
    pub appends: core::cell::Cell<usize>,
    pub fail_rename: core::cell::Cell<bool>,
    pub error: core::cell::Cell<paperpaw_sdk::StorageError>,
}

impl<'a> Wrapped<'a> {
    pub(crate) fn new(inner: &'a MemoryStorage) -> Self {
        Self {
            inner,
            appends: core::cell::Cell::new(usize::MAX),
            fail_rename: core::cell::Cell::new(false),
            error: core::cell::Cell::new(paperpaw_sdk::StorageError::Io),
        }
    }
}

impl Storage for Wrapped<'_> {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.read_at(p, o, b))
    }
    fn size(&self, p: &str) -> Result<u64, paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.size(p))
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.write(p, d))
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
        let left = self.appends.get();
        if left == 0 {
            return Err(self.error.get());
        }
        self.appends.set(left.saturating_sub(1).min(left));
        heap::paused(|| self.inner.append(p, d))
    }
    fn write_at(&self, p: &str, o: u64, d: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.write_at(p, o, d))
    }
    fn truncate(&self, p: &str, l: u64) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.truncate(p, l))
    }
    fn sync(&self) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.sync())
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.list(d, v))
    }
    fn remove(&self, p: &str) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.remove(p))
    }
    fn modified(&self, p: &str) -> Option<i64> {
        heap::paused(|| self.inner.modified(p))
    }
    fn exists(&self, p: &str) -> bool {
        heap::paused(|| self.inner.exists(p))
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), paperpaw_sdk::StorageError> {
        if self.fail_rename.get() {
            return Err(self.error.get());
        }
        heap::paused(|| self.inner.rename(a, b))
    }
    fn create_dir(&self, p: &str) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.create_dir(p))
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), paperpaw_sdk::StorageError> {
        heap::paused(|| self.inner.remove_dir_all(p))
    }
}
