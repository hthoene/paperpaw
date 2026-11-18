#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::lib_moves_journal::{Opened, SET_ASIDE};
use crate::native_host::lib_service::moves::Entry;
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_sdk::{MemoryStorage, Storage};
use std::{string::ToString, vec};

fn book(path: &str, id: u64, bak: u64) -> Book {
    Book {
        path: path.to_string(),
        id,
        bak,
    }
}

fn delete() -> JobRecord {
    JobRecord::Delete {
        with_notes: true,
        books: vec![book("books/a.epub", 5, 6), book("books/sub/b.txt", 0, 0)],
        removed: 0b01,
    }
}

fn import() -> JobRecord {
    JobRecord::Import {
        from: "data/feed/x.epub".to_string(),
        to: "books/x.epub".to_string(),
        importer: "feed".to_string(),
        replace: true,
        identity: 0xdead_beef,
        arrived: false,
    }
}

#[test]
fn records_round_trip() {
    for job in [delete(), import()] {
        assert_eq!(decode(&encode(&job).unwrap()).unwrap(), Some(job));
    }
    let arrived = JobRecord::Import {
        from: "data/feed/x.epub".to_string(),
        to: "books/x.epub".to_string(),
        importer: "feed".to_string(),
        replace: false,
        identity: 7,
        arrived: true,
    };
    assert_eq!(decode(&encode(&arrived).unwrap()).unwrap(), Some(arrived));
}

#[test]
fn limits_are_refused() {
    let many = JobRecord::Delete {
        with_notes: false,
        books: (0..=DELETE_MAX)
            .map(|n| book(&std::format!("b{n}.txt"), 1, 0))
            .collect(),
        removed: 0,
    };
    assert_eq!(encode(&many), Err(ERR_INVALID));
    let none = JobRecord::Delete {
        with_notes: false,
        books: vec![],
        removed: 0,
    };
    assert_eq!(encode(&none), Err(ERR_INVALID));
    let long = JobRecord::Delete {
        with_notes: false,
        books: vec![book(&"x".repeat(MAX_PATH + 1), 1, 1)],
        removed: 0,
    };
    assert_eq!(encode(&long), Err(ERR_INVALID));
    // Seven paths of the longest size fit the reader's bound.
    let widest = JobRecord::Delete {
        with_notes: false,
        books: (0..DELETE_MAX)
            .map(|_| book(&"x".repeat(MAX_PATH), u64::MAX, u64::MAX))
            .collect(),
        removed: 0x7f,
    };
    // A bit past the books is no record.
    let past = JobRecord::Delete {
        with_notes: false,
        books: vec![book("a.txt", 1, 0)],
        removed: 0b10,
    };
    assert_eq!(encode(&past), Err(ERR_INVALID));
    assert!(encode(&widest).unwrap().len() <= crate::native_host::lib_moves_journal::MAX_RECORD);
}

#[test]
fn any_damage_is_an_error_not_a_job() {
    let good = encode(&delete()).unwrap();
    for at in 0..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 0x55;
        assert!(decode(&bad).is_err(), "byte {at}");
    }
    for cut in 0..good.len() {
        assert!(decode(&good[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn one_entry_of_one_kind() {
    let card = MemoryStorage::new();
    let journal = Journal(&card);
    journal.write_job(&delete()).unwrap();
    assert_eq!(journal.read_job().unwrap(), Some(delete()));
    // Neither another job nor a move writes over it.
    assert_eq!(journal.write_job(&import()), Err(ERR_BUSY));
    let entry = Entry {
        from: "a",
        to: "b",
        replace: false,
        identity: 1,
        renamed: false,
    };
    assert_eq!(journal.write_entry(&entry), Err(ERR_BUSY));
    assert_eq!(journal.read_job().unwrap(), Some(delete()));
    // The move machine leaves it alone: no entry for it, nothing set aside.
    assert_eq!(
        journal
            .open_or_set_aside(&mut |_| panic!("not a move"))
            .unwrap(),
        Opened::Other
    );
    assert!(!card.exists(SET_ASIDE));
    journal.clear_entry().unwrap();
    // And a move's entry blocks a job (and is not a job).
    journal.write_entry(&entry).unwrap();
    assert_eq!(journal.read_job().unwrap(), None);
    assert_eq!(journal.write_job(&delete()), Err(ERR_BUSY));
}

#[test]
fn the_removed_mask_is_written_whole_again() {
    let card = MemoryStorage::new();
    let journal = Journal(&card);
    // No entry, a move's, an import: no delete to mark.
    assert_eq!(journal.mark_removed(1), Err(ERR_STATE));
    journal.write_job(&import()).unwrap();
    assert_eq!(journal.mark_removed(1), Err(ERR_STATE));
    journal.clear_entry().unwrap();
    journal.write_job(&delete()).unwrap();
    journal.mark_removed(0b11).unwrap();
    let Some(JobRecord::Delete {
        removed,
        books,
        with_notes,
    }) = journal.read_job().unwrap()
    else {
        panic!("a delete");
    };
    assert_eq!((removed, books.len(), with_notes), (0b11, 2, true));
    // A bit past the books is refused and changes nothing.
    assert_eq!(journal.mark_removed(0b100), Err(ERR_INVALID));
    assert!(matches!(
        journal.read_job().unwrap(),
        Some(JobRecord::Delete { removed: 0b11, .. })
    ));
}

#[test]
fn a_record_that_is_not_whole_is_set_aside_by_the_move_machine() {
    let card = MemoryStorage::new();
    let mut bytes = encode(&delete()).unwrap();
    // Whole frame, payload that says nothing sane: count 0.
    bytes[PREFIX] = 0;
    let sum = check(&bytes[..bytes.len() - 8]);
    let at = bytes.len() - 8;
    bytes[at..].copy_from_slice(&sum.to_le_bytes());
    card.write(".paperpaw/library/journal", &bytes).unwrap();
    let journal = Journal(&card);
    assert_eq!(
        journal.open_or_set_aside(&mut |_| ()).unwrap(),
        Opened::SetAside
    );
    assert!(card.exists(SET_ASIDE));
    assert_eq!(journal.write_job(&delete()), Ok(()));
    // A kind no firmware knows is no job either.
    let mut other = encode(&delete()).unwrap();
    other[5] = 9;
    let at = other.len() - 8;
    let sum = check(&other[..at]);
    other[at..].copy_from_slice(&sum.to_le_bytes());
    assert!(decode(&other).is_err());
}

#[test]
fn a_failed_write_leaves_no_entry() {
    struct NoRename(MemoryStorage);
    impl paperpaw_platform::Storage for NoRename {
        fn read_at(
            &self,
            p: &str,
            a: u64,
            b: &mut [u8],
        ) -> Result<usize, paperpaw_platform::StorageError> {
            self.0.read_at(p, a, b)
        }
        fn size(&self, p: &str) -> Result<u64, paperpaw_platform::StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            self.0.append(p, d)
        }
        fn list(
            &self,
            d: &str,
            v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), paperpaw_platform::StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
            self.0.remove(p)
        }
        fn rename(&self, f: &str, t: &str) -> Result<(), paperpaw_platform::StorageError> {
            // The rename reaches the card, then the card says it failed.
            self.0.rename(f, t)?;
            Err(paperpaw_platform::StorageError::Io)
        }
    }
    let card = NoRename(MemoryStorage::new());
    assert_eq!(Journal(&card).write_job(&delete()), Err(ERR_IO));
    assert!(!card.0.exists(".paperpaw/library/journal"));
}
