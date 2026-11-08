//! The kernel's changes to settings as they were declared: their
//! defaults, bounds and help.

use super::SettingValue;
#[cfg(any(test, feature = "kernel"))]
use super::{SettingError, SettingKind, Settings};
use crate::Id;

/// A default one plugin set on another's setting, with the default it
/// replaced, which comes back when that plugin goes.
#[derive(Debug)]
pub(super) struct DefaultBy {
    pub(super) by: Id,
    #[cfg_attr(
        not(any(test, feature = "kernel")),
        allow(dead_code, reason = "only the kernel changes defaults")
    )]
    pub(super) was: SettingValue,
}

#[cfg(any(test, feature = "kernel"))]
impl Settings {
    /// Makes room for `specs` more settings and `groups` more groups, so that
    /// defining them cannot fail for lack of memory.
    ///
    /// # Errors
    /// The heap refused the room (some may be kept).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn try_reserve(
        &mut self,
        specs: usize,
        groups: usize,
    ) -> Result<(), alloc::collections::TryReserveError> {
        self.specs.try_reserve(specs)?;
        self.groups.try_reserve(groups)
    }

    /// Forgets the settings and groups of `plugin` (ids `<plugin>.…`) and the
    /// defaults it set, e.g. when a plugin unloads. The values the reader
    /// chose stay in their store, for the plugin's next load.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn remove_plugin(&mut self, plugin: &str) {
        // The defaults the plugin set on the settings of others go back to
        // what they were (those of its own settings go with them).
        for (setting, set) in self.defaults_by.iter() {
            if set.by.namespace() == plugin
                && setting.namespace() != plugin
                && let Ok(spec) = self.specs.get_mut(setting.as_str())
                && spec.kind.accepts(&set.was)
            {
                spec.default = set.was.clone();
            }
        }
        self.specs.remove_plugin(plugin);
        self.groups.retain(|group| group.id.namespace() != plugin);
        self.defaults_by
            .retain(|setting, set| setting.namespace() != plugin && set.by.namespace() != plugin);
    }

    /// Whether `specs` more settings still find a place (a made setting
    /// takes one; so does a fixed one changed after it was set up).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn has_room_for(&self, specs: usize) -> bool {
        self.specs.has_room_for(specs)
    }

    /// Changes a setting's default (e.g. device-specific defaults chosen by
    /// a shell). The user's own values still take precedence.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_default_from_text(&mut self, id: &str, text: &str) -> Result<(), SettingError> {
        let spec = self.specs.get_mut(id)?;
        spec.default = spec.parse(text).ok_or(SettingError::InvalidValue)?;
        Ok(())
    }

    /// Changes a setting's default as plugin `by` gives it (its
    /// `settings.ini` or [`Registrar::set_setting_default`](crate::Registrar::set_setting_default)),
    /// which [`Settings::default_from`] names from then on (where the
    /// setting is another plugin's).
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_default_by(&mut self, id: &str, text: &str, by: Id) -> Result<(), SettingError> {
        let was = self.specs.get(id).map(|spec| spec.default.clone());
        self.set_default_from_text(id, text)?;
        let setting = self.specs.get_mut(id)?.id.clone();
        if setting.namespace() == by.as_str() {
            // A plugin's own settings: their own default, nothing to tell.
            self.defaults_by.remove(&setting);
        } else if let Some(set) = self.defaults_by.get_mut(&setting) {
            // The default before the first of them is what comes back.
            set.by = by;
        } else if let Some(was) = was {
            self.defaults_by.insert(setting, DefaultBy { by, was });
        }
        Ok(())
    }

    /// Changes the bounds of a [`SettingKind::Int`] setting, e.g. once the
    /// font it sizes is known: what may be set from then on (a value set
    /// before stays as it is). Whether it did: not for an unknown setting
    /// or one of another kind.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_range(&mut self, id: &str, min: i64, max: i64) -> bool {
        let max = max.max(min);
        match self.specs.get_mut(id) {
            Ok(spec) if matches!(spec.kind, SettingKind::Int { .. }) => {
                spec.kind = SettingKind::Int { min, max };
                true
            }
            _ => false,
        }
    }

    /// Changes a setting's help text (its translation key), e.g. to say
    /// why its bounds are what they are. Whether it did: not for an
    /// unknown setting.
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_description(&mut self, id: &str, key: &'static str) -> bool {
        match self.specs.get_mut(id) {
            Ok(spec) => {
                spec.description = Some(crate::Str::new_static(key));
                true
            }
            Err(_) => false,
        }
    }
}
