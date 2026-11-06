//! Typed command submission using caller-owned scratch storage.
use super::{
    Error, Host,
    binding::status,
    settings::SettingValue,
    wire::{commands_wire as wire, settings_wire::SettingValueRef},
};
use core::fmt;
use paperpaw_native::host_glue::PpBytes;

/// Submit typed commands during one callback without allocating.
/// Each method stages its batch immediately. Dropping sends nothing, and an
/// error does not prevent later calls. The host applies accepted commands after
/// the callback returns, in submission order.
///
/// The sender cannot retain local scratch beyond its scope:
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, commands::CommandSender};
/// fn escape<'cb>(host: &'cb Host<'cb>) -> CommandSender<'cb, 'static> {
///     let mut scratch = [0; 128];
///     CommandSender::new(host, &mut scratch)
/// }
/// ```
pub struct CommandSender<'cb, 'buf> {
    host: &'cb Host<'cb>,
    scratch: &'buf mut [u8],
}

impl fmt::Debug for CommandSender<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandSender")
            .field("capacity", &self.scratch.len())
            .finish_non_exhaustive()
    }
}

impl<'cb, 'buf> CommandSender<'cb, 'buf> {
    /// Bind a callback's host and reusable command encoding buffer.
    pub fn new(host: &'cb Host<'cb>, scratch: &'buf mut [u8]) -> Self {
        Self { host, scratch }
    }

    /// Stage one setting change. The host validates setting ownership, kind and
    /// authority before accepting the batch. No command is submitted on a local
    /// error; earlier accepted batches remain staged.
    ///
    /// # Errors
    /// Returns Denied for `core.grant.*`, Invalid for absent values or an empty
    /// key, NoSpace for encoded limits or short scratch, or the host's refusal.
    pub fn set_setting(
        &mut self,
        key: impl AsRef<str>,
        value: SettingValue<'_>,
    ) -> Result<(), Error> {
        let key = key.as_ref();
        if key.starts_with("core.grant.") {
            return Err(Error::Denied);
        }
        let value = match value {
            SettingValue::Absent => SettingValueRef::Absent,
            SettingValue::Bool(value) => SettingValueRef::Bool(value),
            SettingValue::Int(value) => SettingValueRef::Int(value),
            SettingValue::Text(value) => SettingValueRef::Text(value),
        };
        let length =
            wire::commands_encode(&[wire::Command::SetSetting { key, value }], self.scratch)
                .map_err(Error::from_status)?;
        // SAFETY: the host binding and initialized scratch prefix live for this
        // synchronous call; the host copies any command data it retains.
        status(unsafe {
            (self.host.table.commands_submit)(
                self.host.ctx,
                PpBytes {
                    ptr: self.scratch.as_ptr(),
                    len: length,
                },
            )
        })
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
