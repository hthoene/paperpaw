//! The on/off state of native plugins, as interface 6 gave it to every plugin
//! (`[plugins] <id> = on|off` in `paperpaw.ini`, the plugin manager's switch,
//! a plugin shipped off, safe mode): the shell reads the configuration once
//! ([`Kernel::native_config`]), asks [`Kernel::native_decide`] for **each**
//! plugin before it hosts it, hosts only one that says [`NativeDecision::Host`],
//! and lists every native plugin it has ([`Kernel::list_native`]) so the
//! plugin manager shows it, a switch is saved (`Command::SetPluginEnabled`,
//! for the next start, as in Beta 3) and a removal drops it from the list.
//!
//! - **Decision.** A plugin switched off in any configuration (the file on the
//!   device, the shell's overrides) is off; otherwise one switched on is on;
//!   otherwise the shipped default (`default_on`).
//! - **Safe mode** (the start went without code plugins,
//!   [`KernelBuilder::safe_mode`](crate::KernelBuilder::safe_mode)), as for
//!   code plugins in interface 6 (`code_plugin::hold_back`): a plugin loaded
//!   from the card is not hosted; the one the crash was in is switched off
//!   **in the file** (also a built-in one), or, when that is not known, every
//!   card plugin that was on. Built-in plugins otherwise run. The ones switched
//!   off are added to [`SafeMode::switched_off`](paperpaw_sdk::native::SafeMode).
//!   A plugin that crashes at every start is thus off from the next one on.
//! - **Off is listed, not hosted.** An off plugin costs no instance and no
//!   declarations: its row says off (`PluginState::Off`); switching it on
//!   asks for a restart, as before.
use crate::{Kernel, RESERVED_PLUGIN_ID, config::read_config};
use alloc::string::String;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID};
use paperpaw_sdk::{Id, PluginInfo, PluginProblem, native::SafeModeReason};

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

/// The configuration the on/off decisions are made from, read once at the
/// start ([`Kernel::native_config`]): the file on the device and the shell's
/// overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeConfig {
    file: String,
    overrides: String,
}

/// What [`Kernel::native_decide`] says about a native plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NativeDecision {
    /// Host it, then list it running.
    Host,
    /// Switched off: list it off, do not host it.
    Off,
    /// Safe mode: not hosted this start ([`PluginProblem::SafeMode`]);
    /// `enabled` is whether it stays switched on for the next start (false:
    /// safe mode switched it off, no crash was known).
    SafeMode {
        /// Still switched on in the file.
        enabled: bool,
    },
    /// The plugin the last crash was in: switched off in the file
    /// ([`PluginProblem::Crashed`]).
    Crashed,
}

impl NativeDecision {
    /// Whether the plugin is to be hosted.
    #[must_use]
    pub const fn hosts(self) -> bool {
        matches!(self, Self::Host)
    }

    /// Whether to list it as switched on.
    #[must_use]
    pub const fn enabled(self) -> bool {
        match self {
            Self::Host => true,
            Self::Off | Self::Crashed => false,
            Self::SafeMode { enabled } => enabled,
        }
    }

    /// Why it is not hosted, where safe mode is the reason.
    #[must_use]
    pub const fn problem(self) -> Option<PluginProblem> {
        match self {
            Self::SafeMode { .. } => Some(PluginProblem::SafeMode),
            Self::Crashed => Some(PluginProblem::Crashed),
            Self::Host | Self::Off => None,
        }
    }
}

impl Kernel {
    /// The configuration to decide the native plugins' on/off state from: read
    /// once per start, whatever the number of plugins. A file that cannot be
    /// read says nothing; the copy read at start stands in.
    #[must_use]
    pub fn native_config(&self) -> NativeConfig {
        let file = read_config(self.shared.storage.as_ref())
            .ok()
            .flatten()
            .unwrap_or_else(|| self.keys.user.clone());
        NativeConfig {
            file,
            overrides: self.keys.shell.clone(),
        }
    }

    /// Whether, and how, the native plugin `id` runs this start. Ask before
    /// hosting it; host it only when this says [`NativeDecision::Host`], and
    /// list it either way ([`Self::list_native`]).
    ///
    /// `default_on` is the shipped default (a plugin shipped off passes
    /// `false`); `from_card` says the plugin is loaded from the card (a `.ppn`)
    /// rather than built into the firmware. In safe mode the one the crash was
    /// in is switched off in the file, and with none known so is every card
    /// plugin that was on; this saves the file.
    pub fn native_decide(
        &mut self,
        config: &NativeConfig,
        id: &str,
        default_on: bool,
        from_card: bool,
    ) -> NativeDecision {
        if !decide(Some(&config.file), &config.overrides, id, default_on) {
            return NativeDecision::Off;
        }
        let Some(safe) = &self.shared.native_safe_mode else {
            return NativeDecision::Host;
        };
        let culprit = match safe.reason {
            SafeModeReason::FailedStarts => safe.crashed.clone(),
            _ => None,
        };
        let crashed_here = culprit.as_deref() == Some(id);
        // Built-in plugins run in safe mode, except the one that crashed.
        if !from_card && !crashed_here {
            return NativeDecision::Host;
        }
        let switch_off = culprit.is_none() || crashed_here;
        if switch_off {
            self.save_config("plugins", id, "off");
            if let (Some(safe), Ok(id)) = (&mut self.shared.native_safe_mode, Id::try_parse(id)) {
                safe.switched_off.push(id);
            }
        }
        if culprit.is_some() && switch_off {
            NativeDecision::Crashed
        } else {
            NativeDecision::SafeMode {
                enabled: !switch_off,
            }
        }
    }

    /// Lists a native plugin for the plugin manager: `info` says what it is,
    /// its state (`Running` once hosted, `Off` when it is not hosted, `Failed`
    /// when hosting failed), whether it is switched on and, for a plugin held
    /// back, why ([`NativeDecision::problem`]). Call it after the plugin is
    /// hosted, so a slot only it fills marks it as one that stays on.
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
