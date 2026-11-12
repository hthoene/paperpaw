//! Reusable host catalog buffers; strings outlive only their owned copies.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    formatting::{self, Arg, FormatBuffers},
};

/// The host's catalog texts, copied. Its two transport buffers are made when
/// first needed and kept: a message's starts at [`FIRST_MESSAGE`] bytes and
/// grows to the length of a longer one (at most 4 KiB), the arguments' at
/// [`FIRST_ARGUMENTS`] (at most 1 KiB). Most texts are a line or two, so most
/// screens never hold the 5 KiB a long text needs (#248); a screen that only
/// holds a `Messages` pays for none.
#[derive(Debug, Default)]
pub struct Messages {
    out: Vec<u8>,
    arguments: Vec<u8>,
}

/// The first size of the message buffer: a few lines of text.
pub const FIRST_MESSAGE: usize = 512;
/// The first size of the arguments' buffer: a few short arguments.
pub const FIRST_ARGUMENTS: usize = 256;

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
        fit(&mut self.out, FIRST_MESSAGE)?;
        Ok(&mut self.out)
    }
    pub fn copy(&mut self, host: &Host<'_>, key: &str) -> Result<String, Error> {
        let small = self.out()?.len() < formatting::MAX_MESSAGE_BYTES;
        match formatting::copy_message(host, key, &mut self.out) {
            Ok(text) => return crate::text::copy(text),
            // Longer than the buffer: once more with room for any message.
            Err(Error::NoSpace) if small => {}
            Err(error) => return Err(error),
        }
        fit(&mut self.out, formatting::MAX_MESSAGE_BYTES)?;
        crate::text::copy(formatting::copy_message(host, key, &mut self.out)?)
    }
    pub fn format(
        &mut self,
        host: &Host<'_>,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
        self.out()?;
        fit(&mut self.arguments, FIRST_ARGUMENTS)?;
        let mut length = match self.format_once(host, key, args) {
            // The arguments did not fit their buffer: once more with all of it.
            Err(Error::NoSpace) if self.arguments.len() < formatting::MAX_ARGUMENT_BYTES => {
                fit(&mut self.arguments, formatting::MAX_ARGUMENT_BYTES)?;
                self.format_once(host, key, args)?
            }
            other => other?,
        };
        if length > self.out.len() && length <= formatting::MAX_MESSAGE_BYTES {
            // The host said how long the message is: once more with that room.
            fit(&mut self.out, length)?;
            length = self.format_once(host, key, args)?;
        }
        let bytes = self.out.get(..length).ok_or(Error::NoSpace)?;
        crate::text::copy(core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?)
    }
    fn format_once(
        &mut self,
        host: &Host<'_>,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<usize, Error> {
        let mut buffers = FormatBuffers::new(&mut self.arguments, &mut self.out);
        formatting::format_message(host, key, args, &mut buffers)
    }
}

/// `bytes` holds at least `len` bytes: a smaller buffer is let go of before
/// the larger is made, so the two are never held at once.
fn fit(bytes: &mut Vec<u8>, len: usize) -> Result<(), Error> {
    if bytes.len() >= len {
        return Ok(());
    }
    *bytes = Vec::new();
    *bytes = buffer(len)?;
    Ok(())
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
        let mut length = formatting::digits(host, text, self.out()?)?;
        if length > self.out.len() && length <= formatting::MAX_MESSAGE_BYTES {
            fit(&mut self.out, length)?;
            length = formatting::digits(host, text, &mut self.out)?;
        }
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
        assert_eq!(messages.out.len(), FIRST_MESSAGE);
        // The same buffer again, not a second one.
        let at = messages.out.as_ptr();
        messages.out().unwrap();
        assert_eq!(messages.out.as_ptr(), at);
        assert_eq!(messages.arguments.capacity(), 0);
    }

    #[test]
    fn a_buffer_grows_to_a_longer_text_and_never_shrinks() {
        let mut bytes = Vec::new();
        fit(&mut bytes, FIRST_MESSAGE).unwrap();
        assert_eq!(bytes.len(), FIRST_MESSAGE);
        fit(&mut bytes, 1500).unwrap();
        assert_eq!(bytes.len(), 1500);
        assert_eq!(bytes.capacity(), 1500);
        // A shorter text later keeps the room it has.
        fit(&mut bytes, 100).unwrap();
        assert_eq!(bytes.len(), 1500);
    }
}
