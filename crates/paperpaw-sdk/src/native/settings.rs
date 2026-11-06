//! Borrowed settings values decoded in caller-owned scratch space.
use super::{
    Error, Host,
    wire::settings_wire::{self as wire, settings_decode},
};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

/// A decoded settings value. Text borrows the scratch buffer supplied to [`read_setting`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingValue<'a> {
    /// The setting has no value.
    Absent,
    /// A Boolean value.
    Bool(bool),
    /// A signed integer value.
    Int(i64),
    /// Text borrowed from the caller's scratch buffer.
    Text(&'a str),
}

/// A host-held user permission belonging to the caller.
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
            Self::Style => "core.grant.style",
            Self::Install => "core.grant.install",
        }
    }
}

/// Read the caller's own user grant through the host's relative grant key.
/// The host identifies the caller and enforces grant ownership. Users change
/// grants through the host's settings UI.
///
/// # Errors
/// Returns Invalid for a malformed or non-Boolean reply, or the host's refusal.
pub fn own_grant(host: &Host<'_>, kind: GrantKind) -> Result<bool, Error> {
    let mut scratch = [0; wire::SETTINGS_HEADER_BYTES + 1];
    let length = read_raw(host, kind.key(), &mut scratch)?;
    let value = scratch.get(..length).ok_or(Error::Invalid)?;
    match settings_decode(value).map_err(|_| Error::Invalid)? {
        wire::SettingValueRef::Bool(value) => Ok(value),
        _ => Err(Error::Invalid),
    }
}

/// Read a setting without allocating. `scratch` must fit the complete wire value,
/// including its header; a truncated value is never decoded.
/// The host enforces access to protected settings. Grant keys are read through
/// [`own_grant`]; this function refuses the `core.grant.*` namespace.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, settings::{self, SettingValue}};
/// fn escape(host: &Host<'_>) -> SettingValue<'static> {
///     let mut scratch = [0; 64];
///     settings::read_setting(host, "battery.show-percent", &mut scratch).unwrap()
/// }
/// ```
///
/// # Errors
/// Returns Denied for a grant key, NoSpace when the complete value does not fit
/// in `scratch`, decoder errors for malformed values, or the host's refusal.
pub fn read_setting<'a>(
    host: &Host<'_>,
    key: impl AsRef<str>,
    scratch: &'a mut [u8],
) -> Result<SettingValue<'a>, Error> {
    let key = key.as_ref();
    if key.starts_with("core.grant.") {
        return Err(Error::Denied);
    }
    let length = read_raw(host, key, scratch)?;
    let value = scratch.get(..length).ok_or(Error::NoSpace)?;
    Ok(match settings_decode(value).map_err(Error::from_status)? {
        wire::SettingValueRef::Absent => SettingValue::Absent,
        wire::SettingValueRef::Bool(value) => SettingValue::Bool(value),
        wire::SettingValueRef::Int(value) => SettingValue::Int(value),
        wire::SettingValueRef::Text(value) => SettingValue::Text(value),
    })
}

fn read_raw(host: &Host<'_>, key: &str, scratch: &mut [u8]) -> Result<usize, Error> {
    // SAFETY: the key and exclusively borrowed output remain live for this call.
    let result = unsafe {
        (host.table.settings_read)(
            host.ctx,
            PpBytes {
                ptr: key.as_ptr(),
                len: key.len(),
            },
            PpBuffer {
                ptr: scratch.as_mut_ptr(),
                cap: scratch.len(),
            },
        )
    };
    usize::try_from(result).map_err(|_| Error::from_status(result))
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
