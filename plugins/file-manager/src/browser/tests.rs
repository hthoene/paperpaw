#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::string::{String, ToString};
use std::vec::Vec;

use paperpaw_sdk::MemoryStorage;

use super::*;
use crate::card::CardStorage;
use crate::card::tests::Fake;

fn card() -> MemoryStorage {
    let mut storage = MemoryStorage::new()
        .with("Books/a.epub", "a")
        .with("Books/b.epub", "bb")
        .with("Books/sub/c.txt", "c")
        .with("Notes/n.txt", "n")
        .with("top.txt", "t")
        .with(".paperpaw/plugins/stats/books.ini", "keep");
    for n in 0..8 {
        storage = storage.with(&std::format!("Many/f{n}.txt"), "x");
    }
    storage
}

fn names(browser: &Browser) -> Vec<String> {
    browser.page().iter().map(|e| e.name.clone()).collect()
}

fn at(storage: &MemoryStorage, dir: &str, rows: usize) -> Browser {
    let mut browser = Browser::new(dir.to_string(), false, rows);
    browser.reload(storage, None);
    browser
}

#[test]
fn the_top_lists_folders_first_and_hides_the_dot_folders() {
    let storage = card();
    let browser = at(&storage, "", 10);
    assert_eq!(names(&browser), ["Books", "Many", "Notes", "top.txt"]);
    assert_eq!(browser.len(), 4);
    let mut shown = at(&storage, "", 10);
    assert!(shown.set_hidden(true));
    shown.reload(&storage, None);
    assert_eq!(shown.len(), 5, ".paperpaw shows when asked");
}

#[test]
fn only_a_page_is_held_and_the_focus_turns_the_pages() {
    let storage = card();
    let mut browser = at(&storage, "Many", 3);
    assert_eq!((browser.len(), names(&browser).len()), (8, 3));
    for _ in 0..4 {
        browser.step(&storage, 1);
    }
    // The fifth entry is on the second page.
    assert_eq!((browser.cursor(), browser.first()), (4, 3));
    assert_eq!(names(&browser), ["f3.txt", "f4.txt", "f5.txt"]);
    assert!(browser.turn(&storage, true));
    assert_eq!((browser.cursor(), browser.first()), (7, 6));
    assert_eq!(names(&browser), ["f6.txt", "f7.txt"]);
    assert!(browser.turn(&storage, false));
    assert!(browser.turn(&storage, false));
    assert_eq!((browser.cursor(), browser.first()), (1, 0));
    assert!(browser.turn(&storage, false));
    assert_eq!((browser.cursor(), browser.first()), (0, 0));
    assert!(!browser.turn(&storage, false));
}

#[test]
fn the_focus_goes_round_over_the_list_and_the_bar() {
    let storage = card();
    let mut browser = at(&storage, "Notes", 5);
    assert_eq!(browser.len(), 1);
    browser.step(&storage, 1);
    assert_eq!(browser.bar(), Some(0));
    browser.step(&storage, 1);
    assert_eq!((browser.bar(), browser.cursor()), (None, 0));
    browser.step(&storage, -1);
    assert_eq!(browser.bar(), Some(0), "back from the first to the bar");
    browser.step(&storage, -1);
    assert_eq!((browser.bar(), browser.cursor()), (None, 0));
}

#[test]
fn a_folder_opens_and_back_leaves_it_chosen() {
    let storage = card();
    let mut browser = at(&storage, "", 10);
    browser.enter(&storage, 2);
    assert_eq!(browser.dir, "Notes");
    assert_eq!(names(&browser), ["n.txt"]);
    assert!(browser.up(&storage));
    assert_eq!(browser.dir, "");
    assert_eq!(browser.entry(browser.cursor()).unwrap().name, "Notes");
    assert!(!browser.up(&storage), "nothing above the top");
    // A file does not open.
    browser.focus_entry(3);
    browser.enter(&storage, 3);
    assert_eq!(browser.dir, "");
}

#[test]
fn picked_and_pasted_a_file_is_copied_under_a_free_name() {
    let storage = card();
    let mut browser = at(&storage, "Books", 10);
    browser.pick(&storage, "Books/a.epub", true);
    assert!(matches!(&browser.note, Some(Note::Picked { copy: true, name }) if name == "a.epub"));
    assert_eq!(browser.buttons(), [Button::Paste, Button::Cancel]);
    browser.paste(&storage);
    assert_eq!(storage.get("Books/a (2).epub").unwrap(), b"a");
    assert_eq!(storage.get("Books/a.epub").unwrap(), b"a");
    assert!(browser.picked.is_none() && browser.note.is_none());
    assert_eq!(browser.entry(browser.cursor()).unwrap().name, "a (2).epub");
}

