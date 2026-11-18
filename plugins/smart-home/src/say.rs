//! The catalog's texts, made fallibly, so that the rows and notes are made
//! without a host in tests, and how an entity's state is worded.
use crate::model::{Entity, Reading};
use alloc::string::String;
use paperpaw_sdk::native::{Error, Host, formatting::Arg};
use paperpaw_ui_kit::Messages;

/// The words of the catalog.
pub trait Say {
    /// The text of `key`.
    ///
    /// # Errors
    /// `NoMemory`, or what the host says.
    fn copy(&mut self, key: &str) -> Result<String, Error>;
    /// The text of `key` with its names filled in; `Arg::Count` selects the
    /// plural form.
    ///
    /// # Errors
    /// `NoMemory`, or what the host says.
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error>;
}

/// The host's catalog.
pub struct HostSay<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl<'h, 'a> HostSay<'h, 'a> {
    /// The catalog's texts through `host`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(host: &'h Host<'a>) -> Result<Self, Error> {
        Ok(Self {
            host,
            messages: Messages::new()?,
        })
    }
}

impl Say for HostSay<'_, '_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }

    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }
}

/// How `entity`'s state reads, in the reader's language.
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn value_text(say: &mut impl Say, entity: &Entity) -> Result<String, Error> {
    match entity.reading()? {
        Reading::Text(text) => Ok(text),
        Reading::Word(key) => say.copy(key),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A catalog that answers with the key (and the first argument's text).
    #[derive(Default)]
    pub(crate) struct Keys;

    impl Say for Keys {
        fn copy(&mut self, key: &str) -> Result<String, Error> {
            Ok(key.into())
        }

        fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
            let mut out = String::from(key);
            for arg in args {
                match arg {
                    Arg::Text { value, .. } => {
                        out.push(' ');
                        out.push_str(value);
                    }
                    Arg::Count(n) => out.push_str(&alloc::format!(" {n}")),
                    _ => {}
                }
            }
            Ok(out)
        }
    }
}
