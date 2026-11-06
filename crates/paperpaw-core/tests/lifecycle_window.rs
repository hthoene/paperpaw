//! Gate V2 of the interface-7 storage contract (`docs/abi/proposals/
//! interface-7-events-storage-services.md`, D1.2): what a plugin writes
//! while it hears `core.suspend` or `core.stopping` is on the card when
//! `Kernel::suspend` / `Kernel::stop` return. Shells cut power, restart or
//! exit only after that return (see `docs/porting.md`, "Lifecycle window"),
//! so the plugin's storage calls complete, and are written back, before the
//! power goes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::Harness;
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::events;
use paperpaw_core::sdk::{
    DirEntry, Manifest, MemoryStorage, Plugin, RegisterError, Registrar, Storage, StorageError,
};
use paperpaw_platform::Instant;

/// What the storage was told, in order, and how much of it no sync has
/// written back yet.
#[derive(Default)]
struct Journal {
    ops: Vec<String>,
    unsynced: usize,
}

/// A storage that keeps the order of what it is told and what is still
/// kept back (as the block cache of the X4's card is until `sync`).
#[derive(Clone, Default)]
struct Ordered {
    files: Rc<MemoryStorage>,
    journal: Rc<RefCell<Journal>>,
}

impl Ordered {
    fn note(&self, op: String, kept_back: bool) {
        let mut journal = self.journal.borrow_mut();
        journal.ops.push(op);
        journal.unsynced += usize::from(kept_back);
    }

    fn ops(&self) -> Vec<String> {
        self.journal.borrow().ops.clone()
    }

    fn unsynced(&self) -> usize {
        self.journal.borrow().unsynced
    }
}

impl Storage for Ordered {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note(format!("write {path}"), true);
        self.files.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.note(format!("append {path}"), true);
        self.files.append(path, data)
    }

    fn sync(&self) -> Result<(), StorageError> {
        let mut journal = self.journal.borrow_mut();
        journal.ops.push(String::from("sync"));
        journal.unsynced = 0;
        Ok(())
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.note(format!("remove {path}"), true);
        self.files.remove(path)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.note(format!("create_dir {path}"), true);
        self.files.create_dir(path)
    }
}

/// A plugin that saves when it hears `core.suspend` or `core.stopping`,
/// as Battery, Statistics and the sync plugins do: a file replaced, a log
/// appended to.
struct Saver {
    results: Rc<RefCell<Vec<Result<(), StorageError>>>>,
}

impl Plugin for Saver {
    fn manifest(&self) -> Manifest {
        common::manifest("saver")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (event, name) in [(events::SUSPEND, "suspend"), (events::STOPPING, "stopping")] {
            let results = self.results.clone();
            r.subscribe(
                event,
                Box::new(move |env, _, _| {
                    let storage = env.storage();
                    let mut results = results.borrow_mut();
                    results.push(storage.write(&format!("saver/{name}"), b"state"));
                    results.push(storage.append("saver/log", name.as_bytes()));
                }),
            )?;
        }
        Ok(())
    }
}

fn kernel(storage: &Ordered, results: &Rc<RefCell<Vec<Result<(), StorageError>>>>) -> Kernel {
    let harness = Harness::new();
    let mut kernel = Kernel::builder(common::DEVICE)
        .plugin(harness.plugin())
        .plugin(Saver {
            results: results.clone(),
        })
        .storage(storage.clone())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    kernel
}

/// The position of the last `sync` in `ops`.
fn last_sync(ops: &[String]) -> Option<usize> {
    ops.iter().rposition(|op| op == "sync")
}

/// What `core.suspend` made the plugin do is complete and written back
/// when `suspend` returns: the plugin's calls all came before the last
/// sync, and nothing is kept back after it.
#[test]
fn a_plugins_saves_in_suspend_are_on_the_card_when_it_returns() {
    let storage = Ordered::default();
    let results = Rc::default();
    let mut kernel = kernel(&storage, &results);
    storage.journal.borrow_mut().ops.clear();
    kernel.suspend(Instant(1000));
    let ops = storage.ops();
    assert_eq!(*results.borrow(), [Ok(()), Ok(())], "{ops:?}");
    let saved = ops.iter().position(|op| op == "write saver/suspend");
    let appended = ops.iter().position(|op| op == "append saver/log");
    let synced = last_sync(&ops);
    assert!(
        matches!((saved, appended, synced), (Some(w), Some(a), Some(s)) if w < s && a < s),
        "the plugin's calls come before the last sync: {ops:?}"
    );
    assert_eq!(ops.last().map(String::as_str), Some("sync"), "{ops:?}");
    assert_eq!(storage.unsynced(), 0, "nothing kept back: {ops:?}");
}

/// The same for `core.stopping` (a restart, an app closing).
#[test]
fn a_plugins_saves_in_stopping_are_on_the_card_when_it_returns() {
    let storage = Ordered::default();
    let results = Rc::default();
    let mut kernel = kernel(&storage, &results);
    storage.journal.borrow_mut().ops.clear();
    kernel.stop(Instant(1000));
    let ops = storage.ops();
    assert_eq!(*results.borrow(), [Ok(()), Ok(())], "{ops:?}");
    let saved = ops.iter().position(|op| op == "write saver/stopping");
    let appended = ops.iter().position(|op| op == "append saver/log");
    let synced = last_sync(&ops);
    assert!(
        matches!((saved, appended, synced), (Some(w), Some(a), Some(s)) if w < s && a < s),
        "the plugin's calls come before the last sync: {ops:?}"
    );
    assert_eq!(ops.last().map(String::as_str), Some("sync"), "{ops:?}");
    assert_eq!(storage.unsynced(), 0, "nothing kept back: {ops:?}");
}

/// A sleep that is followed by a stop (the device restarts while asleep,
/// or an app is closed after it was paused) saves again, and again leaves
/// nothing kept back.
#[test]
fn suspend_then_stop_leave_nothing_kept_back_each_time() {
    let storage = Ordered::default();
    let results = Rc::default();
    let mut kernel = kernel(&storage, &results);
    kernel.suspend(Instant(1000));
    assert_eq!(storage.unsynced(), 0);
    kernel.stop(Instant(2000));
    assert_eq!(storage.unsynced(), 0);
    assert_eq!(results.borrow().len(), 4);
    assert_eq!(
        storage.files.size("saver/log"),
        Ok(("suspend".len() + "stopping".len()) as u64)
    );
}
