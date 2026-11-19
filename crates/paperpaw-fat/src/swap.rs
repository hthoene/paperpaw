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
//! then starts the card again and mounts it anew (nothing the old mount
//! kept is used, whatever card it is), and the storage the kernel holds
//! ([`Swappable`]) reads and writes the new mount. The shell tells the
//! kernel (`Kernel::usb_drive_ended`), which walks the library again,
//! unless the card is the one it was, as it was left (below).
//!
//! Each mount is a generation: the blocks of a mount ([`Watched`]) carry
//! theirs, and only the current one reaches the card. A remount moves to
//! the next generation before the card is started again, so the old
//! mount's blocks fail from then on, whatever still holds them (a file
//! system writing back as it is dropped, a cache).
//!
//! The old mount is dropped as the new one takes its place, with all it
//! holds (its file system, open files, folders, the disk's blocks and the
//! free space it counted: [`crate::mount`] owns them), so a card may be
//! swapped as often as it is: every mount takes the memory the first took
//! (#151).
//!
//! A card may count as lost that never left: reads that failed in a row
//! with the card in (a bus disturbed, a card busy). A card taken out
//! answers nothing until it is started again, so a lost card is first
//! asked as it is: one that answers never left, and its mount stays, with
//! all it keeps ([`Swapped::Kept`]). One that does not is let go for good.
//!
//! A card that lost its power and has it back (a loose contact, a card
//! pushed in again) is mounted anew, as it must be, but need not be
//! another card: what tells a card is its volume's boot sector (its serial
//! number, its sizes), and what tells that nothing else wrote it is its FS
//! information sector (FAT32: a computer that takes clusters writes its
//! count or its hint there), both as they last passed to or from the card.
//! The same card found as it was left is [`Swapped::Back`]: the shell
//! walks no library for it and the device sleeps when it would have. What
//! this cannot see is a computer that only removed files, on FAT32, and
//! wrote no count; and a FAT12 or FAT16 card has no such sector, so it
//! always counts as changed. The same card found again and again is left
//! longer each time before it is mounted anew ([`AGAIN_MS`]); another card
//! is mounted at once.

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
/// After this long (ms) without a use of the storage, the next one looks
/// at the card first (see the module).
pub const PROBE_MS: u64 = 2000;
/// How many times a card that started again is tried without a file
/// system PaperPaw can read on it, before that is given up: a card not
/// quite in, or one PaperPaw cannot read.
pub const MOUNT_TRIES: u8 = 10;
/// How long (ms) the same card, mounted anew, is left before it is mounted
/// anew again: by how many times in a row it was found again, the last
/// from then on. Another card in its place waits as long.
pub const AGAIN_MS: [u64; 4] = [RETRY_MS, 10_000, 30_000, 60_000];
/// A mount that held this long (ms) ends such a row.
pub const CALM_MS: u64 = 120_000;

/// A card that can be started again, as after it was put in: what its
/// driver does at the start, its size read anew.
pub trait Restart: Blocks {
    /// Starts the card as if it had just been put in.
    fn restart(&mut self) -> Result<(), DiskError>;
}

/// What tells the card of a mount from another, and from itself written
/// elsewhere: its volume's boot sector and FS information sector, hashed
/// as the card holds them (as they last passed to or from it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Seen {
    /// The boot sector; `None` before a volume was found.
    boot: Option<u32>,
    /// The block of the FS information sector (FAT32); 0 for none.
    info_at: u32,
    /// That sector; `None` where it is not known (not read yet, or a
    /// write of it failed).
    info: Option<u32>,
}

impl Seen {
    /// Of the volume at block `first` with the boot sector `boot`.
    fn found(first: u32, boot: &[u8; BLOCK]) -> Self {
        // Not the bytes that say "in use" (FAT12 and FAT16 at 0x25, FAT32
        // at 0x41): written while it is mounted.
        // exFAT says it at 106 and 107, and how full it is at 112.
        let exfat = boot[3..11] == *b"EXFAT   ";
        let stable = boot
            .iter()
            .enumerate()
            .filter(|(at, _)| !matches!(at, 0x25 | 0x41) && !(exfat && matches!(at, 106 | 107 | 112)))
            .map(|(_, byte)| *byte);
        let fat32 = boot[0x52..0x55] == *b"FAT";
        let blocks = u32::from(u16::from_le_bytes([boot[11], boot[12]])) / BLOCK as u32;
        let sector = u32::from(u16::from_le_bytes([boot[48], boot[49]]));
        Self {
            boot: Some(crate::fnv(stable) as u32),
            info_at: if fat32 && sector != 0 {
                first.saturating_add(sector * blocks)
            } else {
                0
            },
            info: None,
        }
    }

