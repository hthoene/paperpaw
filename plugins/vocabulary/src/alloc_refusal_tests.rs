//! The data layer when the heap refuses: each allocation of a call is
//! refused in turn. An allocation that would abort (an infallible
//! `format!`, `String::from`, `push`) ends the whole test binary; a refused
//! one must come back as an error, with the files as they were, never a
//! panic or half a list.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

use crate::store::{self, Word};
use crate::sync::tests::heap::{armed, fixture};

/// A storage whose own allocations are not the plugin's: nothing is refused
/// or counted while it works.
#[derive(Debug, Default)]
struct Quiet(MemoryStorage);

impl Quiet {
    fn with(self, path: &str, data: impl Into<Vec<u8>>) -> Self {
        Quiet(self.0.with(path, data.into()))
    }
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        fixture(|| self.0.get(path))
    }
}

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        fixture(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        fixture(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // The entries are collected on the storage's side; what the visitor
        // does with them is the plugin's heap, and is counted and refused.
        let mut entries: Vec<(String, bool, u64, Option<i64>)> = Vec::new();
        fixture(|| {
            self.0.list(dir, &mut |e| {
                entries.push((String::from(e.name), e.is_dir, e.size, e.modified));
            })
        })?;
        for (name, is_dir, size, modified) in &entries {
            let mut entry = DirEntry::new(name, *is_dir, *size);
            entry.modified = *modified;
            visit(entry);
        }
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.0.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        fixture(|| self.0.rename(from, to))
    }
}

fn word(name: &str) -> Word {
    Word {
        word: name.into(),
        language: "en".into(),
        section: 3,
        offset: 40,
        path: "books/a.epub".into(),
        title: "A".into(),
        sentence: "The \"quire\" is here.".into(),
    }
}

/// A list of `n` words, `w0` the oldest.
fn list(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        word(&alloc::format!("w{i}")).push_line(&mut out).unwrap();
    }
    out
}

