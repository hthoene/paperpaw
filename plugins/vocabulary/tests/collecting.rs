//! Collecting words with the real kernel: each word the reader announces
//! as looked up is kept once (newest last in `words.txt`, its sentence
//! on one line), not when switched off, not when blank; the list shows
//! them newest first, deletes one held once asked, and exports all for
//! flash cards.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const WORDS: &str = ".paperpaw/plugins/vocabulary/words.txt";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Card(Rc<MemoryStorage>);

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
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
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

impl Card {
    fn text(&self, path: &str) -> String {
        String::from_utf8(self.0.get(path).unwrap_or_default()).unwrap()
    }
}

struct Vocabulary {
    kernel: Kernel,
    card: Card,
    now: u64,
}

impl Vocabulary {
    fn new(config: &str) -> Self {
        let card = Card::default();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(card.clone())
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_vocabulary::Vocabulary)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            card,
            now: 0,
        }
    }

    /// The reader says `word` was looked up in `sentence`.
    fn looked_up(&mut self, word: &str, sentence: &str) {
        self.now += 100;
        let looked_up = events::WordLookedUp::new(
            word.to_string(),
            "en".to_string(),
            sentence.to_string(),
            "books/alice.epub".to_string(),
            "Alice\tin\nWonderland".to_string(),
            3,
            1234,
        );
        self.kernel.send_event(
            Instant(self.now),
            events::WORD_LOOKED_UP.id().clone(),
            looked_up.to_params(),
        );
    }

    fn press(&mut self, action: Id) {
        self.now += 100;
        let mut effects = self.kernel.run_action(Instant(self.now), action);
        for _ in 0..1000 {
            match effects.wake_at() {
                Some(at) if at.0 <= self.now + 1000 => {
                    self.now = self.now.max(at.0);
                    effects = self.kernel.tick(Instant(self.now));
                }
                _ => break,
            }
        }
    }

    fn screen(&self) -> String {
        self.kernel
            .active_screen()
            .map(|id| id.as_str().to_string())
            .unwrap_or_default()
    }

    fn described(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    /// The words in the file, oldest first.
    fn words(&self) -> Vec<String> {
        self.card
            .text(WORDS)
            .lines()
            .map(|l| l.split('\t').next().unwrap().to_string())
            .collect()
    }
}

#[test]
fn a_word_looked_up_is_kept_with_its_sentence_on_one_line() {
    let mut vocabulary = Vocabulary::new("");
    vocabulary.looked_up("rabbit", "The White\tRabbit\nran by.");
    let text = vocabulary.card.text(WORDS);
    assert_eq!(
        text,
        "rabbit\ten\t3\t1234\tbooks/alice.epub\tAlice in Wonderland\tThe White Rabbit ran by.\n"
    );
}

#[test]
fn the_same_word_again_is_kept_once_at_its_newest_place() {
    let mut vocabulary = Vocabulary::new("");
    vocabulary.looked_up("rabbit", "One.");
    vocabulary.looked_up("hole", "Two.");
    vocabulary.looked_up("Rabbit", "Three.");
    assert_eq!(vocabulary.words(), ["hole", "Rabbit"]);
    assert!(vocabulary.card.text(WORDS).ends_with("Three.\n"));
}

#[test]
fn blank_words_are_not_kept_and_spaces_go() {
    let mut vocabulary = Vocabulary::new("");
    vocabulary.looked_up("   ", "Nothing.");
    vocabulary.looked_up("", "Nothing.");
    assert!(!vocabulary.card.exists(WORDS));
    vocabulary.looked_up("  alice \n", "She.");
    assert_eq!(vocabulary.words(), ["alice"]);
}

#[test]
fn nothing_is_kept_when_switched_off() {
    let mut vocabulary = Vocabulary::new("[vocabulary]\ncollect = false\n");
    vocabulary.looked_up("rabbit", "The rabbit.");
    assert!(!vocabulary.card.exists(WORDS));
}

