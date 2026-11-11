//! The ids of the plugins linked into the firmware (`Kernel::register_linked`),
//! for `core.files` root 2: a built-in plugin has no install folder on the
//! card, whatever a leftover `plugins/<id>/` holds.
//!
//! A card (`.ppn`) plugin never takes a linked plugin's id, and nothing
//! replaces a linked plugin but a firmware update (`device.md` 5.5: reason 15,
//! "id taken (built in, or another source's)", `ERR_DENIED`). An install or a
//! check refuses with it; a `.ppn` found on the card at start with a linked id
//! is not loaded (the linked plugin stays; the card file is logged and left
//! alone, never deleted). So for root 2 a linked plugin's `plugins/<id>/` is
//! always empty, whatever the card holds there.
//!
//! The kernel adds an id before it registers the plugin and takes it back
//! when the registration fails, so an id is in the set only for a plugin that
//! is linked. Every allocation is fallible; a set that is in use answers "yes"
//! (the safe side: the leftover folder stays unreadable).
use alloc::{string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY};

/// The linked plugin ids, device-wide.
#[derive(Default)]
pub(crate) struct LinkedPlugins(RefCell<Vec<String>>);

impl core::fmt::Debug for LinkedPlugins {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LinkedPlugins")
            .field("ids", &self.0.try_borrow().map_or(0, |ids| ids.len()))
            .finish()
    }
}

impl LinkedPlugins {
    /// Gives back the room kept while the ids were added one by one.
    pub(crate) fn shrink_to_fit(&self) {
        if let Ok(mut ids) = self.0.try_borrow_mut() {
            super::shrink::shrink(&mut ids);
        }
    }

    /// Adds `id` (once).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`; `ERR_BUSY` when the set is in use.
    pub(crate) fn add(&self, id: &str) -> Result<(), i32> {
        let mut ids = self.0.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        if ids.iter().any(|have| have == id) {
            return Ok(());
        }
        ids.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        let mut copy = String::new();
        copy.try_reserve_exact(id.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        copy.push_str(id);
        ids.push(copy);
        Ok(())
    }

    /// Takes `id` back (a registration that failed).
    pub(crate) fn remove(&self, id: &str) {
        if let Ok(mut ids) = self.0.try_borrow_mut() {
            ids.retain(|have| have != id);
        }
    }

    /// Whether `id` is a linked plugin. A set in use says yes.
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.0
            .try_borrow()
            .map_or(true, |ids| ids.iter().any(|have| have == id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_in_once_added_and_out_once_taken_back() {
        let set = LinkedPlugins::default();
        assert!(!set.contains("home"));
        set.add("home").unwrap();
        set.add("home").unwrap();
        assert!(set.contains("home") && !set.contains("hom") && !set.contains("homes"));
        set.remove("home");
        assert!(!set.contains("home"));
    }

    #[test]
    fn a_set_in_use_says_yes() {
        let set = LinkedPlugins::default();
        let _held = set.0.borrow_mut();
        assert!(set.contains("anything"));
        assert_eq!(set.add("x"), Err(ERR_BUSY));
    }
}
