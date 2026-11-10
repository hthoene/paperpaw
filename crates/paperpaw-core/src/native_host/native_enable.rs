//! The on/off state of native plugins, as interface 6 gave it to every plugin
//! (`[plugins] <id> = on|off` in `paperpaw.ini`, the plugin manager's switch,
//! a plugin shipped off): the shell asks [`Kernel::native_enabled`] **before**
//! it hosts a plugin, hosts only an enabled one, and lists every native plugin
//! it has with [`Kernel::list_native`], so the plugin manager shows it, a
//! switch is saved (`Command::SetPluginEnabled`, for the next start, as in
//! Beta 3), and a removal drops it from the list.
//!
//! - **Decision.** A plugin switched off in any configuration (the file on the
//!   device, the shell's overrides) is off; otherwise one switched on is on;
//!   otherwise the shipped default (`default_on`). The file is read anew at
//!   each call, so a switch saved since is known.
//! - **Off is listed, not hosted.** An off plugin costs no instance and no
//!   declarations: its row says off ([`PluginState::Off`]); switching it on
//!   asks for a restart, as before.
use crate::{Kernel, RESERVED_PLUGIN_ID, config::read_config};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID};
use paperpaw_sdk::PluginInfo;

/// What `source` says about the plugin `id` in its `[plugins]` section: `off`
/// anywhere wins over `on`; `None` where it says nothing.
fn switched(source: &str, id: &str) -> Option<bool> {
    let mut found = None;
    for entry in paperpaw_config::parse(source).filter_map(Result::ok) {
        if entry.section != "plugins" || entry.key != id {
            continue;
        }
        match entry.value.as_bool() {
            Some(false) => return Some(false),
            Some(true) => found = Some(true),
            None => {}
        }
    }
    found
}

/// Whether the plugin `id` is on, by the configurations (`file`, then the
/// shell's `overrides`) and the shipped default.
pub(crate) fn decide(file: Option<&str>, overrides: &str, id: &str, default_on: bool) -> bool {
    let (a, b) = (
        file.and_then(|source| switched(source, id)),
        switched(overrides, id),
    );
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => false,
        (Some(true), _) | (_, Some(true)) => true,
        (None, None) => default_on,
    }
}

impl Kernel {
    /// Whether the native plugin `id` runs this start: switched off in a
    /// configuration, no; switched on, yes; otherwise `default_on` (a plugin
    /// shipped off passes `false`). Ask before hosting it; host it only when
    /// this says yes, and list it either way ([`Self::list_native`]).
    #[must_use]
    pub fn native_enabled(&self, id: &str, default_on: bool) -> bool {
        // A file that cannot be read says nothing; the copy read at start
        // stands in (it is what the builder used for the other plugins).
        let file = read_config(self.shared.storage.as_ref())
            .ok()
            .flatten()
            .unwrap_or_else(|| self.keys.user.clone());
        decide(Some(&file), &self.keys.shell, id, default_on)
    }

    /// Lists a native plugin for the plugin manager: `info` says what it is,
    /// its state (`Running` once hosted, `Off` when it is not hosted, `Failed`
    /// when hosting failed) and whether it is switched on. Call it after the
    /// plugin is hosted, so a slot only it fills marks it as one that stays on.
    ///
    /// # Errors
    /// `ERR_DENIED` for the kernel's own id; `ERR_INVALID` for an id that is
    /// listed already.
    pub fn list_native(&mut self, info: PluginInfo) -> Result<(), i32> {
        let id = info.manifest.id.clone();
        if id.as_str() == RESERVED_PLUGIN_ID {
            return Err(ERR_DENIED);
        }
        if self.shared.plugins.get(id.as_str()).is_some() {
            return Err(ERR_INVALID);
        }
        self.shared.registry_generation.bump();
        self.shared.plugins.push(info);
        self.find_required_plugins();
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_enable_tests.rs"]
mod tests;
