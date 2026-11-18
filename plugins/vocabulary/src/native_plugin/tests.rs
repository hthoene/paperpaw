//! The recovery at start, over the data folder as the host gives it (names
//! inside the folder, root 0).

use paperpaw_sdk::{MemoryStorage, Storage, StorageError};

use super::{NativeVocabulary, State, recover};

#[test]
fn a_word_list_left_half_written_is_put_right() {
    // The new list was written and the old one gone: the new one is taken.
    let folder = MemoryStorage::new();
    folder.write("words.tmp", b"new\n").unwrap();
    recover(&folder);
    assert_eq!(folder.get("words.txt").unwrap(), b"new\n");
    assert_eq!(folder.size("words.tmp"), Err(StorageError::NotFound));
    // The old one is there: the unfinished one goes.
    let folder = MemoryStorage::new();
    folder.write("words.txt", b"old\n").unwrap();
    folder.write("words.tmp", b"half").unwrap();
    recover(&folder);
    assert_eq!(folder.get("words.txt").unwrap(), b"old\n");
    assert_eq!(folder.size("words.tmp"), Err(StorageError::NotFound));
}

#[test]
fn the_decks_files_are_put_right_too() {
    let folder = MemoryStorage::new();
    folder.write("cards/due.tmp", b"index\n").unwrap();
    folder.write("cards/decks.txt", b"decks\n").unwrap();
    folder.write("cards/decks.txt.part", b"half").unwrap();
    recover(&folder);
    assert_eq!(folder.get("cards/due.txt").unwrap(), b"index\n");
    assert_eq!(folder.size("cards/due.tmp"), Err(StorageError::NotFound));
    assert_eq!(folder.get("cards/decks.txt").unwrap(), b"decks\n");
    assert_eq!(
        folder.size("cards/decks.txt.part"),
        Err(StorageError::NotFound)
    );
}

#[test]
fn nothing_to_put_right_changes_nothing() {
    let folder = MemoryStorage::new();
    folder.write("words.txt", b"w\n").unwrap();
    recover(&folder);
    assert_eq!(folder.get("words.txt").unwrap(), b"w\n");
}

/// At rest the plugin is one pointer and a length: the state (the screens open,
/// the widgets, the question asked) is made on first use (#248).
#[test]
fn at_rest_the_plugin_holds_a_pointer_and_not_the_state() {
    assert_eq!(
        core::mem::size_of::<NativeVocabulary>(),
        core::mem::size_of::<Option<alloc::boxed::Box<[State]>>>()
    );
    assert!(core::mem::size_of::<State>() > core::mem::size_of::<NativeVocabulary>());
    assert!(NativeVocabulary::default().state.is_none());
}
