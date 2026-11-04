//! A card taken out and put back while PaperPaw runs (#145): its storage
//! mounted anew, never written with what was kept of the card before.
//!
//! The devices have no pin that says a card was taken out. What says it
//! is the card itself: taken out, it loses its power, and put back (the
//! same card or another) it waits to be started, answering no read or
//! write. So a run of failed reads and writes ([`LOST_AFTER`] in a row)
//! counts as the card lost, and from then on nothing reaches the card
//! through the storage that was mounted on it: its blocks, its file
//! system and what both keep (folders, open files, blocks not yet
//! written back) belong to the card that was, which may not be the one in
//! the slot now.
//!
//! The storage's caches answer without the card, so a card swapped while
//! only cached blocks are read would show the old card's folders without
//! a failure. So the first use of the storage after [`PROBE_MS`] without
//! one looks at the card itself first (its first block, past every
//! cache): taking a card out and putting one back takes longer than
//! that, so the first use after it finds the card lost.
//!
//! [`CardSwap::check`], which the shell calls between the kernel's steps,
//! then starts the card again and mounts it anew (any card is taken as
//! another one: nothing kept is used), and the storage the kernel holds
//! ([`Swappable`]) reads and writes the new mount. The shell tells the
//! kernel (`Kernel::usb_drive_ended`), which walks the library again.
//!
//! Each mount is a generation: the blocks of a mount ([`Watched`]) carry
//! theirs, and only the current one reaches the card. A remount moves to
//! the next generation before the card is started again, so the old
//! mount's blocks fail from then on, whatever still holds them (a file
//! system writing back as it is dropped, a cache).

use alloc::boxed::Box;
use alloc::rc::Rc;
use core::cell::RefCell;

use paperpaw_platform::{DirEntry, Storage, StorageError};

use crate::disk::{BLOCK, Blocks, DiskError};

/// After this many reads or writes of the card in a row fail, it counts
/// as taken out. Each has been tried again by the card's driver already;
/// a card that is there fails one now and then, not three in a row.
pub const LOST_AFTER: u32 = 3;
/// How often a lost card is started again (ms): one that is not back yet,
/// or not quite in, is tried again this much later.
pub const RETRY_MS: u64 = 3000;
/// How many times a card is mounted anew in one run. Each new mount keeps
/// the old one's memory (about 13 KB, its file system stays allocated:
/// #151); past this the storage fails until PaperPaw starts again.
pub const REMOUNTS: u8 = 1;
/// After this long (ms) without a use of the storage, the next one looks
/// at the card first (see the module).
pub const PROBE_MS: u64 = 2000;
/// How many times a card that started again is tried without a file
/// system PaperPaw can read on it, before that is given up: a card not
/// quite in, or one PaperPaw cannot read.
pub const MOUNT_TRIES: u8 = 10;

/// A card that can be started again, as after it was put in: what its
/// driver does at the start, its size read anew.
pub trait Restart: Blocks {
    /// Starts the card as if it had just been put in.
    fn restart(&mut self) -> Result<(), DiskError>;
}

/// The card and what is known of it.
struct Slot<B> {
    card: B,
    /// The mount whose blocks reach the card.
    generation: u32,
    /// Reads and writes in a row that failed.
    failures: u32,
    /// Taken out: nothing reaches the card until it is started again.
    lost: bool,
}

impl<B: Blocks> Slot<B> {
    /// Reads the card's first block (twice at most: a card that is there
    /// answers the second time); lost where it does not answer.
    fn probe(&mut self) -> bool {
        if !self.lost {
            let mut block = [0u8; BLOCK];
            let first = self.card.read_block(0, &mut block);
            if first.is_err() && self.card.read_block(0, &mut block).is_err() {
                self.lost = true;
            }
        }
        self.lost
    }
}

/// The card's slot as the storage looks at it: when it last did, by the
/// shell's clock.
struct Looked<B> {
    slot: Rc<RefCell<Slot<B>>>,
    clock: fn() -> u64,
    last_ms: core::cell::Cell<Option<u64>>,
}

impl<B: Blocks> Looked<B> {
    /// Looks at the card where the storage was not used for
    /// [`PROBE_MS`].
    fn before_use(&self) {
        let now = (self.clock)();
        let due = self
            .last_ms
            .get()
            .is_none_or(|last| now.saturating_sub(last) >= PROBE_MS);
        self.last_ms.set(Some(now));
        if due && let Ok(mut slot) = self.slot.try_borrow_mut() {
            slot.probe();
        }
    }
}

/// What the storage does before each use: [`Looked::before_use`].
trait BeforeUse {
    fn before_use(&self);
}

impl<B: Blocks> BeforeUse for Looked<B> {
    fn before_use(&self) {
        Looked::before_use(self);
    }
}

/// The card's blocks for one mount: they fail without reaching the card
/// once it is lost or mounted anew.
pub struct Watched<B> {
    slot: Rc<RefCell<Slot<B>>>,
    generation: u32,
}

