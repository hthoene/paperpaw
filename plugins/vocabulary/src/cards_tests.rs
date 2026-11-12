use super::*;
use alloc::format;
use paperpaw_sdk::MemoryStorage;

/// [`super::due`], where the tests want the cards.
fn due(storage: &dyn Storage, until: i64, new: usize, most: usize, only: Option<u32>) -> Vec<Due> {
    super::due(storage, until, new, most, only).unwrap()
}

fn texts(front: &str, back: &str) -> Texts {
    Texts {
        front: front.into(),
        back: back.into(),
        example: String::new(),
    }
}

const HUNDRED_DAYS: i64 = 100 * DAY;

#[test]
fn texts_with_tabs_and_line_breaks_come_back_as_they_were() {
    let card = Card {
        state: State {
            id: 3,
            due: 5,
            last: 4,
            stability: 317,
            difficulty: 528,
            reps: 1,
            lapses: 0,
        },
        front: "a\tb".into(),
        back: "line\nnext \\ end".into(),
        example: String::new(),
    };
    let line = card.line();
    assert_eq!(line.matches('\t').count(), 9, "{line}");
    assert_eq!(line.matches('\n').count(), 1);
    assert_eq!(Card::parsed(line.trim_end_matches('\n')), Some(card));
    assert_eq!(Card::parsed("not a card"), None);
}

#[test]
fn decks_and_cards_are_added_changed_and_deleted() {
    let storage = MemoryStorage::new();
    let words = add_deck(&storage, "  Words ").unwrap();
    assert_eq!(add_deck(&storage, " "), Err(Problem::Empty));
    assert_eq!(deck_named(&storage, "words"), Ok(words));
    let other = deck_named(&storage, "Phrases").unwrap();
    assert_ne!(other, words);
    assert_eq!(decks(&storage).unwrap().len(), 2);

    let added = add(
        &storage,
        words,
        [
            texts("Hund", "dog"),
            texts(" ", "no front"),
            texts("Katze", "cat"),
        ],
    );
    assert_eq!(added, Ok(2));
    assert_eq!(add(&storage, 99, [texts("x", "y")]), Err(Problem::Missing));
    let (cards, total) = page(&storage, words, 0, 10);
    assert_eq!(total, 2);
    assert_eq!(cards[0].front, "Hund");
    assert_eq!((cards[0].state.id, cards[1].state.id), (1, 2));
    assert!(cards[0].state.is_new());

    edit(&storage, words, 2, None, &texts("Katze", "the cat")).unwrap();
    assert_eq!(page(&storage, words, 1, 1).0[0].back, "the cat");
    assert_eq!(
        edit(&storage, words, 9, None, &texts("a", "b")),
        Err(Problem::Missing)
    );
    delete(&storage, words, 1, None).unwrap();
    let (cards, total) = page(&storage, words, 0, 10);
    assert_eq!((total, cards[0].front.as_str()), (1, "Katze"));
    // A new card after a deleted one keeps a number of its own.
    add(&storage, words, [texts("Maus", "mouse")]).unwrap();
    assert_eq!(page(&storage, words, 1, 1).0[0].state.id, 3);
    assert_eq!(sizes(&storage).unwrap().get(&words).copied().unwrap(), 2);

    rename_deck(&storage, other, "Sätze").unwrap();
    assert_eq!(decks(&storage).unwrap()[1].name, "Sätze");
    delete_deck(&storage, other).unwrap();
    assert_eq!(decks(&storage).unwrap().len(), 1);
    assert_eq!(delete_deck(&storage, other), Err(Problem::Missing));
}

