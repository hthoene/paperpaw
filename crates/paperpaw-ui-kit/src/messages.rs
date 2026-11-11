//! Reusable host catalog buffers; strings outlive only their owned copies.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    formatting::{self, Arg, FormatBuffers},
};

/// The host's catalog texts, copied. Its two transport buffers (a message's
/// 4 KiB, the arguments' 1 KiB) are made when first needed and kept: a screen
/// that only holds a `Messages` (or one whose texts all come from `copy`) never
/// pays for the arguments' buffer, and one that asks for nothing pays for none.
#[derive(Debug, Default)]
pub struct Messages {
    out: Vec<u8>,
    arguments: Vec<u8>,
}
impl Messages {
    /// An empty set of buffers; they are allocated by the first call that
    /// needs them.
    ///
    /// # Errors
    /// None today; kept fallible for callers that propagate it.
    pub const fn new() -> Result<Self, Error> {
        Ok(Self {
            out: Vec::new(),
            arguments: Vec::new(),
        })
    }
    /// The message buffer, made at the first use.
    fn out(&mut self) -> Result<&mut Vec<u8>, Error> {
        if self.out.is_empty() {
            self.out = buffer(formatting::MAX_MESSAGE_BYTES)?;
        }
        Ok(&mut self.out)
    }
    pub fn copy(&mut self, host: &Host<'_>, key: &str) -> Result<String, Error> {
        let out = self.out()?;
        crate::text::copy(formatting::copy_message(host, key, out)?)
    }
    pub fn format(
        &mut self,
        host: &Host<'_>,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
        self.out()?;
        if self.arguments.is_empty() {
            self.arguments = buffer(formatting::MAX_ARGUMENT_BYTES)?;
        }
        let mut buffers = FormatBuffers::new(&mut self.arguments, &mut self.out);
        let length = formatting::format_message(host, key, args, &mut buffers)?;
        let bytes = self.out.get(..length).ok_or(Error::NoSpace)?;
        crate::text::copy(core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?)
    }
}
fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}
impl Messages {
    /// The number `text` written in the language's digits.
    ///
    /// # Errors
    /// Propagates host formatting errors.
    pub fn digits(&mut self, host: &Host<'_>, text: &str) -> Result<String, Error> {
        let length = formatting::digits(host, text, self.out()?)?;
        let bytes = self.out.get(..length).ok_or(Error::NoSpace)?;
        crate::text::copy(core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_set_holds_no_buffers_until_one_is_asked_for() {
        let mut messages = Messages::new().unwrap();
        assert_eq!(messages.out.capacity() + messages.arguments.capacity(), 0);
        messages.out().unwrap();
        assert_eq!(messages.out.len(), formatting::MAX_MESSAGE_BYTES);
        // The same buffer again, not a second one.
        let at = messages.out.as_ptr();
        messages.out().unwrap();
        assert_eq!(messages.out.as_ptr(), at);
        assert_eq!(messages.arguments.capacity(), 0);
    }
}