impl<B> core::fmt::Debug for Watched<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Watched")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl<B: Blocks> Watched<B> {
    /// `op` on the card, where it is this mount's and not lost; a failure
    /// counted, a success ends the run of failures.
    fn on_card<T>(&self, op: impl FnOnce(&mut B) -> Result<T, DiskError>) -> Result<T, DiskError> {
        let mut slot = self.slot.try_borrow_mut().map_err(|_| DiskError)?;
        if slot.lost || slot.generation != self.generation {
            return Err(DiskError);
        }
        match op(&mut slot.card) {
            Ok(value) => {
                slot.failures = 0;
                Ok(value)
            }
            Err(e) => {
                slot.failures = slot.failures.saturating_add(1);
                if slot.failures >= LOST_AFTER {
                    slot.lost = true;
                }
                Err(e)
            }
        }
    }
}

impl<B: Blocks> Blocks for Watched<B> {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.on_card(|card| card.read_block(index, block))
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.on_card(|card| card.write_block(index, block))
    }

    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        self.on_card(|card| card.read_blocks(index, data))
    }

    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        self.on_card(|card| card.write_blocks(index, data))
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        self.on_card(|card| card.blocks())
    }
}

/// The storage the kernel holds: the card's current mount, changed by
/// [`CardSwap::check`]; the card looked at first after a pause.
pub struct Swappable {
    storage: Rc<RefCell<Box<dyn Storage>>>,
    looked: Box<dyn BeforeUse>,
}

impl core::fmt::Debug for Swappable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Swappable").finish_non_exhaustive()
    }
}

impl Swappable {
    /// `with` on the current mount; while it is being changed (never
    /// during a call of the kernel's), `or`.
    fn with<T>(&self, or: T, with: impl FnOnce(&dyn Storage) -> T) -> T {
        self.looked.before_use();
        match self.storage.try_borrow() {
            Ok(storage) => with(storage.as_ref()),
            Err(_) => or,
        }
    }
}

const IO: Result<(), StorageError> = Err(StorageError::Io);

impl Storage for Swappable {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.with(Err(StorageError::Io), |s| s.read_at(path, offset, buf))
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.with(Err(StorageError::Io), |s| s.size(path))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.with(IO, |s| s.write(path, data))
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.with(IO, |s| s.append(path, data))
    }

    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        self.with(IO, |s| s.write_at(path, offset, data))
    }

    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.with(IO, |s| s.truncate(path, len))
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.with(IO, |s| s.sync())
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.with(IO, |s| s.list(dir, visit))
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.with(IO, |s| s.remove(path))
    }

    fn modified(&self, path: &str) -> Option<i64> {
        self.with(None, |s| s.modified(path))
    }

    fn exists(&self, path: &str) -> bool {
        self.with(false, |s| s.exists(path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.with(IO, |s| s.rename(from, to))
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.with(IO, |s| s.create_dir(path))
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.with(IO, |s| s.remove_dir_all(path))
    }

    fn ignores_case(&self) -> bool {
        self.with(true, |s| s.ignores_case())
    }

    fn free_space(&self) -> Option<u64> {
        self.with(None, |s| s.free_space())
    }

    fn total_space(&self) -> Option<u64> {
        self.with(None, |s| s.total_space())
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.with((), |s| s.local_path(path, with));
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.with(Err(StorageError::Io), |s| s.list_from(dir, from, visit))
    }
}

/// The storage of a card lost for this run: everything fails with
/// [`StorageError::CARD_GONE`].
struct Gone;

const GONE: Result<(), StorageError> = Err(StorageError::CARD_GONE);

impl Storage for Gone {
    fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
        Err(StorageError::CARD_GONE)
    }

    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Err(StorageError::CARD_GONE)
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        GONE
    }

    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        GONE
    }

    fn write_at(&self, _: &str, _: u64, _: &[u8]) -> Result<(), StorageError> {
        GONE
    }

    fn truncate(&self, _: &str, _: u64) -> Result<(), StorageError> {
        GONE
    }

    fn sync(&self) -> Result<(), StorageError> {
        GONE
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        GONE
    }

    fn remove(&self, _: &str) -> Result<(), StorageError> {
        GONE
    }

    fn exists(&self, _: &str) -> bool {
        false
    }

    fn rename(&self, _: &str, _: &str) -> Result<(), StorageError> {
        GONE
    }

    fn create_dir(&self, _: &str) -> Result<(), StorageError> {
        GONE
    }

    fn remove_dir_all(&self, _: &str) -> Result<(), StorageError> {
        GONE
    }

    fn list_from(
        &self,
        _: &str,
        _: u64,
        _: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        Err(StorageError::CARD_GONE)
    }
}

/// What [`CardSwap::check`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Swapped {
    /// Nothing: the card is there (or it is not time to try again).
    No,
    /// The card was lost and is mounted anew: the shell tells the kernel.
    Remounted,
    /// The card is lost and could not be started or mounted (not back
    /// yet): tried again later.
    Waiting,
    /// The card is lost for this run: mounted anew as often as allowed
    /// ([`REMOUNTS`]), or nothing PaperPaw reads on it. The storage fails
    /// until PaperPaw starts again.
    GivenUp,
}

