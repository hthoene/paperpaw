#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The record of the store's installs: read as interface 6 wrote it, and
//! taken over once where a device kept none.
extern crate std;
use super::*;
use alloc::format;
use paperpaw_sdk::MemoryStorage;

/// A kernel with data plugins `(id, version)` on `card`.
fn start(card: MemoryStorage, plugins: &[(&str, &str)]) -> crate::Kernel {
    let mut card = card;
    for (id, version) in plugins {
        let ini = format!("[plugin]\nname = {id}\nversion = {version}\nformats = 4\n");
        card = card.with(&format!("plugins/{id}/plugin.ini"), ini);
    }
    crate::Kernel::builder(crate::tests::DEVICE)
        .storage(card)
        .build()
        .unwrap()
}

fn record(kernel: &crate::Kernel) -> Option<std::string::String> {
    let storage = kernel.shared.storage.as_ref();
    let size = usize::try_from(storage.size(STORE_INSTALLS).ok()?).unwrap();
    let mut bytes = alloc::vec![0; size];
    storage.read_at(STORE_INSTALLS, 0, &mut bytes).unwrap();
    Some(std::string::String::from_utf8(bytes).unwrap())
}

#[test]
fn a_device_that_kept_no_record_takes_its_plugins_for_the_stores_once() {
    let kernel = start(
        MemoryStorage::new(),
        &[("cosy", "1.2.0-beta.1"), ("dark", "2.1")],
    );
    let kept = record(&kernel).unwrap();
    assert_eq!(kept, "cosy\t1.2.0-beta.1\ndark\t2.1.0\n");
    let read = StoreRecord::read(kernel.shared.storage.as_ref()).unwrap();
    let v = |text| Version::parse(text).unwrap();
    assert_eq!(
        read.release("cosy", v("1.2.0-beta.1")),
        Some("1.2.0-beta.1")
    );
    assert_eq!(read.release("dark", v("2.1.0")), Some("2.1.0"));
    assert_eq!(read.release("dark", v("2.0.0")), None);
}

#[test]
fn a_kept_record_is_never_taken_over_again() {
    let card = MemoryStorage::new().with(STORE_INSTALLS, "dark\t1.0.0\n");
    let kernel = start(card, &[("cosy", "1.0.0"), ("dark", "2.0.0")]);
    // Plugins put in place since (cosy) or replaced (dark 2.0.0) stay local.
    assert_eq!(record(&kernel).as_deref(), Some("dark\t1.0.0\n"));
}

#[test]
fn the_last_line_of_a_release_counts_and_an_unreadable_one_is_none() {
    let card = MemoryStorage::new().with(
        STORE_INSTALLS,
        b"cosy\t1.0.0\nbroken\nodd\t\xff\ncosy\t1.0\n".to_vec(),
    );
    let read = StoreRecord::read(&card).unwrap();
    let one = Version::parse("1.0.0").unwrap();
    assert_eq!(read.release("cosy", one), Some("1.0"));
    assert_eq!(read.release("odd", one), None);
    assert_eq!(read.release("broken", one), None);
    // No record at all: nobody's.
    let none = StoreRecord::read(&MemoryStorage::new()).unwrap();
    assert_eq!(none.release("cosy", one), None);
}
