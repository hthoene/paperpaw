//! Borrowed settings values decoded in caller-owned scratch space.
use super::{Error, Host, wire::settings_wire::settings_decode};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

/// A decoded settings value. Text borrows the scratch buffer supplied to [`read`].
pub use super::wire::settings_wire::SettingValueRef as Value;

/// Read a setting without allocating. `scratch` must fit the complete wire value,
/// including its header; a truncated value is never decoded.
/// The host enforces access to protected settings.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, settings::{self, Value}};
/// fn escape(host: &Host<'_>) -> Value<'static> {
///     let mut scratch = [0; 64];
///     settings::read(host, "battery.show-percent", &mut scratch).unwrap()
/// }
/// ```
///
/// # Errors
/// Returns NoSpace when the complete value does not fit in `scratch`, errors
/// from the generated decoder for malformed values, or the host's refusal.
pub fn read<'a>(
    host: &Host<'_>,
    key: impl AsRef<str>,
    scratch: &'a mut [u8],
) -> Result<Value<'a>, Error> {
    let key = key.as_ref();
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
    let length = usize::try_from(result).map_err(|_| Error::from_status(result))?;
    let value = scratch.get(..length).ok_or(Error::NoSpace)?;
    settings_decode(value).map_err(Error::from_status)
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
