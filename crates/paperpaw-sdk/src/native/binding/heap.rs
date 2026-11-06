//! The plugin side of the heap word (`abi/heap-word.md`): checked reads of the
//! host's context-free `PpHeap` table, the allocation calls behind the global
//! allocator `native_plugin!` installs, and its panic path. Implementation
//! details for the macro; not an application API.
//!
//! The table is read through the generated checked readers (`read_heap_*`):
//! alignment, major and `size` first, never as a reference to the whole
//! `PpHeap`, since a host built for an older minor may give a shorter one.
//! Nothing is cached: a plugin has no writable static to cache in, and the
//! checks are two loads.
use core::alloc::Layout;
use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::ptr;
pub use paperpaw_native::host_glue::PpHeap;
use paperpaw_native::host_glue::{
    HEAP_ALIAS, HEAP_SECTION, HEAP_SYMBOL, MAX_ALIGN, read_heap_abort, read_heap_alloc,
    read_heap_dealloc, read_heap_realloc,
};

/// `abort` code of a panic.
pub const ABORT_PANIC: i32 = 1;
/// `abort` code of a failed infallible allocation (`handle_alloc_error`).
pub const ABORT_ALLOCATION: i32 = 2;
/// How many bytes of a panic message reach the host.
pub const ABORT_MESSAGE_BYTES: usize = 96;

/// The names the macro's assembly uses, as the generated contract has them
/// (a test keeps them equal).
pub const NAMES: (&str, &str, &str) = (HEAP_SYMBOL, HEAP_ALIAS, HEAP_SECTION);

/// `alloc` through the table: null where the table, the entry or the host
/// refuses, or the alignment is above `MAX_ALIGN`.
///
/// # Safety
///
/// As [`GlobalAlloc::alloc`](core::alloc::GlobalAlloc::alloc), and `table` as
/// for the heap word.
pub unsafe fn alloc(table: *const PpHeap, layout: Layout) -> *mut u8 {
    if layout.align() > MAX_ALIGN {
        return ptr::null_mut();
    }
    // SAFETY: as documented.
    match unsafe { read_heap_alloc(table) } {
        // SAFETY: the host's allocator with a valid layout.
        Some(alloc) => unsafe { alloc(layout.size(), layout.align()) }.cast(),
        None => ptr::null_mut(),
    }
}

/// `dealloc` through the table. A table without the entry cannot have given
/// the block (same minor), so nothing is freed then.
///
/// # Safety
///
/// As [`GlobalAlloc::dealloc`](core::alloc::GlobalAlloc::dealloc), and `table`
/// as for the heap word.
pub unsafe fn dealloc(table: *const PpHeap, block: *mut u8, layout: Layout) {
    // SAFETY: as documented.
    if let Some(dealloc) = unsafe { read_heap_dealloc(table) } {
        // SAFETY: a block of this heap with the layout it was made with.
        unsafe { dealloc(block.cast(), layout.size(), layout.align()) };
    }
}

/// `realloc` through the table; null leaves the block as it was.
///
/// # Safety
///
/// As [`GlobalAlloc::realloc`](core::alloc::GlobalAlloc::realloc), and
/// `table` as for the heap word.
pub unsafe fn realloc(
    table: *const PpHeap,
    block: *mut u8,
    layout: Layout,
    new_size: usize,
) -> *mut u8 {
    // SAFETY: as documented.
    match unsafe { read_heap_realloc(table) } {
        // SAFETY: a block of this heap with its layout, and a nonzero size
        // that fits the alignment (the caller's contract).
        Some(realloc) => unsafe {
            realloc(block.cast(), layout.size(), layout.align(), new_size).cast()
        },
        None => ptr::null_mut(),
    }
}

/// The host's `abort`, where the table is usable and has it.
///
/// # Safety
///
/// `table` as for the heap word.
unsafe fn abort_entry(
    table: *const PpHeap,
) -> Option<unsafe extern "C" fn(i32, *const u8, usize) -> !> {
    // SAFETY: as documented.
    unsafe { read_heap_abort(table) }
}

/// The first bytes of a panic message, cut at a character boundary.
#[derive(Debug)]
pub struct Message {
    bytes: [u8; ABORT_MESSAGE_BYTES],
    len: usize,
}

impl Message {
    /// Empty.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; ABORT_MESSAGE_BYTES],
            len: 0,
        }
    }

    /// What was kept.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only whole characters are copied in, so this never fails.
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }

    /// The `abort` code for this message: a failed allocation says so.
    #[must_use]
    pub fn code(&self) -> i32 {
        if self.as_str().starts_with("memory allocation of") {
            ABORT_ALLOCATION
        } else {
            ABORT_PANIC
        }
    }
}

impl Default for Message {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for Message {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for c in text.chars() {
            let mut utf8 = [0; 4];
            let piece = c.encode_utf8(&mut utf8).as_bytes();
            let Some(to) = self.bytes.get_mut(self.len..self.len + piece.len()) else {
                // Full: the rest is dropped, and formatting stops.
                return Err(fmt::Error);
            };
            to.copy_from_slice(piece);
            self.len += piece.len();
        }
        Ok(())
    }
}

/// The panic path: the host's `abort` with the message's first bytes, or the
/// profile's trap where there is no usable heap word (`abi/heap-word.md` 6).
/// Allocates nothing.
///
/// # Safety
///
/// `table` as for the heap word: the value read from `PAPERPAW_HEAP`.
pub unsafe fn panic(table: *const PpHeap, info: &PanicInfo<'_>) -> ! {
    let mut message = Message::new();
    // A cut message is still worth sending.
    let _ = write!(message, "{}", info.message());
    // SAFETY: as documented.
    match unsafe { abort_entry(table) } {
        // SAFETY: the host's abort; the message stays readable for the call.
        Some(abort) => unsafe { abort(message.code(), message.bytes.as_ptr(), message.len) },
        None => trap(),
    }
}

/// The profile's trap instruction: the crash slot blames the running plugin.
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
pub fn trap() -> ! {
    // SAFETY: `unimp` raises an illegal-instruction exception; nothing returns.
    unsafe { core::arch::asm!("unimp", options(noreturn, nomem, nostack)) }
}

/// The profile's trap instruction: the crash slot blames the running plugin.
#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub fn trap() -> ! {
    // SAFETY: `ud2` raises an invalid-opcode exception; nothing returns.
    unsafe { core::arch::asm!("ud2", options(noreturn, nomem, nostack)) }
}

/// A target without a profile trap: the plugin stops here.
#[cfg(not(any(
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "x86_64",
    target_arch = "x86"
)))]
pub fn trap() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
#[path = "heap_tests.rs"]
mod tests;