/// Refuses each allocation of `run` in turn on a fresh storage made by
/// `make`: with none refused it must be `Ok`; with one refused it must be
/// `Err(TooLarge)` or `Ok` (an allocation that is optional), and then the
/// storage must have kept the list whole (`intact`, which holds for the
/// list as it was and as the call makes it).
fn sweep<T: core::fmt::Debug>(
    name: &str,
    make: impl Fn() -> Quiet,
    run: impl Fn(&Quiet) -> Result<T, StorageError>,
    intact: impl Fn(&Quiet),
) {
    let storage = fixture(&make);
    let (whole, count) = armed(usize::MAX, || run(&storage));
    whole.unwrap_or_else(|error| panic!("{name}: {error:?} with nothing refused"));
    assert!(
        count > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
    for at in 1..=count {
        let storage = fixture(&make);
        let (result, _) = armed(at, || run(&storage));
        match result {
            Ok(_) | Err(StorageError::TooLarge) => {}
            Err(error) => panic!("{name}: refusing allocation {at}: {error:?}"),
        }
        intact(&storage);
    }
}

fn words(storage: &Quiet) -> Vec<String> {
    fixture(|| {
        store::range(storage, 0, usize::MAX)
            .unwrap()
            .into_iter()
            .map(|(_, w)| w.word)
            .collect()
    })
}

#[test]
fn reading_the_words_refuses_cleanly() {
    let make = || Quiet::default().with(store::file(), list(40));
    sweep("count", make, |s| store::count(s), |_| {});
    sweep("range", make, |s| store::range(s, 5, 8), |_| {});
    sweep("summary", make, |s| store::summary(s), |_| {});
}

#[test]
fn adding_a_word_refuses_cleanly_and_leaves_the_list() {
    let before = list(40);
    let make = || Quiet::default().with(store::file(), list(40));
    let (new, known) = (word("new"), word("w3"));
    sweep(
        "add",
        make,
        |s| store::add(s, &new),
        |s| {
            // Whole: the old list, or the old list and the word.
            let now = String::from_utf8(s.get(store::file()).unwrap()).unwrap();
            assert!(now == before || now.starts_with(&before), "{now}");
        },
    );
    // A word already in the list (the list is written anew).
    sweep(
        "add known",
        make,
        |s| store::add(s, &known),
        |s| {
            let now = String::from_utf8(s.get(store::file()).unwrap()).unwrap();
            // Written anew (40 words), untouched, or, where the list could
            // not be read to find the word, only added to (as for a read error).
            assert!(
                now == before || now.lines().count() == 40 || now.starts_with(&before),
                "{now}"
            );
        },
    );
}

#[test]
fn removing_a_word_refuses_cleanly_and_leaves_the_list() {
    let before = list(300);
    let make = || Quiet::default().with(store::file(), list(300));
    let gone = word("w7");
    sweep(
        "remove",
        make,
        |s| store::remove(s, &gone),
        |s| {
            let now = String::from_utf8(s.get(store::file()).unwrap()).unwrap();
            assert!(now == before || now.lines().count() == 299, "{now}");
            assert!(s.get(".paperpaw/plugins/vocabulary/words.tmp").is_none());
        },
    );
}

#[test]
fn exporting_refuses_cleanly() {
    let make = || Quiet::default().with(store::file(), list(40));
    sweep("export", make, |s| store::export(s, "out.tsv"), |_| {});
    assert_eq!(words(&make()).len(), 40);
}

// ---- The decks and cards ----------------------------------------------

use crate::cards::{self, Problem, Texts};

const CARDS: &str = ".paperpaw/plugins/vocabulary/cards";

fn texts(i: usize) -> Texts {
    Texts {
        front: alloc::format!("front {i}"),
        back: alloc::format!("back {i} with a\ttab"),
        example: alloc::format!("an example, {i}"),
    }
}

/// The calls report a refusal as `Storage(TooLarge)`; the others pass.
fn storage_error<T>(result: Result<T, Problem>) -> Result<T, StorageError> {
    result.map_err(|problem| match problem {
        Problem::Storage(error) => error,
        other => panic!("{other:?}"),
    })
}

/// Two decks, 60 cards in the first, 5 in the second; some of them answered
/// (so the index, the answers and the decks all hold something).
fn decks_of_cards() -> Quiet {
    let storage = Quiet::default();
    fixture(|| {
        let words = cards::add_deck(&storage, "Words").unwrap();
        let phrases = cards::add_deck(&storage, "Phrases").unwrap();
        cards::add(&storage, words, (0..60).map(texts)).unwrap();
        cards::add(&storage, phrases, (0..5).map(texts)).unwrap();
        let first = cards::due(&storage, 10, 1, 1, Some(words)).unwrap()[0];
        let state = first.state.answered(
            crate::fsrs::Grade::Good,
            100,
            false,
            crate::fsrs::Fx::hundredths(90),
        );
        cards::record(&storage, words, 0, &state).unwrap();
    });
    storage
}

/// What every call leaves, refused or not: the decks and cards read, the
/// card ids are each once, and no half-written file is left beside them.
fn healthy(storage: &Quiet) {
    fixture(|| {
        for deck in cards::decks(storage).unwrap() {
            let mut ids = Vec::new();
            cards::each_card(storage, deck.id, |_, card| {
                ids.push(card.state.id);
                core::ops::ControlFlow::Continue(())
            })
            .unwrap();
            let all = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), all, "a card twice in deck {}", deck.id);
        }
        let mut left = Vec::new();
        storage
            .list(CARDS, &mut |entry| {
                let name = entry.name;
                if name.ends_with(".tmp") || name.ends_with(".part") {
                    left.push(String::from(name));
                }
            })
            .unwrap();
        assert!(left.is_empty(), "left behind: {left:?}");
    });
    index_agrees(storage);
}

