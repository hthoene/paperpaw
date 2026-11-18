//! Every allocation of the queue's and the requests' code may be refused: each
//! ends in `NoMemory` or succeeds, never aborts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_queue::{Mark, Queue, note, report};
use paperpaw_plugin_store::native_request::{Request, entries, finished, names, outcome, wanted};
use paperpaw_sdk::native::Error;
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

#[test]
fn the_queue_survives_refused_allocations() {
    let card = Card(MemoryStorage::new());
    fail_alloc::sweep(|| {
        // A refused run leaves its files: each run starts from none.
        fail_alloc::exempt(|| {
            for name in ["queue.txt", "queue-quiet.txt"] {
                let _ = card.remove(name);
            }
        });
        let mut queue = Queue::read(&card)?;
        queue.add(["alpha", "beta", "gamma"], Mark::Quiet)?;
        queue.add(["delta"], Mark::Requested)?;
        queue.advance();
        queue.keep(&card)?;
        let read = Queue::read(&card)?;
        assert!(read.requested_waiting());
        note(&card, "Alpha", true)?;
        note(&card, "Beta", false)?;
        report(&card, &Queue::default())?;
        Ok::<(), Error>(())
    });
}

#[test]
fn the_requests_survive_refused_allocations() {
    let card = Card(MemoryStorage::new());
    let ids: Vec<String> = ["pack-a", "pack-b"]
        .iter()
        .map(ToString::to_string)
        .collect();
    fail_alloc::sweep(|| {
        fail_alloc::exempt(|| {
            for name in ["request.txt", "request-result.txt"] {
                let _ = card.remove(name);
            }
        });
        let got = wanted(["a", "b", "a", "c"], &|id| id == "c")?;
        assert_eq!(got.len(), 2);
        Request::start(&card, "reader", &ids)?;
        Request::open(&card)?.expect("open");
        outcome(&card, "pack-a", true)?;
        let list = entries(&card, &ids)?;
        names(&list)?;
        finished(&card)?.expect("finished");
        Request::again(&card, "reader", &ids)?;
        Request::close(&card);
        Ok::<(), Error>(())
    });
}
