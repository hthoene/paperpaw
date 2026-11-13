//! A shell's part of the interface-7 host table (ADR 0034, M1): the process's
//! heap and clock behind [`paperpaw_native::host_glue`], and the
//! table with its context, built once for the process.
//!
//! The heap is the process's global allocator (in the emulator the budget
//! allocator, so what a plugin allocates through `heap.*` is counted against
//! the emulated device's heap budget like everything else). The other
//! entries answer `ERR_UNSUPPORTED` until the core adapters are wired.

use std::alloc::Layout;
use std::ffi::c_void;
use std::sync::OnceLock;

use paperpaw_native::host_glue::{PpHost, Services, ShellCtx, host_table};

/// The process's heap and clock.
#[derive(Debug)]
pub struct StdServices {
    start: crate::clock::Moment,
}

impl StdServices {
    /// The process's heap and clock, the clock starting now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: crate::clock::Moment::now(),
        }
    }
}

impl Default for StdServices {
    fn default() -> Self {
        Self::new()
    }
}

impl Services for StdServices {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        match Layout::from_size_align(size, align) {
            // SAFETY: the layout is valid and its size is not zero (checked by the caller).
            Ok(layout) if size != 0 => unsafe { std::alloc::alloc(layout) },
            _ => std::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        if let Ok(layout) = Layout::from_size_align(size, align) {
            // SAFETY: `ptr` is this heap's block of this layout, freed once (the trait's contract).
            unsafe { std::alloc::dealloc(ptr, layout) };
        }
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        old_size: usize,
        align: usize,
        new_size: usize,
    ) -> *mut u8 {
        // `realloc` is undefined when `new_size` rounded up to the alignment passes
        // `isize::MAX`, so the new layout is checked here too, not only by the caller.
        match (
            Layout::from_size_align(old_size, align),
            Layout::from_size_align(new_size, align),
        ) {
            // SAFETY: as `dealloc`; both layouts are valid and `new_size` is not zero.
            (Ok(layout), Ok(_)) if new_size != 0 => unsafe {
                std::alloc::realloc(ptr, layout, new_size)
            },
            _ => std::ptr::null_mut(),
        }
    }

    fn micros(&self) -> u64 {
        self.start.elapsed_micros()
    }
}

/// The host table and the `ctx` a plugin passes back with every call.
#[allow(
    missing_debug_implementations,
    reason = "the generated table has no Debug"
)]
pub struct StdHost {
    table: PpHost,
    ctx: usize,
}

impl StdHost {
    /// The table. Its entries are called with [`ctx`](Self::ctx) first.
    #[must_use]
    pub fn table(&self) -> &PpHost {
        &self.table
    }

    /// The context for the table's calls; it lives as long as the process.
    #[must_use]
    pub fn ctx(&self) -> *mut c_void {
        self.ctx as *mut c_void
    }
}

/// The process's host table, made on first use. The services and the context
/// are kept for the life of the process, as loaded plugin libraries are.
pub fn std_host() -> &'static StdHost {
    static HOST: OnceLock<StdHost> = OnceLock::new();
    HOST.get_or_init(|| {
        let services: &'static StdServices = Box::leak(Box::new(StdServices {
            start: crate::clock::Moment::now(),
        }));
        let ctx: &'static mut ShellCtx<'static> = Box::leak(Box::new(ShellCtx::new(services)));
        StdHost {
            table: host_table(),
            ctx: std::ptr::from_mut(ctx) as usize,
        }
    })
}

#[cfg(test)]
mod tests {
    use paperpaw_native::host_glue::{ERR_UNSUPPORTED, OK, PpBuffer};

    use super::*;

    #[test]
    fn the_table_allocates_reallocates_frees_and_tells_time_through_the_shell() {
        let host = std_host();
        let (t, ctx) = (host.table(), host.ctx());
        let mut block: *mut c_void = std::ptr::null_mut();
        // SAFETY: the context is the table's own; `block` is writable.
        assert_eq!(unsafe { (t.heap_alloc)(ctx, 64, 16, &mut block) }, OK);
        assert!(!block.is_null());
        assert_eq!(block as usize % 16, 0, "alignment");
        // SAFETY: 64 bytes were allocated.
        unsafe { block.cast::<u8>().write_bytes(0xa5, 64) };
        let mut grown: *mut c_void = std::ptr::null_mut();
        // SAFETY: `block` is this heap's 64-byte block of alignment 16.
        let status = unsafe { (t.heap_realloc)(ctx, block, 64, 16, 256, &mut grown) };
        assert_eq!(status, OK);
        // SAFETY: 256 bytes are allocated and the first 64 kept.
        assert_eq!(unsafe { *grown.cast::<u8>().add(63) }, 0xa5);
        // SAFETY: `grown` is this heap's 256-byte block of alignment 16, freed once.
        assert_eq!(unsafe { (t.heap_dealloc)(ctx, grown, 256, 16) }, OK);
        let (mut a, mut b) = (0u64, 0u64);
        // SAFETY: both are writable.
        unsafe {
            assert_eq!((t.clock_micros)(ctx, &mut a), OK);
            assert_eq!((t.clock_micros)(ctx, &mut b), OK);
        }
        assert!(b >= a);
    }

    #[test]
    fn what_is_not_wired_is_refused_and_the_table_is_the_same_each_time() {
        assert!(std::ptr::eq(std_host(), std_host()));
        let host = std_host();
        let mut bytes = [0u8; 8];
        let out = PpBuffer {
            ptr: bytes.as_mut_ptr(),
            cap: bytes.len(),
        };
        // SAFETY: the context is the table's own; the entry answers without reading.
        let status = unsafe { (host.table().env_snapshot)(host.ctx(), out) };
        assert_eq!(status, ERR_UNSUPPORTED);
    }
}
