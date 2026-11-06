//! Read-only access to the calling plugin's host-held user grants.
use super::{
    Error, Host,
    declare::settings_value::{SETTINGS_HEADER_BYTES, SettingValueRef, settings_decode},
};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

const PREFIX: &str = "core.grant.";

/// Read a grant such as `core.grant.style.example.reader` as yes or no.
/// The host enforces ownership: reading another plugin's grant is denied.
/// Grants can be changed only through the host's settings UI.
///
/// # Errors
/// Returns Invalid for an incomplete grant key or a non-Boolean host reply,
/// or the host's error, including Denied for another plugin's grant.
pub fn read(host: &Host<'_>, key: impl AsRef<str>) -> Result<bool, Error> {
    let key = key.as_ref();
    let Some((kind, owner)) = key
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once('.'))
    else {
        return Err(Error::Invalid);
    };
    if kind.is_empty() || owner.is_empty() {
        return Err(Error::Invalid);
    }
    let mut out = [0; SETTINGS_HEADER_BYTES + 1];
    // SAFETY: the key and exclusively borrowed output remain live for this call.
    let result = unsafe {
        (host.table.settings_read)(
            host.ctx,
            PpBytes {
                ptr: key.as_ptr(),
                len: key.len(),
            },
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    let length = usize::try_from(result).map_err(|_| Error::from_status(result))?;
    let bytes = out.get(..length).ok_or(Error::Invalid)?;
    match settings_decode(bytes).map_err(Error::from_status)? {
        SettingValueRef::Bool(value) => Ok(value),
        _ => Err(Error::Invalid),
    }
}

#[cfg(test)]
#[path = "grants_tests.rs"]
mod tests;
