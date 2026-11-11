//! The shells' part of the interface-7 host table (ADR 0034, M1): the
//! entries a shell supplies itself (the heap and the clock), and a table
//! whose other entries answer `ERR_UNSUPPORTED` until their adapters over
//! the kernel and graphics crates are wired (Kernel Engineer's core side).
//!
//! The table's types, the version and the status values are generated
//! (`abi/generated/paperpaw_abi.rs`); nothing is copied by hand.

use core::ffi::c_void;
use core::ptr;

pub mod buffers;

#[allow(
    dead_code,
    clippy::all,
    clippy::pedantic,
    missing_docs,
    missing_debug_implementations,
    unsafe_op_in_unsafe_fn,
    reason = "generated ABI includes entry declarations before their loader wiring"
)]
mod generated {
    include!("host_glue/paperpaw_abi.rs");
}

pub use generated::{ENTRY_SYMBOL, METADATA_SYMBOL, PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin};
// The heap word and its context-free table (`abi/heap-word.md`).
pub use generated::{
    HEAP_ALIAS, HEAP_SECTION, HEAP_SYMBOL, PpHeap, read_heap_abort, read_heap_alloc,
    read_heap_dealloc, read_heap_realloc,
};
pub use generated::{
    PLUGIN_PREFIX_BYTES, read_plugin_dispatch_event, read_plugin_draw, read_plugin_drop,
    read_plugin_input, read_plugin_size, read_plugin_state,
};

pub use generated::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY,
    ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_TIMEOUT, ERR_UNSUPPORTED, ERR_UTF8, MAJOR, MINOR,
    OK,
};

/// The largest alignment an allocation may ask for.
pub const MAX_ALIGN: usize = 4096;

/// What a shell provides to the host table.
pub trait Services {
    /// `size` bytes aligned to `align` (a power of two up to [`MAX_ALIGN`]),
    /// or null.
    fn alloc(&self, size: usize, align: usize) -> *mut u8;

    /// Frees an allocation of this allocator.
    ///
    /// # Safety
    ///
    /// `ptr` came from [`alloc`](Self::alloc) or [`realloc`](Self::realloc)
    /// of this value with the same `size` and `align`, and is freed once.
    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize);

    /// Resizes an allocation; null leaves the old one valid.
    ///
    /// # Safety
    ///
    /// As [`dealloc`](Self::dealloc), for `ptr`, `old_size` and `align`.
    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        old_size: usize,
        align: usize,
        new_size: usize,
    ) -> *mut u8;

    /// Microseconds since any fixed point, not going back.
    fn micros(&self) -> u64;
}

/// The host table's `ctx`: what the shell's entries reach.
#[allow(missing_debug_implementations, reason = "holds a trait object")]
#[non_exhaustive]
pub struct ShellCtx<'a> {
    /// The shell's heap and clock.
    pub services: &'a dyn Services,
}

impl<'a> ShellCtx<'a> {
    /// A context over the shell's `services`.
    #[must_use]
    pub const fn new(services: &'a dyn Services) -> Self {
        Self { services }
    }
}

/// Reads `ctx` as a [`ShellCtx`], or `None` for null.
///
/// # Safety
///
/// A non-null `ctx` points to a live [`ShellCtx`] for the call.
unsafe fn shell<'a>(ctx: *mut c_void) -> Option<&'a ShellCtx<'a>> {
    // SAFETY: as documented.
    unsafe { ctx.cast::<ShellCtx<'a>>().as_ref() }
}

/// Whether `size` and `align` make a layout the allocator may be asked for:
/// not empty, the alignment a power of two within [`MAX_ALIGN`], and the size
/// rounded up to the alignment still at most `isize::MAX` (as
/// `Layout::from_size_align` demands: the allocator's own precondition).
fn valid_layout(size: usize, align: usize) -> bool {
    size != 0 && align <= MAX_ALIGN && core::alloc::Layout::from_size_align(size, align).is_ok()
}

