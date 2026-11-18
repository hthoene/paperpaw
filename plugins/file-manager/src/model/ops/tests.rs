use super::*;
use crate::model::testing::{Quiet, fixture, sweep};
use alloc::format;
use alloc::vec;
use paperpaw_sdk::MemoryStorage;

/// A file larger than the copy's buffer, so it is copied in pieces.
fn big() -> Vec<u8> {
    (0..CHUNK * 2 + 123).map(|i| (i % 251) as u8).collect()
}

fn tree() -> MemoryStorage {
    MemoryStorage::new()
        .with("src/a.txt", "alpha")
        .with("src/big.bin", big())
        .with("src/sub/b.txt", "beta")
        .with("src/sub/deep/c.txt", "gamma")
        .with("keep/old.txt", "old")
}

fn no_parts(q: &Quiet) {
    assert!(
        q.files().iter().all(|f| !f.ends_with(".part")),
        "{:?}",
        q.files()
    );
}

#[test]
fn a_tree_is_copied_with_everything_in_it() {
    let storage = tree();
    copy(&storage, "src", "dst").unwrap();
    assert_eq!(storage.get("dst/a.txt").unwrap(), b"alpha");
    assert_eq!(storage.get("dst/big.bin").unwrap(), big());
    assert_eq!(storage.get("dst/sub/deep/c.txt").unwrap(), b"gamma");
    assert_eq!(storage.get("src/a.txt").unwrap(), b"alpha");
    // A file replaces what is there; no part is left.
    copy(&storage, "src/a.txt", "keep/old.txt").unwrap();
    assert_eq!(storage.get("keep/old.txt").unwrap(), b"alpha");
    assert!(storage.get("keep/old.txt.part").is_none());
}

#[test]
fn a_folder_of_more_than_a_page_is_copied_whole() {
    let mut storage = MemoryStorage::new();
    for i in 0..30 {
        storage = storage.with(&format!("many/f{i:02}"), format!("{i}"));
    }
    copy(&storage, "many", "copy").unwrap();
    for i in 0..30 {
        assert_eq!(
            storage.get(&format!("copy/f{i:02}")).unwrap(),
            format!("{i}").as_bytes()
        );
    }
}

#[test]
fn a_folder_is_not_copied_or_moved_into_itself() {
    let storage = tree();
    assert_eq!(copy(&storage, "src", "src/sub/in"), Err(Error::Invalid));
    assert_eq!(move_to(&storage, "src", "src/sub/in"), Err(Error::Invalid));
    // Moving onto itself is the storage's own business, not a loop.
    assert!(is_dir(&storage, "src"));
}

#[test]
fn a_file_is_not_copied_or_moved_onto_itself_where_that_would_empty_it() {
    let mut q = Quiet::new(tree());
    q.cannot_rename = true;
    assert_eq!(copy(&q, "src/a.txt", "src/a.txt"), Err(Error::Invalid));
    assert_eq!(copy(&q, "src/a.txt", "/src/a.txt/"), Err(Error::Invalid));
    assert_eq!(move_to(&q, "src/a.txt", "src/a.txt"), Err(Error::Invalid));
    assert_eq!(q.get("src/a.txt").unwrap(), b"alpha");
    // On a storage that ignores case, a name alike but for case is the same file.
    q.folds = true;
    assert_eq!(copy(&q, "src/a.txt", "src/A.txt"), Err(Error::Invalid));
    assert_eq!(move_to(&q, "src/a.txt", "src/A.TXT"), Err(Error::Invalid));
    assert_eq!(q.get("src/a.txt").unwrap(), b"alpha");
    // Where the storage renames, a move onto itself is its own business.
    let ok = Quiet::new(tree());
    assert_ne!(copy(&ok, "src/a.txt", "src/a.txt"), Ok(()));
}

#[test]
fn folders_nested_too_deep_are_refused() {
    let mut deep = String::from("d");
    let mut storage = MemoryStorage::new();
    for _ in 0..=MAX_DEPTH {
        deep.push_str("/x");
    }
    storage = storage.with(&format!("{deep}/f"), "f");
    assert_eq!(copy(&storage, "d", "copy"), Err(Error::NoSpace));
}

#[test]
fn a_move_renames_or_copies_and_deletes_where_the_storage_cannot_rename() {
    let storage = tree();
    move_to(&storage, "src/a.txt", "keep/a.txt").unwrap();
    assert!(storage.get("src/a.txt").is_none());
    assert_eq!(storage.get("keep/a.txt").unwrap(), b"alpha");
    let mut q = Quiet::new(tree());
    q.cannot_rename = true;
    move_to(&q, "src", "moved").unwrap();
    assert_eq!(q.get("moved/sub/deep/c.txt").unwrap(), b"gamma");
    assert!(q.get("src/a.txt").is_none());
    no_parts(&q);
}

