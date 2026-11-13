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
        self.copy_from(host, key)
    }
    pub fn format(
        &mut self,
        host: &Host<'_>,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
        self.format_from(host, key, args)
    }
    /// The number `text` written in the language's digits.
    ///
    /// # Errors
    /// Propagates host formatting errors.
    pub fn digits(&mut self, host: &Host<'_>, text: &str) -> Result<String, Error> {
        self.digits_from(host, text)
    }

    fn copy_from(&mut self, source: &impl Source, key: &str) -> Result<String, Error> {
        let small = self.out()?.len() < formatting::MAX_MESSAGE_BYTES;
        match source.copy(key, &mut self.out) {
            Ok(text) => return crate::text::copy(text),
            // Longer than the buffer: once more with room for any message.
            Err(Error::NoSpace) if small => {}
            Err(error) => return Err(error),
        }
        fit(&mut self.out, formatting::MAX_MESSAGE_BYTES)?;
        crate::text::copy(source.copy(key, &mut self.out)?)
    }
    fn format_from(
        &mut self,
        source: &impl Source,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
        self.out()?;
        fit(&mut self.arguments, FIRST_ARGUMENTS)?;
        let mut length = match source.format(key, args, &mut self.arguments, &mut self.out) {
            // The arguments did not fit their buffer: once more with all of it.
            Err(Error::NoSpace) if self.arguments.len() < formatting::MAX_ARGUMENT_BYTES => {
                fit(&mut self.arguments, formatting::MAX_ARGUMENT_BYTES)?;
                source.format(key, args, &mut self.arguments, &mut self.out)?
            }
            other => other?,
        };
        if length > self.out.len() && length <= formatting::MAX_MESSAGE_BYTES {
            // The host said how long the message is: once more with that room.
            fit(&mut self.out, length)?;
            length = source.format(key, args, &mut self.arguments, &mut self.out)?;
        }
        utf8(&self.out, length)
    }
    fn digits_from(&mut self, source: &impl Source, text: &str) -> Result<String, Error> {
        let mut length = source.digits(text, self.out()?)?;
        if length > self.out.len() && length <= formatting::MAX_MESSAGE_BYTES {
            fit(&mut self.out, length)?;
            length = source.digits(text, &mut self.out)?;
        }
        utf8(&self.out, length)
    }
}

/// The first `length` bytes of `out`, copied as text; `NoSpace` where the
/// host's text is longer than any buffer may be.
fn utf8(out: &[u8], length: usize) -> Result<String, Error> {
    let bytes = out.get(..length).ok_or(Error::NoSpace)?;
    crate::text::copy(core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?)
}

