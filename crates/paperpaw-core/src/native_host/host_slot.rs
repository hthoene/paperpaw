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

fn begin_slot(id: Id) -> Slot {
    super::frame_owner::note_host_call();
    #[cfg(feature = "slot-names")]
    LAST.store(id as u8, Ordering::Relaxed);
    #[cfg(not(feature = "slot-names"))]
    let _ = id;
    enter_registration()
}

/// Which slot began last (`slot-names`, the measurement build of the X4): the
/// begin hook reads it, as [`name_of`] says what it is.
#[cfg(feature = "slot-names")]
static LAST: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// The number of slot names, `other` included (`slot-names`).
#[cfg(feature = "slot-names")]
pub const SLOT_NAMES: usize = NAMES.len();

/// The slot that began last, as a number for [`name_of`] (`slot-names`).
#[cfg(feature = "slot-names")]
#[must_use]
pub fn last_id() -> u8 {
    LAST.load(Ordering::Relaxed)
}

/// The name of slot `id` (`slot-names`).
#[cfg(feature = "slot-names")]
#[must_use]
pub fn name_of(id: u8) -> &'static str {
    NAMES.get(usize::from(id)).copied().unwrap_or("?")
}

/// A host slot the counters of `slot-counts` tell apart; `Other` is every
/// slot not named here.
macro_rules! slot_ids {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        #[allow(dead_code, reason = "named for the counters of `slot-counts`")]
        pub(super) enum Id {
            $($variant),+,
            Other,
        }

        #[cfg(any(feature = "slot-counts", feature = "slot-names"))]
        const NAMES: &[&str] = &[$($name),+, "other"];
    };
}

slot_ids! {
        BlobRead => "blob_read",
        BlobRelease => "blob_release",
        CanvasClip => "canvas_clip",
        CanvasDescribe => "canvas_describe",
        CanvasDrawLine => "canvas_draw_line",
        CanvasFillRect => "canvas_fill_rect",
        CanvasFillRound => "canvas_fill_round",
        CanvasStrokeRound => "canvas_stroke_round",
        CommandsSubmit => "commands_submit",
        EnvSnapshot => "env_snapshot",
        FontAcquire => "font_acquire",
        HandleRelease => "handle_release",
        I18nCopy => "i18n_copy",
        I18nDigits => "i18n_digits",
        I18nFormat => "i18n_format",
        IconGlyph => "icon_glyph",
        ImageDraw => "image_draw",
        ImageInfo => "image_info",
        ServiceCall => "service_call",
        ServiceOpen => "service_open",
        ServicesQuery => "services_query",
        SettingsRead => "settings_read",
        StorageAppend => "storage_append",
        StorageReadAt => "storage_read_at",
        StorageRemove => "storage_remove",
        StorageRename => "storage_rename",
        StorageSize => "storage_size",
        StorageSync => "storage_sync",
        StorageWrite => "storage_write",
        StageRun => "stage_run",
        TextCovers => "text_covers",
        TextDraw => "text_draw",
        TextDrawInked => "text_draw_inked",
        TextInk => "text_ink",
        TextHyphenate => "text_hyphenate",
        TextMeasure => "text_measure",
        TextMetrics => "text_metrics",
        UiCompose => "ui_compose"
}

/// Begins a host slot, naming it: hold the result until the slot returns. The
/// call is noted where the plugin's frame records them (its `make`, #248); it
/// is counted with the `slot-counts` feature (a host-cost guard of the tests)
/// and named to the measurement build with `slot-names`.
#[inline(always)]
pub(super) fn enter_as(id: Id) -> Slot {
    #[cfg(feature = "slot-counts")]
    slot_counts::note(id);
    begin_slot(id)
}

/// As [`enter_as`], for `registrations.submit`: the one call `make` is meant to
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

/// How often each host slot was entered on this thread, for a test that
/// guards the cost of a user action in host crossings (the device spends
/// about a millisecond or more on each). Off by default; no shell turns it on.
/// A fixed array of cells: counting never allocates (the memory tests count
/// the simulated X4's heap) and never fails.
#[cfg(feature = "slot-counts")]
pub mod slot_counts {
    extern crate std;
    use super::{Id, NAMES};
    use alloc::vec::Vec;
    use core::cell::Cell;

    std::thread_local! {
        static COUNTS: [Cell<u32>; NAMES.len()] = const { [const { Cell::new(0) }; NAMES.len()] };
    }

    pub(super) fn note(id: Id) {
        // A thread being torn down has no counts to keep.
        let _ = COUNTS.try_with(|counts| {
            let count = &counts[id as usize];
            count.set(count.get().saturating_add(1));
        });
    }

    /// Forgets the counts of this thread.
    pub fn reset() {
        let _ = COUNTS.try_with(|counts| counts.iter().for_each(|count| count.set(0)));
    }

    /// The counts of this thread since `reset`, by slot name, the slots
    /// entered only.
    #[must_use]
    pub fn snapshot() -> Vec<(&'static str, u32)> {
        COUNTS
            .try_with(|counts| {
                NAMES
                    .iter()
                    .zip(counts)
                    .filter(|(_, count)| count.get() > 0)
                    .map(|(name, count)| (*name, count.get()))
                    .collect()
            })
            .unwrap_or_default()
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
                    let guarded = function.contains("host_slot::enter_as(")
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
            "host entries without `host_slot::enter_as()`: {missing:#?}"
        );
    }
}