#[test]
fn delete_takes_a_file_or_a_whole_folder() {
    let storage = tree();
    delete(&storage, "src/a.txt").unwrap();
    assert!(storage.get("src/a.txt").is_none());
    delete(&storage, "src").unwrap();
    assert!(!is_dir(&storage, "src"));
    // A missing one is not an error, as the storage says.
    assert_eq!(delete(&storage, "gone"), Ok(()));
}

#[test]
fn a_rename_goes_to_a_free_name_or_through_one_between() {
    let storage = tree();
    assert_eq!(rename(&storage, "src/a.txt", "z.txt").unwrap(), "src/z.txt");
    // The name is taken: a free one beside it.
    let storage = tree();
    assert_eq!(
        rename(&storage, "src/a.txt", "big.bin").unwrap(),
        "src/big (2).bin"
    );
    assert_eq!(storage.get("src/big.bin").unwrap(), big());
    assert_eq!(storage.get("src/big (2).bin").unwrap(), b"alpha");
    // Case only, on a storage that ignores case: through a name between.
    let mut q = Quiet::new(tree());
    q.folds = true;
    assert_eq!(rename(&q, "src/a.txt", "A.txt").unwrap(), "src/A.txt");
    assert_eq!(q.get("src/A.txt").unwrap(), b"alpha");
    assert!(q.files().iter().all(|f| !f.contains("renaming")));
}

fn copied(q: &Quiet) -> Result<(), Error> {
    copy(q, "src", "dst")
}

#[test]
fn a_refused_allocation_leaves_the_source_and_no_part() {
    let make = || Quiet::new(tree());
    let whole = |q: &Quiet| {
        no_parts(q);
        // The source is untouched, and a copy is whole where it is there.
        assert_eq!(q.get("src/big.bin").unwrap(), big());
        assert_eq!(q.get("keep/old.txt").unwrap(), b"old");
        for (path, data) in [("dst/a.txt", &b"alpha"[..]), ("dst/sub/b.txt", b"beta")] {
            assert!(q.get(path).is_none_or(|got| got == data), "{path}");
        }
        if let Some(got) = q.get("dst/big.bin") {
            assert_eq!(got, big(), "a half file looks whole");
        }
    };
    sweep("copy a tree", make, copied, whole);
    sweep(
        "copy a file over one",
        make,
        |q| copy(q, "src/big.bin", "keep/old.txt"),
        |q| {
            no_parts(q);
            let got = q.get("keep/old.txt").unwrap();
            assert!(got == b"old" || got == big(), "{} bytes", got.len());
        },
    );
    sweep(
        "move without rename",
        || {
            let mut q = Quiet::new(tree());
            q.cannot_rename = true;
            q
        },
        |q| move_to(q, "src", "moved"),
        |q| {
            no_parts(q);
            // Deleted only after the copy was whole: the source is whole or gone.
            if q.get("src/sub/deep/c.txt").is_some() {
                assert_eq!(q.get("src/big.bin").unwrap(), big());
            } else {
                assert_eq!(q.get("moved/sub/deep/c.txt").unwrap(), b"gamma");
            }
        },
    );
}

#[test]
fn a_refused_allocation_in_a_rename_keeps_the_entry() {
    let alive = |q: &Quiet| {
        let there = q.get("src/a.txt").is_some() || q.get("src/z.txt").is_some();
        assert!(there || q.get("src/A.txt").is_some(), "{:?}", q.files());
        assert!(
            q.files().iter().all(|f| !f.contains("renaming")),
            "{:?}",
            q.files()
        );
    };
    sweep(
        "rename",
        || Quiet::new(tree()),
        |q| rename(q, "src/a.txt", "z.txt").map(|_| ()),
        alive,
    );
    sweep(
        "rename taken",
        || Quiet::new(tree()),
        |q| rename(q, "src/a.txt", "big.bin").map(|_| ()),
        alive,
    );
    sweep(
        "rename case only",
        || {
            let mut q = Quiet::new(tree());
            q.folds = true;
            q
        },
        |q| rename(q, "src/a.txt", "A.txt").map(|_| ()),
        alive,
    );
    let _ = (fixture(|| vec![0u8; 1]), ());
}