/// The index (what the widget counts from) agrees with the decks, as it
/// does after the next start (`recover`): where a refusal left it out of
/// step it was taken away and is made anew then; cards added when it
/// happened are counted in (`adding.txt`); answers are folded.
fn index_agrees(storage: &Quiet) {
    fixture(|| {
        cards::recover(storage);
        let mut held = 0usize;
        for deck in cards::decks(storage).unwrap() {
            cards::each_card(storage, deck.id, |_, _| {
                held += 1;
                core::ops::ControlFlow::Continue(())
            })
            .unwrap();
        }
        let waiting = storage
            .get(&alloc::format!("{CARDS}/looked-up.txt"))
            .map_or(0, |d| d.iter().filter(|b| **b == b'\n').count());
        let counted = cards::due_count(storage, i64::MAX / 2, usize::MAX).unwrap();
        assert_eq!(
            counted,
            held + waiting,
            "the index and the decks disagree: {:?} {:?}",
            storage
                .get(&alloc::format!("{CARDS}/decks.txt"))
                .map(String::from_utf8),
            storage
                .get(&alloc::format!("{CARDS}/due.txt"))
                .map(String::from_utf8)
        );
    });
}

fn cards_in(storage: &Quiet, deck: u32) -> usize {
    fixture(|| cards::page(storage, deck, 0, 0).1)
}

#[test]
fn reading_decks_and_what_is_due_refuses_cleanly() {
    sweep("decks", decks_of_cards, |s| cards::decks(s), |_| {});
    sweep("sizes", decks_of_cards, |s| cards::sizes(s), healthy);
    sweep(
        "due",
        decks_of_cards,
        |s| cards::due(s, 1_000, 20, 10, None),
        healthy,
    );
    sweep(
        "due_count",
        decks_of_cards,
        |s| cards::due_count(s, 1_000, 20),
        healthy,
    );
    sweep(
        "card_at",
        decks_of_cards,
        |s| Ok(cards::card_at(s, 1, 0).map(|c| c.front)),
        |_| {},
    );
    sweep(
        "visit_page",
        decks_of_cards,
        |s| Ok(cards::visit_page(s, 1, 3, |_| true)),
        |_| {},
    );
    sweep(
        "next_due",
        decks_of_cards,
        |s| Ok(cards::next_due(s, 0)),
        |_| {},
    );
}

#[test]
fn changing_decks_refuses_cleanly_and_keeps_the_cards() {
    sweep(
        "add_deck",
        decks_of_cards,
        |s| storage_error(cards::add_deck(s, "Verbs")),
        |s| {
            healthy(s);
            assert!(fixture(|| cards::decks(s).unwrap().len()) >= 2);
        },
    );
    sweep(
        "rename_deck",
        decks_of_cards,
        |s| storage_error(cards::rename_deck(s, 2, "Sentences")),
        healthy,
    );
    sweep(
        "delete_deck",
        decks_of_cards,
        |s| storage_error(cards::delete_deck(s, 2)),
        healthy,
    );
    sweep(
        "add",
        decks_of_cards,
        |s| {
            let more = fixture(|| (100..130).map(texts).collect::<Vec<_>>());
            storage_error(cards::add(s, 1, more))
        },
        |s| {
            healthy(s);
            // Whole cards only: the 60, or all 90 (a refusal part way may
            // have written the parts before it).
            assert!(cards_in(s, 1) >= 60);
        },
    );
    sweep(
        "edit",
        decks_of_cards,
        |s| {
            let changed = fixture(|| texts(7));
            storage_error(cards::edit(s, 1, 8, None, &changed))
        },
        |s| {
            healthy(s);
            assert_eq!(cards_in(s, 1), 60);
        },
    );
    sweep(
        "delete",
        decks_of_cards,
        |s| storage_error(cards::delete(s, 1, 9, None)),
        |s| {
            healthy(s);
            assert!(cards_in(s, 1) >= 59);
        },
    );
}

