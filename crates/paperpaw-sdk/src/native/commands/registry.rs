//! Registry writes are staged and applied after the callback.
use super::{CommandSender, Error, check_id, wire};
use crate::native::{
    settings::SettingValue,
    wire::settings_wire::{SettingValueRef, settings_encode},
};
use alloc::vec::Vec;
use core::mem::MaybeUninit;

// registry.md section 4.4: keys and encoded values of one preview set.
const PREVIEW_MAX_BYTES: usize = 2 * 1024;

/// One setting in a temporary preview. Absent previews its default instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PreviewSetting<'a> {
    key: &'a str,
    value: SettingValue<'a>,
}
impl<'a> PreviewSetting<'a> {
    /// Preview one value or, with Absent, the default as if no value were set.
    #[must_use]
    pub const fn new(key: &'a str, value: SettingValue<'a>) -> Self {
        Self { key, value }
    }
}

/// Settings to return to their defaults. Mass resets may require user consent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResetTarget<'a> {
    /// One setting.
    Setting(&'a str),
    /// All settings in a group.
    Group(&'a str),
    /// All settings of a plugin.
    Plugin(&'a str),
    /// Every setting the caller may reset.
    All,
    /// Values associated with a widget placement.
    Placement(&'a str),
}

/// A user's key binding, distinct from returning to declared bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyBinding<'a> {
    /// Run this action.
    Action(&'a str),
    /// Bind nothing.
    None,
    /// Return to the declared and preset binding.
    Default,
}

/// Whether a plugin should run after the next start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginEnabled {
    /// Switch on.
    On,
    /// Switch off.
    Off,
}

/// Result of one requested plugin installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginOutcome {
    /// The host confirms that the plugin is installed.
    Installed,
    /// The user declined installation.
    Declined,
    /// Installation failed.
    Failed,
    /// No matching plugin was found.
    NotFound,
}
impl PluginOutcome {
    pub(crate) const fn wire(self) -> u32 {
        match self {
            Self::Installed => 0,
            Self::Declined => 1,
            Self::Failed => 2,
            Self::NotFound => 3,
        }
    }
}

/// Whether preset values are kept or temporarily previewed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PresetMode {
    /// Keep the values.
    Apply,
    /// Preview until the owning screen closes or the preview ends.
    Preview,
}

