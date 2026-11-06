//! Read-only access to the calling plugin's host-held user grants.
use super::{
    Error, Host,
    declare::settings_value::{SETTINGS_HEADER_BYTES, SettingValueRef, settings_decode},
};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

const PREFIX: &str = "core.grant.";
// The plugin folder grammar is documented by install::is_plugin_name.
const MAX_PLUGIN_BYTES: usize = 40;
const LONGEST_KIND: &str = "install";

/// A user permission held and edited only by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrantKind {
    /// Override the user's reading style.
    Style,
    /// Install plugins, fonts and data without asking each time.
    /// Firmware changes always require confirmation.
    Install,
}
impl GrantKind {
    const fn key(self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Install => "install",
        }
    }
}

/// Read the caller's grant as yes or no. `plugin` is the caller's own plugin id.
/// The host checks ownership and denies a request naming any other plugin.
/// Grants can be changed only through the host's settings UI.
///
/// # Errors
/// Returns Invalid for an invalid plugin id or a non-Boolean or malformed host
/// reply, or the host's error, including Denied for another plugin's grant.
pub fn granted(host: &Host<'_>, kind: GrantKind, plugin: &str) -> Result<bool, Error> {
    if !crate::install::is_plugin_name(plugin) {
        return Err(Error::Invalid);
    }
    let mut key = [0; PREFIX.len() + LONGEST_KIND.len() + 1 + MAX_PLUGIN_BYTES];
    let mut length = 0;
    for part in [PREFIX, kind.key(), ".", plugin] {
        let end = length + part.len();
        let slot = key.get_mut(length..end).ok_or(Error::Invalid)?;
        slot.copy_from_slice(part.as_bytes());
        length = end;
    }
    let mut out = [0; SETTINGS_HEADER_BYTES + 1];
    // SAFETY: the key and exclusively borrowed output remain live for this call.
    let result = unsafe {
        (host.table.settings_read)(
            host.ctx,
            PpBytes {
                ptr: key.as_ptr(),
                len: length,
            },
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    let length = usize::try_from(result).map_err(|_| Error::from_status(result))?;
    let bytes = out.get(..length).ok_or(Error::Invalid)?;
    match settings_decode(bytes).map_err(|_| Error::Invalid)? {
        SettingValueRef::Bool(value) => Ok(value),
        _ => Err(Error::Invalid),
    }
}

#[cfg(test)]
#[path = "grants_tests.rs"]
mod tests;