/// Where the texts come from: the host's catalog (a stand-in in the tests).
trait Source {
    /// The message `key` into `out`; `NoSpace` where it is longer.
    fn copy<'a>(&self, key: &str, out: &'a mut [u8]) -> Result<&'a str, Error>;
    /// The message `key` with `args` into `out`; its whole length.
    fn format(
        &self,
        key: &str,
        args: &[Arg<'_>],
        scratch: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error>;
    /// `text` in the language's digits into `out`; its whole length.
    fn digits(&self, text: &str, out: &mut [u8]) -> Result<usize, Error>;
}

impl Source for Host<'_> {
    fn copy<'a>(&self, key: &str, out: &'a mut [u8]) -> Result<&'a str, Error> {
        formatting::copy_message(self, key, out)
    }
    fn format(
        &self,
        key: &str,
        args: &[Arg<'_>],
        scratch: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        formatting::format_message(self, key, args, &mut FormatBuffers::new(scratch, out))
    }
    fn digits(&self, text: &str, out: &mut [u8]) -> Result<usize, Error> {
        formatting::digits(self, text, out)
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

    /// A catalog whose every text is `len` bytes of `x` and whose arguments
    /// take `args` bytes; it counts the calls and the buffer sizes it got.
    struct Catalog {
        len: usize,
        args: usize,
        calls: core::cell::RefCell<Vec<(usize, usize)>>,
    }

    impl Catalog {
        fn new(len: usize, args: usize) -> Self {
            Self {
                len,
                args,
                calls: core::cell::RefCell::new(Vec::new()),
            }
        }
        fn sizes(&self) -> Vec<(usize, usize)> {
            self.calls.borrow().clone()
        }
        fn fill(&self, out: &mut [u8]) -> usize {
            let n = self.len.min(out.len());
            out[..n].fill(b'x');
            self.len
        }
    }

    impl Source for Catalog {
        fn copy<'a>(&self, _: &str, out: &'a mut [u8]) -> Result<&'a str, Error> {
            self.calls.borrow_mut().push((0, out.len()));
            if self.len > out.len() {
                return Err(Error::NoSpace);
            }
            self.fill(out);
            core::str::from_utf8(&out[..self.len]).map_err(|_| Error::Utf8)
        }
        fn format(
            &self,
            _: &str,
            _: &[Arg<'_>],
            scratch: &mut [u8],
            out: &mut [u8],
        ) -> Result<usize, Error> {
            self.calls.borrow_mut().push((scratch.len(), out.len()));
            if self.args > scratch.len() {
                return Err(Error::NoSpace);
            }
            Ok(self.fill(out))
        }
        fn digits(&self, _: &str, out: &mut [u8]) -> Result<usize, Error> {
            self.calls.borrow_mut().push((0, out.len()));
            Ok(self.fill(out))
        }
    }

    const MAX: usize = formatting::MAX_MESSAGE_BYTES;

    #[test]
    fn a_short_text_is_copied_at_once_from_the_small_buffer() {
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(40, 0));
        assert_eq!(m.copy_from(&catalog, "k").unwrap().len(), 40);
        assert_eq!(catalog.sizes(), [(0, FIRST_MESSAGE)]);
    }

    #[test]
    fn a_long_copy_is_asked_again_with_4_kib_and_one_too_long_fails() {
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(1000, 0));
        assert_eq!(m.copy_from(&catalog, "k").unwrap().len(), 1000);
        assert_eq!(catalog.sizes(), [(0, FIRST_MESSAGE), (0, MAX)]);
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(MAX + 1, 0));
        assert_eq!(m.copy_from(&catalog, "k"), Err(Error::NoSpace));
        assert_eq!(catalog.sizes(), [(0, FIRST_MESSAGE), (0, MAX)]);
    }

    #[test]
    fn a_long_format_is_asked_again_with_its_length_and_one_too_long_fails() {
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(1500, 10));
        assert_eq!(m.format_from(&catalog, "k", &[]).unwrap().len(), 1500);
        assert_eq!(
            catalog.sizes(),
            [(FIRST_ARGUMENTS, FIRST_MESSAGE), (FIRST_ARGUMENTS, 1500)]
        );
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(MAX + 1, 10));
        assert_eq!(m.format_from(&catalog, "k", &[]), Err(Error::NoSpace));
        assert_eq!(catalog.sizes(), [(FIRST_ARGUMENTS, FIRST_MESSAGE)]);
    }

    #[test]
    fn arguments_too_long_are_encoded_again_with_1_kib_and_then_fail() {
        let args = formatting::MAX_ARGUMENT_BYTES;
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(20, 600));
        assert_eq!(m.format_from(&catalog, "k", &[]).unwrap().len(), 20);
        assert_eq!(
            catalog.sizes(),
            [(FIRST_ARGUMENTS, FIRST_MESSAGE), (args, FIRST_MESSAGE)]
        );
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(20, args + 1));
        assert_eq!(m.format_from(&catalog, "k", &[]), Err(Error::NoSpace));
        assert_eq!(
            catalog.sizes(),
            [(FIRST_ARGUMENTS, FIRST_MESSAGE), (args, FIRST_MESSAGE)]
        );
    }

    #[test]
    fn long_digits_are_asked_again_with_their_length_and_too_long_fail() {
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(700, 0));
        assert_eq!(m.digits_from(&catalog, "12").unwrap().len(), 700);
        assert_eq!(catalog.sizes(), [(0, FIRST_MESSAGE), (0, 700)]);
        let (mut m, catalog) = (Messages::new().unwrap(), Catalog::new(MAX + 1, 0));
        assert_eq!(m.digits_from(&catalog, "12"), Err(Error::NoSpace));
        assert_eq!(catalog.sizes(), [(0, FIRST_MESSAGE)]);
    }
}
