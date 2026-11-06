//! The smallest interface-7 plugin for the Xteink X4: an entry, the
//! metadata record, and a plugin that does nothing. It is what
//! `paperpaw-sign pack` is tried on (format 5, `abi/ppn5.md`); see
//! `README.md`. Plugin-owned code on the published raw contract, with no
//! facade: nothing here calls the host.

#![no_std]
#![no_main]

#[rustfmt::skip]
#[path = "../../../abi/generated/paperpaw_abi.rs"]
#[allow(dead_code, reason = "the sample uses only the entry and the plugin table")]
mod abi;

use abi::{ERR_INVALID, ERR_UNSUPPORTED, MAJOR, MINOR, OK, PpBytes, PpEntry, PpHost, PpPlugin};
use core::{ffi::c_void, mem, ptr};

/// A plugin that is stopped by its host and never panics: no code here can
/// fail, so nothing is told.
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

/// The metadata record the generator wrote (`tools/abi-metadata.py`, from
/// `metadata.json`): the packer reads it back from this symbol, which is
/// exactly its length.
#[unsafe(link_section = ".rodata.paperpaw_metadata")]
#[unsafe(no_mangle)]
pub static PAPERPAW_METADATA: [u8; include_bytes!("../metadata.bin").len()] =
    *include_bytes!("../metadata.bin");

/// The entry: immutable, and with raw pointers inside, so told to be shared.
#[repr(transparent)]
pub struct Entry(PpEntry);
// SAFETY: it points only to immutable metadata and a function.
unsafe impl Sync for Entry {}

#[unsafe(link_section = ".rodata.paperpaw_entry")]
#[unsafe(no_mangle)]
pub static PAPERPAW_ENTRY: Entry = Entry(PpEntry {
    size: mem::size_of::<PpEntry>() as u32,
    abi_major: MAJOR,
    needs_minor: MINOR,
    metadata: PpBytes {
        ptr: PAPERPAW_METADATA.as_ptr(),
        len: PAPERPAW_METADATA.len(),
    },
    make: Some(make),
});

#[repr(transparent)]
struct Plugin(PpPlugin);
// SAFETY: it holds no state and points only to a function.
unsafe impl Sync for Plugin {}

/// The plugin: no state, nothing to dispatch, a `drop` that does nothing.
static PLUGIN: Plugin = Plugin(PpPlugin {
    size: mem::size_of::<PpPlugin>() as u32,
    state: ptr::null_mut(),
    drop: Some(drop_plugin),
    dispatch_event: None,
    draw: None,
});

/// The plugin's `drop`: nothing to free.
unsafe extern "C" fn drop_plugin(_state: *mut c_void) {}

/// Whether the host table is readable as far as its version. Hot code (the
/// SDK's `hot!` puts a function in this section): the packer finds it between
/// `PAPERPAW_HOT` and `PAPERPAW_HOT_END` and patches the calls that reach
/// across its edge. `make` calls it, so the file has patches to relocate.
#[inline(never)]
#[unsafe(link_section = ".text.paperpaw_hot.table_ok")]
extern "C" fn table_ok(size: u32) -> bool {
    size as usize >= mem::offset_of!(PpHost, heap_alloc)
}

/// The factory (`abi/entry.md`): reads the host table's size first, as a
/// short table must not be read as the full one, and gives the plugin.
unsafe extern "C" fn make(host: *const PpHost, _ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    if out.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: the host gives valid, writable storage for the output.
    unsafe { out.write(ptr::null()) };
    if host.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: the host's table is readable as far as its own `size` says,
    // and `size` is its first field.
    let size = unsafe { ptr::addr_of!((*host).size).read() };
    if !table_ok(size) {
        return ERR_INVALID;
    }
    // SAFETY: `major` lies within the `size` bytes just checked.
    if unsafe { ptr::addr_of!((*host).major).read() } != MAJOR {
        return ERR_UNSUPPORTED;
    }
    // SAFETY: as above, the output is writable.
    unsafe { out.write(&PLUGIN.0) };
    OK
}
