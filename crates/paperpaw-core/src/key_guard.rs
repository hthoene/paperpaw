//! The key map while a screen guards the device (`docs/abi/ui.md` section 4.3,
//! "Key maps"): a key resolves only to the guarding plugin's own actions and to
//! the host's (`core.*`), whichever layer or source binds it (declared, preset,
//! user, or another plugin's row into the guard's layer, which the plugin that
//! asks cannot make run its own code past the lock). Any other binding does
//! nothing and is logged once. Checked where a key is looked up, not when it is
//! bound.
use crate::{Kernel, keymap::InputKey};
use alloc::format;
use core::{cell::Cell, fmt::Write};
use paperpaw_sdk::Id;

/// The namespace of the host's own actions (`core.menu`, `core.back`, ...).
const HOST: &str = "core";

/// Refused bindings told of at most this many times in a run.
const WARNED_MAX: usize = 16;

/// The bindings already warned of, by a 32-bit hash of the layer, the input
/// and the action: a fixed table, checked before any text is made, so a key
/// pressed again and again costs no allocation and no second warning, and it
/// survives [`Kernel::take_warnings`]. Full, no further binding is told of
/// (they do nothing all the same); a collision silences a binding at worst.
#[derive(Debug, Default)]
pub(crate) struct Warned {
    seen: [Cell<u32>; WARNED_MAX],
    used: Cell<usize>,
}

impl Warned {
    /// Whether `hash` is new, and then remembered; `false` for one seen
    /// before and for any while the table is full.
    fn first(&self, hash: u32) -> bool {
        let used = self.used.get();
        if used == WARNED_MAX || self.seen[..used].iter().any(|seen| seen.get() == hash) {
            return false;
        }
        self.seen[used].set(hash);
        self.used.set(used + 1);
        true
    }
}

/// FNV-1a over what is written to it: hashes a name without making it.
struct Fnv(u32);

impl Fnv {
    const START: u32 = 0x811c_9dc5;

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
        }
    }
}

impl Write for Fnv {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        self.bytes(text.as_bytes());
        Ok(())
    }
}

impl Kernel {
    /// What `key` is bound to on screen `index` (see [`Self::layers_of`]) and
    /// may run now: the action of the first layer that binds it, unless a guard
    /// is up and the action is not allowed to run (see the module).
    pub(crate) fn bound_action(&mut self, index: usize, key: &InputKey) -> Option<Id> {
        let (layers, count) = self.layers_of(index);
        let (at, action) = self.shared.keymap.resolve_layer(&layers[..count], key)?;
        if self.guarded_binding_runs(action) {
            return Some(action.clone());
        }
        let mut hash = Fnv(Fnv::START);
        let _ = key.write_name(&mut hash);
        hash.bytes(layers[at].as_bytes());
        hash.bytes(action.as_str().as_bytes());
        if self.shared.key_guard_warned.first(hash.0) {
            let message = format!(
                "the binding of {} in {} to {action} does nothing while a screen guards the device",
                key.name(),
                layers[at]
            );
            self.warn_runtime(message);
        }
        None
    }

    /// The same for the action of a tap in a zone of a layer's own (which layer
    /// owns it is not kept: only a host action or the guard's plugin's own runs).
    pub(crate) fn zone_action(&mut self, action: Id) -> Option<Id> {
        if self.guarded_binding_runs(&action) {
            return Some(action);
        }
        let mut hash = Fnv(Fnv::START);
        hash.bytes(b"zone");
        hash.bytes(action.as_str().as_bytes());
        if self.shared.key_guard_warned.first(hash.0) {
            let message = format!(
                "the binding of a zone to {action} does nothing while a screen guards the device"
            );
            self.warn_runtime(message);
        }
        None
    }

    /// Whether the binding of `action` runs for the screen at `index`: always
    /// with no guard up; under a guard only an action of the guarding plugin or
    /// of the host (`ui.md` 4.3, ruled by the action's owner: a plugin's layer
    /// of its own, or any plugin's row into it, makes no other action run).
    fn guarded_binding_runs(&self, action: &Id) -> bool {
        let Some(guard) = self.guard() else {
            return true;
        };
        self.is_host_action(action)
            || self
                .stack
                .get(guard)
                .is_some_and(|active| active.id.namespace() == action.namespace())
    }

    /// Whether `action` is the host's own: `core.*`, and not an action of a
    /// plugin whose id merely begins with `core.` (a plugin id is not reserved
    /// yet): no loaded plugin has an id that is a leading part of it.
    fn is_host_action(&self, action: &Id) -> bool {
        if action.namespace() != HOST {
            return false;
        }
        let text = action.as_str();
        !text
            .match_indices('.')
            .skip(1)
            .any(|(at, _)| self.shared.native_instances.find(&text[..at]).is_some())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
    use super::*;

    #[test]
    fn a_hash_is_told_of_once_and_the_table_is_bounded() {
        let warned = Warned::default();
        assert!(warned.first(1));
        assert!(!warned.first(1));
        for hash in 2..=WARNED_MAX as u32 {
            assert!(warned.first(hash));
        }
        // Full: nothing more is told of, and nothing is lost or overwritten.
        assert!(!warned.first(1_000));
        assert!(!warned.first(1));
    }

    /// A plugin id is not reserved yet: `core.evil.x` is that plugin's action,
    /// not the host's, so it does not run under a guard; `core.menu` does.
    #[test]
    fn an_action_of_a_plugin_whose_id_begins_with_core_is_not_the_hosts() {
        let mut kernel = Kernel::builder(crate::tests::DEVICE).build().unwrap();
        kernel
            .shared
            .native_instances
            .test_insert(Id::try_parse("core.evil").unwrap(), None, true)
            .unwrap();
        assert!(kernel.is_host_action(&Id::try_parse("core.menu").unwrap()));
        assert!(kernel.is_host_action(&Id::try_parse("core.page-next").unwrap()));
        assert!(!kernel.is_host_action(&Id::try_parse("core.evil.x").unwrap()));
        assert!(!kernel.is_host_action(&Id::try_parse("other.run").unwrap()));
    }
}