impl CommandSender<'_, '_> {
    /// Replace the complete preview in one batch. The caller supplies value
    /// encoding scratch separately from the sender's command output buffer.
    /// Value scratch must hold the sum of the encoded values, at most eight
    /// values of 512 bytes each; keys plus values must fit the 2 KiB set budget.
    /// Ownership of the top screen is checked by the host.
    ///
    /// # Errors
    /// Returns Invalid for an empty set or invalid key, NoSpace for the batch
    /// limit, the 2 KiB key/value budget or short scratch, Unsupported for a
    /// grant preview, or a host refusal including State for screen ownership.
    /// Oversized sets are never split into different previews.
    pub fn preview_settings(
        &mut self,
        values: &[PreviewSetting<'_>],
        value_scratch: &mut [u8],
    ) -> Result<(), Error> {
        if values.is_empty() {
            return Err(Error::Invalid);
        }
        if values.len() > wire::COMMAND_MAX_RECORDS {
            return Err(Error::NoSpace);
        }
        let mut commands = [wire::Command::PreviewEnd { flags: 0 }; wire::COMMAND_MAX_RECORDS];
        let mut remaining = value_scratch;
        let mut total: usize = 0;
        for (slot, value) in commands.iter_mut().zip(values) {
            check_id(value.key, wire::COMMAND_PREVIEW_SETTING_KEY_MAX_BYTES)?;
            if value.key.starts_with("core.grant.") {
                return Err(Error::Unsupported);
            }
            let absent = matches!(value.value, SettingValue::Absent);
            let encoded = if absent {
                &[]
            } else {
                let value = match value.value {
                    SettingValue::Absent => SettingValueRef::Absent,
                    SettingValue::Bool(value) => SettingValueRef::Bool(value),
                    SettingValue::Int(value) => SettingValueRef::Int(value),
                    SettingValue::Text(value) => SettingValueRef::Text(value),
                };
                // SAFETY: u8 and MaybeUninit<u8> have identical layouts. The
                // settings_encode contract initializes exactly the first
                // min(required, out.len()) slots on success and touches no
                // other slot. It never stores uninitialized bytes. Check its
                // returned required length before borrowing that prefix.
                let out = unsafe {
                    core::slice::from_raw_parts_mut(
                        remaining.as_mut_ptr().cast::<MaybeUninit<u8>>(),
                        remaining.len(),
                    )
                };
                let length = settings_encode(value, out).map_err(Error::from_status)?;
                if length > remaining.len()
                    || length > wire::COMMAND_PREVIEW_SETTING_VALUE_MAX_BYTES
                {
                    return Err(Error::NoSpace);
                }
                let (encoded, rest) = remaining.split_at_mut(length);
                remaining = rest;
                &*encoded
            };
            total = total
                .checked_add(value.key.len())
                .and_then(|n| n.checked_add(encoded.len()))
                .ok_or(Error::NoSpace)?;
            if total > PREVIEW_MAX_BYTES {
                return Err(Error::NoSpace);
            }
            *slot = wire::Command::PreviewSetting {
                key: value.key,
                value: encoded,
                flags: u32::from(absent),
            };
        }
        self.submit_batch(&commands[..values.len()])
    }

    /// Reset settings. The host checks authority and obtains any required consent.
    ///
    /// # Errors
    /// Returns validation, encoding or host errors, including Denied.
    pub fn reset_setting(&mut self, target: ResetTarget<'_>) -> Result<(), Error> {
        let (key, flags) = match target {
            ResetTarget::Setting(key) => (key, 0),
            ResetTarget::Group(key) => (key, 1),
            ResetTarget::Plugin(key) => (key, 2),
            ResetTarget::All => ("", 4),
            ResetTarget::Placement(key) => (key, 8),
        };
        if flags != 4 {
            check_id(key, wire::COMMAND_RESET_SETTING_KEY_MAX_BYTES)?;
        }
        self.submit(wire::Command::ResetSetting { key, flags })
    }

    /// End the preview owned by this plugin and screen.
    ///
    /// # Errors
    /// Returns encoding errors or the host's refusal, including State for another owner.
    pub fn end_preview(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::PreviewEnd { flags: 0 })
    }

    /// Keep a book's own settings while this screen is open (`registry.md`
    /// 4.1): the book's root-1 `path`, or `None` to end it. The settings
    /// declared [`scoped`](crate::native::declare::SpecDecl::scoped) then read
    /// the book's value where it has one ("Only this book" in the settings
    /// editor) and the global value otherwise, and the editor writes there. The
    /// host picks the file (in this plugin's data, by the book's key: where
    /// Beta 3's reader kept it) and moves and removes it with the book. The
    /// scope also ends when the screen closes or the plugin stops.
    ///
    /// # Errors
    /// Returns encoding or host errors: State unless this plugin owns the top
    /// screen (and in a work run), Invalid for a path that names no book (absolute,
    /// under `.paperpaw` or `plugins`, over 512 bytes).
    pub fn set_scope(&mut self, path: Option<&str>) -> Result<(), Error> {
        self.submit(wire::Command::SetScope {
            path: path.unwrap_or(""),
        })
    }

