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

/// How many steps are open (a job's close inside another of its steps, a
/// displaced job closed while a step runs): the shell hears the outermost
/// only. The core runs on one thread; tests count per test thread.
#[cfg(not(test))]
static DEPTH: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Adds `up` (1 or -1 as wrapping) to the open steps; the count before.
#[cfg(not(test))]
fn depth(up: u32) -> u32 {
    DEPTH.fetch_add(up, Ordering::AcqRel)
}

#[cfg(test)]
fn depth(up: u32) -> u32 {
    extern crate std;
    std::thread_local! {
        static DEPTH: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    }
    DEPTH.with(|depth| {
        let before = depth.get();
        depth.set(before.wrapping_add(up));
        before
    })
}

/// Sets what runs when a background job step begins and ends (once, at start;
/// a shell with one core and one set of hooks). Each `begin` is followed by
/// its `end`, also when the step fails. Steps inside a step are counted:
/// `begin` runs when the first opens, `end` when the last has ended.
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
    if depth(1) == 0 {
        call(&BEGIN);
    }
    Step(())
}

impl Drop for Step {
    fn drop(&mut self) {
        // Each `Step` added one, so this never goes below zero.
        if depth(u32::MAX) == 1 {
            call(&END);
        }
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

    #[test]
    fn steps_inside_a_step_tell_only_the_outermost() {
        let steps = count();
        {
            let _outer = super::enter();
            {
                let _inner = super::enter();
                let _innermost = super::enter();
                assert_eq!(steps(), (1, 0));
            }
            assert_eq!(steps(), (1, 0), "the outer step is still open");
        }
        assert_eq!(steps(), (1, 1));
        let _again = super::enter();
        assert_eq!(steps(), (2, 1), "a new step after the last ended");
    }
}
