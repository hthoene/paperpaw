//! What crosses between the firmware and a plugin before the SDK's types
//! do: the plugin's [`Entry`], and the firmware's [`Api`], the few things
//! a plugin cannot bring itself.

use core::ffi::c_void;

/// The symbol of a plugin's [`Entry`], a static in [`ENTRY_SECTION`].
pub const ENTRY_SYMBOL: &str = "PAPERPAW_ENTRY";
/// The section a plugin's [`Entry`] is put in: the first of its image.
pub const ENTRY_SECTION: &str = ".rodata.paperpaw_entry";
/// The symbol of the word that is to hold the address of the firmware's
/// [`Api`]; `paperpaw-plugin.x` makes it, a plugin reads it as an extern
/// static.
pub const API_SYMBOL: &str = "PAPERPAW_API";
/// The symbols of the words `paperpaw-plugin.x` makes for the families
/// of shared functions (`paperpaw_table`), by family number: the loader
/// writes the address of the firmware's table of each into them.
pub const TABLE_SYMBOLS: [&str; 8] = [
    "PAPERPAW_TABLE_0",
    "PAPERPAW_TABLE_1",
    "PAPERPAW_TABLE_2",
    "PAPERPAW_TABLE_3",
    "PAPERPAW_TABLE_4",
    "PAPERPAW_TABLE_5",
    "PAPERPAW_TABLE_6",
    "PAPERPAW_TABLE_7",
];
/// The bytes of an [`Entry`] on the chips plugin files are for.
pub(crate) const ENTRY_LEN: usize = 32;

/// What a plugin exports, at [`Header::entry`](crate::Header::entry).
#[repr(C)]
#[derive(Debug)]
pub struct Entry {
    /// The plugin's fingerprint, as its file's header says (the packer
    /// copies it from here).
    pub fingerprint: u64,
    /// Makes the plugin. What it returns is the SDK's to read (its
    /// `native::take`): the firmware owns it.
    pub make: unsafe extern "C" fn() -> *mut c_void,
    /// The bytes of the firmware's [`Api`] the plugin may use: the
    /// [`Api::SIZE`] of the SDK that built it. A firmware whose table is
    /// shorter does not load it.
    pub needs_api: u16,
    /// The plugin interface the plugin was built for (the SDK's
    /// `native::INTERFACE`): the packer copies it to the file's header, so
    /// a firmware can say "older" or "newer" rather than "another build".
    pub interface: u16,
    /// The revision of each family of shared functions the SDK that built
    /// the plugin has (`paperpaw_table::REVISIONS`). The packer copies
    /// those of the families the plugin's code uses to the header.
    pub families: [u16; 8],
}

impl Entry {
    /// The fingerprint at `entry`, read before the rest: an entry of
    /// another interface may be shorter than this one, so a loader that
    /// has an entry's address reads its fingerprint through this and
    /// takes the whole [`Entry`] only where it is its own.
    ///
    /// # Safety
    ///
    /// `entry` must point to at least 8 readable bytes, aligned to 8 (the
    /// fingerprint every interface's entry starts with).
    #[must_use]
    pub unsafe fn fingerprint_at(entry: *const Entry) -> u64 {
        // SAFETY: the caller's contract.
        unsafe { entry.cast::<u64>().read() }
    }
}

/// What the firmware gives every plugin: its heap (every allocation is
/// the kernel's, so what one side boxes the other may drop), its panics,
/// its clock, and what plugins would otherwise each carry a copy of. It
/// grows only at its end, and says its size.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Api {
    /// This table's bytes ([`Api::SIZE`] of the firmware's build).
    pub size: u32,
    /// Allocates as `GlobalAlloc::alloc` does; null when there is no
    /// room.
    pub alloc: unsafe extern "C" fn(size: usize, align: usize) -> *mut u8,
    /// Frees what `alloc` or `realloc` gave, with the same size and
    /// alignment.
    pub dealloc: unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize),
    /// Resizes as `GlobalAlloc::realloc` does.
    pub realloc:
        unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8,
    /// The plugin panicked, saying why (UTF-8): the firmware's own panic,
    /// which keeps the message and starts the device again.
    pub panic: unsafe extern "C" fn(message: *const u8, len: usize) -> !,
    /// Microseconds since the device started.
    pub micros: extern "C" fn() -> u64,
    /// The character of the icon named `name` (UTF-8, `len` bytes) in the
    /// icon font, 0 if there is none: the names of all icons (some 125 KB)
    /// stay in the firmware (the SDK's `icons::char` in a plugin built
    /// with `--cfg paperpaw_plugin`).
    pub icon: unsafe extern "C" fn(name: *const u8, len: usize) -> u32,
    /// Allocates as `alloc` does, where `placement` says: 1 for data read
    /// all the time (internal RAM), 2 for much data (PSRAM), where the
    /// device has both; else anywhere. Null when there is no room. Freed
    /// with `dealloc`.
    pub alloc_in: unsafe extern "C" fn(size: usize, align: usize, placement: u32) -> *mut u8,
    /// The revision of each family of shared functions the firmware has
    /// (`paperpaw_table::Revisions`), 0 for a family it does not share.
    pub family_revs: [u16; 8],
    /// The oldest revision of each family whose plugins the firmware still
    /// runs: a plugin built with an older one is refused.
    pub family_min: [u16; 8],
    /// The firmware's version and the hash of its tables, UTF-8,
    /// `build_len` bytes: for logs and crash reports.
    pub build: *const u8,
    /// The bytes of `build`.
    pub build_len: u32,
    // Fields are only appended after `build_len`, the last of interface 5:
    // such a field is free (the interface test), and a plugin that uses
    // one says so (`needs_api`), so it is refused where `size` lacks it.
}

// SAFETY: function pointers and a pointer to a constant string of the
// firmware, never written: shared freely between threads.
unsafe impl Sync for Api {}
// SAFETY: as above.
unsafe impl Send for Api {}

impl Api {
    /// The size of this crate's table.
    pub const SIZE: u32 = core::mem::size_of::<Api>() as u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_is_as_long_as_the_format_says() {
        // The chips are 32-bit; on a computer `make` is 8 bytes, and the
        // fingerprint aligns the entry to 8.
        let make = core::mem::size_of::<usize>() - 4;
        assert_eq!(
            core::mem::size_of::<Entry>(),
            (ENTRY_LEN + make).next_multiple_of(8)
        );
        assert_eq!(core::mem::align_of::<Entry>(), 8);
        assert_eq!(
            core::mem::offset_of!(Entry, families),
            ENTRY_LEN - 16 + make
        );
    }
}
