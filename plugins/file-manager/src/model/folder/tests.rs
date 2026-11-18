use super::*;
use alloc::format;
use alloc::string::String;
use paperpaw_sdk::MemoryStorage;

fn names(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

fn entry(is_dir: bool, name: &str) -> Entry {
    Entry::new(name, is_dir, 0).unwrap()
}

/// A folder with two folders, a hidden file and 25 files of both cases.
pub(crate) fn card() -> MemoryStorage {
    let mut card = MemoryStorage::new()
        .with("d/Zeta/x", "")
        .with("d/alpha/y", "")
        .with("d/.hidden", "");
    for i in 0..25 {
        let name = format!("{}ile {i:02}.txt", if i % 2 == 0 { "F" } else { "f" });
        card = card.with(&format!("d/{name}"), "");
    }
    card
}

/// The order, spelled out: folders (alpha, Zeta), then the files by name
/// regardless of case (`.hidden` first), `File 00` before `file 01`.
fn expected(hidden: bool) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    all.push(String::from("alpha"));
    all.push(String::from("Zeta"));
    if hidden {
        all.push(String::from(".hidden"));
    }
    for i in 0..25 {
        all.push(format!(
            "{}ile {i:02}.txt",
            if i % 2 == 0 { "F" } else { "f" }
        ));
    }
    all
}

#[test]
fn pages_follow_the_order() {
    let card = card();
    let folder = Folder {
        storage: &card,
        dir: "d",
        hidden: false,
    };
    let all = expected(false);
    let len = folder.count().unwrap();
    assert_eq!(len, all.len());
    let mut walked = Vec::new();
    for page in 0..len.div_ceil(4) {
        let entries = folder.page(page, 4, len).unwrap();
        walked.extend(names(&entries).into_iter().map(String::from));
    }
    assert_eq!(walked, all);
    // Page by page forwards and back, as the buttons go.
    let second = folder.after(None, 4).unwrap();
    let third = folder.after(second.last(), 4).unwrap();
    assert_eq!(folder.before(third.first(), 4).unwrap(), second);
}

#[test]
fn the_page_of_an_entry() {
    let card = card();
    let folder = Folder {
        storage: &card,
        dir: "d",
        hidden: true,
    };
    let (first, page) = folder.page_of(&entry(false, "file 13.txt"), 6).unwrap();
    // `.hidden`, two folders, then 13 files before it: index 16.
    assert_eq!(first, 12);
    assert_eq!(page[16 - 12].name, "file 13.txt");
    assert_eq!(page.len(), 6);
}

#[test]
fn an_empty_or_missing_folder() {
    let card = MemoryStorage::new();
    let folder = Folder {
        storage: &card,
        dir: "",
        hidden: false,
    };
    assert_eq!(folder.count().unwrap(), 0);
    assert!(folder.page(0, 8, 0).unwrap().is_empty());
    let missing = Folder {
        storage: &card,
        dir: "gone",
        hidden: false,
    };
    assert_eq!(missing.count(), Err(Error::NotFound));
}

#[test]
fn a_refused_allocation_comes_back_whole_or_not_at_all() {
    use crate::model::testing::{Quiet, sweep};
    let all = expected(true);
    let make = || Quiet::new(card());
    sweep(
        "after/before",
        make,
        |s| {
            let folder = Folder {
                storage: s,
                dir: "d",
                hidden: true,
            };
            assert_eq!(folder.count()?, all.len());
            let page = folder.after(None, 5)?;
            let back = folder.before(page.last(), 3)?;
            assert_eq!((page.len(), back.len()), (5, 3));
            assert_eq!(page[0].name, "alpha");
            assert_eq!(page[2].name, ".hidden");
            Ok(())
        },
        |_| {},
    );
    sweep(
        "page/page_of",
        make,
        |s| {
            let folder = Folder {
                storage: s,
                dir: "d",
                hidden: true,
            };
            let len = folder.count()?;
            let last = folder.page(5, 5, len)?;
            assert_eq!(last.last().map(|e| e.name.as_str()), Some("File 24.txt"));
            let (first, page) = folder.page_of(&Entry::new("file 13.txt", false, 0)?, 6)?;
            assert_eq!((first, page.len()), (12, 6));
            Ok(())
        },
        |_| {},
    );
}