#[test]
fn answers_and_lookups_refuse_cleanly() {
    let state = crate::cards::State {
        id: 3,
        due: 500,
        last: 100,
        stability: 200,
        difficulty: 500,
        reps: 1,
        lapses: 0,
    };
    sweep(
        "record",
        decks_of_cards,
        |s| cards::record(s, 1, 0, &state),
        healthy,
    );
    sweep(
        "fold",
        decks_of_cards,
        |s| cards::fold(s),
        |s| {
            healthy(s);
            assert_eq!(cards_in(s, 1), 60);
        },
    );
    sweep(
        "queue_lookup",
        decks_of_cards,
        |s| cards::queue_lookup(s, "quire", "A \"quire\" is\tfolded."),
        |_| {},
    );
    // Enough words waiting that the list is taken in more than one part.
    let waiting = || {
        let storage = decks_of_cards();
        fixture(|| {
            for i in 0..70 {
                cards::queue_lookup(&storage, &alloc::format!("word{i}"), "a sentence").unwrap();
            }
        });
        storage
    };
    sweep(
        "take_lookups",
        waiting,
        |s| storage_error(cards::take_lookups(s, "Looked up")),
        |s| {
            healthy(s);
            // Every word is waiting still or is a card.
            let waiting = s
                .get(&alloc::format!("{CARDS}/looked-up.txt"))
                .map_or(0, |d| d.iter().filter(|b| **b == b'\n').count());
            let looked_up = fixture(|| {
                cards::decks(s)
                    .unwrap()
                    .iter()
                    .find(|d| d.name == "Looked up")
                    .map_or(0, |d| cards::page(s, d.id, 0, 0).1)
            });
            assert!(waiting + looked_up >= 70, "{waiting} + {looked_up}");
        },
    );
}

#[test]
fn recovering_after_a_cut_refuses_cleanly() {
    let cut = || {
        let storage = decks_of_cards();
        fixture(|| {
            storage
                .write(&alloc::format!("{CARDS}/adding.txt"), b"1\n")
                .unwrap();
            storage
                .write(&alloc::format!("{CARDS}/deck-2.tmp"), b"half a deck")
                .unwrap();
        });
        storage
    };
    sweep(
        "recover",
        cut,
        |s| {
            cards::recover(s);
            Ok(())
        },
        |s| {
            // Left behind is allowed where the cleaning was refused: it is
            // taken at the next start; the decks must still read.
            fixture(|| assert_eq!(cards::decks(s).unwrap().len(), 2));
        },
    );
}

/// Where the heap refuses a line buffer while a word is added again, the
/// word is only appended and the old entry stays (as for a read error): the
/// list then has the word twice. Adding it again once it is no longer the
/// newest removes every older entry: nothing is lost, duplicates heal.
#[test]
fn a_later_add_heals_duplicates_left_by_a_refused_read() {
    let make = || Quiet::default().with(store::file(), list(10));
    let again = word("w3");
    let (_, count) = armed(usize::MAX, || store::add(&make(), &again));
    let mut healed = 0;
    for at in 1..=count {
        let storage = fixture(make);
        let _ = armed(at, || store::add(&storage, &again));
        let text = String::from_utf8(storage.get(store::file()).unwrap()).unwrap();
        if text.lines().filter(|l| l.starts_with("w3\t")).count() < 2 {
            continue;
        }
        healed += 1;
        fixture(|| {
            // Another word, then the word once more: one entry of it.
            store::add(&storage, &word("x")).unwrap();
            store::add(&storage, &again).unwrap();
            let words: Vec<String> = store::range(&storage, 0, usize::MAX)
                .unwrap()
                .into_iter()
                .map(|(_, w)| w.word)
                .collect();
            assert_eq!(words.iter().filter(|w| *w == "w3").count(), 1, "{words:?}");
            assert_eq!(words.last().map(String::as_str), Some("w3"));
            assert_eq!(words.len(), 10 + 1, "{words:?}");
        });
    }
    assert!(
        healed > 0,
        "no refusal left a duplicate: the case proves nothing"
    );
}

// ---- Cards from CSV, TSV and TXT files ---------------------------------

use crate::table;

/// A spreadsheet export: a header, quotes, a record over two lines, HTML.
fn csv() -> Quiet {
    let mut file = String::from("#paperpaw:deck 1\nId;Front;Back;Example\n");
    for i in 0..40 {
        file.push_str(&alloc::format!(
            "{i};\"word {i}\";<b>the</b> meaning&amp;more;\"two\nlines, \"\"quoted\"\"\"\n"
        ));
    }
    Quiet::default().with("f", file)
}

