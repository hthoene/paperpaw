//! What the panel reads of the plugin's own dictionaries: every definition
//! whole (past the 2,048 bytes an entry of the lookup service holds), within
//! what a lookup reads at most, and how many dictionaries the folder holds.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_plugin_dictionary::{
    native_disk::Routed,
    native_model::{Item, Listing},
    native_popup::{BUDGET, read_own},
    native_source::Dictionaries,
};
use paperpaw_sdk::{MemoryStorage, Storage, native::Error};

/// The folder `dicts` holds `big.ifo` and what goes with it.
struct Tree;
impl Listing for Tree {
    fn list(&mut self, dir: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
        if dir != "dicts" {
            return Err(Error::NotFound);
        }
        for name in ["big.ifo", "big.idx", "big.dict"] {
            each(Item {
                name,
                is_dir: false,
            });
        }
        Ok(())
    }
}

/// `cat` with a definition of `len` bytes, `dog` with a short one.
fn card(len: usize) -> MemoryStorage {
    let card = MemoryStorage::new();
    let cat = "c".repeat(len);
    let dog = "a loyal animal";
    let mut idx = Vec::new();
    for (word, at, text) in [("cat", 0u32, cat.as_str()), ("dog", len as u32, dog)] {
        idx.extend_from_slice(word.as_bytes());
        idx.push(0);
        idx.extend_from_slice(&at.to_be_bytes());
        idx.extend_from_slice(&(text.len() as u32).to_be_bytes());
    }
    let ifo = format!(
        "StarDict's dict ifo file\nversion=2.4.2\nwordcount=2\nidxfilesize={}\nbookname=Big\nsametypesequence=m\n",
        idx.len()
    );
    card.write("dicts/big.ifo", ifo.as_bytes()).unwrap();
    card.write("dicts/big.idx", &idx).unwrap();
    card.write("dicts/big.dict", format!("{cat}{dog}").as_bytes())
        .unwrap();
    card
}

/// Makes the indexes, as the background work does.
fn index(files: &MemoryStorage, data: &MemoryStorage) {
    let storage = Routed { files, data };
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    for pending in &mut source.pending().unwrap() {
        while !pending.builder.step(&storage, 64 * 1024).unwrap() {}
    }
}

fn read(files: &MemoryStorage, data: &MemoryStorage, word: &str) -> (usize, Vec<(String, usize)>) {
    let storage = Routed { files, data };
    let mut found = Vec::new();
    let count = read_own(&mut Tree, &storage, "dicts", word, &mut found)
        .unwrap()
        .count;
    (
        count,
        found
            .into_iter()
            .map(|e| (e.source, e.text.len()))
            .collect(),
    )
}

#[test]
fn a_definition_over_the_entry_limit_is_shown_whole() {
    let (files, data) = (card(5_000), MemoryStorage::new());
    index(&files, &data);
    let (count, found) = read(&files, &data, "cat");
    assert_eq!(count, 1);
    assert_eq!(found, [("Big".to_string(), 5_000)]);
    let (_, found) = read(&files, &data, "dog");
    assert_eq!(found, [("Big".to_string(), 14)]);
    assert!(read(&files, &data, "bird").1.is_empty());
}

#[test]
fn a_huge_definition_is_cut_where_a_lookup_stops_reading() {
    let (files, data) = (card(BUDGET + 10_000), MemoryStorage::new());
    index(&files, &data);
    let (_, found) = read(&files, &data, "cat");
    assert_eq!(found.len(), 1);
    assert!(found[0].1 <= BUDGET && found[0].1 > BUDGET - 8);
}

#[test]
fn a_dictionary_without_indexes_is_counted_but_says_nothing_and_no_folder_holds_none() {
    let (files, data) = (card(100), MemoryStorage::new());
    let (count, found) = read(&files, &data, "cat");
    assert_eq!((count, found.len()), (1, 0), "not ready: counted, not read");
    // A folder that cannot be listed holds none.
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let mut found = Vec::new();
    let own = read_own(&mut Tree, &storage, "missing", "cat", &mut found).unwrap();
    assert_eq!((own.count, own.listed), (0, false));
}