#[test]
fn the_list_shows_the_newest_first_and_deletes_a_word_held() {
    let mut vocabulary = Vocabulary::new("");
    for word in ["first", "second", "third"] {
        vocabulary.looked_up(word, &format!("The {word} one."));
    }
    vocabulary.press(paperpaw_plugin_vocabulary::OPEN);
    assert_eq!(vocabulary.screen(), "vocabulary.list");
    let shown = vocabulary.described();
    let rows: Vec<&String> = shown
        .iter()
        .filter(|l| {
            ["first", "second", "third"]
                .iter()
                .any(|w| l.starts_with(w))
        })
        .collect();
    assert_eq!(rows.len(), 3, "{shown:?}");
    assert!(
        rows[0].starts_with("third") && rows[2].starts_with("first"),
        "{rows:?}"
    );
    assert!(rows[0].contains("The third one."), "{rows:?}");
    // Held (the menu button) on the second row: *Delete* is the last
    // option (the book is not on the card, so no *Go to the book*).
    vocabulary.press(actions::DOWN);
    vocabulary.press(actions::MENU);
    let options = vocabulary.described();
    assert!(options.iter().any(|l| l == "Delete"), "{options:?}");
    assert!(
        !options.iter().any(|l| l == "Go to the book"),
        "{options:?}"
    );
    vocabulary.press(actions::DOWN);
    vocabulary.press(actions::CONFIRM);
    // Asked first: *Keep*, where the focus starts, keeps the word.
    let asked = vocabulary.described();
    assert!(
        asked.iter().any(|l| l.contains("Delete “second”?")),
        "{asked:?}"
    );
    assert!(asked.iter().any(|l| l == "Keep"), "{asked:?}");
    vocabulary.press(actions::CONFIRM);
    assert_eq!(vocabulary.screen(), "vocabulary.list");
    assert_eq!(vocabulary.words(), ["first", "second", "third"]);
    // Asked again, *Delete* deletes it.
    vocabulary.press(actions::MENU);
    vocabulary.press(actions::DOWN);
    vocabulary.press(actions::CONFIRM);
    vocabulary.press(actions::LEFT);
    vocabulary.press(actions::CONFIRM);
    assert_eq!(vocabulary.screen(), "vocabulary.list");
    assert_eq!(vocabulary.words(), ["first", "third"]);
    let shown = vocabulary.described();
    assert!(!shown.iter().any(|l| l.starts_with("second")), "{shown:?}");
}

#[test]
fn the_words_are_exported_for_flash_cards() {
    let mut vocabulary = Vocabulary::new("");
    vocabulary.looked_up("rabbit", "The rabbit ran.");
    vocabulary.looked_up("hole", "Down the hole.");
    vocabulary.press(paperpaw_plugin_vocabulary::OPEN);
    // The last row: *Export*.
    vocabulary.press(actions::DOWN);
    vocabulary.press(actions::DOWN);
    vocabulary.press(actions::CONFIRM);
    let exported = vocabulary.card.text("Notes/Vocabulary.tsv");
    // After the lines that tell Anki how to read the rest.
    assert_eq!(
        exported,
        "#separator:tab\n#html:false\n\
         rabbit\tThe rabbit ran.\tAlice in Wonderland\nhole\tDown the hole.\tAlice in Wonderland\n"
    );
}

#[test]
fn without_words_the_list_says_where_they_come_from() {
    let mut vocabulary = Vocabulary::new("");
    vocabulary.press(paperpaw_plugin_vocabulary::OPEN);
    let shown = vocabulary.described();
    assert!(
        shown.iter().any(|l| l.contains("No words yet")),
        "{shown:?}"
    );
    // Nothing to choose, nothing breaks.
    vocabulary.press(actions::CONFIRM);
    vocabulary.press(actions::MENU);
    assert_eq!(vocabulary.screen(), "vocabulary.list");
}

/// With `cards-from-lookups` on, each word looked up becomes a card of
/// the deck *Looked up*, once, its sentence the example; off at first. A
/// lookup only notes the word; the flash cards, opened, make the cards.
#[test]
fn words_looked_up_become_cards_when_wanted() {
    let decks = ".paperpaw/plugins/vocabulary/cards/decks.txt";
    let waiting = ".paperpaw/plugins/vocabulary/cards/looked-up.txt";
    let mut off = Vocabulary::new("");
    off.looked_up("curious", "Curiouser and curiouser!");
    assert!(!off.card.0.exists(waiting));

    let mut on = Vocabulary::new("[vocabulary]\ncards-from-lookups = true\n");
    on.looked_up("curious", "Curiouser and curiouser!");
    on.looked_up("Curious", "Again.");
    assert_eq!(on.card.text(waiting).lines().count(), 2);
    assert!(
        !on.card.0.exists(decks),
        "no deck read nor made by a lookup"
    );
    on.press(Id::new_static("vocabulary.flash-cards"));
    on.looked_up("rabbit", "A white rabbit.");
    on.looked_up("curious", "Once more.");
    on.press(actions::BACK);
    on.press(Id::new_static("vocabulary.flash-cards"));
    assert!(!on.card.0.exists(waiting));
    assert!(
        on.card.text(decks).contains("Looked up"),
        "{}",
        on.card.text(decks)
    );
    let deck = on
        .card
        .text(".paperpaw/plugins/vocabulary/cards/deck-1.txt");
    let fronts: Vec<(&str, &str)> = deck
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[7], f[9])
        })
        .collect();
    assert_eq!(
        fronts,
        [
            ("curious", "Curiouser and curiouser!"),
            ("rabbit", "A white rabbit.")
        ]
    );
}