#[test]
fn reading_a_table_refuses_cleanly() {
    sweep("detect", csv, |s| table::detect(s, "f"), |_| {});
    sweep(
        "each",
        csv,
        |s| {
            let layout = fixture(|| table::detect(s, "f").unwrap());
            let mut seen = 0;
            table::each(s, "f", &layout, None, |_| {
                seen += 1;
                core::ops::ControlFlow::Continue(())
            })?;
            assert_eq!(seen, 40);
            Ok(())
        },
        |_| {},
    );
    sweep(
        "scan in steps",
        csv,
        |s| {
            let layout = fixture(|| table::detect(s, "f").unwrap());
            let mut from = table::Resume {
                at: layout.start,
                line: layout.line,
            };
            while let Some(next) = table::scan(s, "f", &layout, from, Some(300), |_| {
                core::ops::ControlFlow::Continue(())
            })? {
                from = next;
            }
            Ok(())
        },
        |_| {},
    );
    let latin1 =
        || Quiet::default().with("f", b"front;back\nstra\xdfe;b\xe4r\nh\x80llo;x\n".to_vec());
    sweep("latin1 detect", latin1, |s| table::detect(s, "f"), |_| {});
    // The same file read: Windows-1252 fields decoded into reserved strings.
    sweep(
        "latin1 each",
        latin1,
        |s| {
            let layout = fixture(|| table::detect(s, "f").unwrap());
            assert!(layout.latin1);
            let mut records = 0;
            table::each(s, "f", &layout, None, |_| {
                records += 1;
                core::ops::ControlFlow::Continue(())
            })?;
            assert_eq!(records, 2);
            Ok(())
        },
        |_| {},
    );
    sweep(
        "latin1 scan in steps",
        latin1,
        |s| {
            let layout = fixture(|| table::detect(s, "f").unwrap());
            let mut from = table::Resume {
                at: layout.start,
                line: layout.line,
            };
            while let Some(next) = table::scan(s, "f", &layout, from, Some(10), |_| {
                core::ops::ControlFlow::Continue(())
            })? {
                from = next;
            }
            Ok(())
        },
        |_| {},
    );
    // A record read again by its offset (an error where the heap refuses:
    // no card, never a half one).
    sweep(
        "record_at",
        csv,
        |s| {
            let layout = fixture(|| table::detect(s, "f").unwrap());
            let offset = fixture(|| {
                let (mut second, mut n) = (0, 0);
                table::each(s, "f", &layout, None, |item| {
                    if let table::Item::Record(record) = item {
                        n += 1;
                        second = record.offset;
                    }
                    if n == 2 {
                        core::ops::ControlFlow::Break(())
                    } else {
                        core::ops::ControlFlow::Continue(())
                    }
                })
                .unwrap();
                second
            });
            // Whole or not there; a record is never half made.
            if let Some(record) = table::record_at(s, "f", &layout, offset)? {
                assert_eq!(record.front, "word 1");
                assert!(record.back.contains("meaning"));
            }
            Ok(())
        },
        |_| {},
    );
}

#[test]
fn turning_html_into_text_refuses_cleanly() {
    let html = alloc::format!(
        "{}<br>a &lt;b&gt; &amp; c&nbsp;d <p>x</p>",
        "<b>word</b> ".repeat(20)
    );
    let make = || Quiet::default();
    sweep("plain", make, |_| table::plain(&html), |_| {});
    assert_eq!(
        table::plain("<div>a</div>b <p>&lt;c&gt;</p>&amp;lt; &quot;d&quot;").unwrap(),
        "a b <c> &lt; \"d\""
    );
}

/// Decks and cards, but no index: it was taken away after a refusal, and
/// is made anew from two decks, one at a time. A refusal on the second
/// must not leave the index of the first as the index.
fn without_index() -> Quiet {
    let storage = decks_of_cards();
    fixture(|| {
        storage.remove(&alloc::format!("{CARDS}/due.txt")).unwrap();
        storage
            .remove(&alloc::format!("{CARDS}/answers.txt"))
            .unwrap();
    });
    storage
}

