use super::*;
use crate::cards::{self, Texts};
use crate::sync::tests::{
    FOLDER, Wrapped, cards_of, deck_id, fronts, heap, learn, ledger_text, put, run,
};
use crate::sync::{self, FileReport, Mode};
use paperpaw_sdk::MemoryStorage;

/// A save run through, with the steps it took.
fn save(storage: &dyn Storage, deck: u32) -> (Report, usize) {
    let mut saver = Saver::new(FOLDER, deck);
    for steps in 1..100_000 {
        if let Step::Done(report) = saver.step(storage) {
            return (report, steps);
        }
    }
    panic!("the save does not end");
}

/// What the screen does: the file read first where it changed, then the save.
fn save_as_the_screen(storage: &dyn Storage, deck: u32) -> (Vec<FileReport>, Option<Report>) {
    let mut reports = Vec::new();
    if let Some(mut job) = update_first(storage, FOLDER, deck) {
        loop {
            match job.step(storage) {
                sync::Step::Done => break,
                sync::Step::Ask(_) => job.answer(storage, true),
                sync::Step::Working => {}
            }
        }
        reports = job.reports;
    }
    if !may_save(&reports) {
        return (reports, None);
    }
    (reports, Some(save(storage, deck).0))
}

fn read(storage: &dyn Storage, name: &str) -> String {
    let bytes = paperpaw_sdk::read_file(storage, &alloc::format!("{FOLDER}/{name}"), 1 << 24)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    String::from_utf8(bytes).unwrap()
}

fn ledger(storage: &dyn Storage) -> sync::Ledger {
    sync::sources(storage).unwrap()
}

#[test]
fn a_deck_is_saved_with_its_marker_its_ids_and_quoted_where_needed() {
    let storage = MemoryStorage::new();
    put(
        &storage,
        "Words.tsv",
        "hello\tworld\n\"tab\there\"\t\"say \"\"hi\"\"\"\tex, with comma\n\"two\nlines\"\tb\n",
    );
    run(&storage, false, true);
    let deck = deck_id(&storage, "Words");
    let before = read(&storage, "Words.tsv");
    let (report, _) = save(&storage, deck);
    assert_eq!(report.outcome, Outcome::Saved, "{report:?}");
    assert_eq!((report.cards, report.file.as_str()), (3, "Words.tsv"));
    let ids: Vec<u32> = cards_of(&storage, deck)
        .iter()
        .map(|c| c.state.id)
        .collect();
    assert_eq!(
        read(&storage, "Words.tsv"),
        alloc::format!(
            "#paperpaw:deck {deck}\nid\tfront\tback\texample\n\
             {}\thello\tworld\t\n\
             {}\t\"tab\there\"\t\"say \"\"hi\"\"\"\tex, with comma\n\
             {}\t\"two\nlines\"\tb\t\n",
            ids[0],
            ids[1],
            ids[2]
        )
    );
    // What was there is kept, under a name no deck is made of.
    assert_eq!(report.backup.as_deref(), Some("_Words.bak.tsv"));
    assert_eq!(read(&storage, "_Words.bak.tsv"), before);
    assert!(!storage.exists("Flashcards/Words.tsv.part"));
    let (_, found) = sync::files(&storage, FOLDER);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_saved_file_is_found_unchanged_by_the_next_update() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    save(&storage, deck);
    // Neither read again by an automatic update, nor different by hash.
    assert!(Job::new(&storage, FOLDER, Mode::Auto).is_empty());
    let again = run(&storage, true, true);
    assert_eq!(again[0].outcome, sync::Outcome::Same, "{again:?}");
    assert_eq!(cards_of(&storage, deck).len(), 2);
}

#[test]
fn save_edit_on_the_computer_update_keeps_what_was_learnt() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\ncasa\thouse\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    learn(&storage, deck, "gato");
    learn(&storage, deck, "casa");
    save(&storage, deck);
    // On the computer: a front changed (its id stays), a back changed, a
    // line added, a line deleted.
    let file = read(&storage, "W.tsv");
    let edited = file
        .replace("\tgato\t", "\tgatito\t")
        .replace("\tdog\t", "\tthe dog\t")
        .lines()
        .filter(|l| !l.contains("\tcasa\t"))
        .chain(["\tmesa\ttable\t"])
        .collect::<Vec<_>>()
        .join("\n");
    put(&storage, "W.tsv", &alloc::format!("{edited}\n"));
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].outcome, sync::Outcome::Merged, "{reports:?}");
    assert_eq!(
        (reports[0].added, reports[0].changed),
        (1, 2),
        "{reports:?}"
    );
    let cards = cards_of(&storage, deck);
    let card = |front: &str| cards.iter().find(|c| c.front == front).unwrap();
    assert!(
        !card("gatito").state.is_new(),
        "the front changed, the card stays"
    );
    assert_eq!(card("perro").back, "the dog");
    assert!(card("mesa").state.is_new());
    // The line is gone, the card stays (the switch is off).
    assert!(!card("casa").state.is_new());
    assert_eq!(cards.len(), 4);
}

