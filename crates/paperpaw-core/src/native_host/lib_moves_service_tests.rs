#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The library's move job as the device holds it: started by a move, carried
//! by the pump with the kernel's `Env`, a busy source, once at wiring.
extern crate std;
use super::*;
use crate::{
    Instant, Kernel,
    native_host::{
        files_service::FilesHost,
        lib_moves_journal::JOURNAL,
        lib_service::moves::{Entry, MoveHost},
        storage_files::{BusyKind, StorageFiles},
    },
};
use alloc::{borrow::Cow, vec::Vec};
use paperpaw_platform::{DirEntry, StorageError};
use paperpaw_sdk::{FormatSpec, Id, MemoryStorage, document::DocumentError, library};

/// One card in memory, shared by the kernel and the job (as on a device).
struct Card(Rc<MemoryStorage>);

impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
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
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.0.rename(f, t)
    }
    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.0.same_entry(a, b)
    }
    fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
        self.0.entry_identity(p)
    }
}

/// A kernel on a card with `.epub` as a format.
fn device() -> (Kernel, Rc<MemoryStorage>) {
    let card = Rc::new(MemoryStorage::new());
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(Card(card.clone()))
        .build()
        .unwrap();
    kernel
        .shared
        .formats
        .add(FormatSpec::new(
            Id::new_static("test.epub"),
            "EPUB",
            alloc::vec![Cow::Borrowed("epub")],
            alloc::boxed::Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
        .unwrap();
    (kernel, card)
}

fn pump_all(moves: &LibraryMoves, kernel: &Kernel) -> (Vec<Command>, usize) {
    let mut sent = Vec::new();
    let env = kernel.shared.env(Instant(0));
    let mut pumps = 0;
    loop {
        pumps += 1;
        assert!(pumps < 100, "the job ends");
        match moves.pump(&env, &mut |c| sent.push(c)) {
            Pump::More => {}
            Pump::Idle => return (sent, pumps),
            Pump::Wait => panic!("the card is fine"),
        }
    }
}

#[test]
fn a_move_is_carried_by_the_pump_with_the_kernels_catalog() {
    let (kernel, card) = device();
    card.write("Books/old/a.epub", b"book").unwrap();
    library::set_favourite(&*card, "Books/old/a.epub", true);
    let moves = LibraryMoves::new(Rc::new(Card(card.clone())));
    // Nothing runs before a move.
    let env = kernel.shared.env(Instant(0));
    assert_eq!(moves.pump(&env, &mut |_| ()), Pump::Idle);
    moves.begin("Books/old", "Books/new", false).unwrap();
    assert!(moves.moves().active(), "the entry is open until carried");
    assert!(card.exists("Books/new/a.epub") && card.exists(JOURNAL));
    let (sent, pumps) = pump_all(&moves, &kernel);
    assert!(pumps >= 3, "bounded steps: {pumps}");
    assert!(!moves.moves().active() && !card.exists(JOURNAL));
    assert_eq!(library::favourites(&*card), ["Books/new/a.epub"]);
    let moved = sent
        .iter()
        .map(|c| std::format!("{c:?}"))
        .filter(|c| c.contains("book-moved") && c.contains("Books/old/a.epub"))
        .count();
    assert_eq!(moved, 1);
    // The journal's job is cleared: the next move can start.
    moves.begin("Books/new", "Books/newer", false).unwrap();
}

#[test]
fn a_journal_left_by_a_power_cut_is_finished_by_the_first_pump() {
    let (kernel, card) = device();
    card.write("Books/new/a.epub", b"book").unwrap();
    library::set_favourite(&*card, "Books/old/a.epub", true);
    // The cut: the rename happened (the moved entry's identity is at `to`),
    // the journal is there, the memory of the job is gone.
    let identity = card.entry_identity("Books/new").unwrap().unwrap();
    let moves = LibraryMoves::new(Rc::new(Card(card.clone())));
    moves
        .card()
        .write_entry(&Entry {
            from: "Books/old",
            to: "Books/new",
            replace: false,
            identity,
            renamed: false,
        })
        .unwrap();
    let moves = LibraryMoves::new(Rc::new(Card(card.clone())));
    let (_, _) = pump_all(&moves, &kernel);
    assert!(!card.exists(JOURNAL));
    assert_eq!(library::favourites(&*card), ["Books/new/a.epub"]);
}

#[test]
fn wiring_registers_the_busy_source_once_and_a_files_move_starts_the_job() {
    use paperpaw_native::host_glue::ERR_STATE;
    let (_kernel, card) = device();
    card.write("Books/old/a.epub", b"book").unwrap();
    let hosts = HostServices::default();
    assert!(!hosts.busy_sources().has(BusyKind::LibraryMoves));
    let storage: Rc<dyn Storage> = Rc::new(Card(card.clone()));
    let moves = hosts.wire_library_moves(storage.clone()).unwrap();
    assert!(hosts.busy_sources().has(BusyKind::LibraryMoves));
    assert!(hosts.library_moves().is_some());
    assert_eq!(
        hosts.wire_library_moves(storage.clone()).err(),
        Some(ERR_STATE)
    );
    // `core.files` moves of the user's files are the job; others are not.
    let files = StorageFiles::new(storage.clone(), hosts.busy_sources().clone(), Rc::default())
        .with_moves(moves.clone());
    files
        .rename(&*storage, "Books/old", "Books/new", false, true)
        .unwrap();
    assert!(moves.moves().active(), "a root-1 move is a job");
    // While it runs every move is busy (`moving`), and the old name is held.
    assert!(files.moving());
    assert!(files.busy(
        "Books/old/x",
        crate::native_host::files_service::Access::Write
    ));
    card.write("Elsewhere/f.txt", b"x").unwrap();
    // Another move waits for the job (one journal entry): busy at the
    // service, and `ERR_BUSY` from the machine if asked anyway.
    assert_eq!(
        files.rename(&*storage, "Elsewhere/f.txt", "Elsewhere/g.txt", false, true),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    // A move that is not the library's (a plugin's own folder) is just a
    // rename.
    files
        .rename(
            &*storage,
            "Elsewhere/f.txt",
            "Elsewhere/g.txt",
            false,
            false,
        )
        .unwrap();
    assert!(card.exists("Elsewhere/g.txt"));
}