#[test]
fn rebuilding_the_index_refuses_cleanly() {
    sweep(
        "recover without an index",
        without_index,
        |s| {
            cards::recover(s);
            Ok(())
        },
        healthy,
    );
    sweep(
        "due without an index",
        without_index,
        |s| cards::due(s, 1_000, 20, 10, None),
        healthy,
    );
    sweep(
        "rebuild_index",
        without_index,
        |s| cards::rebuild_index(s),
        healthy,
    );
}

// ---- The card folder: listing, the ledger and the job ----------------------

mod folder {
    use super::*;
    use crate::cards;
    use crate::sync::tests::{FOLDER, put, run};
    use crate::sync::{self, Job, Mode, Outcome, Step};

    /// A card folder with decks, made and merged once, then changed.
    fn card() -> Quiet {
        let q = Quiet::default();
        fixture(|| {
            put(&q.0, "Spanish.tsv", "casa\thouse\ngato\tcat\nperro\tdog\n");
            put(&q.0, "French.csv", "maison,house\nchat,cat\n");
            put(&q.0, "_Read me.txt", "how");
            put(&q.0, "notes.pdf", "no deck");
            run(&q.0, false, true);
            // One line changed, one added, one file new.
            put(
                &q.0,
                "Spanish.tsv",
                "casa\ta house\ngato\tcat\nperro\tdog\nlibro\tbook\n",
            );
            put(&q.0, "Italian.tsv", "casa\thouse\n");
        });
        q
    }

    /// Every deck of the plugin still reads whole, and so does the ledger.
    fn whole(q: &Quiet) {
        fixture(|| {
            for deck in cards::decks(q).unwrap() {
                cards::each_card(q, deck.id, |_, _| core::ops::ControlFlow::Continue(())).unwrap();
            }
            sync::sources(q).unwrap();
        });
    }

    #[test]
    fn the_listing_refuses_cleanly() {
        sweep(
            "files",
            card,
            |s| {
                let (there, found) = sync::files(s, FOLDER)?;
                assert!(there && found.len() == 3);
                Ok(())
            },
            |_| {},
        );
        sweep(
            "pending",
            card,
            |s| sync::pending(s, FOLDER).map(|p| assert_eq!(p.todo, 2)),
            |_| {},
        );
        sweep(
            "make_folder",
            Quiet::default,
            |s| sync::make_folder(s, "Cards", "how to"),
            |_| {},
        );
    }

    #[test]
    fn names_refuse_cleanly() {
        let listed =
            fixture(|| alloc::vec![String::from("Spanish.csv"), String::from("Spanish.tsv")]);
        sweep(
            "deck names",
            Quiet::default,
            |_| {
                sync::deck_name("Spanish.tsv")?;
                sync::deck_name_among("Spanish.tsv", &listed)?;
                sync::folder_of(" /Cards/ ")?;
                Ok(())
            },
            |_| {},
        );
    }

    #[test]
    fn the_ledger_refuses_cleanly_and_is_never_written_half() {
        let before = fixture(|| {
            let q = card();
            sync::sources(&q).unwrap()
        });
        assert!(!before.sources.is_empty());
        sweep(
            "sources",
            card,
            |s| sync::sources(s).map(|l| assert_eq!(l.sources.len(), before.sources.len())),
            |_| {},
        );
        sweep(
            "write_sources",
            card,
            |s| {
                let ledger = fixture(|| sync::sources(s).unwrap());
                sync::write_sources(s, &ledger)
            },
            whole,
        );
    }