#[test]
fn a_file_saved_from_another_deck_has_no_card_numbers_for_this_one() {
    // A file copied from another reader: its ids mean other cards.
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "cat\tKatze\ndog\tHund\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    learn(&storage, deck, "cat");
    put(
        &storage,
        "W.tsv",
        &alloc::format!(
            "#paperpaw:deck {}\nid\tfront\tback\n2\tcat\tKatze\n1\tdog\tder Hund\n",
            deck + 40
        ),
    );
    let reports = run(&storage, false, true);
    assert_eq!(
        (reports[0].added, reports[0].changed),
        (0, 1),
        "{reports:?}"
    );
    let cards = cards_of(&storage, deck);
    assert!(
        !cards
            .iter()
            .find(|c| c.front == "cat")
            .unwrap()
            .state
            .is_new()
    );
    assert_eq!(
        cards.iter().find(|c| c.front == "dog").unwrap().back,
        "der Hund"
    );
}

#[test]
fn an_edit_on_the_computer_is_read_before_a_save_and_never_written_over() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    save(&storage, deck);
    // Edited on the computer; no update since.
    let edited = read(&storage, "W.tsv").replace("\tcat\t", "\tthe cat\t") + "\tmesa\ttable\t\n";
    put(&storage, "W.tsv", &edited);
    let (reports, saved) = save_as_the_screen(&storage, deck);
    assert_eq!(reports[0].outcome, sync::Outcome::Merged, "{reports:?}");
    let saved = saved.expect("saved");
    assert_eq!(saved.outcome, Outcome::Saved);
    let now = read(&storage, "W.tsv");
    assert!(now.contains("the cat") && now.contains("mesa"), "{now}");
    assert_eq!(cards_of(&storage, deck).len(), 3);
    // The file that was replaced is the edited one.
    assert_eq!(read(&storage, "_W.bak.tsv"), edited);
}

#[test]
fn a_file_that_cannot_be_read_first_is_not_written_over() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    save(&storage, deck);
    // Changed to something that is no deck: more lines than a deck holds.
    let lines: String = (0..=cards::MAX_CARDS)
        .map(|i| alloc::format!("w{i}\tb\n"))
        .collect();
    put(&storage, "W.tsv", &lines);
    let (reports, saved) = save_as_the_screen(&storage, deck);
    assert_eq!(reports[0].outcome, sync::Outcome::TooLarge);
    assert!(saved.is_none());
    assert_eq!(read(&storage, "W.tsv"), lines);
    // The copy from the first save is as it was: nothing was saved over it.
    assert_eq!(read(&storage, "_W.bak.tsv"), "gato\tcat\n");
}

#[test]
fn a_deck_with_no_file_gets_a_file_named_for_it_that_no_other_has() {
    let storage = MemoryStorage::new();
    // A file of the name Words is there, no deck's (not read yet).
    put(&storage, "Words.tsv", "x\ty\n");
    let mut files = Vec::new();
    for name in ["Spa/n:ish*?", "_hidden", "...", "Words"] {
        let deck = cards::add_deck(&storage, name).unwrap();
        let (report, _) = save(&storage, deck);
        assert_eq!(report.outcome, Outcome::Saved, "{name}: {report:?}");
        files.push(report.file);
    }
    assert_eq!(
        files,
        ["Spa_n_ish__.tsv", "hidden.tsv", "Deck.tsv", "Words (2).tsv"]
    );
    // The file that was there is as it was.
    assert_eq!(read(&storage, "Words.tsv"), "x\ty\n");
    // The decks are linked to their files: an update has nothing new but the
    // file that was there before, whose deck is made then.
    let again = run(&storage, false, true);
    assert_eq!(again.len(), 1, "{again:?}");
    assert_eq!(again[0].file, "Words.tsv");
}

