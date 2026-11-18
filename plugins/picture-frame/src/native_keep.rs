//! The server's password among the plugin's secrets (`core.secrets`, the name
//! `password`, `secrets.md` section 7), the text type that zeroes itself and the
//! `Authorization` header made of the user and the password. Every buffer that held a
//! secret is wiped, also where a call fails.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{
    Error, Host,
    secrets::{SecretName, SecretWrite, Secrets},
};
use paperpaw_ui_kit::{
    mem::{buffer, wipe, wipe_text},
    text::copy,
};

/// The server's password.
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

impl Secret {
    /// `Basic ` and the Base64 of `user:password`: what a server that wants an
    /// account is sent. The pieces it is made of are zeroed as it is made.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn basic(user: &str, password: &str) -> Result<Self, Error> {
        let mut plain = Vec::new();
        plain
            .try_reserve_exact(user.len() + 1 + password.len())
            .map_err(|_| Error::NoMemory)?;
        plain.extend_from_slice(user.as_bytes());
        plain.push(b':');
        plain.extend_from_slice(password.as_bytes());
        let made = Self::basic_of(&plain);
        wipe(&mut plain);
        made
    }

    fn basic_of(plain: &[u8]) -> Result<Self, Error> {
        const DIGITS: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        out.try_reserve_exact(6 + plain.len().div_ceil(3) * 4)
            .map_err(|_| Error::NoMemory)?;
        out.push_str("Basic ");
        for group in plain.chunks(3) {
            let at = |i: usize| group.get(i).copied().map_or(0, u32::from);
            let n = at(0) << 16 | at(1) << 8 | at(2);
            for shift in 0..4usize {
                if shift <= group.len() {
                    let index = usize::try_from((n >> (18 - 6 * shift)) & 63).unwrap_or(0);
                    out.push(char::from(DIGITS.get(index).copied().unwrap_or(b'A')));
                } else {
                    out.push('=');
                }
            }
        }
        Ok(Self(out))
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

/// Whether the secret `name` is kept (without copying it).
///
/// # Errors
/// What the service says; `NoMemory`.
pub fn present(secrets: &Secrets, host: &Host<'_>, name: &str) -> Result<bool, Error> {
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let found = match secrets.get(host, name, &mut input, &mut reply) {
        Ok(value) => Ok(!value.value().is_empty()),
        Err(Error::NotFound) => Ok(false),
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

    #[test]
    fn the_account_header_is_basic_and_base64() {
        for (user, password, header) in [
            (
                "Aladdin",
                "open sesame",
                "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==",
            ),
            ("a", "", "Basic YTo="),
            ("ab", "c", "Basic YWI6Yw=="),
        ] {
            assert_eq!(Secret::basic(user, password).unwrap().as_str(), header);
        }
    }
}