#[test]
fn picked_and_pasted_a_file_moves_and_the_library_is_not_told() {
    let storage = card();
    let mut browser = at(&storage, "Notes", 10);
    browser.pick(&storage, "Books/a.epub", false);
    browser.paste(&storage);
    assert_eq!(storage.get("Notes/a.epub").unwrap(), b"a");
    assert!(storage.get("Books/a.epub").is_none());
    assert_eq!(browser.note, Some(Note::NotTold));
    // Moved where it is: nothing to do, and nothing to say.
    browser.pick(&storage, "Notes/a.epub", false);
    browser.paste(&storage);
    assert_eq!(storage.get("Notes/a.epub").unwrap(), b"a");
    assert_eq!(browser.note, None);
}

#[test]
fn a_folder_is_not_moved_into_itself_and_stays_whole() {
    let storage = card();
    let mut browser = at(&storage, "Books/sub", 10);
    browser.pick(&storage, "Books", false);
    browser.paste(&storage);
    assert_eq!(browser.note, Some(Note::Failed(Error::Invalid)));
    assert_eq!(storage.get("Books/a.epub").unwrap(), b"a");
    assert_eq!(storage.get("Books/sub/c.txt").unwrap(), b"c");
}

#[test]
fn papers_own_folder_is_never_moved_renamed_or_deleted() {
    let storage = card();
    let mut browser = at(&storage, "", 10);
    browser.pick(&storage, ".paperpaw/plugins", false);
    assert_eq!(browser.note, Some(Note::OwnData));
    assert!(browser.picked.is_none());
    browser.rename(&storage, ".paperpaw/plugins/stats/books.ini", "x.ini");
    assert_eq!(browser.note, Some(Note::OwnData));
    browser.delete(&storage, ".paperpaw");
    assert_eq!(browser.note, Some(Note::OwnData));
    browser.delete(&storage, ".paperpaw/plugins/stats/books.ini");
    assert_eq!(
        storage.get(".paperpaw/plugins/stats/books.ini").unwrap(),
        b"keep"
    );
    // Case does not get round it on the card.
    assert!(is_own_data(&storage, ".paperpaw/x"));
}

#[test]
fn deleting_a_folder_takes_what_is_in_it_and_the_focus_stays() {
    let storage = card();
    let mut browser = at(&storage, "", 10);
    browser.focus_entry(0);
    browser.delete(&storage, "Books");
    assert!(storage.get("Books/a.epub").is_none() && storage.get("Books/sub/c.txt").is_none());
    assert_eq!(names(&browser), ["Many", "Notes", "top.txt"]);
    assert_eq!(browser.cursor(), 0);
    assert_eq!(browser.note, None);
    // What was picked and went with it is not offered any more.
    browser.pick(&storage, "Notes/n.txt", false);
    browser.delete(&storage, "Notes");
    assert!(browser.picked.is_none());
}

#[test]
fn renaming_keeps_the_data_and_a_taken_name_gets_a_free_one() {
    let storage = card();
    let mut browser = at(&storage, "Books", 10);
    browser.rename(&storage, "Books/a.epub", "b.epub");
    assert_eq!(storage.get("Books/b.epub").unwrap(), b"bb", "not replaced");
    assert_eq!(storage.get("Books/b (2).epub").unwrap(), b"a");
    assert!(storage.get("Books/a.epub").is_none());
    assert_eq!(browser.note, Some(Note::NotTold));
    assert_eq!(browser.entry(browser.cursor()).unwrap().name, "b (2).epub");
    for bad in ["", "  ", "a/b", "x:y", "dot.", "q?"] {
        browser.rename(&storage, "Books/b.epub", bad);
        assert_eq!(browser.note, Some(Note::BadName), "{bad:?}");
    }
    assert_eq!(storage.get("Books/b.epub").unwrap(), b"bb");
    // The same name is no change.
    browser.note = None;
    browser.rename(&storage, "Books/b.epub", "b.epub");
    assert_eq!(browser.note, None);
}

#[test]
fn a_new_folder_is_made_beside_what_has_its_name_and_focused() {
    // The card keeps empty folders (the memory storage does not).
    let card = CardStorage(Fake::with(&[("a.txt", "1")], &["Books"]));
    let mut browser = Browser::new(String::new(), false, 10);
    browser.reload(&card, None);
    browser.new_folder(&card, "Books");
    assert!(is_dir(&card, "Books (2)"));
    assert_eq!(browser.entry(browser.cursor()).unwrap().name, "Books (2)");
    browser.new_folder(&card, "a|b");
    assert_eq!(browser.note, Some(Note::BadName));
    browser.note = None;
    browser.new_folder(&card, "   ");
    assert_eq!(browser.note, None, "an empty name asks for nothing");
}

#[test]
fn a_folder_that_cannot_be_read_shows_empty_and_says_why() {
    let storage = card();
    let browser = at(&storage, "Gone", 10);
    assert_eq!(browser.len(), 0);
    assert_eq!(browser.note, Some(Note::Failed(Error::NotFound)));
}

#[test]
fn names_that_work_on_the_card() {
    assert_eq!(valid_name("  Books "), Some("Books"));
    assert_eq!(valid_name("a.b"), Some("a.b"));
    for bad in ["", ".", "a\\b", "a\nb", "end.", "<x>"] {
        assert_eq!(valid_name(bad), None, "{bad:?}");
    }
}