#[test]
fn a_csv_keeps_its_separator_and_gets_a_byte_order_mark() {
    let storage = MemoryStorage::new();
    put(
        &storage,
        "German.csv",
        "Front;Back
M\u{fc}ll;rubbish\nKatze;cat, the animal\n",
    );
    run(&storage, false, true);
    let deck = deck_id(&storage, "German");
    let (report, _) = save(&storage, deck);
    assert_eq!(report.outcome, Outcome::Saved);
    let text = read(&storage, "German.csv");
    assert!(text.starts_with("\u{feff}#paperpaw:deck "), "{text:?}");
    assert!(text.contains("id;front;back;example\n"), "{text}");
    assert!(
        text.contains(";M\u{fc}ll;rubbish;\n") && text.contains(";Katze;cat, the animal;\n"),
        "{text}"
    );
    // And it is read back the same (the byte order mark and the marker).
    let again = run(&storage, true, true);
    assert_eq!(again[0].outcome, sync::Outcome::Same, "{again:?}");
    // With the marker and ids, an edit of a front keeps the card.
    learn(&storage, deck, "Katze");
    put(
        &storage,
        "German.csv",
        &text.replace(";Katze;", ";Kater-Katze;"),
    );
    let reports = run(&storage, false, true);
    assert_eq!(
        (reports[0].added, reports[0].changed),
        (0, 1),
        "{reports:?}"
    );
    let cards = cards_of(&storage, deck);
    assert!(
        !cards
            .iter()
            .find(|c| c.front == "Kater-Katze")
            .unwrap()
            .state
            .is_new()
    );
}

#[test]
fn a_field_with_a_line_break_a_quote_or_the_separator_is_in_quotes() {
    let mut out = String::new();
    for (text, sep) in [
        ("plain", '\t'),
        ("a\tb", '\t'),
        ("a,b", '\t'),
        ("a,b", ','),
        ("say \"hi\"", '\t'),
        ("a\r\nb", '\t'),
        ("", '\t'),
    ] {
        out.clear();
        field(&mut out, text, sep);
        let quoted = out.starts_with('"');
        assert_eq!(
            quoted,
            text != "plain" && !text.is_empty() && !(text == "a,b" && sep == '\t'),
            "{text:?} {out:?}"
        );
    }
    out.clear();
    field(&mut out, "a\r\nb \"q\"", '\t');
    assert_eq!(out, "\"a\r\nb \"\"q\"\"\"");
}

#[test]
fn file_names_are_made_safe() {
    for (deck, file) in [
        ("Spanish", "Spanish.tsv"),
        ("a/b\\c:d", "a_b_c_d.tsv"),
        ("  .hidden. ", "hidden.tsv"),
        ("\u{7}x\u{0}", "x.tsv"),
        ("", "Deck.tsv"),
        ("__", "Deck.tsv"),
        ("Gr\u{fc}\u{df}e", "Gr\u{fc}\u{df}e.tsv"),
    ] {
        assert_eq!(file_name_of(deck), file, "{deck:?}");
    }
    assert_eq!(file_name_of(&"x".repeat(200)).len(), cards::MAX_NAME + 4);
    assert_eq!(backup_name("W.tsv"), "_W.bak.tsv");
    assert_eq!(backup_name("W"), "_W.bak");
}

#[test]
fn a_save_that_fails_leaves_the_old_file_and_the_ledger_as_they_were() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    let old = read(&storage, "W.tsv");
    let ledger_before = ledger_text(&storage);
    for (appends, rename) in [(0, false), (1, false), (usize::MAX, true)] {
        let wrapped = Wrapped::new(&storage);
        wrapped.appends.set(appends);
        wrapped.fail_rename.set(rename);
        let (report, _) = save(&wrapped, deck);
        assert_eq!(
            report.outcome,
            Outcome::Failed,
            "{appends} {rename}: {report:?}"
        );
        assert_eq!(read(&storage, "W.tsv"), old);
        assert_eq!(ledger_text(&storage), ledger_before);
        assert!(!storage.exists("Flashcards/W.tsv.part"), "no part is left");
        assert_eq!(cards_of(&storage, deck).len(), 2);
    }
    // A full card is told as that.
    let wrapped = Wrapped::new(&storage);
    wrapped.appends.set(1);
    wrapped.error.set(paperpaw_sdk::StorageError::Full);
    assert_eq!(save(&wrapped, deck).0.outcome, Outcome::Full);
    assert_eq!(read(&storage, "W.tsv"), old);
}

