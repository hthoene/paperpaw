//! What the start does about plugin changes a cut left (`docs/abi/device.md`
//! 5.4), before plugins are found: finish or set aside the journal of an
//! install or a removal (`plugin_swap::replay`), then sweep what no journal
//! owns, at most [`SWEEP_MAX`] folders a start.
//!
//! Swept: a folder `.removing-<id>` of a removal, in `plugins/` and in
//! `.paperpaw/plugins/`, removed in bounded pieces ([`SWEEP_PIECES`] a folder a
//! start; what is left goes on at the next start), and a half-written journal. Never a stage
//! `.<id>.part` (the shell hands stages over to be installed after the start,
//! and the next install of the id removes its own leftover stage), and never
//! an `.old-<id>` folder: it can be the only copy of a plugin, and the journal
//! alone decides what becomes of it. Nothing is swept while a journal is still
//! there.
//!
//! Nothing here writes to the card unless there is something to do.
use alloc::{format, string::String, vec::Vec};
use paperpaw_platform::Storage;
use paperpaw_sdk::builtin::PLUGIN_DIR;

use super::plugin_swap::{JOURNAL, Replayed, TMP, replay};

/// Most folders swept at a start.
pub(crate) const SWEEP_MAX: usize = 8;

/// Finishes what a cut left (before any plugin is found); each thing worth a
/// line in the log is given to `say`. `true`: a removal journal waits for the
/// host to forget its plugin's secrets and grants, which the kernel's first pass
/// does (`plugin_swap::resume`); the journal stays until then.
pub(crate) fn replay_at_start(storage: &dyn Storage, say: &mut dyn FnMut(String)) -> bool {
    // A journal that was being written when the power went is never the
    // journal: only the renamed one counts.
    if storage.size(TMP).is_ok() {
        let _ = storage.remove(TMP);
    }
    match replay(storage) {
        Replayed::Nothing
        | Replayed::Finished { .. }
        | Replayed::Removed { .. }
        | Replayed::Unforgotten { .. } => {}
        Replayed::Forgetting => return true,
        Replayed::SetAside => say(String::from(
            "plugins/.journal could not be finished: set aside as .journal.bad, no plugin was changed by it",
        )),
        Replayed::Failed(why) => {
            say(format!(
                "plugins/.journal not finished (reason {why}): tried again at the next start"
            ));
        }
    }
    // Secrets that stayed after a removal: the first pass tries again.
    // (a list that cannot be read counts as there: the pass fails closed).
    !matches!(
        storage.size(super::plugin_unforgotten::FILE),
        Err(paperpaw_platform::StorageError::NotFound)
    )
}

/// Sweeps the leftovers no journal owns. Nothing is swept while a journal is
/// still there (one that could not be finished stays for the next start).
pub(crate) fn sweep(storage: &dyn Storage) {
    if storage.size(JOURNAL).is_ok() {
        return;
    }
    for dir in [PLUGIN_DIR, ".paperpaw/plugins"] {
        sweep_in(storage, dir);
    }
}

/// Whether `name` is a leftover this start owns.
fn leftover(name: &str) -> bool {
    name.starts_with(".removing-")
}

fn sweep_in(storage: &dyn Storage, dir: &str) {
    let mut found: Vec<String> = Vec::new();
    let _ = storage.list_from(dir, 0, &mut |entry| {
        if entry.is_dir && leftover(entry.name) && found.len() < SWEEP_MAX {
            let mut path = String::new();
            if path
                .try_reserve_exact(dir.len() + 1 + entry.name.len())
                .is_ok()
                && found.try_reserve(1).is_ok()
            {
                path.push_str(dir);
                path.push('/');
                path.push_str(entry.name);
                found.push(path);
            }
        }
        found.len() < SWEEP_MAX
    });
    for path in found {
        sweep_tree(storage, &path);
    }
}

/// Most bounded pieces one folder gets at a start (`remove_tree_step` removes
/// at most 64 entries and reads at most 256 per piece): a larger folder goes
/// on at the next start, still named `.removing-…`.
const SWEEP_PIECES: u32 = 32;

fn sweep_tree(storage: &dyn Storage, path: &str) {
    let mut place = 0;
    for _ in 0..SWEEP_PIECES {
        match storage.remove_tree_step(path, false, place) {
            Ok(step) if !step.done => place = step.next,
            Ok(_) | Err(_) => return,
        }
    }
}

#[cfg(test)]
#[path = "plugin_start_tests.rs"]
mod tests;
