//! The host's `PpHeap` table (`abi/heap-word.md` section 3): the context-free
//! door to the same allocator as the heap slots of the host table.
//!
//! Both doors end in the same functions ([`host_glue::heap_alloc`] and its
//! siblings) over the same [`Services`]: one implementation, one block
//! format. The heap slots take the plugin from their `ctx`; this table takes it
//! from the frame owner ([`frame_owner`]), whose context the table entries of
//! the shell's services type `S` read back.

use super::{Context, Services, with_shell};
use crate::native_host::frame_owner;
use core::any::TypeId;
use core::ffi::c_void;
use core::ptr;
use paperpaw_native::host_glue::{self, MAJOR, MINOR, OK, PpHeap};

impl<S: 'static> Context<S> {
    /// Runs `call`, which enters the code of this plugin (`make`, its
    /// destructor, a callback), with the plugin as the frame owner. Every
    /// entry of host code into plugin code goes through here.
    pub(crate) fn run_plugin<R>(&self, call: impl FnOnce() -> R) -> R {
        frame_owner::within(
            self.resources.owner_id(),
            ptr::from_ref(self).cast(),
            TypeId::of::<S>(),
            call,
        )
    }
}

/// The `PpHeap` table of `S`. Its `size` covers the entries up to `realloc`:
/// no `abort` yet, so the SDK's panic handler executes the profile's trap
/// instruction (section 6, "no usable heap word").
#[must_use]
pub const fn heap_table<S: Services + 'static>() -> PpHeap {
    PpHeap {
        size: core::mem::offset_of!(PpHeap, abort) as u32,
        major: MAJOR,
        minor: MINOR,
        alloc: Some(alloc::<S>),
        dealloc: Some(dealloc::<S>),
        realloc: Some(realloc::<S>),
        abort: None,
    }
}

/// The `PpHeap` table of `S` as one static, as [`SharedTable`](super::SharedTable)
/// is for the host table.
pub(super) struct SharedHeap<S>(core::marker::PhantomData<S>);

impl<S: Services + 'static> SharedHeap<S> {
    /// Evaluated at compile time and promoted to a `'static` in read-only data.
    pub(super) const TABLE: &'static PpHeap = &heap_table::<S>();
}

/// The `PpHeap` table every plugin hosted with `S` is handed, for the loader
/// to write the address of into the plugin's heap word.
///
/// The identity of `S` for the frame owner is its `TypeId`, not the address of
/// this table: Rust does not promise one address per promoted constant (each
/// codegen unit may carry its own copy, identical constants may be merged).
#[must_use]
pub fn handed_heap<S: Services + 'static>() -> &'static PpHeap {
    SharedHeap::<S>::TABLE
}

/// The context of the frame owner, if the frame was pushed for the table of
/// this `S`: only then is it a `Context<S>`.
fn frame_context<S: Services + 'static>() -> Option<*mut c_void> {
    frame_owner::context_of(TypeId::of::<S>()).map(|context| context.cast_mut())
}

/// `alloc`: null outside a frame, and for every refusal of the shared heap
/// (a zero size, a bad alignment, no room).
unsafe extern "C" fn alloc<S: Services + 'static>(size: usize, align: usize) -> *mut c_void {
    let Some(ctx) = frame_context::<S>() else {
        return ptr::null_mut();
    };
    let mut block = ptr::null_mut();
    // SAFETY: `ctx` is the pinned `Context<S>` of the plugin whose code runs
    // (pushed by `run_plugin`); `block` is writable.
    let status = unsafe {
        with_shell::<S>(ctx, |shell| {
            host_glue::heap_alloc(shell, size, align, &raw mut block)
        })
    };
    if status == OK { block } else { ptr::null_mut() }
}

/// `dealloc`: a null `ptr` is ignored. Outside a frame there is no heap to
/// give the block to, so it stays where it is (a leak, not a fault): the
/// block's own owner replaces the frame once blocks carry one.
unsafe extern "C" fn dealloc<S: Services + 'static>(block: *mut c_void, size: usize, align: usize) {
    if block.is_null() {
        return;
    }
    let Some(ctx) = frame_context::<S>() else {
        return;
    };
    // SAFETY: as `alloc`; the block is this heap's with this size and
    // alignment (the plugin's contract, section 3).
    let _ = unsafe {
        with_shell::<S>(ctx, |shell| {
            host_glue::heap_dealloc(shell, block, size, align)
        })
    };
}

/// `realloc`: null outside a frame; the old block is untouched on null.
unsafe extern "C" fn realloc<S: Services + 'static>(
    block: *mut c_void,
    old_size: usize,
    align: usize,
    new_size: usize,
) -> *mut c_void {
    let Some(ctx) = frame_context::<S>() else {
        return ptr::null_mut();
    };
    let mut moved = ptr::null_mut();
    // SAFETY: as `alloc` and `dealloc`.
    let status = unsafe {
        with_shell::<S>(ctx, |shell| {
            host_glue::heap_realloc(shell, block, old_size, align, new_size, &raw mut moved)
        })
    };
    if status == OK { moved } else { ptr::null_mut() }
}