#[test]
fn a_deck_too_big_for_a_file_is_not_saved() {
    let storage = MemoryStorage::new();
    let deck = cards::add_deck(&storage, "Big").unwrap();
    put(&storage, "Big.tsv", "keep\tme\n");
    let long = "x".repeat(cards::MAX_TEXT);
    cards::add_each(&storage, deck, |push| {
        for i in 0..3000 {
            let t = Texts {
                front: alloc::format!("{i}{long}"),
                back: long.clone(),
                example: long.clone(),
            };
            if !push(t) {
                break;
            }
        }
        Ok(())
    })
    .unwrap();
    let (report, _) = save(&storage, deck);
    assert_eq!(report.outcome, Outcome::TooLarge, "{report:?}");
    assert_eq!(
        read(&storage, "Big (2).tsv".replace(" (2)", "").as_str()),
        "keep\tme\n"
    );
    assert!(!storage.exists("Flashcards/Big (2).tsv.part"));
    assert!(!storage.exists("Flashcards/Big (2).tsv"));
}

#[test]
fn a_deck_that_is_gone_is_told() {
    let storage = MemoryStorage::new();
    assert_eq!(save(&storage, 77).0.outcome, Outcome::NoDeck);
    assert!(sync::files(&storage, FOLDER).1.is_empty());
}

#[test]
fn a_ledger_that_cannot_be_read_stops_a_save_and_is_not_written_over() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "a\tb\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    let newer = "paperpaw-sources 9\nW.tsv\t1\t4\t0\tff\t0\n";
    storage
        .write(&sync::ledger_path(), newer.as_bytes())
        .unwrap();
    assert_eq!(save(&storage, deck).0.outcome, Outcome::Unreadable);
    assert_eq!(ledger_text(&storage), newer.as_bytes());
    assert_eq!(read(&storage, "W.tsv"), "a\tb\n");
}

#[test]
fn a_big_deck_is_saved_in_steps_with_a_small_heap_and_read_back_the_same() {
    let storage = MemoryStorage::new();
    let deck = cards::add_deck(&storage, "Big").unwrap();
    cards::add_each(&storage, deck, |push| {
        for i in 0..cards::MAX_CARDS {
            let t = Texts {
                front: alloc::format!("word {i}"),
                back: alloc::format!("meaning of the word {i}"),
                example: String::from("An example sentence for it."),
            };
            if !push(t) {
                break;
            }
        }
        Ok(())
    })
    .unwrap();
    let wrapped = Wrapped::new(&storage);
    let ((report, steps), biggest, peak) = heap::measure(|| save(&wrapped, deck));
    assert_eq!(
        (report.outcome, report.cards),
        (Outcome::Saved, cards::MAX_CARDS),
        "{report:?}"
    );
    assert!(steps > 20, "{steps} steps");
    // What the save itself holds, the storage's own buffers left out.
    assert!(biggest <= 16 * 1024, "one allocation of {biggest} bytes");
    assert!(peak <= 85 * 1024, "{peak} bytes at once");
    std::eprintln!(
        "save of {} cards: largest allocation {biggest} B, peak {peak} B, {steps} steps",
        cards::MAX_CARDS
    );
    // The update of it: the same, and unread.
    assert!(Job::new(&storage, FOLDER, Mode::Auto).is_empty());
    let again = run(&storage, true, true);
    assert_eq!(again[0].outcome, sync::Outcome::Same, "{again:?}");
}

#[test]
fn the_switch_removes_the_cards_a_file_lacks_and_off_keeps_them() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\ncasa\thouse\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    learn(&storage, deck, "perro");
    // Off to begin with: a line deleted, the card stays.
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\n");
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].removed, 0);
    assert_eq!(cards_of(&storage, deck).len(), 3);
    // On: the next update removes what the file lacks.
    assert!(sync::set_mirror(&storage, deck, true));
    assert!(sync::link_of(&ledger(&storage), deck).unwrap().mirror);
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\nmesa\ttable\n");
    let reports = run(&storage, false, true);
    assert_eq!(
        (reports[0].removed, reports[0].added),
        (1, 1),
        "{reports:?}"
    );
    let cards = cards_of(&storage, deck);
    assert_eq!(fronts(&cards), ["gato", "perro", "mesa"]);
    assert!(!cards[1].state.is_new(), "what was learnt stays");
    // The switch stays through the update, and a second update changes nothing.
    assert!(sync::link_of(&ledger(&storage), deck).unwrap().mirror);
    let again = run(&storage, true, true);
    assert_eq!(again[0].outcome, sync::Outcome::Same);
    // Off again: back to keeping.
    assert!(sync::set_mirror(&storage, deck, false));
    put(&storage, "W.tsv", "gato\tcat\n");
    run(&storage, false, true);
    assert_eq!(cards_of(&storage, deck).len(), 3);
}