#[test]
fn answers_are_kept_folded_and_counted() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    add(
        &storage,
        deck,
        [texts("a", "1"), texts("b", "2"), texts("c", "3")],
    )
    .unwrap();
    let now = HUNDRED_DAYS;
    // All three are new: as many as may be shown.
    assert_eq!(due_count(&storage, now, 20).unwrap(), 3);
    assert_eq!(due_count(&storage, now, 2).unwrap(), 2);
    let queue = due(&storage, now, 20, 100, None);
    assert_eq!(queue.len(), 3);
    let first = card_at(&storage, deck, queue[0].offset).unwrap();
    assert_eq!(first.front, "a");

    // a: Good (due in three days); b: Again (due in ten minutes).
    let retention = Fx::hundredths(90);
    let good = queue[0].state.answered(Grade::Good, now, false, retention);
    record(&storage, deck, 0, &good).unwrap();
    let again = queue[1].state.answered(Grade::Again, now, false, retention);
    record(&storage, deck, 0, &again).unwrap();
    assert_eq!(good.due, now + 3 * DAY);
    // Due by a time is due before it: up to the quarter hour after a.
    let until = now + 3 * DAY + QUARTER;
    assert_eq!(again.due, now + fsrs::AGAIN_SECONDS);
    // Counted from the answers, before they are folded in.
    assert_eq!(due_count(&storage, now, 20).unwrap(), 1, "c, new");
    assert_eq!(
        due_count(&storage, now + 3600, 20).unwrap(),
        2,
        "and b again"
    );
    assert_eq!(due_count(&storage, until, 20).unwrap(), 3, "and a");

    fold(&storage).unwrap();
    assert!(!storage.exists(ANSWERS));
    assert_eq!(due_count(&storage, until, 20).unwrap(), 3);
    let (cards, _) = page(&storage, deck, 0, 3);
    assert_eq!(cards[0].state, good);
    assert_eq!(cards[1].state, again);
    assert!(cards[2].state.is_new());

    // Later: the reviews due first, then what is new.
    let later = due(&storage, until, 20, 100, Some(deck));
    let fronts: Vec<String> = later
        .iter()
        .map(|d| card_at(&storage, deck, d.offset).unwrap().front)
        .collect();
    assert_eq!(fronts, ["b", "a", "c"]);
    assert_eq!(due(&storage, until, 0, 1, None).len(), 1);
    assert!(due(&storage, until, 20, 100, Some(deck + 1)).is_empty());
    // b is due within the hour of now, a three days later.
    assert_eq!(next_due(&storage, now), Some(now + 3 * DAY));
}

#[test]
fn cards_are_added_from_a_file_a_line_at_a_time() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    let file = [
        "Hund\tdog\tDer Hund bellt.",
        "",
        "Katze\tcat",
        "two\\nlines\tb",
    ]
    .join("\n");
    storage.write("import.tsv", file.as_bytes()).unwrap();
    assert_eq!(add_from(&storage, deck, "import.tsv"), Ok(3));
    let (cards, total) = page(&storage, deck, 0, 9);
    assert_eq!(total, 3);
    assert_eq!(cards[0].example, "Der Hund bellt.");
    assert_eq!(cards[2].front, "two\nlines");
    assert_eq!(sizes(&storage).unwrap().get(&deck).copied().unwrap(), 3);
    assert_eq!(add_from(&storage, 7, "import.tsv"), Err(Problem::Missing));
}

#[test]
fn a_deck_cut_off_while_written_is_put_right() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    add(&storage, deck, [texts("a", "1")]).unwrap();
    storage
        .write(&deck_temporary(deck).unwrap(), b"paperpaw-cards 1\n1\t0")
        .unwrap();
    recover(&storage);
    assert!(!storage.exists(&deck_temporary(deck).unwrap()));
    assert_eq!(page(&storage, deck, 0, 9).1, 1);
}

