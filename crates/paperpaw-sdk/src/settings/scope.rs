//! The kernel's side of the active scope: the values of the book being
//! read, which take the place of the global ones of scoped settings.

use alloc::vec::Vec;

use super::{SettingError, SettingValue, Settings};
use crate::collections::VecMap;
use crate::id::Id;

impl Settings {
    /// Activates a scope with its values (only valid scoped ones are
    /// kept), or ends it. Where the heap has no room to check a value
    /// ([`Settings::try_set_scope`]) no scope is active afterwards.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_scope(&mut self, values: Option<VecMap<Id, SettingValue>>) {
        if self.try_set_scope(values).is_err() {
            self.scope = None;
        }
    }

    /// [`Settings::set_scope`] that tells when the values could not all be
    /// checked: the scope is as it was then, so that a part of its values
    /// is never taken for all of them.
    ///
    /// # Errors
    /// [`SettingError::Full`]: the heap has no room to read what a plugin
    /// loaded from the card describes of a setting.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn try_set_scope(
        &mut self,
        values: Option<VecMap<Id, SettingValue>>,
    ) -> Result<(), SettingError> {
        let Some(mut values) = values else {
            self.scope = None;
            return Ok(());
        };
        let ids: Vec<Id> = values.keys().cloned().collect();
        for id in ids {
            let scoped = self.specs.locate(id.as_str()).is_some_and(|at| at.scoped());
            let valid = match values.get(id.as_str()) {
                Some(value) if scoped => self
                    .specs
                    .with(id.as_str(), |spec| spec.accepts(value))?
                    .unwrap_or(false),
                _ => false,
            };
            if !valid {
                values.remove(id.as_str());
            }
        }
        self.scope = Some(values);
        Ok(())
    }

    /// Sets a scoped setting in the active scope. Switching the scope off
    /// ([`SCOPE_ONLY`](crate::builtin::settings::SCOPE_ONLY)) drops its
    /// values.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_in_scope(&mut self, id: &str, value: SettingValue) -> Result<(), SettingError> {
        let id = self
            .specs
            .with(id, |spec| {
                if !spec.scoped || !spec.accepts(&value) {
                    return Err(SettingError::InvalidValue);
                }
                spec.id.try_clone().ok_or(SettingError::Full)
            })?
            .ok_or(SettingError::Unknown)??;
        let scope = self.scope.as_mut().ok_or(SettingError::Unknown)?;
        if id == crate::builtin::settings::SCOPE_ONLY && value == SettingValue::Bool(false) {
            scope.clear();
            return Ok(());
        }
        scope.insert(id, value);
        Ok(())
    }
}