    /// Replace menu rows in order. None restores declared rows; Some of an empty slice clears them.
    /// No row is truncated. Limits come from the generated command codec.
    ///
    /// # Errors
    /// Returns Invalid for empty IDs, embedded separators or list item limits,
    /// NoSpace for total field overflow, NoMemory for the temporary joined list,
    /// or the host's refusal, including Denied without uses.registry.
    pub fn set_menu(&mut self, menu: impl AsRef<str>, items: Option<&[&str]>) -> Result<(), Error> {
        let menu = menu.as_ref();
        check_id(menu, wire::COMMAND_SET_MENU_MENU_MAX_BYTES)?;
        let length = {
            let ids = items.unwrap_or(&[]);
            let needed = menu_list_bytes(ids)?;
            let mut joined = Vec::new();
            joined
                .try_reserve_exact(needed)
                .map_err(|_| Error::NoMemory)?;
            joined.resize(needed, 0);
            let text = join_ids(
                ids,
                &mut joined,
                wire::COMMAND_SET_MENU_ITEMS_MAX_ITEMS,
                wire::COMMAND_SET_MENU_ITEMS_ITEM_MAX_BYTES,
                wire::COMMAND_SET_MENU_ITEMS_SEPARATOR,
            )?;
            self.encode_batch(&[wire::Command::SetMenu {
                menu,
                items: text,
                flags: u32::from(items.is_none()),
            }])?
        };
        self.submit_encoded(length)
    }

    /// Set a binding or return it to its declared default.
    ///
    /// # Errors
    /// Returns validation, encoding or host errors. The host protects escape bindings.
    pub fn bind_key(
        &mut self,
        layer: impl AsRef<str>,
        input: impl AsRef<str>,
        binding: KeyBinding<'_>,
    ) -> Result<(), Error> {
        let (layer, input) = (layer.as_ref(), input.as_ref());
        check_id(layer, wire::COMMAND_BIND_KEY_LAYER_MAX_BYTES)?;
        check_id(input, wire::COMMAND_BIND_KEY_INPUT_MAX_BYTES)?;
        let (action, flags) = match binding {
            KeyBinding::Action(action) => {
                check_id(action, wire::COMMAND_BIND_KEY_ACTION_MAX_BYTES)?;
                (action, 0)
            }
            KeyBinding::None => ("", 0),
            KeyBinding::Default => ("", 1),
        };
        self.submit(wire::Command::BindKey {
            layer,
            input,
            action,
            flags,
        })
    }

    /// Bind a declared screen to a slot. None restores the highest-priority offer.
    ///
    /// # Errors
    /// Returns validation, encoding or host errors, including Invalid for a screen not offered.
    pub fn bind_slot(&mut self, slot: impl AsRef<str>, screen: Option<&str>) -> Result<(), Error> {
        let slot = slot.as_ref();
        check_id(slot, wire::COMMAND_BIND_SLOT_SLOT_MAX_BYTES)?;
        if let Some(screen) = screen {
            check_id(screen, wire::COMMAND_BIND_SLOT_SCREEN_MAX_BYTES)?;
        }
        self.submit(wire::Command::BindSlot {
            slot,
            screen: screen.unwrap_or(""),
        })
    }

    /// Switch a plugin on or off for the next start.
    ///
    /// # Errors
    /// Returns validation, encoding or host errors, including Denied for required plugins.
    pub fn set_plugin_enabled(
        &mut self,
        plugin: impl AsRef<str>,
        enabled: PluginEnabled,
    ) -> Result<(), Error> {
        let plugin = plugin.as_ref();
        if !paperpaw_native::bundle::is_plugin_id(plugin) {
            return Err(Error::Invalid);
        }
        check_id(plugin, wire::COMMAND_SET_PLUGIN_ENABLED_PLUGIN_MAX_BYTES)?;
        self.submit(wire::Command::SetPluginEnabled {
            plugin,
            enabled: u32::from(matches!(enabled, PluginEnabled::On)),
        })
    }

