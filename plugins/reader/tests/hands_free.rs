//! Reading hands-free with the real kernel: pages turned by themselves at
//! the interval set (a page turned by hand starts the count anew, the
//! menu stops it, the end of the book too), and reading aloud sentence
//! by sentence (what is said, the next sentence handed over at once, any
//! button pausing it and reading on from there, pages turned along, a
//! device that cannot speak, speech that fails).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_plugin_reader::actions as reader;
use paperpaw_sdk::builtin::actions;

const BOOK: &str = "books/numbered.txt";

fn auto_turning(paragraphs: usize) -> Reading {
    let text = numbered_book(paragraphs, 12);
    let mut reading = Reading::new(
        BUTTONS,
        Shared::default(),
        BOOK,
        text.as_bytes(),
        "[reader]\nauto-turn-seconds = 10\n",
    );
    reading.open();
    reading
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn pages_turn_by_themselves_at_the_interval_set() {
    let mut reading = auto_turning(100);
    reading.press(reader::AUTO_TURN);
    let started = reading.now;
    reading.pass_until(started + 9_900);
    assert_eq!(reading.turned(), 0);
    reading.pass_until(started + 10_100);
    assert_eq!(reading.turned(), 1);
    reading.pass_until(started + 19_900);
    assert_eq!(reading.turned(), 1);
    reading.pass_until(started + 20_100);
    assert_eq!(reading.turned(), 2);
    reading.pass_until(started + 60_100);
    assert_eq!(reading.turned(), 6);
    // The pages turned are the next ones, as by hand.
    let starts: Vec<_> = reading
        .heard
        .borrow()
        .events
        .iter()
        .filter(|(n, _)| *n == "turned")
        .map(|(_, p)| reading_of(p).at())
        .collect();
    assert!(starts.windows(2).all(|p| p[0] < p[1]), "{starts:?}");
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_page_turned_by_hand_starts_the_count_anew() {
    let mut reading = auto_turning(100);
    reading.press(reader::AUTO_TURN);
    let started = reading.now;
    reading.pass_until(started + 6_000);
    reading.press(actions::NEXT_PAGE);
    let turned = reading.now;
    assert_eq!(reading.turned(), 1);
    // Not ten seconds after the start, but after the page turned.
    reading.pass_until(turned + 9_800);
    assert_eq!(reading.turned(), 1);
    reading.pass_until(turned + 10_200);
    assert_eq!(reading.turned(), 2);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn the_menu_stops_turning_and_nothing_turns_after() {
    let mut reading = auto_turning(100);
    reading.press(reader::AUTO_TURN);
    let started = reading.now;
    reading.pass_until(started + 10_100);
    assert_eq!(reading.turned(), 1);
    // The menu button stops it (and opens no menu).
    reading.press(actions::MENU);
    assert_eq!(reading.screen(), "reader.screen");
    reading.pass_until(started + 120_000);
    assert_eq!(reading.turned(), 1);
    // Started again with its action, stopped with it too.
    reading.press(reader::AUTO_TURN);
    let again = reading.now;
    reading.pass_until(again + 10_100);
    assert_eq!(reading.turned(), 2);
    reading.press(reader::AUTO_TURN);
    reading.pass_until(again + 60_000);
    assert_eq!(reading.turned(), 2);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn turning_stops_at_the_end_of_the_book() {
    let mut reading = auto_turning(8);
    reading.press(reader::AUTO_TURN);
    let started = reading.now;
    reading.pass_until(started + 300_000);
    let turned = reading.turned();
    assert!(turned >= 1);
    assert!(reading.events().contains(&"finished"));
    // Stopped: a page turned back by hand stays, nothing turns on.
    reading.press(actions::PREV_PAGE);
    let back = reading.turned();
    reading.pass_until(reading.now + 60_000);
    assert_eq!(reading.turned(), back);
}

/// A book of short sentences: `Sentence n has words.`
fn sentences(count: usize) -> String {
    let mut text = String::new();
    for n in 0..count {
        text.push_str(&format!("Sentence {n} has a few words in it. "));
        if n % 5 == 4 {
            text.push_str("\n\n");
        }
    }
    text
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn reading_aloud_says_the_sentences_one_after_another() {
    let text = sentences(200);
    let mut reading = Reading::speaking(BUTTONS, Shared::default(), BOOK, text.as_bytes());
    reading.open();
    reading.speech();
    reading.press(reader::READ_ALOUD);
    // The first sentence, and the next one handed over to follow it.
    let (said, _) = reading.speech();
    let texts: Vec<&str> = said.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Sentence 0 has a few words in it.",
            "Sentence 1 has a few words in it."
        ]
    );
    // Each sentence done: the one after the next is handed over.
    reading.spoken(said[0].0);
    let (said, _) = reading.speech();
    let texts: Vec<&str> = said.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(texts, ["Sentence 2 has a few words in it."]);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn any_button_pauses_reading_aloud_and_it_goes_on_from_there() {
    let text = sentences(200);
    let mut reading = Reading::speaking(BUTTONS, Shared::default(), BOOK, text.as_bytes());
    reading.open();
    reading.press(reader::READ_ALOUD);
    // Sentences 0 and 1 handed over; both said.
    let (first, _) = reading.speech();
    reading.spoken(first[0].0);
    reading.spoken(first[1].0);
    reading.speech();
    // Now "Sentence 2" is being said: the page button only pauses.
    reading.press(actions::NEXT_PAGE);
    let (said, stopped) = reading.speech();
    assert!(stopped);
    assert!(said.is_empty());
    assert_eq!(reading.turned(), 0);
    // Again: from the sentence paused in.
    reading.press(reader::READ_ALOUD);
    let (said, _) = reading.speech();
    assert_eq!(
        said.first().map(|(_, t)| t.as_str()),
        Some("Sentence 2 has a few words in it.")
    );
    // Pausing does nothing else: the page turns after.
    reading.press(actions::BACK);
    reading.press(actions::NEXT_PAGE);
    assert_eq!(reading.turned(), 1);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn reading_aloud_turns_the_pages() {
    let text = sentences(200);
    let mut reading = Reading::speaking(BUTTONS, Shared::default(), BOOK, text.as_bytes());
    reading.open();
    reading.press(reader::READ_ALOUD);
    let mut said_all = Vec::new();
    let mut pending: Vec<(u32, String)> = reading.speech().0;
    for _ in 0..200 {
        if reading.turned() >= 2 || pending.is_empty() {
            break;
        }
        let (id, text) = pending.remove(0);
        said_all.push(text);
        reading.spoken(id);
        pending.extend(reading.speech().0);
    }
    assert!(reading.turned() >= 2, "{said_all:?}");
    // In order, none left out nor said twice.
    for (n, text) in said_all.iter().enumerate() {
        assert_eq!(*text, format!("Sentence {n} has a few words in it."));
    }
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn reading_aloud_stops_at_the_end_of_the_book() {
    let text = sentences(3);
    let mut reading = Reading::speaking(BUTTONS, Shared::default(), BOOK, text.as_bytes());
    reading.open();
    reading.press(reader::READ_ALOUD);
    let mut pending: Vec<(u32, String)> = reading.speech().0;
    let mut said = 0;
    while let Some((id, _)) = pending.first().cloned() {
        pending.remove(0);
        said += 1;
        reading.spoken(id);
        pending.extend(reading.speech().0);
        assert!(said < 10);
    }
    assert_eq!(said, 3);
    // Done: buttons turn pages again (here: none to turn to).
    reading.press(actions::NEXT_PAGE);
    let (_, stopped) = reading.speech();
    assert!(!stopped);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn speech_that_fails_ends_reading_aloud() {
    let text = sentences(50);
    let mut reading = Reading::speaking(BUTTONS, Shared::default(), BOOK, text.as_bytes());
    reading.open();
    reading.press(reader::READ_ALOUD);
    let (said, _) = reading.speech();
    reading.now += 100;
    let effects = reading
        .kernel
        .speech_done(paperpaw_sdk::Instant(reading.now), said[0].0, false);
    reading.settle(effects);
    reading.speech();
    // Not paused but ended: the page button turns the page.
    reading.press(actions::NEXT_PAGE);
    assert_eq!(reading.turned(), 1);
    // Started again: from the start of the page shown.
    reading.press(reader::READ_ALOUD);
    let (said, _) = reading.speech();
    assert!(!said.is_empty());
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_device_that_cannot_speak_says_nothing() {
    let text = sentences(50);
    let mut reading = Reading::new(BUTTONS, Shared::default(), BOOK, text.as_bytes(), "");
    reading.open();
    reading.press(reader::READ_ALOUD);
    let (said, _) = reading.speech();
    assert!(said.is_empty());
    // Nothing waits to be paused: the page turns.
    reading.press(actions::NEXT_PAGE);
    assert_eq!(reading.turned(), 1);
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn reading_aloud_from_the_selected_word() {
    let text = sentences(50);
    let mut reading = Reading::speaking(BUTTONS, Shared::default(), BOOK, text.as_bytes());
    reading.open();
    reading.press(reader::SELECT);
    // The first word of the third sentence: 8 words a sentence.
    for _ in 0..16 {
        reading.press(actions::RIGHT);
    }
    reading.press(actions::CONFIRM);
    reading.press(reader::READ_FROM);
    let (said, _) = reading.speech();
    assert_eq!(
        said.first().map(|(_, t)| t.as_str()),
        Some("Sentence 2 has a few words in it.")
    );
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn turned_while_reading_aloud_the_speech_pauses_and_the_book_stays() {
    let text = sentences(200);
    let mut reading = Reading::speaking(BUTTONS, Shared::default(), BOOK, text.as_bytes());
    reading.open();
    reading.speech();
    reading.press(reader::READ_ALOUD);
    let (said, _) = reading.speech();
    assert!(!said.is_empty(), "it speaks");
    reading.resize(800, 480);
    assert_eq!(reading.screen(), "reader.screen");
    let (_, stopped) = reading.speech();
    assert!(stopped, "the speech was stopped");
    assert!(
        !reading.events().contains(&"closed"),
        "{:?}",
        reading.events()
    );
    // Going on reads aloud again, from the page shown.
    reading.press(reader::READ_ALOUD);
    let (said, _) = reading.speech();
    assert!(!said.is_empty(), "it speaks on");
}
