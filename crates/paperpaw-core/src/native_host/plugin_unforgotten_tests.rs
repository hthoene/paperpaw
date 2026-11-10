#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;

#[test]
fn an_id_is_listed_once_and_taken_off_when_the_host_forgets_it() {
    let card = MemoryStorage::new();
    assert!(!has(&card, "dials"));
    assert!(add(&card, "dials") && add(&card, "dials") && add(&card, "clock"));
    assert!(has(&card, "dials") && has(&card, "clock") && !has(&card, "dial"));
    assert_eq!(read(&card).unwrap(), ["dials", "clock"]);
    // A host that cannot: the list stays. One that can: it goes.
    assert_eq!(retry(&card, &mut |_| false), 2);
    assert_eq!(retry(&card, &mut |id| id == "dials"), 1);
    assert!(!has(&card, "dials") && has(&card, "clock"));
    assert_eq!(retry(&card, &mut |_| true), 0);
    assert!(!card.exists(FILE), "an empty list is no file");
}

#[test]
fn the_list_is_bounded_and_a_ninth_id_is_not_recorded() {
    let card = MemoryStorage::new();
    for n in 0..MAX {
        assert!(add(&card, &std::format!("p{n}")));
    }
    assert!(!add(&card, "one-too-many"));
    assert!(!has(&card, "one-too-many"));
    assert_eq!(read(&card).unwrap().len(), MAX);
}

#[test]
fn a_damaged_list_is_read_defensively() {
    let card = MemoryStorage::new();
    let long = "x".repeat(ID_MAX + 1);
    card.write(
        FILE,
        std::format!("dials\n\n{long}\n  clock  \n\u{0}\n").as_bytes(),
    )
    .unwrap();
    assert!(has(&card, "dials") && has(&card, "clock"));
    assert!(!has(&card, &long));
    card.write(FILE, &[0xff, 0xfe, b'\n']).unwrap();
    assert!(!has(&card, "dials"));
}

/// A card whose reads of the list fail (a transient error).
struct Unreadable(MemoryStorage, StorageError);

impl Storage for Unreadable {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        if p == FILE {
            return Err(self.1);
        }
        self.0.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.0.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.0.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.0.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove(p)
    }
}

/// The list file's bytes.
fn file_bytes(card: &MemoryStorage) -> std::vec::Vec<u8> {
    let mut bytes = std::vec![0u8; usize::try_from(card.size(FILE).unwrap()).unwrap()];
    let got = card.read_at(FILE, 0, &mut bytes).unwrap();
    bytes.truncate(got);
    bytes
}

#[test]
fn a_list_that_cannot_be_read_blocks_installs_and_is_never_rewritten() {
    for why in [
        StorageError::Io,
        StorageError::Other(7),
        StorageError::CARD_GONE,
    ] {
        let card = Unreadable(MemoryStorage::new(), why);
        assert!(add(&card.0, "dials") && add(&card.0, "clock"));
        let before = file_bytes(&card.0);
        // Any id is "listed": the install is refused, not let through.
        assert!(has(&card, "dials") && has(&card, "other"));
        // Nothing is added, removed or retried; the file is untouched.
        assert!(!add(&card, "third"));
        assert!(!remove(&card, "dials"));
        let mut tried = false;
        assert_eq!(
            retry(&card, &mut |_| {
                tried = true;
                true
            }),
            1
        );
        assert!(!tried, "nothing is forgotten from a list nobody can read");
        assert_eq!(file_bytes(&card.0), before);
    }
}
