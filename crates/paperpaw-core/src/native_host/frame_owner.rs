//! The frame owner (`abi/heap-word.md` section 4): which plugin's code runs
//! now, and so who pays for an allocation made through the context-free
//! `PpHeap` table.
//!
//! Every entry from host code into plugin code goes through [`within_recorded`], which
//! pushes the plugin and restores the previous one on every way out (a guard
//! value, also when the call unwinds on a hosted target): the factory, the
//! destructor, the event, draw and input callbacks of a registration, a
//! declared token and a command stage. An entry of a new kind is not accepted
//! without going through here and a test (`frame_owner_tests.rs`).
//!
//! The record lives on the stack of the call. The pointer to the innermost
//! is thread-local wherever there are threads (the desktop, Android, the web
//! and every test binary run plugin code on more than one thread, and a
//! pointer shared across them would point into another thread's finished
//! stack), and one static on bare metal (`target_os = "none"`: one core runs
//! plugin code there, as for [`host_slot`](super::host_slot)).

use core::any::TypeId;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

/// One pushed frame.
struct Frame {
    /// The host owner key of the plugin whose code runs.
    #[allow(
        dead_code,
        reason = "charged by the allocator's budget (heap-word PR 2); read by tests"
    )]
    owner: u64,
    /// Its `Context<S>`, for the typed table entries of that shell.
    context: *const c_void,
    /// The services type `S` of the shell it runs in (its `TypeId`): a table
    /// entry reads the context only if the frame was pushed for its own `S`.
    kind: TypeId,
    /// Where a host call made by this frame's own code is noted, or null:
    /// set while `make` runs ([`within_recorded`], parked plugins #248).
    calls: *const Cell<bool>,
    /// The frame this one was pushed over, or null.
    previous: *const Frame,
}

#[cfg(target_os = "none")]
mod current {
    use super::Frame;
    use core::ptr;
    use core::sync::atomic::{AtomicPtr, Ordering};

    static CURRENT: AtomicPtr<Frame> = AtomicPtr::new(ptr::null_mut());

    pub(super) fn get() -> *const Frame {
        CURRENT.load(Ordering::Acquire)
    }

    pub(super) fn set(frame: *const Frame) {
        CURRENT.store(frame.cast_mut(), Ordering::Release);
    }
}

#[cfg(not(target_os = "none"))]
mod current {
    extern crate std;
    use super::Frame;
    use core::cell::Cell;
    use core::ptr;

    std::thread_local! {
        static CURRENT: Cell<*const Frame> = const { Cell::new(ptr::null()) };
    }

    // `try_with`: a thread that is ending has no frame (and never panics here).
    pub(super) fn get() -> *const Frame {
        CURRENT.try_with(Cell::get).unwrap_or(ptr::null())
    }

    pub(super) fn set(frame: *const Frame) {
        let _ = CURRENT.try_with(|current| current.set(frame));
    }
}

/// Puts the frame that was current back, however the call ends.
struct Restore(*const Frame);

impl Drop for Restore {
    fn drop(&mut self) {
        current::set(self.0);
    }
}

/// Runs `call`, which enters the code of the plugin with host owner key
/// `owner`, context `context` (a pinned `Context<S>`) and the `TypeId` `kind` of its
/// services type `S`, with that plugin as the frame owner; the
/// previous owner is back when it returns.
#[cfg(test)]
pub(crate) fn within<R>(
    owner: u64,
    context: *const c_void,
    kind: TypeId,
    call: impl FnOnce() -> R,
) -> R {
    push(owner, context, kind, ptr::null(), call)
}

/// Runs `call` with the plugin as the frame owner, as `within` does (tests),
/// and every host call the plugin makes from this frame
/// itself (not from a frame pushed over it) sets `calls`
/// ([`note_host_call`]): what `make` called besides registering.
pub(crate) fn within_recorded<R>(
    owner: u64,
    context: *const c_void,
    kind: TypeId,
    calls: &Cell<bool>,
    call: impl FnOnce() -> R,
) -> R {
    push(owner, context, kind, ptr::from_ref(calls), call)
}

fn push<R>(
    owner: u64,
    context: *const c_void,
    kind: TypeId,
    calls: *const Cell<bool>,
    call: impl FnOnce() -> R,
) -> R {
    let frame = Frame {
        owner,
        context,
        kind,
        calls,
        previous: current::get(),
    };
    let _restore = Restore(frame.previous);
    current::set(ptr::from_ref(&frame));
    call()
}

/// Notes a host call of the plugin whose code runs now, where its frame
/// records them ([`within_recorded`]); nothing otherwise.
pub(crate) fn note_host_call() {
    let frame = current::get();
    // SAFETY: as for [`context_of`]; `calls` is null or borrowed by the
    // `within_recorded` that pushed the frame, for as long as it is current.
    if let Some(calls) = unsafe { frame.as_ref() }.and_then(|frame| unsafe { frame.calls.as_ref() })
    {
        calls.set(true);
    }
}

/// The context of the plugin whose code runs now, if the frame was pushed
/// for the services type `kind`; `None` outside any frame and for a frame of
/// another services type.
pub(crate) fn context_of(kind: TypeId) -> Option<*const c_void> {
    let frame = current::get();
    // SAFETY: a non-null current frame is on the stack of a `within` that has
    // not returned (its guard clears it on the way out).
    unsafe { frame.as_ref() }
        .filter(|frame| frame.kind == kind)
        .map(|frame| frame.context)
}

/// The host owner key of the plugin whose code runs now, or `None` outside
/// any frame.
#[allow(
    dead_code,
    reason = "charged by the allocator's budget (heap-word PR 2); read by tests"
)]
pub(crate) fn owner() -> Option<u64> {
    let frame = current::get();
    // SAFETY: as for [`context`].
    unsafe { frame.as_ref() }.map(|frame| frame.owner)
}

#[cfg(test)]
#[path = "frame_owner_tests.rs"]
mod tests;
