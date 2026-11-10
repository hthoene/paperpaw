//! What the start does about plugin changes a cut left (`docs/abi/device.md`
//! 5.4), before plugins are found: finish or set aside the journal of an
//! install or a removal (`plugin_swap::replay`), then sweep what no journal
//! owns, at most [`SWEEP_MAX`] folders a start.
//!
//! Swept: a stage `plugins/.<id>.part` (an install that was stopped or cut
//! before its journal), and a folder `.removing-<id>` of a removal, in
//! `plugins/` and in `.paperpaw/plugins/`. Never an `.old-<id>` folder: it can
//! be the only copy of a plugin, and the journal alone decides what becomes of
//! it. Nothing is swept while a journal is still there.
use alloc::{format, string::String, vec::Vec};
use paperpaw_platform::Storage;
use paperpaw_sdk::builtin::PLUGIN_DIR;

use super::plugin_swap::{JOURNAL, Replayed, TMP, replay};

/// Most folders swept at a start.
pub(crate) const SWEEP_MAX: usize = 8;

/// Finishes what a cut left (before any plugin is found); each thing worth a
/// line in the log is given to `say`.
pub(crate) fn replay_at_start(storage: &dyn Storage, say: &mut dyn FnMut(String)) {
    // A journal that was being written when the power went is never the
    // journal: only the renamed one counts.
    let _ = storage.remove(TMP);
    match replay(storage) {
        Replayed::Nothing | Replayed::Finished { .. } | Replayed::Removed { .. } => {}
        Replayed::SetAside => say(String::from(
            "plugins/.journal could not be finished: set aside as .journal.bad, no plugin was changed by it",
        )),
        Replayed::Failed(why) => {
            say(format!(
                "plugins/.journal not finished (reason {why}): tried again at the next start"
            ));
        }
    }
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
fn leftover(name: &str, dir: &str) -> bool {
    name.starts_with(".removing-")
        || (dir == PLUGIN_DIR && name.starts_with('.') && name.ends_with(".part") && name.len() > 6)
}

fn sweep_in(storage: &dyn Storage, dir: &str) {
    let mut found: Vec<String> = Vec::new();
    let _ = storage.list_from(dir, 0, &mut |entry| {
        if entry.is_dir && leftover(entry.name, dir) && found.len() < SWEEP_MAX {
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
        let _ = storage.remove_dir_all(&path);
    }
}

#[cfg(test)]
#[path = "plugin_start_tests.rs"]
mod tests;