/// A read error on an intact deck is not a missing deck: the unfinished
/// copy is kept, never put over the deck, and goes once the deck can be told.
#[test]
fn a_deck_that_cannot_be_told_keeps_its_place_and_the_unfinished_copy() {
    let storage = Unreadable::default();
    let deck = add_deck(&storage, "Words").unwrap();
    add(&storage, deck, [texts("a", "1")]).unwrap();
    let file = deck_path(deck).unwrap();
    let temporary = deck_temporary(deck).unwrap();
    let whole = storage.inner.get(&file);
    storage
        .write(&temporary, b"paperpaw-cards 1\n1\t0")
        .unwrap();
    storage.unsized_.borrow_mut().push(file.clone());
    recover(&storage);
    assert_eq!(storage.inner.get(&file), whole, "the deck stays");
    assert!(storage.inner.exists(&temporary), "the part is kept");
    storage.unsized_.borrow_mut().clear();
    recover(&storage);
    assert_eq!(storage.inner.get(&file), whole);
    assert!(
        !storage.inner.exists(&temporary),
        "the deck was there: part goes"
    );
}

/// The same for the whole files (list of decks, state, index).
#[test]
fn a_file_that_cannot_be_told_is_not_replaced_by_its_part() {
    let storage = Unreadable::default();
    add_deck(&storage, "Words").unwrap();
    let part = part_of(DECKS).unwrap();
    let whole = storage.inner.get(DECKS);
    storage.write(&part, b"unfinished").unwrap();
    storage.unsized_.borrow_mut().push(DECKS.into());
    recover_whole(&storage, DECKS, &part);
    assert_eq!(storage.inner.get(DECKS), whole);
    assert!(storage.inner.exists(&part));
    storage.unsized_.borrow_mut().clear();
    recover_whole(&storage, DECKS, &part);
    assert!(!storage.inner.exists(&part));
}

#[test]
fn new_cards_are_counted_by_day() {
    let storage = MemoryStorage::new();
    let now = HUNDRED_DAYS + 3600;
    // Two hours ahead of UTC: that day ends at 22:00 UTC.
    let day = Day::of(now, now + 7200);
    assert_eq!(day.end, HUNDRED_DAYS + DAY - 7200);
    assert_eq!(new_left(&storage, day, 20), 20);
    new_shown(&storage, day, now);
    new_shown(&storage, day, now);
    assert_eq!(new_left(&storage, day, 20), 18);
    let tomorrow = Day::of(now + DAY, now + DAY + 7200);
    assert_eq!(new_left(&storage, tomorrow, 20), 20, "another day");
    // The time last known stays for a device without a clock.
    assert_eq!(kept(&storage).clock, now);
}

#[test]
fn cuts_leave_nothing_lost_nor_overwritten() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    // A last line cut off: the next card goes on a line of its own.
    storage
        .append(&deck_path(deck).unwrap(), b"7\t0\t0\t0\t0\t0\t0\tcut")
        .unwrap();
    add(&storage, deck, [texts("Hund", "dog")]).unwrap();
    let (cards, total) = page(&storage, deck, 0, 9);
    assert_eq!(total, 2);
    assert_eq!(
        (cards[0].front.as_str(), cards[1].front.as_str()),
        ("cut", "Hund")
    );
    assert_eq!(cards[1].state.id, 8);

    // Cards added while the power went (counted nowhere yet): counted at
    // the start.
    storage
        .write(ADDING, format!("{deck}\n").as_bytes())
        .unwrap();
    storage
        .append(
            &deck_path(deck).unwrap(),
            b"9\t0\t0\t0\t0\t0\t0\tKatze\tcat\t\n",
        )
        .unwrap();
    // (The card put there by hand above was not counted either: the index,
    // made from the deck when the first card was added, has the two before.)
    assert_eq!(sizes(&storage).unwrap().get(&deck).copied().unwrap(), 2);
    recover(&storage);
    assert_eq!(sizes(&storage).unwrap().get(&deck).copied().unwrap(), 3);
    assert!(!storage.exists(ADDING));

    // A line that is no card stays as it is when the deck is written anew.
    storage
        .append(&deck_path(deck).unwrap(), b"a later version's line\n")
        .unwrap();
    delete(&storage, deck, 8, None).unwrap();
    let file = String::from_utf8(storage.get(&deck_path(deck).unwrap()).unwrap()).unwrap();
    assert!(file.contains("a later version's line\n"), "{file}");
    assert!(!file.contains("Hund"));

    // A deck's file left without its list: a new deck goes past it.
    storage
        .write(&deck_path(deck + 1).unwrap(), b"paperpaw-cards 1\n")
        .unwrap();
    let other = add_deck(&storage, "Phrases").unwrap();
    assert_eq!(other, deck + 2);

    // The list cut while written whole: the new one, where the old is gone.
    let list = path("decks.txt");
    let whole = storage.get(&list).unwrap();
    storage.remove(&list).unwrap();
    storage
        .write(&crate::files::part_of(&list).unwrap(), &whole)
        .unwrap();
    recover(&storage);
    assert_eq!(decks(&storage).unwrap().len(), 2);
}