    #[test]
    fn a_job_refuses_cleanly_and_leaves_the_decks_whole() {
        sweep(
            "job",
            card,
            |s| {
                let mut job = Job::new(s, FOLDER, Mode::Manual)?;
                loop {
                    match job.step(s) {
                        Step::Working => {}
                        Step::Ask => job.answer(s, true),
                        Step::Done => break,
                    }
                }
                // Nothing here is the file's fault: a refusal is never a verdict
                // on the file (`Crowded` would mark it failed for good).
                assert!(
                    job.reports.iter().all(|r| r.outcome != Outcome::Crowded),
                    "a refused allocation was reported as Crowded"
                );
                // A refusal is told, as a report or as the job's flag.
                let unreadable = job.reports.iter().any(|r| r.outcome == Outcome::Unreadable);
                if job.refused() || unreadable {
                    return Err(StorageError::TooLarge);
                }
                assert_eq!(job.reports.len(), 2);
                Ok(())
            },
            whole,
        );
    }

    /// A save refused anywhere is cut off: the file is as it was or whole,
    /// no part is left, and the decks and the ledger read whole.
    fn saving(name: &str, loose: bool) {
        use crate::export::{self, Outcome as Saved, Saver, Step as Saving};
        let make = move || {
            let q = card();
            if loose {
                fixture(|| {
                    cards::add_deck(&q, "Loose").unwrap();
                    put(&q.0, "Loose.tsv", "someone else's file");
                });
            }
            q
        };
        let deck_of = |q: &Quiet| {
            fixture(|| cards::decks(q).unwrap())
                .into_iter()
                .find(|d| d.name == name)
                .unwrap()
                .id
        };
        let save = |q: &Quiet| -> Result<(), StorageError> {
            let deck = deck_of(q);
            let mut saver = Saver::new(FOLDER, deck)?;
            loop {
                match saver.step(q) {
                    Saving::Working => {}
                    Saving::Done(report) if report.outcome == Saved::Saved => return Ok(()),
                    // A refusal cuts the save off (or leaves it unlinked).
                    Saving::Done(report)
                        if matches!(
                            report.outcome,
                            Saved::Failed | Saved::NotLinked | Saved::Unreadable
                        ) =>
                    {
                        return Err(StorageError::TooLarge);
                    }
                    Saving::Done(report) => panic!("{name}: {report:?}"),
                }
            }
        };
        let files = |q: &Quiet| -> Vec<(String, Option<Vec<u8>>)> {
            let mut names = Vec::new();
            q.0.list(FOLDER, &mut |e| {
                names.push(alloc::string::String::from(e.name));
            })
            .unwrap();
            names.sort();
            names
                .into_iter()
                .map(|n| {
                    let bytes = q.get(&alloc::format!("{FOLDER}/{n}"));
                    (n, bytes)
                })
                .collect()
        };
        let before = files(&make());
        let after = {
            let q = make();
            save(&q).unwrap();
            files(&q)
        };
        sweep("save", make, save, |q| {
            whole(q);
            let now = files(q);
            // No part is left; every file there was is as it was or as saved
            // (the backup, the file, a new name).
            assert!(now.iter().all(|(n, _)| !n.ends_with(".part")), "{now:?}");
            for (n, bytes) in &before {
                let there = now.iter().find(|(m, _)| m == n);
                let saved = after.iter().find(|(m, _)| m == n);
                assert!(
                    there.is_some_and(|t| t.1 == *bytes || Some(t) == saved),
                    "{name}: {n} is neither as it was nor as saved"
                );
            }
        });
        // The linked deck's file: its update first, then the save.
        sweep(
            "update_first",
            make,
            |q| export::update_first(q, FOLDER, deck_of(q)).map(|_| ()),
            whole,
        );
    }

    #[test]
    fn saving_a_deck_refuses_cleanly_and_keeps_the_files() {
        saving("Spanish", false);
        saving("French", false);
        // No file yet, and the name taken: a free name is looked for.
        saving("Loose", true);
    }

    #[test]
    fn the_names_of_a_save_refuse_cleanly() {
        use crate::export::{backup_name, file_name_of};
        sweep(
            "names",
            Quiet::default,
            |_| {
                assert_eq!(file_name_of("A/B: C")?, "A_B_ C.tsv");
                assert_eq!(file_name_of("  ..")?, "Deck.tsv");
                assert_eq!(backup_name("W.tsv")?, "_W.bak.tsv");
                assert_eq!(backup_name("W")?, "_W.bak");
                Ok(())
            },
            |_| {},
        );
    }
}
