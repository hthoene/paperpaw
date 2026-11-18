//! The declaration cap a plugin was refused at (`docs/abi/registry.md`,
//! `reg_plugin.problem` 14 and 15): kept inside the host, next to the
//! declaration store, until the plugin is admitted again.
//!
//! The room is bounded and every allocation fallible: a refusal that cannot be
//! kept is not shown (its row says the general failure, as for any plugin
//! refused for another cause).
use super::declaration_checks::Cap;
use alloc::{boxed::Box, string::String, vec::Vec};

/// How many plugins' refusals are kept; one more is not.
const MAX: usize = 16;

struct Row {
    id: Box<str>,
    cap: Cap,
}

/// The refusals, at most one per plugin id.
#[derive(Default)]
pub(crate) struct Refusals {
    rows: Vec<Row>,
}

impl Refusals {
    /// Keeps that `plugin` was refused at `cap`, replacing what was kept for
    /// it. Not kept when there is no room.
    pub(crate) fn record(&mut self, plugin: &str, cap: Cap) {
        self.clear(plugin);
        if self.rows.len() >= MAX || self.rows.try_reserve(1).is_err() {
            return;
        }
        let mut id = String::new();
        if id.try_reserve_exact(plugin.len()).is_err() {
            return;
        }
        id.push_str(plugin);
        self.rows.push(Row {
            id: id.into_boxed_str(),
            cap,
        });
    }

    /// Forgets `plugin`'s refusal: it is admitted again, or tried again.
    pub(crate) fn clear(&mut self, plugin: &str) {
        self.rows.retain(|row| &*row.id != plugin);
    }

    /// The cap `plugin` was refused at.
    pub(crate) fn get(&self, plugin: &str) -> Option<Cap> {
        self.rows
            .iter()
            .find(|row| &*row.id == plugin)
            .map(|row| row.cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_kept_per_plugin_replaced_and_cleared() {
        let mut refusals = Refusals::default();
        refusals.record("a", Cap::of(Cap::FORMATS, 8));
        refusals.record("b", Cap::of(Cap::READERS, 4));
        refusals.record("a", Cap::of(Cap::FORMATS, 32));
        assert_eq!(refusals.get("a"), Some(Cap::of(Cap::FORMATS, 32)));
        assert_eq!(refusals.get("b"), Some(Cap::of(Cap::READERS, 4)));
        refusals.clear("a");
        assert_eq!(refusals.get("a"), None);
        assert_eq!(refusals.get("c"), None);
    }

    #[test]
    fn room_is_bounded() {
        let mut refusals = Refusals::default();
        for n in 0..=MAX {
            let id = alloc::format!("p{n}");
            refusals.record(&id, Cap::of(Cap::FORMATS, 8));
        }
        assert_eq!(refusals.get("p0"), Some(Cap::of(Cap::FORMATS, 8)));
        assert_eq!(refusals.get(&alloc::format!("p{MAX}")), None);
        // A kept plugin's own refusal is replaced even when full.
        refusals.record("p0", Cap::of(Cap::READERS, 16));
        assert_eq!(refusals.get("p0"), Some(Cap::of(Cap::READERS, 16)));
    }
}