/// A card whose files listed in `unreadable` cannot be read.
#[derive(Default)]
struct Unreadable {
    inner: MemoryStorage,
    unreadable: core::cell::RefCell<Vec<String>>,
    /// Files whose size cannot be told (a read error, not a missing file).
    unsized_: core::cell::RefCell<Vec<String>>,
}

impl Unreadable {
    fn break_file(&self, path: &str) {
        self.unreadable.borrow_mut().push(path.into());
    }
}

impl Storage for Unreadable {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.unreadable.borrow().iter().any(|p| p == path) {
            return Err(StorageError::Io);
        }
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        if self.unsized_.borrow().iter().any(|p| p == path) {
            return Err(StorageError::Io);
        }
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to)
    }
}

/// Answers that cannot be read are not folded in, and not deleted
/// either: they are folded in once they can be read.
#[test]
fn answers_that_cannot_be_read_are_kept() {
    let storage = Unreadable::default();
    let deck = add_deck(&storage, "Words").unwrap();
    add(&storage, deck, [texts("a", "1")]).unwrap();
    let queue = due(&storage, HUNDRED_DAYS, 20, 100, None);
    let good = queue[0]
        .state
        .answered(Grade::Good, HUNDRED_DAYS, false, Fx::hundredths(90));
    record(&storage, deck, 0, &good).unwrap();
    let answers_before = storage.inner.get(ANSWERS);
    storage.break_file(ANSWERS);
    assert_eq!(fold(&storage), Err(StorageError::Io));
    assert_eq!(storage.inner.get(ANSWERS), answers_before);
    storage.unreadable.borrow_mut().clear();
    fold(&storage).unwrap();
    assert_eq!(page(&storage, deck, 0, 1).0[0].state, good);
}

/// A list of decks that cannot be read is not written anew from what was
/// read of it: adding, renaming, deleting or finding a deck fails, and
/// every deck stays listed.
#[test]
fn decks_that_cannot_be_read_are_not_written_over() {
    let storage = Unreadable::default();
    let words = add_deck(&storage, "Words").unwrap();
    add_deck(&storage, "Phrases").unwrap();
    let list = path("decks.txt");
    let before = storage.inner.get(&list);
    storage.break_file(&list);
    let failed = Err(Problem::Storage(StorageError::Io));
    assert_eq!(add_deck(&storage, "More"), failed.clone());
    assert_eq!(deck_named(&storage, "Looked up"), failed.clone());
    assert_eq!(
        rename_deck(&storage, words, "Wörter"),
        failed.clone().map(|_| ())
    );
    assert_eq!(delete_deck(&storage, words), failed.clone().map(|_| ()));
    queue_lookup(&storage, "Hund", "").unwrap();
    assert_eq!(take_lookups(&storage, "Looked up"), failed.map(|_| 0));
    assert_eq!(storage.inner.get(&list), before);
    assert!(storage.exists(&deck_path(words).unwrap()));
}

