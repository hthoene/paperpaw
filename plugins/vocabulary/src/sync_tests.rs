use super::*;
use crate::cards::{self, Card, Problem};
use crate::fsrs::{Fx, Grade};
use paperpaw_sdk::MemoryStorage;

const FOLDER: &str = "Flashcards";

fn put(storage: &MemoryStorage, name: &str, text: &str) {
    storage
        .write(&alloc::format!("{FOLDER}/{name}"), text.as_bytes())
        .unwrap();
}

/// The job run through, `link` the answer to a question.
fn run(storage: &MemoryStorage, force: bool, link: bool) -> Vec<FileReport> {
    let mut job = Job::new(storage, FOLDER, force);
    for _ in 0..10_000 {
        match job.step(storage) {
            Step::Working => {}
            Step::Ask(_) => job.answer(storage, link),
            Step::Done => return job.reports,
        }
    }
    panic!("the job does not end");
}

fn deck_id(storage: &MemoryStorage, name: &str) -> u32 {
    cards::decks(storage)
        .unwrap()
        .into_iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("no deck {name}"))
        .id
}

fn cards_of(storage: &MemoryStorage, deck: u32) -> Vec<Card> {
    cards::page(storage, deck, 0, 100_000).0
}

fn fronts(cards: &[Card]) -> Vec<String> {
    cards.iter().map(|c| c.front.clone()).collect()
}

/// A card answered Good: its learning state is no longer new.
fn learn(storage: &MemoryStorage, deck: u32, front: &str) {
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
            todo: 0
        }
    );
    assert!(Job::new(&storage, FOLDER, false).is_empty());
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
        &alloc::format!("id\tfront\tback\n{id}\tgatito\tcat\n\tperro\tdog\n"),
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
        &alloc::format!("id\tfront\tback\n{id}\tgatito\tcat\n\tperro\tdog\n\n"),
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
        let mut job = Job::new(&storage, FOLDER, false);
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
    let names: Vec<String> = sources(&storage).into_iter().map(|s| s.name).collect();
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
        .write("Flashcards/Junk.tsv", b"\tonly a back\n\xff\xfe\xfd\n")
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
            todo: 0
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
