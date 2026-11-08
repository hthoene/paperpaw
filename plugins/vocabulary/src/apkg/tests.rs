//! Anki decks made here: an SQLite database with a `notes` table in a ZIP.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use paperpaw_sqlite::{SchemaEntry, Value, Writer, encode_record};
use paperpaw_zip::testing::ZipBuilder;

use super::*;
use crate::sync::tests::{FOLDER, cards_of, deck_id, fronts, run};
use crate::sync::{self, Outcome, Step};

const NOTES_SQL: &str = "CREATE TABLE notes (id integer primary key, guid text not null, \
    mid integer not null, mod integer not null, usn integer not null, tags text not null, \
    flds text not null, sfld integer not null, csum integer not null, flags integer not null, \
    data text not null)";

/// The database of Anki's with a `notes` table of `notes` (their fields
/// joined as Anki does), as bytes.
fn database(notes: &[String]) -> Vec<u8> {
    let storage = MemoryStorage::new();
    let mut writer = Writer::new(&storage, "collection.anki21");
    let rows = notes.iter().enumerate().map(|(i, flds)| {
        let record = encode_record(&[
            Value::Null,
            Value::Text(alloc::format!("guid{i}")),
            Value::Int(1),
            Value::Int(0),
            Value::Int(-1),
            Value::Text(String::new()),
            Value::Text(flds.clone()),
            Value::Text(flds.split('\x1f').next().unwrap_or("").to_string()),
            Value::Int(0),
            Value::Int(0),
            Value::Text(String::new()),
        ]);
        (i as i64 + 1, record)
    });
    let root = writer.table(rows).unwrap();
    let schema = [SchemaEntry {
        kind: "table".to_string(),
        name: "notes".to_string(),
        table: "notes".to_string(),
        root,
        sql: NOTES_SQL.to_string(),
    }];
    writer.finish(&schema, 0).unwrap();
    paperpaw_sdk::read_file(&storage, "collection.anki21", 1 << 24).unwrap()
}

/// The fields of a note as Anki joins them.
fn note(fields: &[&str]) -> String {
    fields.join("\x1f")
}

/// An export of Anki with `notes`, its database in `name`.
fn deck_with(name: &str, notes: &[String]) -> Vec<u8> {
    ZipBuilder::new()
        .deflated(name, database(notes))
        .stored("media", b"{}".to_vec())
        .build()
}

fn put(storage: &MemoryStorage, name: &str, bytes: &[u8]) {
    storage
        .write(&alloc::format!("{FOLDER}/{name}"), bytes)
        .unwrap();
}

fn read(storage: &MemoryStorage, path: &str) -> String {
    String::from_utf8(paperpaw_sdk::read_file(storage, path, 1 << 24).unwrap()).unwrap()
}

#[test]
fn a_deck_is_written_as_plain_text_with_its_first_three_fields() {
    let storage = MemoryStorage::new();
    let notes = [
        note(&[
            "<b>haus</b>",
            "the house<br>big&amp;old [sound:a.mp3]",
            "Das <i>Haus</i>.",
        ]),
        note(&["tab\there", "say \"hi\""]),
        note(&["", "no front"]),
        note(&["gato", "cat"]),
    ];
    put(&storage, "de.apkg", &deck_with("collection.anki21", &notes));
    let made = convert(&storage, &alloc::format!("{FOLDER}/de.apkg")).unwrap();
    assert_eq!(
        made,
        Notes {
            count: 3,
            skipped: 0
        }
    );
    assert_eq!(
        read(&storage, &text_path()),
        "#separator:tab\n\
         haus\tthe house big&old\tDas Haus.\n\
         tab here\t\"say \"\"hi\"\"\"\t\n\
         gato\tcat\t\n"
    );
    // The database is only there while it is read.
    assert!(!storage.exists(&database_path()));
}

#[test]
fn an_older_ankis_database_name_is_read_too() {
    let storage = MemoryStorage::new();
    put(
        &storage,
        "x.apkg",
        &deck_with("collection.anki2", &[note(&["a", "b"])]),
    );
    assert_eq!(
        convert(&storage, &alloc::format!("{FOLDER}/x.apkg")),
        Ok(Notes {
            count: 1,
            skipped: 0
        })
    );
}