/// `heap.alloc`: `size` bytes aligned to `align`, the pointer into `out`
/// (null on every failure).
///
/// # Safety
///
/// `ctx` is null or a live [`ShellCtx`]; `out` is null or writable, aligned for
/// a pointer, and not aliased by the table, the context or the services (the
/// call writes it once; this is a contract, not a check).
pub unsafe extern "C" fn heap_alloc(
    ctx: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: `out` is writable and checked.
    unsafe { out.write(ptr::null_mut()) };
    // SAFETY: as documented.
    let Some(ctx) = (unsafe { shell(ctx) }) else {
        return ERR_INVALID;
    };
    if !valid_layout(size, align) {
        return ERR_INVALID;
    }
    let block = ctx.services.alloc(size, align);
    if block.is_null() {
        return ERR_NO_MEMORY;
    }
    // SAFETY: `out` is writable.
    unsafe { out.write(block.cast()) };
    OK
}

/// `heap.dealloc`: gives an allocation back with the size and alignment it
/// was made with. A null `ptr` is refused (the plugin never owned one).
///
/// # Safety
///
/// `ctx` is null or a live [`ShellCtx`]; a non-null `ptr` is an allocation of
/// this heap with this `size` and `align`, freed once.
pub unsafe extern "C" fn heap_dealloc(
    ctx: *mut c_void,
    block: *mut c_void,
    size: usize,
    align: usize,
) -> i32 {
    // SAFETY: as documented.
    let Some(ctx) = (unsafe { shell(ctx) }) else {
        return ERR_INVALID;
    };
    if block.is_null() || !valid_layout(size, align) {
        return ERR_INVALID;
    }
    // SAFETY: the caller's contract.
    unsafe { ctx.services.dealloc(block.cast(), size, align) };
    OK
}

/// `heap.realloc`: resizes an allocation; a failure leaves the old one
/// intact and `out` null.
///
/// # Safety
///
/// As [`heap_dealloc`] for `block`, `old_size` and `align`; `out` is null or
/// writable, aligned for a pointer, and not aliased by the table, the context or
/// the services (a contract, not a check).
pub unsafe extern "C" fn heap_realloc(
    ctx: *mut c_void,
    block: *mut c_void,
    old_size: usize,
    align: usize,
    new_size: usize,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: `out` is writable and checked.
    unsafe { out.write(ptr::null_mut()) };
    // SAFETY: as documented.
    let Some(ctx) = (unsafe { shell(ctx) }) else {
        return ERR_INVALID;
    };
    if block.is_null() || !valid_layout(old_size, align) || !valid_layout(new_size, align) {
        return ERR_INVALID;
    }
    // SAFETY: the caller's contract.
    let moved = unsafe {
        ctx.services
            .realloc(block.cast(), old_size, align, new_size)
    };
    if moved.is_null() {
        return ERR_NO_MEMORY;
    }
    // SAFETY: `out` is writable.
    unsafe { out.write(moved.cast()) };
    OK
}

/// `clock.micros`: microseconds into `out`.
///
/// # Safety
///
/// `ctx` is null or a live [`ShellCtx`]; `out` is null or writable, aligned for a
/// `u64`, and not aliased by the table, the context or the services (a contract,
/// not a check).
pub unsafe extern "C" fn clock_micros(ctx: *mut c_void, out: *mut u64) -> i32 {
    if out.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: as documented.
    let Some(ctx) = (unsafe { shell(ctx) }) else {
        return ERR_INVALID;
    };
    // SAFETY: `out` is writable and checked.
    unsafe { out.write(ctx.services.micros()) };
    OK
}

// The entries nobody wired yet answer `ERR_UNSUPPORTED`: a slot is never null.

