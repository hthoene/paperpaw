//! The words of the screens: texts as the catalog writes them. Over [`Words`],
//! so the rows of the screens are made and tested without a host; the host's
//! catalog is behind [`HostWords`].

use alloc::string::String;

use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::Messages;

/// Catalog texts of the plugin, made fallibly.
pub(crate) trait Words {
    /// The text of `key`.
    fn copy(&mut self, key: &str) -> Result<String, Error>;
    /// The text of `key` with its names filled in; `Arg::Count` selects the
    /// plural form.
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error>;
}

/// The host's catalog.
pub(crate) struct HostWords<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl<'h, 'a> HostWords<'h, 'a> {
    /// The catalog of the language the reader chose.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn new(host: &'h Host<'a>) -> Result<Self, Error> {
        Ok(Self {
            host,
            messages: Messages::new()?,
        })
    }
}

impl Words for HostWords<'_, '_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }
}