#[test]
fn the_switch_never_empties_a_deck_for_a_file_with_no_card_in_it() {
    let storage = MemoryStorage::new();
    put(&storage, "W.tsv", "gato\tcat\nperro\tdog\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    assert!(sync::set_mirror(&storage, deck, true));
    put(&storage, "W.tsv", "front\tback\n");
    let reports = run(&storage, false, true);
    assert_eq!(
        (reports[0].removed, cards_of(&storage, deck).len()),
        (0, 2),
        "{reports:?}"
    );
}

#[test]
fn the_switch_is_for_a_linked_deck_only_and_old_ledger_lines_read_as_off() {
    let storage = MemoryStorage::new();
    let unlinked = cards::add_deck(&storage, "Loose").unwrap();
    assert!(!sync::set_mirror(&storage, unlinked, true));
    put(&storage, "W.tsv", "a\tb\n");
    run(&storage, false, true);
    let deck = deck_id(&storage, "W");
    // A line of a ledger as part 1 wrote it, five fields.
    let text = String::from_utf8(ledger_text(&storage)).unwrap();
    let old: String = text
        .lines()
        .map(|l| {
            l.rsplit_once('\t')
                .filter(|(_, m)| *m == "0")
                .map_or(l, |(a, _)| a)
                .to_string()
                + "\n"
        })
        .collect();
    storage.write(&sync::ledger_path(), old.as_bytes()).unwrap();
    assert!(!sync::link_of(&ledger(&storage), deck).unwrap().mirror);
    assert!(sync::set_mirror(&storage, deck, true));
    assert!(sync::link_of(&ledger(&storage), deck).unwrap().mirror);
    // A save keeps the switch.
    save(&storage, deck);
    assert!(sync::link_of(&ledger(&storage), deck).unwrap().mirror);
}

#[test]
fn the_switch_removes_nothing_where_the_update_did_not_finish() {
    let storage = MemoryStorage::new();
    let text: String = (0..5000)
        .map(|i| alloc::format!("same\tback {i}\n"))
        .collect();
    put(&storage, "Same.tsv", &text);
    run(&storage, false, true);
    let deck = deck_id(&storage, "Same");
    assert!(sync::set_mirror(&storage, deck, true));
    let text: String = (0..5000)
        .map(|i| alloc::format!("same\tback {}\n", i + 1))
        .collect();
    put(&storage, "Same.tsv", &text);
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].outcome, sync::Outcome::Crowded);
    assert_eq!(cards_of(&storage, deck).len(), 5000);
}

#[test]
fn an_update_of_an_ordinary_size_takes_a_small_heap() {
    // The merge's own heap, the storage's buffers left out.
    let storage = MemoryStorage::new();
    let text: String = (0..6000)
        .map(|i| alloc::format!("word{i}\tback {i}\n"))
        .collect();
    put(&storage, "Ord.tsv", &text);
    run(&storage, false, true);
    assert!(sync::set_mirror(&storage, deck_id(&storage, "Ord"), true));
    let text: String = (0..6000)
        .filter(|i| i % 50 != 0)
        .map(|i| alloc::format!("word{i}\tnew {i}\n"))
        .collect();
    put(&storage, "Ord.tsv", &text);
    let wrapped = Wrapped::new(&storage);
    let ((reports, _), biggest, peak) = heap::measure(|| {
        let mut job = Job::new(&wrapped, FOLDER, Mode::Manual);
        loop {
            match job.step(&wrapped) {
                sync::Step::Done => break (job.reports, 0),
                sync::Step::Ask(_) => job.answer(&wrapped, true),
                sync::Step::Working => {}
            }
        }
    });
    assert_eq!(
        (reports[0].changed, reports[0].removed),
        (5880, 120),
        "{reports:?}"
    );
    assert!(biggest <= 64 * 1024, "one allocation of {biggest} bytes");
    assert!(peak <= 85 * 1024, "{peak} bytes at once");
    std::eprintln!("update of 6000 cards: largest allocation {biggest} B, peak {peak} B");
}
