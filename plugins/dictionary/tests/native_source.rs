//! The provider's source over a card: dictionaries found, indexes made by the
//! builders (the work's steps), words looked up, and the data folder kept apart
//! from the user's files.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_plugin_dictionary::{
    native_disk::{CACHE, Routed, cache_of, failed_file},
    native_model::{self, Item, Listing, Source},
    native_source::Dictionaries,
};
use paperpaw_sdk::{MemoryStorage, Storage, native::Error};

/// The folder `dicts` holds one StarDict dictionary, `en/small`.
struct Tree;
impl Listing for Tree {
    fn list(&mut self, dir: &str, each: &mut dyn FnMut(Item<'_>)) -> Result<(), Error> {
        let entries: &[(&str, bool)] = match dir {
            "dicts" => &[("en", true), ("notes.txt", false)],
            "dicts/en" => &[
                ("small.ifo", false),
                ("small.idx", false),
                ("small.dict", false),
            ],
            _ => return Err(Error::NotFound),
        };
        for (name, is_dir) in entries {
            each(Item {
                name,
                is_dir: *is_dir,
            });
        }
        Ok(())
    }
}

/// A two-word dictionary: `cat` and `dog`.
fn card() -> MemoryStorage {
    let card = MemoryStorage::new();
    let (cat, dog) = ("a small animal", "a loyal animal");
    let mut idx = Vec::new();
    for (word, at, text) in [("cat", 0u32, cat), ("dog", cat.len() as u32, dog)] {
        idx.extend_from_slice(word.as_bytes());
        idx.push(0);
        idx.extend_from_slice(&at.to_be_bytes());
        idx.extend_from_slice(&(text.len() as u32).to_be_bytes());
    }
    let ifo = format!(
        "StarDict's dict ifo file\nversion=2.4.2\nwordcount=2\nidxfilesize={}\nbookname=Small\nsametypesequence=m\n",
        idx.len()
    );
    card.write("dicts/en/small.ifo", ifo.as_bytes()).unwrap();
    card.write("dicts/en/small.idx", &idx).unwrap();
    card.write("dicts/en/small.dict", format!("{cat}{dog}").as_bytes())
        .unwrap();
    card
}

#[test]
fn an_unindexed_dictionary_is_not_ready_until_its_builders_have_run() {
    let (files, data) = (card(), MemoryStorage::new());
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "/dicts/").unwrap();
    assert_eq!(source.ready().unwrap(), 0);
    let mut builders = source.pending().unwrap();
    assert!(!builders.is_empty());
    for pending in &mut builders {
        while !pending.builder.step(&storage, 64).unwrap() {}
    }
    // A fresh source finds the indexes made.
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    assert_eq!(source.ready().unwrap(), 1);
    assert!(source.pending().unwrap().is_empty());
    let page = native_model::page_at(&mut source, "Dog", 0)
        .unwrap()
        .unwrap();
    assert_eq!(page.found.title, "Small");
    assert_eq!(page.found.text, "a loyal animal");
    assert!(!page.more);
    assert!(
        native_model::page_at(&mut source, "bird", 0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn the_indexes_are_made_in_the_data_folder_and_never_among_the_users_files() {
    let (files, data) = (card(), MemoryStorage::new());
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    for pending in &mut source.pending().unwrap() {
        while !pending.builder.step(&storage, 64 * 1024).unwrap() {}
    }
    let cache = cache_of("dicts/en/small.ifo").unwrap();
    let name = cache.strip_prefix(CACHE).unwrap();
    assert_eq!(name.len(), 16);
    assert!(data.size(&format!("{name}/idx.qbi")).is_ok());
    assert!(files.size(&format!("{name}/idx.qbi")).is_err());
    assert!(files.size("dicts/en/small.ifo").is_ok());
}

#[test]
fn a_folder_that_cannot_be_listed_is_an_error_and_a_broken_dictionary_is_left_out() {
    let (files, data) = (card(), MemoryStorage::new());
    files
        .write("dicts/en/small.ifo", b"not a dictionary")
        .unwrap();
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    assert_eq!(source.ready().unwrap(), 0);
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "missing").unwrap();
    assert_eq!(source.ready(), Err(Error::NotFound));
}

#[test]
fn a_cut_index_is_not_ready_and_is_made_again_from_the_start() {
    let (files, data) = (card(), MemoryStorage::new());
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let make = || {
        let mut tree = Tree;
        let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
        for pending in &mut source.pending().unwrap() {
            while !pending.builder.step(&storage, 64 * 1024).unwrap() {}
        }
    };
    make();
    // A power cut left the index file cut short.
    let cache = cache_of("dicts/en/small.ifo").unwrap();
    let name = format!("{}/idx.qbi", cache.strip_prefix(CACHE).unwrap());
    let mut kept = vec![0u8; data.size(&name).unwrap() as usize];
    let n = data.read_at(&name, 0, &mut kept).unwrap();
    data.write(&name, &kept[..n / 2]).unwrap();
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    assert_eq!(source.ready().unwrap(), 0, "a cut index is read as one");
    assert!(!source.pending().unwrap().is_empty());
    make();
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    assert_eq!(source.ready().unwrap(), 1);
    let page = native_model::page_at(&mut source, "cat", 0)
        .unwrap()
        .unwrap();
    assert_eq!(page.found.text, "a small animal");
}

#[test]
fn a_dictionary_that_failed_before_from_the_same_files_is_not_indexed_again() {
    let (files, data) = (card(), MemoryStorage::new());
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let pending = || {
        let mut tree = Tree;
        let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
        source.pending().unwrap()
    };
    let first = pending();
    assert!(!first.is_empty());
    // What the work keeps when the index is damaged: the files' sizes.
    let (cache, stamp) = (first[0].cache.clone(), first[0].stamp.clone());
    assert_eq!(cache, cache_of("dicts/en/small.ifo").unwrap());
    assert!(
        stamp.chars().all(|c| c.is_ascii_digit() || c == ' '),
        "{stamp}"
    );
    storage
        .write(&failed_file(&cache).unwrap(), stamp.as_bytes())
        .unwrap();
    assert!(pending().is_empty(), "not tried again from the same files");
    // Files that changed are tried again.
    storage
        .write(&failed_file(&cache).unwrap(), b"1 2 3")
        .unwrap();
    assert!(!pending().is_empty());
    // The dictionary still counts as found, and its cache as in use.
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    assert_eq!(source.found().unwrap(), 1);
    assert_eq!(source.caches().unwrap(), [cache]);
}

#[test]
fn the_cache_of_a_dictionary_that_cannot_be_opened_is_still_in_use() {
    let (files, data) = (card(), MemoryStorage::new());
    // Its data file is gone: it cannot be opened, yet it was found.
    files.remove("dicts/en/small.dict").unwrap();
    let storage = Routed {
        files: &files,
        data: &data,
    };
    let mut tree = Tree;
    let mut source = Dictionaries::new(&mut tree, &storage, "dicts").unwrap();
    assert_eq!(source.found().unwrap(), 0);
    assert_eq!(
        source.caches().unwrap(),
        [cache_of("dicts/en/small.ifo").unwrap()],
        "a failed open does not let its cache be cleaned away"
    );
}
