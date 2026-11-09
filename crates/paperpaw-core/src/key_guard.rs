//! The key map while a screen guards the device (`docs/abi/ui.md` section 4.3,
//! "Key maps"): only the own key layer of the screen that gets input
//! ([`Command::SetKeyLayer`](crate::Command)) and bindings to host actions or to
//! the guard's plugin's own actions run. A binding of any other layer that names
//! another plugin's action does nothing, and is logged, so a key map can never
//! run another plugin's code past the lock.
use crate::{Kernel, keymap::InputKey};
use alloc::{format, string::String};
use paperpaw_sdk::Id;

/// The namespace of the host's own actions (`core.menu`, `core.back`, ...).
const HOST: &str = "core";

impl Kernel {
    /// What `key` is bound to on screen `index` (see [`Self::layers_of`]) and
    /// may run now: the action of the first layer that binds it, unless a guard
    /// is up and the action is not allowed to run (see the module).
    pub(crate) fn bound_action(&mut self, index: usize, key: &InputKey) -> Option<Id> {
        let (layers, count) = self.layers_of(index);
        let (at, action) = self.shared.keymap.resolve_layer(&layers[..count], key)?;
        if self.guarded_binding_runs(index, layers[at], action) {
            return Some(action.clone());
        }
        let message = format!(
            "the binding of {} in {} to {action} does nothing while a screen guards the device",
            key.name(),
            layers[at]
        );
        self.warn_once(message);
        None
    }

    /// The same for the action of a tap in a zone of a layer's own (which layer
    /// owns it is not kept: only a host action or the guard's plugin's own runs).
    pub(crate) fn zone_action(&mut self, index: usize, action: Id) -> Option<Id> {
        if self.guarded_binding_runs(index, "", &action) {
            return Some(action);
        }
        let message = format!(
            "the binding of a zone to {action} does nothing while a screen guards the device"
        );
        self.warn_once(message);
        None
    }

    /// Whether the binding of `action` found in `layer` runs for the screen at
    /// `index`: always with no guard up; else when it is the own key layer of
    /// that screen, a host action, or an action of the guard's plugin.
    fn guarded_binding_runs(&self, index: usize, layer: &str, action: &Id) -> bool {
        let Some(guard) = self.guard() else {
            return true;
        };
        let own_layer = self
            .stack
            .get(index)
            .and_then(|active| active.key_layer.as_ref())
            .is_some_and(|own| own.as_str() == layer);
        own_layer
            || action.namespace() == HOST
            || self
                .stack
                .get(guard)
                .is_some_and(|active| active.id.namespace() == action.namespace())
    }

    /// A warning said once: the same text twice is kept once.
    fn warn_once(&mut self, message: String) {
        if !self
            .warnings
            .iter()
            .any(|warning| warning.message == message)
        {
            self.warn_runtime(message);
        }
    }
}
