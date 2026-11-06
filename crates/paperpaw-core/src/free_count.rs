//! The card's free space, counted while the kernel is idle (Senior's
//! remark 1 on PR #188).
//!
//! A FAT storage knows its free space only once it has counted its FAT,
//! a part at each ask (`paperpaw-fat`, never the count the card keeps of
//! itself): until then a plugin's reserve is not kept, and each of its
//! writes would pay for a part of the count. So from the start, and again
//! on waking and after the card was a USB drive (the card may be another,
//! counted from its start), the kernel asks for it in its ticks, one part
//! at a time with a pause between for input and screens, until it is
//! known. Never while a press waits to be told single or double, nor
//! shortly before the device goes to sleep by itself; and not on a storage
//! that cannot say how large it is (it counts nothing).

use paperpaw_platform::Instant;

use crate::Kernel;

/// The first part counted after the start or waking, not in the frames
/// it shows first.
const AFTER_START_MS: u64 = 1_000;
/// Between two parts: input and screens run between.
const PAUSE_MS: u64 = 50;
/// Not within this of going to sleep by itself: the count goes on after
/// waking.
const BEFORE_SLEEP_MS: u64 = 2_000;
/// Parts at most each time the count is started: a FAT of 256 MiB (a
/// card of 2 TB in clusters of 32 KiB) takes 2048 of 128 KiB. A storage
/// that never knows is not asked for ever.
const MAX_PARTS: u32 = 4096;

/// The count the kernel still drives.
#[derive(Debug, Default)]
pub(crate) struct FreeCount {
    /// When to ask for the next part; `None` when known, or not counted.
    next: Option<Instant>,
    /// Parts asked for since it was started.
    parts: u32,
}

impl FreeCount {
    /// When the kernel must tick for it, at the latest.
    pub(crate) fn due(&self) -> Option<Instant> {
        self.next
    }

    /// No card reads until the host gives the card back.
    pub(crate) fn pause_for_usb(&mut self) {
        self.next = None;
    }
}

impl Kernel {
    /// Starts the count anew (at the start, on waking, after the card was
    /// a USB drive), where the storage can tell how large it is.
    pub(crate) fn count_free_again(&mut self, now: Instant) {
        self.free_count.parts = 0;
        self.free_count.next = self
            .shared
            .storage
            .total_space()
            .map(|_| now.add_millis(AFTER_START_MS));
    }

    /// Asks for one part of the count, where it is due (see the [module
    /// documentation](self)).
    pub(crate) fn count_free(&mut self, now: Instant) {
        if self.free_count.next.is_none_or(|at| at > now) {
            return;
        }
        if self.shared.storage.total_space().is_none() {
            self.free_count.next = None;
            return;
        }
        let sleep_due = self.sleep_due();
        let sleeps_soon = sleep_due.is_some_and(|due| due <= now.add_millis(BEFORE_SLEEP_MS));
        if self.input.pending.is_some() || sleeps_soon || self.sleep {
            self.free_count.next = sleep_due;
            return;
        }
        self.free_count.parts += 1;
        let known = self.shared.storage.free_space().is_some();
        self.free_count.next =
            (!known && self.free_count.parts < MAX_PARTS).then(|| now.add_millis(PAUSE_MS));
    }
}