    /// The blocks from `index` on passed to or from the card as `data`;
    /// not `reached` where a write of them failed.
    fn passed(&mut self, index: u32, data: &[u8], reached: bool) {
        let at = self.info_at.wrapping_sub(index) as usize;
        if self.info_at == 0 || at >= data.len() / BLOCK {
            return;
        }
        self.info = data
            .get(at * BLOCK..(at + 1) * BLOCK)
            .filter(|_| reached)
            .map(|block| crate::fnv(block.iter().copied()) as u32);
    }

    /// Whether `self` is the card `left`, by its boot sector.
    fn same_card(&self, left: &Self) -> bool {
        self.boot.is_some() && self.boot == left.boot
    }

    /// Whether `self` is the card `left` as it was left.
    fn unchanged(&self, left: &Self) -> bool {
        self.info.is_some() && self == left
    }
}

/// The card and what is known of it.
struct Slot<B> {
    card: B,
    /// What tells the card of the current mount.
    seen: Seen,
    /// The mount whose blocks reach the card.
    generation: u32,
    /// Reads and writes in a row that failed.
    failures: u32,
    /// Taken out: nothing reaches the card until it is started again.
    lost: bool,
}

impl<B: Blocks> Slot<B> {
    /// Whether the card answers a read of its first block as it is (twice
    /// at most: a card that is there answers the second time).
    #[inline(never)]
    fn answers(&mut self) -> bool {
        let mut block = [0u8; BLOCK];
        let first = self.card.read_block(0, &mut block);
        first.is_ok() || self.card.read_block(0, &mut block).is_ok()
    }

    /// Looks at the card ([`Self::answers`]); lost where it does not
    /// answer.
    fn probe(&mut self) -> bool {
        if !self.lost && !self.answers() {
            self.lost = true;
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
    fn on_card<T>(
        &self,
        op: impl FnOnce(&mut Slot<B>) -> Result<T, DiskError>,
    ) -> Result<T, DiskError> {
        let mut slot = self.slot.try_borrow_mut().map_err(|_| DiskError)?;
        if slot.lost || slot.generation != self.generation {
            return Err(DiskError);
        }
        match op(&mut slot) {
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
        self.on_card(|slot| {
            slot.card.read_block(index, block)?;
            slot.seen.passed(index, block, true);
            Ok(())
        })
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        self.on_card(|slot| {
            let written = slot.card.write_block(index, block);
            slot.seen.passed(index, block, written.is_ok());
            written
        })
    }

    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        self.on_card(|slot| {
            slot.card.read_blocks(index, data)?;
            slot.seen.passed(index, data, true);
            Ok(())
        })
    }

    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        self.on_card(|slot| {
            let written = slot.card.write_blocks(index, data);
            slot.seen.passed(index, data, written.is_ok());
            written
        })
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        self.on_card(|slot| slot.card.blocks())
    }

    fn volume_found(&mut self, first: u32, boot: &[u8; BLOCK]) {
        if let Ok(mut slot) = self.slot.try_borrow_mut()
            && slot.generation == self.generation
        {
            slot.seen = Seen::found(first, boot);
        }
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

    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        self.with(Err(StorageError::Io), |s| {
            s.remove_tree_step(root, keep_root, place)
        })
    }

    fn ignores_case(&self) -> bool {
        self.with(true, |s| s.ignores_case())
    }

    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.with(None, |s| s.same_entry(a, b))
    }

    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.with(Err(StorageError::Io), |s| s.entry_identity(path))
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

    fn list_bounded(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.with(Err(StorageError::Io), |s| s.list_bounded(dir, from, visit))
    }

    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.with(IO, |s| s.remove_listed(dir, place))
    }
}

/// The storage of a card lost for this run: everything fails with
/// [`StorageError::CARD_GONE`].
struct Gone;

const GONE: Result<(), StorageError> = Err(StorageError::CARD_GONE);

impl Storage for Gone {
    fn entry_identity(&self, _: &str) -> Result<Option<u64>, StorageError> {
        Err(StorageError::CARD_GONE)
    }

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
    /// The card counted as lost, but it answers as it is: it never left.
    /// Its mount stays, with all it keeps; there is nothing to tell.
    Kept,
    /// The card was lost and is mounted anew: another card, or this one
    /// written elsewhere, as far as can be told. The shell tells the
    /// kernel, which walks the library again.
    Remounted,
    /// The card was lost and is mounted anew, and is the card it was, as
    /// it was left (see the module): what was not written back is gone,
    /// its files are the same. No library is walked for it.
    Back,
    /// The card is lost and could not be started or mounted (not back
    /// yet): tried again later.
    Waiting,
    /// The card is lost for this run: nothing PaperPaw reads on it after
    /// [`MOUNT_TRIES`] starts. The storage fails until PaperPaw starts
    /// again.
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
    mount_tries_left: u8,
    /// When a lost card is started again next (ms).
    next_try_ms: u64,
    /// The generation of the mount the storage reads and writes.
    mounted: u32,
    /// When it was mounted (ms).
    mounted_ms: u64,
    /// What told the card of that mount when it was let go.
    left: Seen,
    /// How many times in a row the same card was found again.
    again: u8,
}

