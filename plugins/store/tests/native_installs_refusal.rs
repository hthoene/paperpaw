//! Every allocation of the snapshot of what is installed may be refused: it
//! ends in `NoMemory` or succeeds, never aborts.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_store::native_install::INSTALLS_FILE;
use paperpaw_plugin_store::native_installs::{Row, Snapshot};
use paperpaw_plugin_store::native_page::Kind;
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
fn the_snapshot_survives_refused_allocations() {
    let card = Card(MemoryStorage::new());
    fail_alloc::sweep(|| {
        // A refused run may leave the record; each run starts from none.
        fail_alloc::exempt(|| {
            let _ = card.remove(INSTALLS_FILE);
        });
        let zeta = Row::new("zeta", Kind::Data, true, false, "1.0.0", None)?;
        let alpha = Row::new("alpha", Kind::Code, true, true, "1.2.0", None)?;
        let builtin = Row::new("builtin", Kind::BuiltIn, true, false, "1.0.0", None)?;
        // The test's own list is not what the sweep refuses.
        let rows = fail_alloc::exempt(|| vec![zeta, alpha, builtin]);
        let snapshot = Snapshot::settle(rows, &card)?;
        assert!(snapshot.have("alpha").is_some());
        Ok::<(), Error>(())
    });
}