/// Mounts a card's blocks as storage; else why not.
pub type Mount<B, E> = fn(Watched<B>) -> Result<Box<dyn Storage>, E>;

/// A card that may be taken out and put back, and the storage mounted on
/// it (see the module).
pub struct CardSwap<B, E> {
    slot: Rc<RefCell<Slot<B>>>,
    storage: Rc<RefCell<Box<dyn Storage>>>,
    mount: Mount<B, E>,
    remounts_left: u8,
    mount_tries_left: u8,
    /// When a lost card is started again next (ms).
    next_try_ms: u64,
}

impl<B, E> core::fmt::Debug for CardSwap<B, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CardSwap")
            .field("remounts_left", &self.remounts_left)
            .finish_non_exhaustive()
    }
}

impl<B: Restart + 'static, E> CardSwap<B, E> {
    /// The started `card`, mounted with `mount`, and the storage for the
    /// kernel, which reads `clock` (ms) to know when to look at the card;
    /// else why not (the card dropped).
    pub fn mount(card: B, mount: Mount<B, E>, clock: fn() -> u64) -> Result<(Self, Swappable), E> {
        let slot = Rc::new(RefCell::new(Slot {
            card,
            generation: 0,
            failures: 0,
            lost: false,
        }));
        let storage = mount(Watched {
            slot: slot.clone(),
            generation: 0,
        })?;
        let storage = Rc::new(RefCell::new(storage));
        let swap = Self {
            slot,
            storage: storage.clone(),
            mount,
            remounts_left: REMOUNTS,
            mount_tries_left: MOUNT_TRIES,
            next_try_ms: 0,
        };
        let looked = Looked {
            slot: swap.slot.clone(),
            clock,
            last_ms: core::cell::Cell::new(Some(clock())),
        };
        let storage = Swappable {
            storage,
            looked: Box::new(looked),
        };
        Ok((swap, storage))
    }

    /// Whether the card counts as taken out.
    pub fn lost(&self) -> bool {
        self.slot.try_borrow().map_or(true, |slot| slot.lost)
    }

    /// Looks at the card once (its first block, read past every cache),
    /// e.g. on waking: one taken out or put back meanwhile counts as lost
    /// at once (a card that was there answers a second time). `true` if
    /// it is lost.
    pub fn probe(&mut self) -> bool {
        self.slot
            .try_borrow_mut()
            .map_or(true, |mut slot| slot.probe())
    }

    /// Where the card is lost, starts it again and mounts it anew (at most
    /// every [`RETRY_MS`], `now_ms` the shell's clock). Call it between the
    /// kernel's steps, never while the kernel uses the storage.
    pub fn check(&mut self, now_ms: u64) -> Swapped {
        if !self.lost() {
            return Swapped::No;
        }
        if self.remounts_left == 0 || self.mount_tries_left == 0 {
            return self.give_up();
        }
        if now_ms < self.next_try_ms {
            return Swapped::Waiting;
        }
        self.next_try_ms = now_ms.saturating_add(RETRY_MS);
        let Some(generation) = self.restart() else {
            return Swapped::Waiting;
        };
        let watched = Watched {
            slot: self.slot.clone(),
            generation,
        };
        let Ok(storage) = (self.mount)(watched) else {
            // Started, but nothing PaperPaw reads on it (yet): lost again,
            // so that the mount that failed reaches the card no more.
            if let Ok(mut slot) = self.slot.try_borrow_mut() {
                slot.lost = true;
            }
            self.mount_tries_left -= 1;
            return if self.mount_tries_left == 0 {
                self.give_up()
            } else {
                Swapped::Waiting
            };
        };
        let Ok(mut current) = self.storage.try_borrow_mut() else {
            if let Ok(mut slot) = self.slot.try_borrow_mut() {
                slot.lost = true;
            }
            return Swapped::Waiting;
        };
        // The old mount goes; what it writes as it goes reaches no card.
        let old = core::mem::replace(&mut *current, storage);
        drop(current);
        drop(old);
        self.remounts_left -= 1;
        self.mount_tries_left = MOUNT_TRIES;
        Swapped::Remounted
    }

    /// The card lost for this run: the storage says so
    /// ([`StorageError::CARD_GONE`]) to everything that uses it, so that
    /// the library can ask for a restart. The mount it replaces reaches
    /// no card (it is lost) as it goes.
    fn give_up(&mut self) -> Swapped {
        if let Ok(mut current) = self.storage.try_borrow_mut() {
            let old = core::mem::replace(&mut *current, Box::new(Gone));
            drop(current);
            drop(old);
        }
        Swapped::GivenUp
    }

    /// Moves to the next generation (the old mount's blocks reach the card
    /// no more), then starts the card again: the new generation, where it
    /// started. Lost still where it did not.
    fn restart(&mut self) -> Option<u32> {
        let mut slot = self.slot.try_borrow_mut().ok()?;
        slot.generation = slot.generation.wrapping_add(1);
        slot.failures = 0;
        slot.card.restart().ok()?;
        slot.lost = false;
        Some(slot.generation)
    }
}