#[test]
fn words_looked_up_wait_and_become_cards_once() {
    let storage = MemoryStorage::new();
    queue_lookup(&storage, "Hund", "Der Hund bellt.").unwrap();
    queue_lookup(&storage, "hund", "Noch einmal.").unwrap();
    queue_lookup(&storage, " ", "").unwrap();
    // Counted as new while they wait.
    assert_eq!(due_count(&storage, HUNDRED_DAYS, 20).unwrap(), 2);
    assert_eq!(take_lookups(&storage, "Looked up"), Ok(1));
    queue_lookup(&storage, "HUND", "").unwrap();
    queue_lookup(&storage, "Katze", "").unwrap();
    assert_eq!(take_lookups(&storage, "Looked up"), Ok(1));
    let deck = decks(&storage).unwrap()[0].id;
    let (cards, _) = page(&storage, deck, 0, 9);
    assert_eq!(cards[0].example, "Der Hund bellt.");
    assert_eq!(cards[1].front, "Katze");
    assert_eq!(due_count(&storage, HUNDRED_DAYS, 20).unwrap(), 2);
}

/// Ids are never given twice: a card or a deck deleted keeps its number,
/// also over a restart, so a change meant for it never reaches a newer
/// one.
#[test]
fn ids_are_never_given_twice() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    add(
        &storage,
        deck,
        [texts("Hund", "dog"), texts("Katze", "cat")],
    )
    .unwrap();
    // The newest card deleted, the reader started again, a card added.
    delete(&storage, deck, 2, None).unwrap();
    recover(&storage);
    add(&storage, deck, [texts("Maus", "mouse")]).unwrap();
    let (cards, _) = page(&storage, deck, 0, 9);
    assert_eq!(cards[1].state.id, 3);
    // A change meant for the card deleted finds nothing.
    assert_eq!(
        edit(&storage, deck, 2, None, &texts("Katze", "the cat")),
        Err(Problem::Missing)
    );
    assert_eq!(delete(&storage, deck, 2, None), Err(Problem::Missing));
    assert_eq!(page(&storage, deck, 1, 1).0[0].front, "Maus");

    // The same for decks.
    let other = add_deck(&storage, "Phrases").unwrap();
    delete_deck(&storage, other).unwrap();
    recover(&storage);
    let more = add_deck(&storage, "More").unwrap();
    assert_eq!(more, other + 1);
    rename_deck(&storage, more, "Sätze").unwrap();
    delete_deck(&storage, more).unwrap();
    assert_eq!(add_deck(&storage, "Again").unwrap(), more + 1);
}

/// Files written before the counter was kept: the next id is the one
/// after the highest; a counter left behind by an older version that
/// added cards past it is never gone back to.
#[test]
fn ids_of_files_from_before_the_counter_go_on_from_the_highest() {
    let storage = MemoryStorage::new();
    storage
        .write(&path("decks.txt"), b"paperpaw-decks 1\n4\tWords\n")
        .unwrap();
    storage
        .write(
            &deck_path(4).unwrap(),
            b"paperpaw-cards 1\n5\t0\t0\t0\t0\t0\t0\tHund\tdog\t\n",
        )
        .unwrap();
    add(&storage, 4, [texts("Katze", "cat")]).unwrap();
    assert_eq!(page(&storage, 4, 1, 1).0[0].state.id, 6);
    assert_eq!(add_deck(&storage, "Phrases"), Ok(5));
    // An older version deleted the deck's newest cards and added others
    // after the counter line, which it keeps as it is.
    storage
        .write(
            &deck_path(4).unwrap(),
            b"paperpaw-cards 1\nnext\t9\n10\t0\t0\t0\t0\t0\t0\tMaus\tmouse\t\n",
        )
        .unwrap();
    add(&storage, 4, [texts("Igel", "hedgehog")]).unwrap();
    assert_eq!(page(&storage, 4, 1, 1).0[0].state.id, 11);
    delete(&storage, 4, 11, None).unwrap();
    add(&storage, 4, [texts("Igel", "hedgehog")]).unwrap();
    assert_eq!(page(&storage, 4, 1, 1).0[0].state.id, 12);
    let file = String::from_utf8(storage.get(&deck_path(4).unwrap()).unwrap()).unwrap();
    assert_eq!(file.matches("next\t").count(), 1, "{file}");
}

