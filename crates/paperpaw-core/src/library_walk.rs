//! What the kernel's catalog work tells the library service of the catalog
//! being made (#460): how far a first walk is, and whether the work has gone
//! quiet while the catalog is still to be made. One handle, shared by the
//! kernel ([`crate::KernelBuilder::library_walk`]), which notes each step of
//! the work, and the shell's catalog environment
//! ([`crate::native_host::shell_wiring::CatalogEnv`]), which reads it when a
//! plugin asks for the library's state. Nothing here keeps a timer: the time
//! stamps are compared only when the state is read.

use alloc::rc::Rc;
use core::cell::Cell;

use paperpaw_sdk::Storage;
use paperpaw_sdk::work::Step;

use crate::kit::library;

/// How long the catalog work may give no sign of life while the catalog is
/// still to be made before the library says the card cannot be read (in
/// milliseconds), as Beta 3's library did (#145).
pub const STALL_MS: u64 = 90_000;

/// The kernel's catalog work as the library service sees it. A clone is
/// another view of the same cell.
#[derive(Clone, Debug, Default)]
pub struct LibraryWalk(Rc<Cell<Seen>>);

#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    /// How far the walk under way is, in parts per million; `None` where no
    /// walk is under way.
    progress: Option<u32>,
    /// The kernel's clock (milliseconds) at the last sign of life of the
    /// catalog work; `None` while nothing is to be done, while it waits for a
    /// screen to show (under a book) and before it was first wanted.
    alive_ms: Option<u64>,
}

impl LibraryWalk {
    /// What `core.library` op 1 says of the catalog being made: `Some(parts
    /// per million)` while a first walk is under way (the catalog is made
    /// where there was none and its books are those found so far), `None`
    /// otherwise (a walk over a whole catalog, catching up and sorting are
    /// state 2). The share is rough: a library's size is known only at the
    /// end, so the view leads with the books so far and this is only a bar.
    /// A first walk cut short that no job has taken up again yet says 0. The
    /// share can fall back to 0 after a restart of the walk (a walk begun
    /// again counts its folders anew); the books so far are what grows.
    #[must_use]
    pub fn walking(&self, storage: &dyn Storage) -> Option<u32> {
        library::catalog::is_growing(storage).then(|| self.0.get().progress.unwrap_or(0))
    }

    /// Whether the catalog work gave no sign of life for [`STALL_MS`] by
    /// `now_ms` (the kernel's clock) while it had something to do: a walk
    /// that stopped, or a catalog that is wanted and never begun.
    #[must_use]
    pub fn stalled(&self, now_ms: u64) -> bool {
        self.0
            .get()
            .alive_ms
            .is_some_and(|alive| now_ms.saturating_sub(alive) >= STALL_MS)
    }

    /// The catalog work took a step at `now_ms` that ended in `step`, with
    /// the walk under way `progress` far (`None`: no walk). A step that goes
    /// on is a sign of life; a failed one is none (the work is tried again
    /// after a while); one that waits for a screen or has nothing more to do
    /// stops the clock.
    pub(crate) fn stepped(&self, now_ms: u64, step: &Step, progress: Option<u32>) {
        let mut seen = self.0.get();
        seen.progress = progress;
        seen.alive_ms = match step {
            Step::Continue | Step::Progress(_) | Step::Wait(_) => Some(now_ms),
            Step::Failed(_) => Some(seen.alive_ms.unwrap_or(now_ms)),
            Step::WaitFor(_) | Step::Done => None,
            _ => Some(now_ms),
        };
        self.0.set(seen);
    }

    /// The device woke at `now_ms`: the time asleep is no silence of the
    /// work, its clock (where it runs) counts from now.
    pub(crate) fn woke(&self, now_ms: u64) {
        let mut seen = self.0.get();
        if seen.alive_ms.is_some() {
            seen.alive_ms = Some(now_ms);
            self.0.set(seen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    /// The notes a first walk leaves on the card while it is under way.
    fn growing() -> MemoryStorage {
        MemoryStorage::new()
            .with(".paperpaw/library/catalog.tmp", b"".to_vec())
            .with(".paperpaw/library/walk.first", b"".to_vec())
    }

    #[test]
    fn walking_is_a_first_walk_and_only_that() {
        let walk = LibraryWalk::default();
        // No walk on the card: state 2 or 0, never 1.
        assert_eq!(walk.walking(&MemoryStorage::new()), None);
        // A walk over a whole catalog (a catalog being made, no first-walk
        // note) is state 2.
        let over = MemoryStorage::new().with(".paperpaw/library/catalog.tmp", b"".to_vec());
        walk.stepped(0, &Step::Continue, Some(250_000));
        assert_eq!(walk.walking(&over), None);
        // A first walk: the job's progress, seen by every clone.
        assert_eq!(walk.clone().walking(&growing()), Some(250_000));
        // A first walk cut short, no job yet: 0, not "ready".
        walk.stepped(0, &Step::Failed("card".into()), None);
        assert_eq!(walk.walking(&growing()), Some(0));
    }

    #[test]
    fn a_catalog_work_quiet_for_90_seconds_is_stalled() {
        let walk = LibraryWalk::default();
        // Never wanted: nothing to wait for.
        assert!(!walk.stalled(10 * STALL_MS));
        walk.stepped(1_000, &Step::Continue, Some(0));
        assert!(!walk.stalled(1_000 + STALL_MS - 1));
        assert!(walk.stalled(1_000 + STALL_MS));
        // Failed steps are no sign of life: the clock runs from the last one.
        walk.stepped(50_000, &Step::Failed("card".into()), None);
        assert!(walk.stalled(1_000 + STALL_MS));
        // A step that goes on starts it again.
        walk.stepped(100_000, &Step::Continue, Some(10));
        assert!(!walk.stalled(100_000 + STALL_MS - 1));
        // Under a book (waiting for a screen) or done: never stalled.
        walk.stepped(
            110_000,
            &Step::WaitFor(crate::sdk::builtin::events::SCREEN_SHOWN.id().clone()),
            Some(10),
        );
        assert!(!walk.stalled(u64::MAX));
        walk.woke(500_000);
        assert!(!walk.stalled(u64::MAX));
        walk.stepped(600_000, &Step::Continue, Some(20));
        walk.stepped(610_000, &Step::Done, None);
        assert!(!walk.stalled(u64::MAX));
    }

    #[test]
    fn the_time_asleep_is_no_silence() {
        let walk = LibraryWalk::default();
        walk.stepped(1_000, &Step::Continue, Some(0));
        walk.woke(3_600_000);
        assert!(!walk.stalled(3_600_000 + STALL_MS - 1));
        assert!(walk.stalled(3_600_000 + STALL_MS));
    }
}
