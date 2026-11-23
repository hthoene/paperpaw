//! The catalogs' passwords among the plugin's secrets (`core.secrets`, one
//! secret a catalog named by the digest of its address, `secrets.md` section
//! 7): read, kept and the `Authorization` header made as every WebDAV plugin
//! does (ui-kit `dav::secret`), taken away and listed here. Every buffer that
//! held a secret is wiped, also where a call fails; no secret is ever shown or
//! logged.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    digest::md5::Md5,
    native::{
        Error, Host,
        secrets::{SecretList, SecretName, Secrets},
    },
};
pub use paperpaw_ui_kit::dav::secret::{Secret, keep, present, read};
use paperpaw_ui_kit::{
    mem::{buffer, push, wipe},
    text::copy,
};

/// The input and the reply a secrets call needs.
const INPUT: usize = 1280;
const REPLY: usize = 4096;
/// Letters of the digest that name a catalog's secret.
const NAME_LEN: usize = 16;

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
    fn names_are_sixteen_letters_of_the_digest() {
        let name = name_of("https://a.example/opds").unwrap();
        assert_eq!(name.len(), 16);
        assert_ne!(name, name_of("https://b.example/opds").unwrap());
        assert!(name.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
