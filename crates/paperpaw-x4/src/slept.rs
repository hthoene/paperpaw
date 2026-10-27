//! A sleep that ends in a start: the X4s cut their power to sleep (or
//! enter a deep sleep that restarts the chip), so waking is a start of the
//! firmware, which plugins would hear as `core.started` alone. The
//! firmware therefore leaves a mark on the card as it goes to sleep, and
//! the next start takes it: that start ends a sleep, and tells the kernel
//! so (`KernelBuilder::after_sleep`, which sends `core.resume` too). A
//! power-up from nothing, by the cable or a battery put in, leaves no mark
//! and is no waking.

use paperpaw_platform::Storage;

/// The mark's file, in PaperPaw's folder on the card.
pub const FILE: &str = ".paperpaw/slept";

/// Marks that the device goes to sleep: written (and kept) before its
/// power goes. Not being able to is no failure of the sleep: the wake
/// then is a start like another.
pub fn mark(storage: &dyn Storage) {
    // The folder is there once PaperPaw has run; a card without it has
    // had nothing written to it yet.
    let _ = storage.create_dir(".paperpaw");
    if storage.write(FILE, b"").is_ok() {
        let _ = storage.sync();
    }
}

/// Whether the last run went to sleep (it left the mark): the mark is
/// removed, so that only the start after that sleep finds it.
pub fn take(storage: &dyn Storage) -> bool {
    storage.exists(FILE) && storage.remove(FILE).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_core::sdk::MemoryStorage;

    #[test]
    fn a_start_after_a_sleep_finds_the_mark_once() {
        let storage = MemoryStorage::new();
        assert!(!take(&storage), "no sleep before");
        mark(&storage);
        assert!(storage.exists(FILE));
        assert!(take(&storage), "the start that ends the sleep");
        assert!(!storage.exists(FILE));
        assert!(!take(&storage), "the next one is no waking");
    }

    #[test]
    fn marking_twice_is_one_sleep() {
        let storage = MemoryStorage::new();
        mark(&storage);
        mark(&storage);
        assert!(take(&storage));
        assert!(!take(&storage));
    }
}