/// A change or a deletion made from what a card was before is refused
/// once the card changed since: the card stays as it is now.
#[test]
fn a_change_to_a_card_seen_before_it_changed_is_refused() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    add(&storage, deck, [texts("Hund", "dog")]).unwrap();
    let seen = page(&storage, deck, 0, 1).0[0].version();
    edit(&storage, deck, 1, Some(seen), &texts("Hund", "the dog")).unwrap();
    assert_eq!(
        edit(&storage, deck, 1, Some(seen), &texts("Hund", "a dog")),
        Err(Problem::Changed)
    );
    assert_eq!(delete(&storage, deck, 1, Some(seen)), Err(Problem::Changed));
    let card = page(&storage, deck, 0, 1).0[0].clone();
    assert_eq!(card.back, "the dog");
    delete(&storage, deck, 1, Some(card.version())).unwrap();
    assert_eq!(page(&storage, deck, 0, 1).1, 0);
}

/// Cards due at the same moment come in the order of the decks in
/// `decks.txt`, whatever their ids (a file edited by hand or synced may
/// list them in another order), as the stable sort had it.
#[test]
fn ties_follow_the_decks_file_not_the_ids() {
    let storage = MemoryStorage::new();
    let first = add_deck(&storage, "First").unwrap();
    let second = add_deck(&storage, "Second").unwrap();
    for (deck, front) in [(first, "a"), (second, "b")] {
        add(&storage, deck, [texts(front, "x")]).unwrap();
        rewrite(&storage, deck, |card| {
            card.state = State {
                id: card.state.id,
                due: 100,
                last: 50,
                stability: 100,
                difficulty: 500,
                reps: 1,
                lapses: 0,
            };
            true
        })
        .unwrap();
    }
    // The decks listed with the higher id first.
    let (mut listed, next) = decks_and_next(&storage).unwrap();
    listed.reverse();
    write_decks(&storage, &listed, next).unwrap();
    assert!(listed[0].id > listed[1].id);
    let order: Vec<u32> = due(&storage, 1000, 0, 10, None)
        .iter()
        .map(|d| d.deck)
        .collect();
    assert_eq!(order, [second, first]);
}

/// A storage whose read of one byte fails and leaves zero in the buffer, as the
/// host's does after a failed read.
struct LastByteFails(MemoryStorage);

impl Storage for LastByteFails {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if buf.len() == 1 {
            buf.fill(0);
            return Err(StorageError::Io);
        }
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
}

#[test]
fn a_last_byte_that_cannot_be_read_is_taken_for_the_end_of_a_line() {
    let storage = LastByteFails(MemoryStorage::new());
    let deck = add_deck(&storage, "Words").unwrap();
    add(&storage, deck, [texts("Hund", "dog")]).unwrap();
    add(&storage, deck, [texts("Katze", "cat")]).unwrap();
    let mut text = alloc::vec![0u8; 4096];
    let path = deck_path(deck).unwrap();
    let n = storage.0.read_at(&path, 0, &mut text).unwrap();
    let text = core::str::from_utf8(&text[..n]).unwrap();
    assert!(
        !text.contains("\n\n"),
        "no empty line between cards: {text:?}"
    );
    assert_eq!(page(&storage, deck, 0, 9).1, 2);
}

#[test]
fn a_blank_line_in_a_deck_is_no_card_and_splits_nothing() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    add(&storage, deck, [texts("Hund", "dog")]).unwrap();
    let path = deck_path(deck).unwrap();
    storage.append(&path, b"\n\n").unwrap();
    add(&storage, deck, [texts("Katze", "cat")]).unwrap();
    let (cards, total) = page(&storage, deck, 0, 9);
    assert_eq!(total, 2, "blank lines are no cards");
    assert_eq!(
        (cards[0].front.as_str(), cards[1].front.as_str()),
        ("Hund", "Katze")
    );
}
