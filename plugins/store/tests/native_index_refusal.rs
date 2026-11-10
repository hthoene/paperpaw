//! Every allocation of the index's code may be refused: each ends in
//! `NoMemory` or succeeds, never aborts (the card double allocates out of the
//! sweep's count, as the host's card does).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_index::{Index, Order, build, hit, line_at, take};
use paperpaw_plugin_store::native_item::Device;
use paperpaw_plugin_store::native_list::Hooks;
use paperpaw_sdk::native::{Error, Fingerprint, INTERFACE};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

/// A card whose own bookkeeping is not what the sweep refuses.
struct Card(MemoryStorage);

impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        fail_alloc::exempt(|| self.0.read_at(p, o, b))
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        fail_alloc::exempt(|| self.0.size(p))
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.write(p, d))
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.append(p, d))
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.list(d, v))
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.remove(p))
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
        fail_alloc::exempt(|| self.0.rename(a, b))
    }
}

fn nobody(_: &str) -> Option<[u8; 32]> {
    None
}
fn none() -> Vec<String> {
    Vec::new()
}

fn device() -> Device<'static> {
    Device {
        name: "x4",
        fingerprint: Some(Fingerprint::of(INTERFACE, 0x4567_89ab_cdef)),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    }
}

fn list(count: usize) -> String {
    let mut text = String::from(r#"{"format": 2, "plugins": ["#);
    for n in 0..count {
        if n > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            r#"{{"id": "plugin-{n:04}", "name": "Äpfel {n}", "summary": "Does {n}.", "group": "look",
 "kind": "data", "author": "A", "licence": "MIT",
 "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30}}}}"#
        ));
    }
    text.push_str(r#"], "sections": {"new": ["plugin-0001"]}}"#);
    text
}

fn hooks() -> Hooks<'static> {
    Hooks {
        author_of: &nobody,
        stopped: &none,
    }
}

#[test]
fn building_and_reading_the_index_survive_refused_allocations() {
    let text = list(6);
    let card = Card(MemoryStorage::new());
    fail_alloc::sweep(|| {
        fail_alloc::exempt(|| card.write("list.json", text.as_bytes()).unwrap());
        let (index, _) = take(&card, "list.json", device(), hooks())?.expect("a list");
        let kept = Index::kept(&card)?.expect("kept");
        assert_eq!(kept.len(), index.len());
        index.section(&card, 1)?;
        index.entry(&card, 2)?.expect("an entry");
        index.search(&card, "ÄPFEL 3")?;
        assert!(hit(&card, 0).is_some());
        for order in [Order::Fair, Order::Name, Order::Added, Order::Updated] {
            index.pick(&card, |_, _| Ok(Some(1)), order, 3)?;
        }
        line_at(&card, "index.head", 0)?;
        let names = ["a.part", "b.part", "c.part", "d.part"];
        fail_alloc::exempt(|| card.write("list.json", text.as_bytes()).unwrap());
        build(&card, "list.json", names, device(), hooks())?;
        Ok::<(), Error>(())
    });
}

#[test]
fn more_than_a_run_survives_refused_allocations() {
    let text = list(540);
    let card = Card(MemoryStorage::new());
    fail_alloc::exempt(|| card.write("list.json", text.as_bytes()).unwrap());
    let index = take(&card, "list.json", device(), hooks())
        .unwrap()
        .expect("a list")
        .0;
    // 540 plugins sorted in two runs and merged.
    fail_alloc::sweep(|| {
        index.pick(&card, |_, _| Ok(Some(1)), Order::Added, usize::MAX)?;
        Ok::<(), Error>(())
    });
}
