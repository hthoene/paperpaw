//! The catalogs' passwords among the plugin's secrets (`core.secrets`, one
//! secret a catalog named by the digest of its address, `secrets.md` section
//! 7), the text type that zeroes itself and the `Authorization` header made of
//! a user name and a password. Every buffer that held a secret is wiped, also
//! where a call fails; no secret is ever shown or logged.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    digest::md5::Md5,
    native::{
        Error, Host,
        secrets::{SecretList, SecretName, SecretWrite, Secrets},
    },
};
use paperpaw_ui_kit::{
    mem::{buffer, push, wipe, wipe_text},
    text::copy,
};

/// The input and the reply a secrets call needs.
const INPUT: usize = 1280;
const REPLY: usize = 4096;
/// Letters of the digest that name a catalog's secret.
const NAME_LEN: usize = 16;

/// A text that is a secret: zeroed when it is dropped, also on an early
/// return, and never shown by `Debug`.
#[derive(Default)]
pub struct Secret(String);

impl Secret {
    /// A second copy of the secret (both are zeroed when dropped).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn try_clone(&self) -> Result<Self, Error> {
        copy(&self.0).map(Self)
    }

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

    /// Whether there is nothing in it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `Basic ` and the Base64 of `user:password`: what a catalog that wants
    /// an account is sent. The pieces it is made of are zeroed as it is made.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn basic(user: &str, password: &Secret) -> Result<Self, Error> {
        let mut plain = Vec::new();
        plain
            .try_reserve_exact(user.len() + 1 + password.as_str().len())
            .map_err(|_| Error::NoMemory)?;
        plain.extend_from_slice(user.as_bytes());
        plain.push(b':');
        plain.extend_from_slice(password.as_str().as_bytes());
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

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Secret {}

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

/// The name of the secret of the catalog at `url`: the first letters of the
/// digest of its address.
///
/// # Errors
/// `NoMemory`.
pub fn name_of(url: &str) -> Result<String, Error> {
    let mut hash = Md5::new();
    hash.update(url.as_bytes());
    let mut hex = [0u8; 32];
    let full = hash.hex_into(&mut hex);
    copy(full.get(..NAME_LEN).ok_or(Error::Invalid)?)
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

/// Whether the secret `name` is kept.
///
/// # Errors
/// What the service says; `NoMemory`.
pub fn present(secrets: &Secrets, host: &Host<'_>, name: &str) -> Result<bool, Error> {
    read(secrets, host, name).map(|secret| secret.is_some())
}

/// Keeps `value` as the secret `name`, called `label` (the user may change it
/// among the passwords and keys).
///
/// # Errors
/// What the service says (`Unsupported` where the device keeps no secrets,
/// `Full` at the plugin's 32); `NoMemory`.
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

/// Takes the secret `name` away; one that is not there is as good.
///
/// # Errors
/// What the service says; `NoMemory`.
pub fn remove(secrets: &Secrets, host: &Host<'_>, name: &str) -> Result<(), Error> {
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let result =
        SecretName::new(name).and_then(|name| secrets.remove(host, name, &mut input, &mut reply));
    wipe(&mut input);
    wipe(&mut reply);
    result
}

/// The names of the plugin's own secrets that are not in `wanted`.
///
/// # Errors
/// What the service says; `NoMemory`.
pub fn others(secrets: &Secrets, host: &Host<'_>, wanted: &[String]) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let mut query = SecretList::own();
    loop {
        let page = secrets.list(host, query, &mut input, &mut reply)?;
        let end = page.end();
        for row in page {
            let name = row?.name();
            if !wanted.iter().any(|w| w == name) {
                push(&mut found, copy(name)?)?;
            }
        }
        match end.next() {
            Some(next) => query = SecretList::own().from(next),
            None => return Ok(found),
        }
    }
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
    fn basic_authorization_is_the_base64_of_user_and_password() {
        // The classic example is "Aladdin" and "open sesame".
        let header = Secret::basic("Aladdin", &Secret::copy("open sesame").unwrap()).unwrap();
        assert_eq!(header.as_str(), "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ==");
        for (user, password, expected) in [
            ("a", "", "Basic YTo="),
            ("ab", "c", "Basic YWI6Yw=="),
            ("", "", "Basic Og=="),
        ] {
            let header = Secret::basic(user, &Secret::copy(password).unwrap()).unwrap();
            assert_eq!(header.as_str(), expected);
        }
    }

    #[test]
    fn names_are_sixteen_letters_of_the_digest() {
        let name = name_of("https://a.example/opds").unwrap();
        assert_eq!(name.len(), 16);
        assert_ne!(name, name_of("https://b.example/opds").unwrap());
        assert!(name.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
