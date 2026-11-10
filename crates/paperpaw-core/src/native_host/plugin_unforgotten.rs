//! The plugins whose secrets the host could not forget when they were removed
//! with their data (`plugin_swap::FORGET_TRIES` starts in a row): a small,
//! bounded list in `plugins/.unforgotten`, one id a line.
//!
//! The removal is finished without them (the journal must not hold every later
//! install), but their secrets stay on the card, and a plugin of the same id
//! installed later could read them. So the list is **kept until the host
//! forgets them** (a try at every start, [`retry`]) and an install or a check
//! of a listed id is refused (`ID_TAKEN`) until then.
//!
//! The file is only ever rewritten whole (it is a few lines) and read
//! defensively: a line that is no id is ignored, a read error is "not listed".
use alloc::{string::String, vec::Vec};
use paperpaw_platform::Storage;

use super::plugin_job::ID_MAX;

/// The list.
pub(crate) const FILE: &str = "plugins/.unforgotten";
/// Most ids on it; a ninth removal that cannot be recorded keeps its journal.
pub(crate) const MAX: usize = 8;
/// Most bytes read: [`MAX`] ids and their line ends.
const READ_MAX: usize = MAX * (ID_MAX + 1);

/// The ids on the list.
fn read(storage: &dyn Storage) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let Ok(size) = storage.size(FILE) else {
        return ids;
    };
    let mut bytes: Vec<u8> = Vec::new();
    let len = usize::try_from(size).unwrap_or(usize::MAX).min(READ_MAX);
    if bytes.try_reserve_exact(len).is_err() {
        return ids;
    }
    bytes.resize(len, 0);
    let Ok(got) = storage.read_at(FILE, 0, &mut bytes) else {
        return ids;
    };
    bytes.truncate(got);
    for line in String::from_utf8_lossy(&bytes).lines() {
        let line = line.trim();
        if line.is_empty() || line.len() > ID_MAX || ids.len() >= MAX {
            continue;
        }
        let mut id = String::new();
        if id.try_reserve_exact(line.len()).is_err() {
            break;
        }
        id.push_str(line);
        if ids.try_reserve(1).is_ok() {
            ids.push(id);
        }
    }
    ids
}

/// Writes `ids` whole; `false`: the card refused.
fn write(storage: &dyn Storage, ids: &[String]) -> bool {
    if ids.is_empty() {
        return matches!(
            storage.remove(FILE),
            Ok(()) | Err(paperpaw_platform::StorageError::NotFound)
        );
    }
    let mut text = String::new();
    let need: usize = ids.iter().map(|id| id.len() + 1).sum();
    if text.try_reserve_exact(need).is_err() {
        return false;
    }
    for id in ids {
        text.push_str(id);
        text.push('\n');
    }
    storage.write(FILE, text.as_bytes()).is_ok()
}

/// Whether the removed plugin `id` still has secrets on the card.
pub(crate) fn has(storage: &dyn Storage, id: &str) -> bool {
    read(storage).iter().any(|listed| listed == id)
}

/// Puts `id` on the list; `false` when it cannot be recorded (the list is full
/// or the card refuses): the removal then waits on.
pub(crate) fn add(storage: &dyn Storage, id: &str) -> bool {
    let mut ids = read(storage);
    if ids.iter().any(|listed| listed == id) {
        return true;
    }
    let mut owned = String::new();
    if ids.len() >= MAX || owned.try_reserve_exact(id.len()).is_err() || ids.try_reserve(1).is_err()
    {
        return false;
    }
    owned.push_str(id);
    ids.push(owned);
    write(storage, &ids)
}

/// Takes `id` off the list.
fn remove(storage: &dyn Storage, id: &str) -> bool {
    let mut ids = read(storage);
    ids.retain(|listed| listed != id);
    write(storage, &ids)
}

/// One try at forgetting each listed plugin's secrets: `forget` says whether
/// it could; those that could leave the list. How many are left.
pub(crate) fn retry(storage: &dyn Storage, forget: &mut dyn FnMut(&str) -> bool) -> usize {
    let ids = read(storage);
    let mut left = ids.len();
    for id in &ids {
        if forget(id) && remove(storage, id) {
            left -= 1;
        }
    }
    left
}

#[cfg(test)]
#[path = "plugin_unforgotten_tests.rs"]
mod tests;
