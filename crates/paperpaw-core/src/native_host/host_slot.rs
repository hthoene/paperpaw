//! Tells a shell when the host's code runs on behalf of a plugin: a plugin's
//! call into the host (storage, services, text, canvas...) is host work, not
//! the plugin's own code, and takes as long as the host takes (a card write
//! on a large card can take seconds).
//!
//! The X4 uses the pair to keep "a host slot is open" in the record that a
//! reset leaves behind, so that the Power-hold escape never blames a plugin
//! that only waits for the host (`shells/xteink/src/safe_mode.rs`). Other
//! shells set nothing and the calls cost one load and a branch.

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

static BEGIN: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static END: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());

/// Sets what runs when a host slot begins and ends (once, at start; a shell
/// with one core and one set of hooks). Calls nest: each `begin` is followed
/// by its `end`, also when the slot fails.
pub fn set(begin: fn(), end: fn()) {
    // A function pointer kept as a data pointer (its provenance with it).
    BEGIN.store(begin as *mut (), Ordering::Release);
    END.store(end as *mut (), Ordering::Release);
}

fn call(hook: &AtomicPtr<()>) {
    let stored = hook.load(Ordering::Acquire);
    if !stored.is_null() {
        // SAFETY: only `set` stores here, and it stores a `fn()` cast to a
        // data pointer; null (nothing set) is excluded above.
        let hook = unsafe { core::mem::transmute::<*mut (), fn()>(stored) };
        hook();
    }
}

/// A host slot is open while this lives.
#[must_use = "the slot ends when this is dropped"]
pub(super) struct Slot(());

/// Begins a host slot: hold the result until the slot returns.
pub(super) fn enter() -> Slot {
    call(&BEGIN);
    Slot(())
}

impl Drop for Slot {
    fn drop(&mut self) {
        call(&END);
    }
}