    /// Ask subscribers to offer installation of these plugins. This installs nothing itself.
    ///
    /// # Errors
    /// Returns Invalid for empty IDs, separators or list item limits,
    /// NoSpace for total field overflow,
    /// or the host's refusal, including Full for too many open requests.
    pub fn request_plugins(&mut self, plugins: &[&str]) -> Result<(), Error> {
        if plugins
            .iter()
            .any(|id| !paperpaw_native::bundle::is_plugin_id(id))
        {
            return Err(Error::Invalid);
        }
        let mut joined = [0; wire::COMMAND_REQUEST_PLUGINS_PLUGINS_MAX_BYTES];
        let plugins = join_ids(
            plugins,
            &mut joined,
            wire::COMMAND_REQUEST_PLUGINS_PLUGINS_MAX_ITEMS,
            wire::COMMAND_REQUEST_PLUGINS_PLUGINS_ITEM_MAX_BYTES,
            wire::COMMAND_REQUEST_PLUGINS_PLUGINS_SEPARATOR,
        )?;
        self.submit(wire::Command::RequestPlugins { plugins })
    }

    /// Answer one plugin request; needs `uses.install`. Until hosts check that
    /// feature for commands, every host refuses this.
    ///
    /// # Errors
    /// Returns validation, encoding or host errors, including Denied without installation authority.
    pub fn answer_plugin(
        &mut self,
        request: crate::native::events::PluginRequestId,
        plugin: impl AsRef<str>,
        outcome: PluginOutcome,
    ) -> Result<(), Error> {
        let plugin = plugin.as_ref();
        if !paperpaw_native::bundle::is_plugin_id(plugin) {
            return Err(Error::Invalid);
        }
        check_id(plugin, wire::COMMAND_ANSWER_PLUGIN_PLUGIN_MAX_BYTES)?;
        self.submit(wire::Command::AnswerPlugin {
            request: request.get(),
            plugin,
            outcome: outcome.wire(),
        })
    }

    /// Apply or preview a registered preset. Authority is checked for every value.
    ///
    /// # Errors
    /// Returns validation, encoding or host errors, including State for preview ownership.
    pub fn apply_preset(&mut self, preset: impl AsRef<str>, mode: PresetMode) -> Result<(), Error> {
        let preset = preset.as_ref();
        check_id(preset, wire::COMMAND_APPLY_PRESET_PRESET_MAX_BYTES)?;
        self.submit(wire::Command::ApplyPreset {
            preset,
            flags: u32::from(matches!(mode, PresetMode::Preview)),
        })
    }
}

fn menu_list_bytes(ids: &[&str]) -> Result<usize, Error> {
    if ids.len() > wire::COMMAND_SET_MENU_ITEMS_MAX_ITEMS {
        return Err(Error::Invalid);
    }
    let mut total = 0usize;
    for (index, id) in ids.iter().enumerate() {
        if id.is_empty()
            || id.contains(wire::COMMAND_SET_MENU_ITEMS_SEPARATOR)
            || id.len() > wire::COMMAND_SET_MENU_ITEMS_ITEM_MAX_BYTES
        {
            return Err(Error::Invalid);
        }
        total = total
            .checked_add(id.len())
            .and_then(|n| n.checked_add(usize::from(index != 0)))
            .filter(|n| *n <= wire::COMMAND_SET_MENU_ITEMS_MAX_BYTES)
            .ok_or(Error::NoSpace)?;
    }
    Ok(total)
}

fn join_ids<'a>(
    ids: &[&str],
    scratch: &'a mut [u8],
    max_items: usize,
    item_max: usize,
    separator: char,
) -> Result<&'a str, Error> {
    if ids.len() > max_items {
        return Err(Error::Invalid);
    }
    let mut used = 0;
    for (index, id) in ids.iter().enumerate() {
        if id.is_empty() || id.contains(separator) {
            return Err(Error::Invalid);
        }
        if id.len() > item_max {
            return Err(Error::Invalid);
        }
        if index != 0 {
            *scratch.get_mut(used).ok_or(Error::NoSpace)? = separator as u8;
            used += 1;
        }
        let end = used.checked_add(id.len()).ok_or(Error::NoSpace)?;
        scratch
            .get_mut(used..end)
            .ok_or(Error::NoSpace)?
            .copy_from_slice(id.as_bytes());
        used = end;
    }
    core::str::from_utf8(&scratch[..used]).map_err(|_| Error::Invalid)
}
