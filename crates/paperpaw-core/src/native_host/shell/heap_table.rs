//! The host's `PpHeap` table (`abi/heap-word.md` section 3): the context-free
//! door to the same allocator as the heap slots of the host table.
//!
//! Both doors end in the same functions ([`host_glue::heap_alloc`] and its
//! siblings) over the same [`Services`]: one implementation, one block
//! format. The heap slots take the plugin from their `ctx`; this table takes it
//! from the frame owner ([`frame_owner`]), whose context the table entries of
//! the shell's services type `S` read back.

use super::{Context, Services};
use crate::native_host::frame_owner;
use crate::native_host::plugin_heap::Charged;
use core::any::TypeId;
use core::ffi::c_void;
use core::ptr;
use paperpaw_native::host_glue::{self, MAJOR, MINOR, OK, PpHeap};
use paperpaw_native::host_glue::{ERR_INVALID, ShellCtx};

/// `Services::enter` and `leave` of a shell, as function pointers.
pub(super) struct Entry<S: 'static> {
    enter: fn(&S, &str) -> bool,
    leave: fn(&S, &str),
}

impl<S: super::Services + 'static> Entry<S> {
    /// One static per services type: a context keeps a pointer to it, one word.
    pub(super) const OF: Self = Self {
        enter: S::enter,
        leave: S::leave,
    };
}

/// Tells the shell that the plugin code it announced has returned, however
/// the call ends.
struct Leave<'a, S: 'static>(&'a Context<S>);

impl<S: 'static> Drop for Leave<'_, S> {
    fn drop(&mut self) {
        if let Some(entry) = self.0.entry.get() {
            (entry.leave)(&self.0.services, self.0.plugin_id());
        }
    }
}

impl<S: super::Services + 'static> Context<S> {
    /// From here every entry of the plugin's code is announced to the shell
    /// (`Services::enter`). Admission does it for a plugin from a file; a
    /// caller that wraps each call itself (`Instance::load`'s `Hooks`) leaves it
    /// alone, so that a call is announced once.
    pub(crate) fn announce_entries(&self) {
        self.entry.set(Some(&Entry::<S>::OF));
    }
}

impl<S: 'static> Context<S> {
    /// Announces an entry of plugin code to the shell (`Services::enter`); a
    /// plugin from a file only. `None`: refused, nothing may run and no
    /// `leave` follows.
    fn announce(&self) -> Option<Leave<'_, S>> {
        match self.entry.get() {
            Some(entry) if !(entry.enter)(&self.services, self.plugin_id()) => None,
            _ => Some(Leave(self)),
        }
    }

    /// Runs `call`, which enters the code of this plugin (a callback, a
    /// token, a stage), with the plugin as the frame owner. Every entry of
    /// host code into plugin code goes through here, `make` and the
    /// destructor through their own door. `Error::Refused` where the shell
    /// refused the entry: the code did not run.
    pub(crate) fn run_plugin<R>(&self, call: impl FnOnce() -> R) -> Result<R, super::Error> {
        let _leave = self.announce().ok_or(super::Error::Refused)?;
        Ok(frame_owner::within_recorded(
            self.resources.owner_id(),
            ptr::from_ref(self).cast(),
            TypeId::of::<S>(),
            &self.calls.after_make,
            call,
        ))
    }

    /// [`Self::run_plugin`] for `make`: its host calls other than
    /// `registrations.submit` and the heap are noted apart (`calls.make`).
    pub(crate) fn run_make<R>(&self, call: impl FnOnce() -> R) -> Result<R, super::Error> {
        let _leave = self.announce().ok_or(super::Error::Refused)?;
        Ok(frame_owner::within_recorded(
            self.resources.owner_id(),
            ptr::from_ref(self).cast(),
            TypeId::of::<S>(),
            &self.calls.make,
            call,
        ))
    }

    /// The destructor, under the same rule as every entry: where the shell
    /// refuses plugin code now (the X4: its stack's band is lost) it does not
    /// run, its blocks leak (the X4 never unloads) and the shell is told, as
    /// for `Instance`'s teardown.
    pub(crate) fn run_destroy(&self, call: impl FnOnce()) {
        let Some(_leave) = self.announce() else {
            let step = crate::native_host::TeardownStep::Release;
            (self.report)(
                &self.services,
                self.plugin_id(),
                step,
                crate::native_host::Error::State,
            );
            return;
        };
        frame_owner::within_recorded(
            self.resources.owner_id(),
            ptr::from_ref(self).cast(),
            TypeId::of::<S>(),
            &self.calls.after_make,
            call,
        );
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

/// Runs `call` with a [`ShellCtx`] over the shell's allocator as the plugin
/// heap has it: owners and a budget ([`Charged`]), charging the plugin whose
/// context `ctx` is. Both doors of the heap come through here.
///
/// # Safety
///
/// `ctx` is null or a live, pinned `Context<S>`.
pub(super) unsafe fn with_heap<S: Services + 'static>(
    ctx: *mut c_void,
    call: impl FnOnce(*mut c_void) -> i32,
) -> i32 {
    // SAFETY: as documented.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return ERR_INVALID;
    };
    let (state, owner) = context.resources.heap();
    let charged = Charged {
        inner: &context.services,
        state,
        owner,
    };
    let mut shell = ShellCtx::new(&charged);
    call(ptr::from_mut(&mut shell).cast())
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
        with_heap::<S>(ctx, |shell| {
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
        with_heap::<S>(ctx, |shell| {
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
        with_heap::<S>(ctx, |shell| {
            host_glue::heap_realloc(shell, block, old_size, align, new_size, &raw mut moved)
        })
    };
    if status == OK { moved } else { ptr::null_mut() }
}
