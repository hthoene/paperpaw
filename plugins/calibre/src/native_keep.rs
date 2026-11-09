//! The password set in Calibre among the plugin's secrets (`core.secrets`, the
//! name `password`, `secrets.md` section 7), and the text type that zeroes
//! itself. Every buffer that held a secret is wiped, also where a call fails.
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host,
    secrets::{SecretName, SecretWrite, Secrets},
};
use paperpaw_ui_kit::{
    mem::{buffer, wipe, wipe_text},
    text::copy,
};

/// The password set in Calibre.
pub const PASSWORD: &str = "password";
/// The input and the reply a secrets call needs.
const INPUT: usize = 1280;
const REPLY: usize = 4096;

/// A text that is a secret: zeroed when it is dropped, also on an early
/// return, and never shown by `Debug`.
#[derive(Default)]
pub struct Secret(String);

impl Secret {
    /// A copy of `text`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn copy(text: &str) -> Result<Self, Error> {
        copy(text).map(Self)
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe_text(core::mem::take(&mut self.0));
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// The secret `name`; `None` where there is none (or an empty one).
///
/// # Errors
/// What the service says; `NoMemory`.
pub fn read(secrets: &Secrets, host: &Host<'_>, name: &str) -> Result<Option<Secret>, Error> {
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let found = match secrets.get(host, name, &mut input, &mut reply) {
        Ok(value) if value.value().is_empty() => Ok(None),
        Ok(value) => Secret::copy(value.value()).map(Some),
        Err(Error::NotFound) => Ok(None),
        Err(error) => Err(error),
    };
    wipe(&mut input);
    wipe(&mut reply);
    found
}

/// Keeps `value` as the secret `name`, called `label` (the user may change it
/// among the passwords and keys).
///
/// # Errors
/// What the service says (`Unsupported` where the device keeps no secrets);
/// `NoMemory`.
pub fn keep(
    secrets: &Secrets,
    host: &Host<'_>,
    name: &str,
    label: &str,
    value: &str,
) -> Result<(), Error> {
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let result = SecretName::new(name)
        .and_then(|name| SecretWrite::new(name, value))
        .and_then(|write| write.label(label))
        .map(SecretWrite::changeable)
        .and_then(|write| secrets.keep(host, write, &mut input, &mut reply));
    wipe(&mut input);
    wipe(&mut reply);
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn a_secret_is_never_shown() {
        let secret = Secret::copy("hunter2").unwrap();
        assert_eq!(secret.as_str(), "hunter2");
        assert_eq!(alloc::format!("{secret:?}"), "Secret(..)");
        assert!(!alloc::format!("{:?}", Some(secret)).contains("hunter"));
    }
}