impl<B, E> core::fmt::Debug for CardSwap<B, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CardSwap")
            .field("mount_tries_left", &self.mount_tries_left)
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
            seen: Seen::default(),
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
            mount_tries_left: MOUNT_TRIES,
            next_try_ms: 0,
            mounted: 0,
            mounted_ms: clock(),
            left: Seen::default(),
            again: 0,
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

    /// Where the card is lost: kept where it answers as it is, else
    /// started again and mounted anew (at most every [`RETRY_MS`], `now_ms`
    /// the shell's clock; the same card found again and again, less often:
    /// [`AGAIN_MS`]). Call it between the kernel's steps, never while the
    /// kernel uses the storage.
    pub fn check(&mut self, now_ms: u64) -> Swapped {
        if !self.lost() {
            return Swapped::No;
        }
        if self.mount_tries_left == 0 {
            return self.give_up();
        }
        if self.still_in() {
            return Swapped::Kept;
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
            self.lose();
            self.mount_tries_left -= 1;
            return if self.mount_tries_left == 0 {
                self.give_up()
            } else {
                Swapped::Waiting
            };
        };
        let Ok(mut current) = self.storage.try_borrow_mut() else {
            self.lose();
            return Swapped::Waiting;
        };
        // The old mount goes; what it writes as it goes reaches no card.
        let old = core::mem::replace(&mut *current, storage);
        drop(current);
        drop(old);
        self.mount_tries_left = MOUNT_TRIES;
        self.mounted = generation;
        self.found(now_ms)
    }

    /// The card itself for `with`, as a USB drive (#32): the storage's
    /// mount is dropped first, which writes back what it kept and clears
    /// the volume's dirty mark, and from then on nothing reaches the card
    /// but `with`. The card then counts as lost, the next [`Self::check`]
    /// starts it again and mounts it anew (any card: a computer had it,
    /// [`Swapped::Remounted`]). Until then every use of the storage fails
    /// with [`StorageError::CARD_GONE`]. `None` where the storage or the
    /// card is in use (never between the kernel's steps). Call it between
    /// the kernel's steps, the kernel suspended and its work written back.
    pub fn lend<R>(&mut self, with: impl FnOnce(&mut B) -> R) -> Option<R> {
        let mut current = self.storage.try_borrow_mut().ok()?;
        let mut slot = self.slot.try_borrow_mut().ok()?;
        // The old mount goes first, while its blocks reach the card.
        let old = core::mem::replace(&mut *current, Box::new(Gone));
        drop(current);
        drop(old);
        // Then no mount's blocks do: the generation moves on.
        slot.generation = slot.generation.wrapping_add(1);
        let result = with(&mut slot.card);
        slot.lost = true;
        slot.failures = 0;
        self.left = Seen::default();
        self.mount_tries_left = MOUNT_TRIES;
        self.next_try_ms = 0;
        Some(result)
    }

    /// The card counts as lost (again).
    fn lose(&mut self) {
        if let Ok(mut slot) = self.slot.try_borrow_mut() {
            slot.lost = true;
        }
    }

    /// Whether the lost card answers as it is, its mount still the one
    /// that reaches it: then it never left (a card taken out answers
    /// nothing until it is started again) and is lost no more. Asked once:
    /// where it does not answer, its mount is let go for good (the next
    /// generation), what told the card kept to know it again.
    fn still_in(&mut self) -> bool {
        let Ok(mut slot) = self.slot.try_borrow_mut() else {
            return false;
        };
        if slot.generation != self.mounted {
            return false;
        }
        if slot.answers() {
            slot.lost = false;
            slot.failures = 0;
            return true;
        }
        self.left = slot.seen;
        slot.generation = slot.generation.wrapping_add(1);
        false
    }

    /// The card just mounted anew, at `now_ms`: the card that was let go,
    /// as it was left ([`Swapped::Back`]), or not. The same card is left
    /// longer before it is mounted anew again, each time in a row.
    fn found(&mut self, now_ms: u64) -> Swapped {
        let found = self.slot.try_borrow().map_or(Seen::default(), |s| s.seen);
        let calm = now_ms.saturating_sub(self.mounted_ms) >= CALM_MS;
        self.mounted_ms = now_ms;
        if !found.same_card(&self.left) {
            self.again = 0;
            return Swapped::Remounted;
        }
        self.again = if calm {
            1
        } else {
            self.again.saturating_add(1)
        };
        let step = usize::from(self.again).clamp(1, AGAIN_MS.len()) - 1;
        let wait = AGAIN_MS.get(step).copied().unwrap_or(RETRY_MS);
        self.next_try_ms = now_ms.saturating_add(wait);
        if found.unchanged(&self.left) {
            Swapped::Back
        } else {
            Swapped::Remounted
        }
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
        slot.seen = Seen::default();
        slot.card.restart().ok()?;
        slot.lost = false;
        Some(slot.generation)
    }
}
