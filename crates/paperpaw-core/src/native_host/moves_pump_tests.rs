#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
//! The library's move job in the service pump: it is stepped only outside
//! callbacks, with the kernel's own `Env` on the card the job was wired over,
//! a refused step waits [`WAIT_MS`], and what a step sends runs after the pump.
extern crate std;
use super::*;
use crate::{Instant, Kernel, native_host::HostResources};
use alloc::{borrow::Cow, rc::Rc, vec::Vec};
use core::cell::Cell;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::{Command, FormatSpec, Id, MemoryStorage, document::DocumentError, library};

/// The card, whose writes can be refused.
struct Card {
    card: Rc<MemoryStorage>,
    refuse: Rc<Cell<bool>>,
}

impl Card {
    fn check(&self) -> Result<(), StorageError> {
        if self.refuse.get() {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}

impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.check()?;
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.check()?;
        self.card.rename(f, t)
    }
    fn ignores_case(&self) -> bool {
        self.card.ignores_case()
    }
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.card.same_entry(a, b)
    }
    fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
        self.card.entry_identity(p)
    }
}

struct World {
    kernel: Kernel,
    card: Rc<MemoryStorage>,
    refuse: Rc<Cell<bool>>,
    moves: Rc<crate::native_host::lib_moves_service::LibraryMoves>,
    /// Keeps the pool (the host views) alive.
    _pool: Rc<HostResources>,
}

/// A kernel on a card with `.epub` as a format, the move job wired over the
/// same card, and one plugin so that the kernel holds the host views.
fn world() -> World {
    let card = Rc::new(MemoryStorage::new());
    let refuse = Rc::new(Cell::new(false));
    let storage = |card: &Rc<MemoryStorage>, refuse: &Rc<Cell<bool>>| Card {
        card: Rc::clone(card),
        refuse: Rc::clone(refuse),
    };
    let mut kernel = Kernel::builder(crate::tests::DEVICE)
        .storage(storage(&card, &refuse))
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
    let pool = Rc::new(HostResources::try_new().unwrap());
    let moves = pool
        .hosts()
        .wire_library_moves(Rc::new(storage(&card, &refuse)))
        .unwrap();
    let owner = pool.try_owner().unwrap();
    let lease = crate::native_host::native_instances::NativeLease::try_new(Idle).unwrap();
    kernel
        .shared
        .native_instances
        .insert(
            owner.try_view().unwrap(),
            Id::new_static("home"),
            Some(lease),
            true,
        )
        .unwrap();
    World {
        kernel,
        card,
        refuse,
        moves,
        _pool: pool,
    }
}