#[test]
fn a_deck_of_the_newest_anki_is_told_and_not_read() {
    let storage = MemoryStorage::new();
    // What Anki leaves for older versions: a deck with one note that says
    // to update, beside the real, compressed one.
    let zip = ZipBuilder::new()
        .deflated(
            "collection.anki2",
            database(&[note(&["Please update", "to the latest"])]),
        )
        .stored("collection.anki21b", b"zstd".to_vec())
        .build();
    put(&storage, "new.apkg", &zip);
    assert_eq!(
        convert(&storage, &alloc::format!("{FOLDER}/new.apkg")),
        Err(Fault::NewerAnki)
    );
}

#[test]
fn what_is_no_deck_or_has_no_notes_is_told() {
    let storage = MemoryStorage::new();
    put(&storage, "text.apkg", b"not a zip at all");
    put(&storage, "empty.apkg", &deck_with("collection.anki21", &[]));
    put(
        &storage,
        "blank.apkg",
        &deck_with("collection.anki21", &[note(&["", "x"])]),
    );
    put(
        &storage,
        "nothing.apkg",
        &ZipBuilder::new()
            .stored("readme.txt", b"hi".to_vec())
            .build(),
    );
    for name in ["text", "empty", "blank", "nothing"] {
        let path = alloc::format!("{FOLDER}/{name}.apkg");
        assert_eq!(convert(&storage, &path), Err(Fault::NoNotes), "{name}");
    }
}

#[test]
fn too_many_files_or_notes_are_told_before_memory_goes() {
    let storage = MemoryStorage::new();
    let mut zip = ZipBuilder::new().deflated("collection.anki21", database(&[note(&["a", "b"])]));
    for i in 0..usize::from(MAX_FILES) {
        zip = zip.stored(&alloc::format!("{i}"), b"x".to_vec());
    }
    put(&storage, "media.apkg", &zip.build());
    assert_eq!(
        convert(&storage, &alloc::format!("{FOLDER}/media.apkg")),
        Err(Fault::TooLarge)
    );
    let notes: Vec<String> = (0..=MAX_NOTES)
        .map(|i| note(&[&alloc::format!("w{i}"), "x"]))
        .collect();
    put(
        &storage,
        "big.apkg",
        &deck_with("collection.anki21", &notes),
    );
    assert_eq!(
        convert(&storage, &alloc::format!("{FOLDER}/big.apkg")),
        Err(Fault::TooLarge)
    );
}

#[test]
fn a_deck_on_the_card_makes_a_deck_like_a_text_file_and_is_not_read_again() {
    let storage = MemoryStorage::new();
    let notes = [
        note(&["casa", "house"]),
        note(&["gato", "cat", "El gato duerme."]),
    ];
    put(
        &storage,
        "Spanish.apkg",
        &deck_with("collection.anki21", &notes),
    );
    let reports = run(&storage, false, true);
    assert_eq!(reports.len(), 1);
    assert_eq!((reports[0].added, reports[0].outcome), (2, Outcome::Merged));
    let cards = cards_of(&storage, deck_id(&storage, "Spanish"));
    assert_eq!(fronts(&cards), ["casa", "gato"]);
    assert_eq!(cards[1].example, "El gato duerme.");
    // The scratch files are gone, and the deck is not read again.
    assert!(!storage.exists(&text_path()) && !storage.exists(&database_path()));
    assert!(run(&storage, false, true).is_empty());
    // The same deck with another note: that one is added, what is there stays.
    let more = [
        note(&["casa", "house"]),
        note(&["gato", "cat", "El gato duerme."]),
        note(&["perro", "dog"]),
    ];
    put(
        &storage,
        "Spanish.apkg",
        &deck_with("collection.anki21", &more),
    );
    let reports = run(&storage, false, true);
    assert_eq!((reports[0].added, reports[0].same), (1, 2), "{reports:?}");
    assert_eq!(
        fronts(&cards_of(&storage, deck_id(&storage, "Spanish"))),
        ["casa", "gato", "perro"]
    );
}

