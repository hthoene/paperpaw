//! The kernel's changes to settings as they were declared: their
//! defaults, bounds and help.

use super::SettingValue;
#[cfg(any(test, feature = "kernel"))]
use super::{SettingError, SettingKind, Settings};
use crate::Id;

/// What plugins set as the default of a setting that is not theirs: the
/// default before the first of them, and each plugin's value in the order they
/// came (the last one is in force). When a plugin goes, its layer goes and the
/// one below takes over; the default before all of them comes back when the
/// last goes.
#[derive(Debug)]
pub(super) struct DefaultBy {
    #[cfg_attr(
        not(any(test, feature = "kernel")),
        allow(dead_code, reason = "only the kernel changes defaults")
    )]
    pub(super) was: SettingValue,
    pub(super) layers: alloc::vec::Vec<Layer>,
}

/// One plugin's default for another's setting.
#[derive(Debug)]
pub(super) struct Layer {
    pub(super) by: Id,
    #[cfg_attr(
        not(any(test, feature = "kernel")),
        allow(dead_code, reason = "only the kernel changes defaults")
    )]
    pub(super) value: SettingValue,
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
        self.specs.try_reserve(specs, 0)?;
        self.groups.try_reserve_exact(groups)
    }

    /// [`Settings::try_reserve`] for `owned` settings made on the heap and
    /// `fixed` kept in the program ([`Settings::define_fixed`]), which need
    /// only their places in the indexes.
    ///
    /// # Errors
    /// The heap refused the room (some may be kept).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn try_reserve_with_fixed(
        &mut self,
        owned: usize,
        fixed: usize,
        groups: usize,
    ) -> Result<(), alloc::collections::TryReserveError> {
        self.specs.try_reserve(owned, fixed)?;
        self.groups.try_reserve_exact(groups)
    }

    /// Forgets the settings and groups of `plugin` (ids `<plugin>.…`) and the
    /// defaults it set, e.g. when a plugin unloads. The values the reader
    /// chose stay in their store, for the plugin's next load.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn remove_plugin(&mut self, plugin: &str) {
        // The defaults the plugin set on the settings of others go back to
        // what the plugins below it set, or to what they were (those of its
        // own settings go with them).
        for (setting, set) in self.defaults_by.iter() {
            // Only the top layer sets the default in force: a layer lower
            // down going changes nothing there.
            if setting.namespace() == plugin
                || !set
                    .layers
                    .last()
                    .is_some_and(|l| l.by.namespace() == plugin)
            {
                continue;
            }
            let mut below = set
                .layers
                .iter()
                .rev()
                .filter(|layer| layer.by.namespace() != plugin)
                .map(|layer| &layer.value);
            if self.specs.is_described(setting.as_str()) {
                // Its kind is as it was when each layer was checked (changed,
                // it would be described no more): nothing is made to check.
                if let Ok(default) = self.specs.default_mut(setting.as_str()) {
                    *default = below.next().unwrap_or(&set.was).clone();
                }
                continue;
            }
            let Ok(spec) = self.specs.get_mut(setting.as_str()) else {
                continue;
            };
            let below = below
                .find(|value| spec.kind.accepts(value))
                .unwrap_or(&set.was);
            if spec.kind.accepts(below) {
                spec.default = below.clone();
            }
        }
        self.specs.remove_plugin(plugin);
        self.groups.retain(|group| group.id.namespace() != plugin);
        for set in self.defaults_by.values_mut() {
            set.layers.retain(|layer| layer.by.namespace() != plugin);
        }
        self.defaults_by
            .retain(|setting, set| setting.namespace() != plugin && !set.layers.is_empty());
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
        if self.specs.is_described(id) {
            // Read once to check the text; the default kept is what counts.
            let value = self
                .specs
                .with(id, |spec| spec.parse(text))?
                .ok_or(SettingError::Unknown)?
                .ok_or(SettingError::InvalidValue)?;
            *self.specs.default_mut(id)? = value;
            return Ok(());
        }
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
        let was = self.specs.locate(id).map(|at| at.default().clone());
        let (value, setting) = self
            .specs
            .with(id, |spec| (spec.parse(text), spec.id.try_clone()))?
            .ok_or(SettingError::Unknown)?;
        let value = value.ok_or(SettingError::InvalidValue)?;
        let setting = setting.ok_or(SettingError::Full)?;
        let foreign = setting.namespace() != by.as_str();
        // All room first, so that a refusal changes nothing. Exactly what is
        // needed: a plain reserve would take four places on the X4.
        let mut first = None;
        if foreign {
            match self.defaults_by.get_mut(id) {
                // The same plugin again takes its own layer's place.
                Some(set) if set.layers.iter().any(|layer| layer.by == by) => {}
                Some(set) => set
                    .layers
                    .try_reserve_exact(1)
                    .map_err(|_| SettingError::Full)?,
                None => {
                    self.defaults_by
                        .try_reserve_exact(1)
                        .map_err(|_| SettingError::Full)?;
                    let mut layers = alloc::vec::Vec::new();
                    layers
                        .try_reserve_exact(1)
                        .map_err(|_| SettingError::Full)?;
                    first = Some(layers);
                }
            }
        }
        self.set_default_from_text(id, text)?;
        if !foreign {
            // A plugin's own settings: their own default, nothing to tell.
            self.defaults_by.remove(&setting);
        } else if let Some(set) = self.defaults_by.get_mut(&setting) {
            // The same plugin again: its layer moves to the top.
            set.layers.retain(|layer| layer.by != by);
            set.layers.push(Layer { by, value });
        } else if let (Some(was), Some(mut layers)) = (was, first) {
            layers.push(Layer { by, value });
            self.defaults_by.insert(setting, DefaultBy { was, layers });
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
