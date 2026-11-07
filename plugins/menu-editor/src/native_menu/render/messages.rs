//! Reusable host catalog buffers; strings outlive only their owned copies.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    formatting::{self, Arg, FormatBuffers},
};

pub(super) struct Messages {
    out: Vec<u8>,
    arguments: Vec<u8>,
}
impl Messages {
    pub(super) fn new() -> Result<Self, Error> {
        Ok(Self {
            out: buffer(formatting::MAX_MESSAGE_BYTES)?,
            arguments: buffer(formatting::MAX_ARGUMENT_BYTES)?,
        })
    }
    pub(super) fn copy(&mut self, host: &Host<'_>, key: &str) -> Result<String, Error> {
        super::super::copy(formatting::copy_message(host, key, &mut self.out)?)
    }
    pub(super) fn format(
        &mut self,
        host: &Host<'_>,
        key: &str,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
        let mut buffers = FormatBuffers::new(&mut self.arguments, &mut self.out);
        let length = formatting::format_message(host, key, args, &mut buffers)?;
        let bytes = self.out.get(..length).ok_or(Error::NoSpace)?;
        super::super::copy(core::str::from_utf8(bytes).map_err(|_| Error::Utf8)?)
    }
}
fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}