#[test]
fn a_deck_that_cannot_be_read_tells_why_and_is_not_tried_again() {
    let storage = MemoryStorage::new();
    put(&storage, "Broken.apkg", b"PK nothing");
    let reports = run(&storage, false, true);
    assert_eq!(reports[0].outcome, Outcome::NoNotes);
    // The opening of the flash cards does not try it again; asking does.
    assert!(
        sync::Job::new(&storage, FOLDER, sync::Mode::Auto)
            .unwrap()
            .is_empty()
    );
    assert!(
        !sync::Job::new(&storage, FOLDER, sync::Mode::Manual)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_job_steps_through_an_anki_deck() {
    let storage = MemoryStorage::new();
    put(
        &storage,
        "A.apkg",
        &deck_with("collection.anki21", &[note(&["a", "b"])]),
    );
    let mut job = sync::Job::new(&storage, FOLDER, sync::Mode::Manual).unwrap();
    let mut steps = 0;
    loop {
        steps += 1;
        match job.step(&storage) {
            Step::Done => break,
            Step::Ask => job.answer(&storage, true),
            Step::Working => {}
        }
        assert!(steps < 100);
    }
    assert_eq!(job.reports[0].added, 1);
}

#[test]
fn deck_names_and_the_listing_know_apkg() {
    assert_eq!(sync::deck_name("Spanish.APKG").unwrap(), "Spanish");
    assert!(is_apkg("x.apkg") && is_apkg("X.ApKg") && !is_apkg("apkg") && !is_apkg("x.txt"));
    assert!(!is_apkg("é.apk"));
}

#[test]
fn a_deck_of_an_anki_file_is_not_saved_over_it() {
    use crate::export::{Outcome as Saved, Saver, Step as Saving};
    let storage = MemoryStorage::new();
    let zip = deck_with("collection.anki21", &[note(&["casa", "house"])]);
    put(&storage, "Spanish.apkg", &zip);
    run(&storage, false, true);
    let deck = deck_id(&storage, "Spanish");
    let mut saver = Saver::new(FOLDER, deck).unwrap();
    let report = loop {
        if let Saving::Done(report) = saver.step(&storage) {
            break report;
        }
    };
    assert_eq!(report.outcome, Saved::AnkiFile);
    assert_eq!(report.file, "Spanish.apkg");
    // Nothing was written: the file is as it was, and no part is left.
    let path = alloc::format!("{FOLDER}/Spanish.apkg");
    assert_eq!(
        paperpaw_sdk::read_file(&storage, &path, 1 << 20).unwrap(),
        zip
    );
    assert!(!storage.exists(&alloc::format!("{path}.part")));
}

#[test]
fn a_note_over_the_cap_is_left_out_before_anything_of_it_is_copied() {
    let storage = MemoryStorage::new();
    let picture = alloc::format!(
        "<img src=\"data:image/png;base64,{}\">",
        "QUJD".repeat(1500)
    );
    let notes = [
        note(&["one", "first"]),
        // 200 KiB: a page of text, or a few pictures.
        note(&["huge", &"x".repeat(200 * 1024)]),
        // Under the cap, with a picture in it: the picture is not a card's.
        note(&["two", &alloc::format!("see {picture} here")]),
        note(&["three", "last"]),
    ];
    put(
        &storage,
        "Big.apkg",
        &deck_with("collection.anki21", &notes),
    );
    let made = convert(&storage, &alloc::format!("{FOLDER}/Big.apkg")).unwrap();
    assert_eq!(
        made,
        Notes {
            count: 3,
            skipped: 1
        }
    );
    assert_eq!(
        read(&storage, &text_path()),
        "#separator:tab\none\tfirst\t\ntwo\tsee here\t\nthree\tlast\t\n"
    );
    // The job counts the note left out, and says so, as it does lines.
    let reports = run(&storage, false, true);
    assert_eq!(
        (reports[0].added, reports[0].skipped),
        (3, 1),
        "{reports:?}"
    );
}

#[test]
fn fields_are_cut_and_open_tags_and_data_addresses_cost_no_more() {
    let long = "é".repeat(5000);
    let text = note_text(&long);
    assert!(text.len() <= table::MAX_FIELD && text.chars().all(|c| c == 'é'));
    // A tag that never closes, then more.
    assert_eq!(note_text("a <b never closed and more"), "a");
    // A data address goes, whatever it is in; the word `data:` alone does not.
    assert_eq!(note_text("x data:image/png;base64,AAAA y"), "x y");
    assert_eq!(
        note_text("metadata: and data: here"),
        "metadata: and data: here"
    );
    assert_eq!(note_text("[sound:a.mp3]word[sound:b"), "word");
    // Any cut falls on a character.
    for most in 0..8 {
        assert!("aé€😀".is_char_boundary(cut("aé€😀", most).len()));
    }
}

#[test]
fn a_zip_comment_does_not_hide_the_number_of_files() {
    let storage = MemoryStorage::new();
    let mut zip = ZipBuilder::new().deflated("collection.anki21", database(&[note(&["a", "b"])]));
    for i in 0..usize::from(MAX_FILES) {
        zip = zip.stored(&alloc::format!("{i}"), b"x".to_vec());
    }
    let mut bytes = zip.build();
    // A comment of 30,000 bytes after the end record.
    let at = bytes.len() - 2;
    bytes[at..].copy_from_slice(&30_000u16.to_le_bytes());
    bytes.extend(core::iter::repeat_n(b'c', 30_000));
    put(&storage, "comment.apkg", &bytes);
    let path = alloc::format!("{FOLDER}/comment.apkg");
    assert_eq!(file_count(&storage, &path), Ok(Some(MAX_FILES + 1)));
    assert_eq!(convert(&storage, &path), Err(Fault::TooLarge));
    // Without a comment, and where there is no end record at all.
    let plain = ZipBuilder::new().stored("a", b"x".to_vec()).build();
    put(&storage, "plain.apkg", &plain);
    assert_eq!(
        file_count(&storage, &alloc::format!("{FOLDER}/plain.apkg")),
        Ok(Some(1))
    );
    put(&storage, "none.apkg", &[7u8; 100_000]);
    assert_eq!(
        file_count(&storage, &alloc::format!("{FOLDER}/none.apkg")),
        Ok(None)
    );
    put(&storage, "tiny.apkg", b"PK");
    assert_eq!(
        file_count(&storage, &alloc::format!("{FOLDER}/tiny.apkg")),
        Ok(None)
    );
}

#[test]
fn a_database_over_the_cap_is_told_before_it_is_inflated() {
    let storage = MemoryStorage::new();
    let filler = "w".repeat(7000);
    let notes: Vec<String> = (0..700)
        .map(|i| note(&[&alloc::format!("n{i}"), &filler]))
        .collect();
    let zip = deck_with("collection.anki21", &notes);
    put(&storage, "Large.apkg", &zip);
    assert_eq!(
        convert(&storage, &alloc::format!("{FOLDER}/Large.apkg")),
        Err(Fault::TooLarge)
    );
    assert!(!storage.exists(&database_path()) && !storage.exists(&text_path()));
}

#[test]
fn what_a_deck_left_behind_is_removed_when_a_job_starts() {
    let storage = MemoryStorage::new();
    // The power went while a deck was being read.
    storage.write(&database_path(), b"half a database").unwrap();
    storage.write(&text_path(), b"half a text").unwrap();
    let _ = sync::Job::new(&storage, FOLDER, sync::Mode::Auto).unwrap();
    assert!(!storage.exists(&database_path()) && !storage.exists(&text_path()));
}

#[test]
fn a_note_of_200_kib_is_passed_over_without_a_big_allocation() {
    let storage = MemoryStorage::new();
    let notes = [
        note(&["one", "first"]),
        note(&["huge", &"x".repeat(200 * 1024)]),
        // Under the payload cap: read, and cut to what a card holds.
        note(&["two", &"y".repeat(6 * 1024)]),
        note(&["three", "last"]),
    ];
    put(
        &storage,
        "Big.apkg",
        &deck_with("collection.anki21", &notes),
    );
    inflate(&storage, &alloc::format!("{FOLDER}/Big.apkg")).unwrap();
    // The reading of the notes, as the plugin's own heap sees it: the
    // storage's files are not counted, and nothing near the note's size
    // is asked for (the X4 holds 256 KiB in all).
    let (made, biggest, peak) = crate::sync::tests::heap::measure(|| notes_of(&storage));
    assert_eq!(
        made.unwrap(),
        Notes {
            count: 3,
            skipped: 1
        }
    );
    assert!(biggest < 64 * 1024, "biggest {biggest}");
    assert!(peak < 96 * 1024, "peak {peak}");
    assert_eq!(fronts_of_text(&storage), ["one", "two", "three"]);
}

fn notes_of(storage: &MemoryStorage) -> Result<Notes, Fault> {
    notes(storage)
}

fn fronts_of_text(storage: &MemoryStorage) -> Vec<String> {
    read(storage, &text_path())
        .lines()
        .skip(1)
        .map(|l| l.split('\t').next().unwrap_or("").to_string())
        .collect()
}
