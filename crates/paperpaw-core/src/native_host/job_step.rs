//! Tells a shell when plugin code runs for a background job of the host's
//! (`doc_service::HOST_JOB` rows: the library's cover job, a book's details
//! job), not for anything the reader asked for or a screen shows: a format
//! plugin preparing, making a cover or closing a book on its own time.
//!
//! The X4 keeps "a job step is open" in the record that a reset leaves behind
//! (`shells/xteink/src/safe_mode.rs`): a crash in such a step is the job's
//! book's (the cover job's marker, `cover_job_marker`), not a failed start,
//! so the next start keeps its native plugins. The hooks run around the
//! host's call into the format, outside every plugin's callback; the plugin
//! calls inside them still go through the shell's `call_begin`/`call_end`
//! (`shell_instance::Hooks`) as any call does. Other shells set nothing and a
//! step costs one load and a branch. Nothing a plugin sees changes.

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

static BEGIN: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
static END: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());

/// Sets what runs when a background job step begins and ends (once, at start;
/// a shell with one core and one set of hooks). Each `begin` is followed by
/// its `end`, also when the step fails; steps do not nest.
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

/// A background job step is open while this lives.
#[must_use = "the step ends when this is dropped"]
pub(crate) struct Step(());

/// Begins a background job step: hold the result until the format's call
/// returned.
pub(crate) fn enter() -> Step {
    call(&BEGIN);
    Step(())
}

impl Drop for Step {
    fn drop(&mut self) {
        call(&END);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;
    use core::cell::Cell;

    std::thread_local! {
        static BEGUN: Cell<u32> = const { Cell::new(0) };
        static ENDED: Cell<u32> = const { Cell::new(0) };
    }

    fn begin() {
        BEGUN.with(|count| count.set(count.get() + 1));
    }

    fn end() {
        ENDED.with(|count| count.set(count.get() + 1));
    }

    /// Counts this thread's steps from now on (the hooks are the process's;
    /// each test thread counts only its own steps). The steps begun and
    /// ended so far.
    pub(crate) fn count() -> impl Fn() -> (u32, u32) {
        super::set(begin, end);
        BEGUN.with(|count| count.set(0));
        ENDED.with(|count| count.set(0));
        || (BEGUN.with(Cell::get), ENDED.with(Cell::get))
    }

    #[test]
    fn a_step_tells_its_begin_and_its_end_once_each() {
        let steps = count();
        {
            let _step = super::enter();
            assert_eq!(steps(), (1, 0));
        }
        assert_eq!(steps(), (1, 1));
    }
}
