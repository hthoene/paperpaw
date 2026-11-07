//! Reusable host catalog buffers; strings outlive only their owned copies.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    formatting::{self, Arg, FormatBuffers},
};

#[derive(Debug)]
pub struct Messages {
    out: Vec<u8>,
    arguments: Vec<u8>,
}
impl Messages {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            out: buffer(formatting::MAX_MESSAGE_BYTES)?,
            arguments: buffer(formatting::MAX_ARGUMENT_BYTES)?,
        })
    }
    pub fn copy(&mut self, host: &Host<'_>, key: &str) -> Result<String, Error> {
        crate::text::copy(formatting::copy_message(host, key, &mut self.out)?)
    }
    pub fn format(
        &mut self,
        host: &Host<'_>,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
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
        let length = formatting::digits(host, text, &mut self.out)?;
        let bytes = self.out.get(..length).ok_or(Error::NoSpace)?;
        crate::text::copy(core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?)
    }
}
