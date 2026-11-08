//! The kernel's changes to settings as they were declared: their
//! defaults, bounds and help.

#[cfg(any(test, feature = "kernel"))]
#[cfg(any(test, feature = "kernel"))]
use super::{SettingError, SettingKind, Settings};
#[cfg(any(test, feature = "kernel"))]
use crate::Id;

#[cfg(any(test, feature = "kernel"))]
impl Settings {
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
            self.defaults_by
                .insert(setting, super::DefaultBy { by, was });
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
