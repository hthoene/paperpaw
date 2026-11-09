//! The key bindings native plugins declared (`key.bind`, `registry.md`
//! section 6) in the kernel's key map.
//!
//! The rows live in the declaration store, charged there, and leave with their
//! owner (`begin_native_unload`); this module reads them when the key map is
//! built (`Kernel::rebuild_keymap`), after the plugins' key map sources and
//! before the device's and the user's, so the user's bindings win. A row with
//! a condition (`when_key`, `when_value`) binds while that setting has that
//! value, as `shown_if` reads it; a change of that setting builds the key map
//! anew.
use super::{
    declaration_store::Kind, native_declarations::NativeDeclarations, registrations::Record,
    registry_service::rules::value_holds,
};
use crate::keymap::Keymap;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::Settings;

impl NativeDeclarations {
    /// Each declared binding whose condition holds, as `(layer, input,
    /// action)`; `ERR_BUSY` when the store is borrowed (none was visited).
    fn key_bindings(
        &self,
        settings: &Settings,
        visit: &mut dyn FnMut(&str, &str, &str),
    ) -> Result<(), i32> {
        let store = self.store.try_borrow().map_err(|_| ERR_BUSY)?;
        store.keys(Kind::KeyBind, &mut |key| {
            store.with_record(key, &mut |record| {
                if let Record::KeyBind(bind) = record
                    && value_holds(settings, bind.when_key, bind.when_value)
                {
                    visit(bind.layer, bind.input, bind.action);
                }
            });
        });
        Ok(())
    }

    /// How many bindings are declared, and whether one is conditioned on the
    /// setting `setting` (a change of it builds the key map anew).
    fn key_summary(&self, setting: &str) -> Result<(usize, bool), i32> {
        let store = self.store.try_borrow().map_err(|_| ERR_BUSY)?;
        let (mut count, mut on_setting) = (0, false);
        store.keys(Kind::KeyBind, &mut |key| {
            count += 1;
            store.with_record(key, &mut |record| {
                if let Record::KeyBind(bind) = record {
                    on_setting |= !setting.is_empty() && bind.when_key == setting;
                }
            });
        });
        Ok((count, on_setting))
    }
}

impl crate::Kernel {
    /// Binds the native plugins' declared bindings in `keymap`.
    pub(crate) fn native_keys_into(&self, keymap: &mut Keymap) {
        let Some(declarations) = &self.shared.registry.native_declarations else {
            return;
        };
        // A store in use (never during a rebuild) leaves the bindings out
        // until the next one.
        let _ = declarations.key_bindings(&self.shared.settings, &mut |layer, input, action| {
            // Checked at admission (`native_binding_ok`).
            keymap.bind_native(layer, input, action);
        });
    }

    /// How many bindings native plugins declared.
    pub(crate) fn native_key_count(&self) -> usize {
        self.shared
            .registry
            .native_declarations
            .as_ref()
            .and_then(|declarations| declarations.key_summary("").ok())
            .map_or(0, |(count, _)| count)
    }

    /// Whether a native binding is conditioned on the setting `setting`.
    pub(crate) fn native_key_when(&self, setting: &str) -> bool {
        self.shared
            .registry
            .native_declarations
            .as_ref()
            .and_then(|declarations| declarations.key_summary(setting).ok())
            .is_some_and(|(_, on_setting)| on_setting)
    }
}
