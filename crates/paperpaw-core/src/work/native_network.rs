//! The network settings of native plugins: each admitted native plugin that
//! has the feature `uses.network` gets `core.network.<id>` and
//! `core.saving.<id>`, the same specs, ids and defaults as an interface-6
//! plugin that uses the network, and the network policy follows them.
use super::{NetworkSettings, network_setting, network_specs, saving_setting};
use crate::Kernel;
use alloc::string::String;
use paperpaw_sdk::{Id, RegisterError};

/// The feature a plugin needs for the settings.
pub(crate) const USES_NETWORK: &str = "uses.network";

impl Kernel {
    /// Defines the two settings of `plugin` that every networked plugin has,
    /// by the list kept in the program where it names the plugin
    /// ([`network_settings!`](crate::network_settings)), else made on the
    /// heap. With `defined_ok`, one that is defined already is left as it is
    /// (a plugin loaded again keeps its settings).
    ///
    /// # Errors
    /// The id is not valid, or a setting cannot be defined.
    pub(super) fn define_network_pair(
        &mut self,
        plugin: &str,
        defined_ok: bool,
    ) -> Result<(), RegisterError> {
        let fixed: &'static [NetworkSettings] = self.works.fixed;
        match fixed.iter().find(|(p, _)| *p == plugin) {
            Some((_, specs)) => (0..specs.len()).try_for_each(|at| {
                if defined_ok && self.shared.settings.get(specs[at].id.as_str()).is_some() {
                    return Ok(());
                }
                self.shared.settings.define_fixed(specs, at)
            }),
            None => {
                let invalid = |_| RegisterError::Invalid {
                    message: "not an id for a network setting".into(),
                };
                let network = Id::parse(&network_setting(plugin)).map_err(invalid)?;
                let saving = Id::parse(&saving_setting(plugin)).map_err(invalid)?;
                for spec in network_specs(network, saving) {
                    if defined_ok && self.shared.settings.get(spec.id.as_str()).is_some() {
                        continue;
                    }
                    self.shared.settings.define(spec)?;
                }
                Ok(())
            }
        }
    }

    /// A native plugin was admitted: with the feature `uses.network`
    /// (`networked`, [`USES_NETWORK`] admitted) it gets its network settings
    /// and its network policy. Never refuses the plugin: where the room is
    /// not there or a setting cannot be defined a warning says so, and the
    /// plugin stays without (and follows no policy).
    pub(crate) fn admit_native_network(&mut self, plugin: &Id, networked: bool) {
        if !networked {
            return;
        }
        let name = plugin.as_str();
        if self.works.natives.iter().any(|known| known == name) {
            return;
        }
        let Some(kept) = copy(name) else {
            self.try_warn_runtime(&[name, ": not enough memory for its network settings"]);
            return;
        };
        if self.works.natives.try_reserve(1).is_err()
            || !self.shared.settings.has_room_for(2)
            || self.shared.settings.try_reserve(2, 0).is_err()
        {
            self.try_warn_runtime(&[name, ": not enough memory for its network settings"]);
            return;
        }
        if let Err(error) = self.define_network_pair(name, true) {
            self.warn_runtime(alloc::format!("{name}: {error:?}"));
            return;
        }
        self.apply_kept_network(name);
        self.works.natives.push(kept);
        self.works.stale = true;
    }

    /// A native plugin was removed: its network policy goes; its settings and
    /// the values the reader chose stay for its next load.
    pub(crate) fn forget_native_network(&mut self, plugin: &str) {
        let before = self.works.natives.len();
        self.works.natives.retain(|known| known != plugin);
        if self.works.natives.len() != before {
            self.works.stale = true;
        }
    }

    /// The values the reader kept for the two settings of `plugin` (the file
    /// was read before the plugin was there to define them), set now.
    fn apply_kept_network(&mut self, plugin: &str) {
        let (network, saving) = (network_setting(plugin), saving_setting(plugin));
        let settings = &mut self.shared.settings;
        // The reader's file, then the shell's own config over it, as at start.
        for source in [&self.keys.user, &self.keys.shell] {
            for entry in paperpaw_config::parse(source).flatten() {
                let id = alloc::format!("{}.{}", entry.section, entry.key);
                if id == network || id == saving {
                    // A value that does not fit stays as the default.
                    let value = entry.value.as_str();
                    let _ = settings.set_from_text(&id, &value);
                }
            }
        }
    }
}

fn copy(text: &str) -> Option<String> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len()).ok()?;
    copy.push_str(text);
    Some(copy)
}