unsafe extern "C" fn unsupported_env_snapshot(_: *mut c_void, _: PpBuffer) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_read(_: *mut c_void, _: PpBytes, _: PpBuffer) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_font_acquire(_: *mut c_void, _: PpBytes, _: *mut u32) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_handle(_: *mut c_void, _: u32) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_fill_rect(
    _: *mut c_void,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
    _: u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_draw_line(
    _: *mut c_void,
    _: i32,
    _: i32,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_fill_round(
    _: *mut c_void,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
    _: u32,
    _: u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_stroke_round(
    _: *mut c_void,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
    _: u32,
    _: u32,
    _: u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_text_measure(
    _: *mut c_void,
    _: u32,
    _: f32,
    _: PpBytes,
    _: *mut f32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_text_metrics(
    _: *mut c_void,
    _: u32,
    _: f32,
    _: *mut f32,
    _: *mut f32,
    _: *mut f32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_text_ink(
    _: *mut c_void,
    _: u32,
    _: f32,
    _: PpBytes,
    _: *mut i32,
    _: *mut i32,
    _: *mut i32,
    _: *mut i32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_canvas_clip(
    _: *mut c_void,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_stage_run(
    _: *mut c_void,
    _: PpBytes,
    _: u32,
    _: PpBytes,
    _: PpBuffer,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_service_call(
    _: *mut c_void,
    _: u32,
    _: u32,
    _: PpBytes,
    _: PpBuffer,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_ui_compose(
    _: *mut c_void,
    _: u32,
    _: u32,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_image_info(_: *mut c_void, _: u32, _: PpBuffer) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_canvas_draw_image(
    _: *mut c_void,
    _: u32,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
    _: u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_text_hyphenate(
    _: *mut c_void,
    _: PpBytes,
    _: PpBytes,
    _: u32,
    _: u32,
    _: PpBuffer,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_text_covers(_: *mut c_void, _: u32, _: PpBytes) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_icon_glyph(_: *mut c_void, _: PpBytes, _: PpBuffer) -> i32 {
    ERR_UNSUPPORTED
}
#[allow(
    clippy::too_many_arguments,
    reason = "one argument per field of the C slot"
)]
unsafe extern "C" fn unsupported_text_draw_inked(
    _: *mut c_void,
    _: u32,
    _: f32,
    _: f32,
    _: i32,
    _: u32,
    _: PpBytes,
    _: u32,
    _: PpBytes,
    _: u32,
    _: *mut f32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_text_draw(
    _: *mut c_void,
    _: u32,
    _: f32,
    _: f32,
    _: i32,
    _: u32,
    _: PpBytes,
    _: *mut f32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_batch(_: *mut c_void, _: PpBytes) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_services_query(
    _: *mut c_void,
    _: PpBytes,
    _: u16,
    _: u16,
    _: *mut u32,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_blob_read(_: *mut c_void, _: u32, _: u64, _: PpBuffer) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_storage_path(_: *mut c_void, _: u32, _: PpBytes) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_storage_size(
    _: *mut c_void,
    _: u32,
    _: PpBytes,
    _: *mut u64,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_storage_read_at(
    _: *mut c_void,
    _: u32,
    _: PpBytes,
    _: u64,
    _: PpBuffer,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_storage_two(
    _: *mut c_void,
    _: u32,
    _: PpBytes,
    _: PpBytes,
) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_storage_sync(_: *mut c_void) -> i32 {
    ERR_UNSUPPORTED
}
unsafe extern "C" fn unsupported_format(
    _: *mut c_void,
    _: PpBytes,
    _: PpBytes,
    _: PpBuffer,
) -> i32 {
    ERR_UNSUPPORTED
}

/// The host table of this build: the shell's entries wired, every other
/// slot answering `ERR_UNSUPPORTED`.
#[must_use]
pub const fn host_table() -> PpHost {
    PpHost {
        size: core::mem::size_of::<PpHost>() as u32,
        major: MAJOR,
        minor: MINOR,
        heap_alloc,
        heap_dealloc,
        heap_realloc,
        clock_micros,
        env_snapshot: unsupported_env_snapshot,
        settings_read: unsupported_read,
        i18n_copy: unsupported_read,
        font_acquire: unsupported_font_acquire,
        handle_release: unsupported_handle,
        canvas_fill_rect: unsupported_fill_rect,
        canvas_draw_line: unsupported_draw_line,
        canvas_fill_round: unsupported_fill_round,
        canvas_stroke_round: unsupported_stroke_round,
        text_measure: unsupported_text_measure,
        text_draw: unsupported_text_draw,
        commands_submit: unsupported_batch,
        registrations_submit: unsupported_batch,
        services_query: unsupported_services_query,
        blob_read: unsupported_blob_read,
        blob_release: unsupported_handle,
        storage_size: unsupported_storage_size,
        storage_read_at: unsupported_storage_read_at,
        storage_write: unsupported_storage_two,
        storage_append: unsupported_storage_two,
        storage_rename: unsupported_storage_two,
        storage_remove: unsupported_storage_path,
        storage_sync: unsupported_storage_sync,
        i18n_format: unsupported_format,
        i18n_digits: unsupported_read,
        text_metrics: unsupported_text_metrics,
        text_ink: unsupported_text_ink,
        canvas_clip: unsupported_canvas_clip,
        stage_run: unsupported_stage_run,
        service_open: unsupported_services_query,
        service_call: unsupported_service_call,
        ui_compose: unsupported_ui_compose,
        image_info: unsupported_image_info,
        canvas_draw_image: unsupported_canvas_draw_image,
        text_hyphenate: unsupported_text_hyphenate,
        text_covers: unsupported_text_covers,
        icon_glyph: unsupported_icon_glyph,
        text_draw_inked: unsupported_text_draw_inked,
    }
}

#[cfg(test)]
mod const_table {
    use super::*;

    /// The table needs no start-up code: a firmware keeps it in a `static`
    /// (flash on the X4, no RAM, no start-up cost).
    static TABLE: PpHost = host_table();

    /// The crate carries its own copy of the generated file (a published or
    /// vendored crate has no `abi/` beside it); in the repository it must be
    /// the generator's output byte for byte. Where `abi/` is absent (a vendored
    /// copy) only the copy exists and there is nothing to compare.
    #[test]
    fn the_crates_copy_is_the_generated_file() {
        extern crate std;
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let Ok(generated) = std::fs::read(here.join("../../abi/generated/paperpaw_abi.rs")) else {
            return;
        };
        let copy = std::fs::read(here.join("src/host_glue/paperpaw_abi.rs")).unwrap();
        assert!(
            generated == copy,
            "crates/paperpaw-native/src/host_glue/paperpaw_abi.rs is not abi/generated/paperpaw_abi.rs: copy it again"
        );
    }

    #[test]
    fn the_table_can_be_a_static() {
        assert_eq!(TABLE.size as usize, core::mem::size_of::<PpHost>());
        assert_eq!((TABLE.major, TABLE.minor), (MAJOR, MINOR));
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::alloc::{Layout, alloc, dealloc, realloc};
    use std::cell::Cell;

    use super::*;

    /// A shell on the test machine's allocator that can be told to fail.
    struct Machine {
        fail: Cell<bool>,
        clock: Cell<u64>,
        /// Calls that reached the shell's allocator.
        calls: Cell<u32>,
    }

    impl Services for Machine {
        fn alloc(&self, size: usize, align: usize) -> *mut u8 {
            self.calls.set(self.calls.get() + 1);
            if self.fail.get() {
                return ptr::null_mut();
            }
            // SAFETY: the layout is valid: the entry checked it.
            unsafe { alloc(Layout::from_size_align(size, align).unwrap()) }
        }
        unsafe fn dealloc(&self, p: *mut u8, size: usize, align: usize) {
            // SAFETY: the trait's contract.
            unsafe { dealloc(p, Layout::from_size_align(size, align).unwrap()) }
        }
        unsafe fn realloc(&self, p: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
            self.calls.set(self.calls.get() + 1);
            if self.fail.get() {
                return ptr::null_mut();
            }
            // SAFETY: the trait's contract.
            unsafe { realloc(p, Layout::from_size_align(old, align).unwrap(), new) }
        }
        fn micros(&self) -> u64 {
            self.clock.set(self.clock.get() + 7);
            self.clock.get()
        }
    }

    fn machine() -> Machine {
        Machine {
            fail: Cell::new(false),
            clock: Cell::new(100),
            calls: Cell::new(0),
        }
    }

    fn ctx(m: &Machine) -> ShellCtx<'_> {
        ShellCtx { services: m }
    }

    #[test]
    fn the_table_says_its_size_and_version_and_has_no_null_slot() {
        let host = host_table();
        assert_eq!(host.size as usize, core::mem::size_of::<PpHost>());
        assert_eq!((host.major, host.minor), (MAJOR, MINOR));
        let def =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../abi/abi.def"))
                .unwrap();
        assert!(
            def.contains("\"major\": 7"),
            "the schema's major is the table's"
        );
        assert!(
            def.contains("\"minor\": 1"),
            "the schema's minor is the table's"
        );
        // An unwired entry answers, it does not crash.
        let mut c = ShellCtx {
            services: &machine(),
        };
        let p: *mut c_void = (&raw mut c).cast();
        // SAFETY: `p` is a live context.
        let status = unsafe { (host.handle_release)(p, 1) };
        assert_eq!(status, ERR_UNSUPPORTED);
    }

    /// A size that fits `isize::MAX` but not once rounded up to the alignment is not a
    /// layout: refused before the shell's allocator is reached, as are the edges
    /// around it (QA review of PR #135).
    #[test]
    fn a_size_that_does_not_fit_its_alignment_is_refused_before_the_allocator() {
        let m = machine();
        let mut c = ShellCtx::new(&m);
        let p: *mut c_void = (&raw mut c).cast();
        let mut out: *mut c_void = ptr::null_mut();
        let max = isize::MAX as usize;
        for (size, align) in [
            (max, 4096),
            (max - 4094, 4096),
            (max + 1, 1),
            (usize::MAX, 8),
            (0, 8),
            (8, 0),
            (8, 3),
            (8, 8192),
        ] {
            // SAFETY: `p` is a live context and `out` is writable.
            let status = unsafe { heap_alloc(p, size, align, &mut out) };
            assert_eq!(status, ERR_INVALID, "alloc {size:#x} align {align}");
            assert!(out.is_null());
        }
        assert!(valid_layout(max - 4095, 4096), "the largest that fits");
        // A good old block, a bad new size: the shell's realloc is never called.
        // SAFETY: as above.
        assert_eq!(unsafe { heap_alloc(p, 32, 4096, &mut out) }, OK);
        let block = out;
        let before = m.calls.get();
        for new_size in [max, max - 4094, usize::MAX, 0] {
            let mut moved: *mut c_void = ptr::null_mut();
            // SAFETY: `block` is this heap's 32-byte block of alignment 4096.
            let status = unsafe { heap_realloc(p, block, 32, 4096, new_size, &mut moved) };
            assert_eq!(status, ERR_INVALID, "realloc to {new_size:#x}");
            assert!(moved.is_null());
        }
        assert_eq!(m.calls.get(), before, "the shell's allocator was reached");
        // SAFETY: the block is still this heap's, freed once.
        assert_eq!(unsafe { heap_dealloc(p, block, 32, 4096) }, OK);
    }

    #[test]
    fn heap_round_trip_keeps_alignment_and_content() {
        let m = machine();
        let mut c = ctx(&m);
        let p: *mut c_void = (&raw mut c).cast();
        let mut out: *mut c_void = ptr::null_mut();
        // SAFETY: a live context and a writable out.
        assert_eq!(unsafe { heap_alloc(p, 100, 64, &mut out) }, OK);
        assert!(!out.is_null());
        assert_eq!(out as usize % 64, 0);
        // SAFETY: 100 bytes were allocated.
        unsafe { out.cast::<u8>().write_bytes(0xAB, 100) };
        let mut bigger: *mut c_void = ptr::null_mut();
        // SAFETY: `out` is the allocation made above.
        assert_eq!(
            unsafe { heap_realloc(p, out, 100, 64, 400, &mut bigger) },
            OK
        );
        // SAFETY: 400 bytes now; the first 100 are kept.
        let kept = unsafe { core::slice::from_raw_parts(bigger.cast::<u8>(), 100) };
        assert!(kept.iter().all(|&b| b == 0xAB));
        // SAFETY: `bigger` is the live allocation.
        assert_eq!(unsafe { heap_dealloc(p, bigger, 400, 64) }, OK);
    }

    #[test]
    fn bad_arguments_are_refused_and_leave_out_null() {
        let m = machine();
        let mut c = ctx(&m);
        let p: *mut c_void = (&raw mut c).cast();
        let mut out: *mut c_void = ptr::dangling_mut::<c_void>();
        // SAFETY: a live context and a writable out in every call here.
        unsafe {
            assert_eq!(heap_alloc(p, 0, 8, &mut out), ERR_INVALID);
            assert!(out.is_null(), "a failed call clears out");
            assert_eq!(
                heap_alloc(p, 8, 3, &mut out),
                ERR_INVALID,
                "not a power of two"
            );
            assert_eq!(heap_alloc(p, 8, MAX_ALIGN * 2, &mut out), ERR_INVALID);
            assert_eq!(heap_alloc(p, usize::MAX, 8, &mut out), ERR_INVALID);
            assert_eq!(heap_alloc(p, 8, 8, ptr::null_mut()), ERR_INVALID);
            assert_eq!(heap_alloc(ptr::null_mut(), 8, 8, &mut out), ERR_INVALID);
            assert_eq!(heap_dealloc(p, ptr::null_mut(), 8, 8), ERR_INVALID);
            assert_eq!(
                heap_realloc(p, ptr::null_mut(), 8, 8, 16, &mut out),
                ERR_INVALID
            );
            assert_eq!(clock_micros(p, ptr::null_mut()), ERR_INVALID);
            assert_eq!(clock_micros(ptr::null_mut(), &mut 0), ERR_INVALID);
        }
    }

    #[test]
    fn an_allocator_failure_is_no_memory_and_realloc_keeps_the_old_block() {
        let m = machine();
        let mut c = ctx(&m);
        let p: *mut c_void = (&raw mut c).cast();
        let mut block: *mut c_void = ptr::null_mut();
        // SAFETY: a live context and writable outs.
        unsafe {
            assert_eq!(heap_alloc(p, 32, 8, &mut block), OK);
            block.cast::<u8>().write_bytes(0x5A, 32);
            m.fail.set(true);
            let mut out: *mut c_void = ptr::dangling_mut::<c_void>();
            assert_eq!(heap_alloc(p, 32, 8, &mut out), ERR_NO_MEMORY);
            assert!(out.is_null());
            assert_eq!(heap_realloc(p, block, 32, 8, 64, &mut out), ERR_NO_MEMORY);
            assert!(out.is_null());
            // The old block is intact and still ours to free.
            assert!(
                core::slice::from_raw_parts(block.cast::<u8>(), 32)
                    .iter()
                    .all(|&b| b == 0x5A)
            );
            m.fail.set(false);
            assert_eq!(heap_dealloc(p, block, 32, 8), OK);
        }
    }

    #[test]
    fn the_clock_goes_forward() {
        let m = machine();
        let mut c = ctx(&m);
        let p: *mut c_void = (&raw mut c).cast();
        let (mut a, mut b) = (0u64, 0u64);
        // SAFETY: a live context and writable outs.
        unsafe {
            assert_eq!(clock_micros(p, &mut a), OK);
            assert_eq!(clock_micros(p, &mut b), OK);
        }
        assert!(b > a);
    }
}
