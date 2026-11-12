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

/// Begins a host slot: hold the result until the slot returns. The call is
/// noted where the plugin's frame records them (its `make`, #248).
pub(super) fn enter() -> Slot {
    super::frame_owner::note_host_call();
    enter_registration()
}

/// As [`enter`], for `registrations.submit`: the one call `make` is meant to
/// make, never noted.
pub(super) fn enter_registration() -> Slot {
    call(&BEGIN);
    Slot(())
}

impl Drop for Slot {
    fn drop(&mut self) {
        call(&END);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
mod tests {
    extern crate std;
    use alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};
    use std::fs;
    use std::path::Path;

    /// Entries of the host table that never wait: the allocator and the clock
    /// (hot and bounded), and the helper they share.
    const FAST: &[&str] = &["heap_alloc", "heap_dealloc", "heap_realloc", "clock_micros"];

    /// Every function a plugin can call through the host table (an
    /// `extern "C" fn` taking the context first) announces its host slot, so
    /// the Power-hold escape never blames a plugin that waits for the host.
    /// A new entry without the guard (a service, a stage runner that calls back
    /// into plugins) fails here.
    #[test]
    fn every_host_table_entry_but_the_allocator_and_the_clock_announces_its_slot() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![root.join("native_host.rs")];
        for entry in fs::read_dir(root.join("native_host")).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            if name.ends_with(".rs") && !name.contains("tests") && name != "shell_core.rs" {
                files.push(path);
            }
        }
        let mut seen = 0;
        let mut missing = Vec::new();
        for path in files {
            let source = fs::read_to_string(&path).unwrap();
            let mut rest = source.as_str();
            while let Some(at) = rest.find("extern \"C\" fn ") {
                let after = &rest[at + "extern \"C\" fn ".len()..];
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                // Up to the next entry, whatever its visibility.
                let body_end = after.find("extern \"C\" fn ").unwrap_or(after.len());
                let function = &after[..body_end];
                let signature = function.split('{').next().unwrap();
                if signature.contains("ctx: *mut c_void") && !FAST.contains(&name.as_str()) {
                    seen += 1;
                    let guarded = function.contains("host_slot::enter()")
                        || (name == "registrations_submit"
                            && function.contains("host_slot::enter_registration()"));
                    if !guarded {
                        missing.push(format!("{}: {name}", path.display()));
                    }
                }
                rest = &after[body_end..];
            }
        }
        assert!(seen >= 25, "the scan found only {seen} host entries");
        assert!(
            missing.is_empty(),
            "host entries without `host_slot::enter()`: {missing:#?}"
        );
    }
}
