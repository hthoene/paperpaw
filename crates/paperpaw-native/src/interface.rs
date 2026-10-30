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
/// The bytes of an [`Entry`] on the chips plugin files are for.
pub(crate) const ENTRY_LEN: usize = 16;

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
    /// How many entries of the firmware's shared function table
    /// ([`Api::shared`]) the plugin was built for: the loader refuses it
    /// where the firmware's table is shorter, before anything of it runs
    /// (`shared::NEEDS` of the SDK that built it).
    pub needs: u32,
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
    /// The firmware's own copies of shared functions, as addresses in the
    /// order of [`shared::index`](crate::shared::index): a plugin built
    /// with `--cfg paperpaw_plugin` for a chip calls them instead of
    /// carrying its own copy. Null where the shell has none (a computer,
    /// Android: plugins keep their own there).
    pub shared: crate::shared::Table,
    /// How many addresses [`Api::shared`] holds.
    pub shared_len: u32,
}

impl Api {
    /// The size of this crate's table.
    pub const SIZE: u32 = core::mem::size_of::<Api>() as u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_is_as_long_as_the_format_says() {
        // The chips are 32-bit. On a computer pointers are wider, and the
        // entry is longer; the fields lie where the chips have them as far
        // as the format reads them (fingerprint, then `make`).
        if core::mem::size_of::<usize>() == 4 {
            assert_eq!(core::mem::size_of::<Entry>(), ENTRY_LEN);
            assert_eq!(core::mem::offset_of!(Entry, needs), 12);
        }
        assert_eq!(core::mem::align_of::<Entry>(), 8);
    }
}