/// A plugin that does nothing.
struct Idle;
impl crate::native_host::native_instances::NativeInstance for Idle {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
}
impl crate::native_host::native_screen::Dispatch for Idle {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(paperpaw_native::host_glue::ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(paperpaw_native::host_glue::ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &crate::native_host::registrations::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(paperpaw_native::host_glue::ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

fn book_moved(commands: &[Command]) -> usize {
    commands
        .iter()
        .map(|c| std::format!("{c:?}"))
        .filter(|c| c.contains("book-moved"))
        .count()
}

/// A move waiting for its pump: `Books/old` holds one book with a favourite.
fn begun(w: &World) {
    w.card.write("Books/old/a.epub", b"book").unwrap();
    library::set_favourite(&*w.card, "Books/old/a.epub", true);
    w.moves.begin("Books/old", "Books/new", false).unwrap();
}

#[test]
fn the_service_pump_carries_the_job_with_the_kernels_catalog_and_its_commands_run_after() {
    let mut w = world();
    // Nothing runs before a move.
    assert!(!w.kernel.native_moves_pump());
    begun(&w);
    assert!(w.moves.moves().active());
    let mut passes = 0;
    while w.moves.moves().active() {
        passes += 1;
        assert!(passes < 50, "the job ends");
        w.kernel.native_service_pump();
    }
    // The books' place in the lists followed on the kernel's own card.
    assert_eq!(library::favourites(&*w.card), ["Books/new/a.epub"]);
    // The events wait in the kernel's queue until the commands run, which
    // is what `finish` does right after the pump.
    assert_eq!(book_moved(&w.kernel.output.commands), 1);
    w.kernel.run_commands();
    assert_eq!(book_moved(&w.kernel.output.commands), 0, "the queue ran");
}

#[test]
fn no_step_runs_inside_a_callback_and_the_pass_asks_again() {
    let mut w = world();
    begun(&w);
    w.kernel.shared.native_instances.test_enter_callback();
    assert!(w.kernel.native_moves_pump(), "the pass asks again");
    assert_eq!(library::favourites(&*w.card), ["Books/old/a.epub"]);
    assert!(w.kernel.output.commands.is_empty());
    w.kernel.shared.native_instances.test_leave_callback();
    while w.kernel.native_moves_pump() {}
    assert_eq!(library::favourites(&*w.card), ["Books/new/a.epub"]);
}

#[test]
fn a_refused_step_waits_two_seconds_and_keeps_the_job() {
    let mut w = world();
    begun(&w);
    w.kernel.now = Instant(10_000);
    w.refuse.set(true);
    // The steps that only read go on; the first one that must write is refused.
    let mut passes = 0;
    while w.kernel.native_moves_pump() {
        passes += 1;
        assert!(passes < 50, "a refused step ends the run of passes");
    }
    assert_eq!(
        w.kernel.shared.native_instances.moves_retry(),
        Some(10_000 + WAIT_MS)
    );
    assert!(w.moves.moves().active(), "the job is kept");
    // The wake-up the main loop is given says so.
    assert_eq!(w.kernel.service_due(), Some(Instant(10_000 + WAIT_MS)));
    // Not before the wait is over, even with the card back.
    w.refuse.set(false);
    w.kernel.now = Instant(10_000 + WAIT_MS - 1);
    assert!(!w.kernel.native_moves_pump());
    assert!(w.moves.moves().active());
    let sent = book_moved(&w.kernel.output.commands);
    // Then the step is tried again, and the job goes on to its end.
    w.kernel.now = Instant(10_000 + WAIT_MS);
    while w.kernel.native_moves_pump() {}
    assert!(!w.moves.moves().active());
    // The events are sent at least once: those of a step that was cut are
    // sent again.
    assert!(book_moved(&w.kernel.output.commands) >= sent.max(1));
    assert_eq!(w.kernel.shared.native_instances.moves_retry(), None);
}

#[test]
fn a_device_without_the_job_has_nothing_to_pump() {
    let mut kernel = Kernel::builder(crate::tests::DEVICE).build().unwrap();
    assert!(!kernel.native_moves_pump());
    kernel.native_service_pump();
}

#[test]
fn a_card_that_stays_broken_is_tried_less_and_less_often() {
    let mut w = world();
    begun(&w);
    w.refuse.set(true);
    let mut now = 10_000;
    let mut waits = Vec::new();
    for _ in 0..70 {
        w.kernel.now = Instant(now);
        while w.kernel.native_moves_pump() {}
        let at = w.kernel.shared.native_instances.moves_retry().unwrap();
        waits.push(at - now);
        now = at;
    }
    assert_eq!(
        waits[..7],
        [2_000, 4_000, 8_000, 16_000, 32_000, 64_000, 64_000]
    );
    // However long the card stays broken, the wait never wraps to a short one.
    assert!(
        waits
            .iter()
            .all(|wait| (WAIT_MS..=WAIT_MAX_MS).contains(wait))
    );
    // A card that is back ends the job and the back-off starts again.
    w.refuse.set(false);
    w.kernel.now = Instant(now);
    while w.kernel.native_moves_pump() {}
    assert!(!w.moves.moves().active());
    assert_eq!(w.kernel.shared.native_instances.moves_retry(), None);
}

#[test]
fn a_move_begun_during_the_back_off_is_not_held_back_by_it() {
    let mut w = world();
    // The state a card that stayed broken left: five refusals, the wait at
    // its longest, a minute away.
    let instances = &w.kernel.shared.native_instances;
    for _ in 0..8 {
        instances.moves_failed();
    }
    w.kernel.now = Instant(100_000);
    instances.set_moves_retry(Some(100_000 + WAIT_MAX_MS));
    assert!(!w.kernel.native_moves_pump(), "held back, nothing begun");
    begun(&w);
    // The new move runs at once and the back-off starts again.
    let mut passes = 0;
    while w.kernel.native_moves_pump() {
        passes += 1;
        assert!(passes < 50, "the job ends");
    }
    assert!(!w.moves.moves().active());
    assert_eq!(library::favourites(&*w.card), ["Books/new/a.epub"]);
    assert_eq!(w.kernel.shared.native_instances.moves_retry(), None);
    // The first refusal after it waits two seconds again.
    begun_again(&w);
    w.refuse.set(true);
    while w.kernel.native_moves_pump() {}
    assert_eq!(
        w.kernel.shared.native_instances.moves_retry(),
        Some(100_000 + WAIT_MS)
    );
}

/// Another move of the same books, from where the first one went.
fn begun_again(w: &World) {
    w.moves.begin("Books/new", "Books/newer", false).unwrap();
}
