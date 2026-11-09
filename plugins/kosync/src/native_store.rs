//! What the native sync keeps: the account's key among the plugin's secrets
//! (`core.secrets`, the name `key`, not changeable), the device id in
//! `device.ini` of the data folder, and the move of a key an earlier version
//! kept in `account.ini`. The parts that read a text are host-free; the calls
//! use the facades with caller-owned buffers, and every buffer that held the key
//! is wiped.
use alloc::{borrow::Cow, string::String};

pub use crate::native_model::Secret;

use paperpaw_sdk::{
    config,
    digest::md5::Md5,
    native::{
        Error, Host,
        secrets::{SecretName, SecretWrite, Secrets},
        storage::{self, MAX_DATA, Root},
    },
};
use paperpaw_ui_kit::{
    mem::{buffer, wipe, wipe_text},
    text::{concat, copy},
};

/// The key's name among the plugin's secrets (`secrets.md` section 7).
pub const KEY: &str = "key";
/// The file of the device's id, in the data folder.
pub const DEVICE: &str = "device.ini";
/// The file an earlier version kept the key in.
pub const OLD_ACCOUNT: &str = "account.ini";
/// The most of those files that is read.
pub const MOST_FILE: usize = 4096;
/// The input and the reply a secrets call needs.
const INPUT: usize = 1280;
const REPLY: usize = 4096;

/// The value of `key` in a file's `text` (the last one; empty lines and
/// lines that do not parse are left out), copied.
///
/// # Errors
/// `NoMemory`.
pub fn value_of(text: &str, key: &str) -> Result<Option<String>, Error> {
    let Some(entry) = config::parse(text)
        .filter_map(Result::ok)
        .filter(|e| e.key == key)
        .last()
    else {
        return Ok(None);
    };
    let value = entry.value.try_as_str().map_err(|_| Error::NoMemory)?;
    let kept = copy(&value);
    // An escaped value is a copy of its own: the key's, when it is one.
    if let Cow::Owned(owned) = value {
        wipe_text(owned);
    }
    kept.map(Some)
}

/// The text of `device.ini` for the id `id`.
///
/// # Errors
/// `NoMemory`.
pub fn device_text(id: &str) -> Result<String, Error> {
    concat(&["[device]\nid = ", id, "\n"])
}

/// An id of 32 upper-case hex digits from `seed` bytes: the MD5 of them. The
/// seed need not be secret: the id only tells the server's other devices apart.
///
/// # Errors
/// `NoMemory`.
pub fn id_from(seed: &[u8]) -> Result<String, Error> {
    let mut md5 = Md5::new();
    md5.update(seed);
    let mut digits = [0; 32];
    md5.hex_into(&mut digits);
    digits.make_ascii_uppercase();
    core::str::from_utf8(&digits).map_or(Err(Error::Invalid), copy)
}

/// A file of the data folder, read whole up to [`MOST_FILE`] bytes; `None` for
/// a file that is not there, or is larger, or is not text. `secret` wipes the
/// buffer before it is dropped.
fn read_file(host: &Host<'_>, name: &str) -> Result<Option<String>, Error> {
    let size = match storage::size(host, Root::Data, name) {
        Ok(size) => usize::try_from(size).unwrap_or(usize::MAX),
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(None),
    };
    if size > MOST_FILE {
        return Ok(None);
    }
    let mut bytes = buffer(size)?;
    let mut at = 0;
    while at < size {
        let end = size.min(at + MAX_DATA);
        let Some(part) = bytes.get_mut(at..end) else {
            break;
        };
        match storage::read_at(host, Root::Data, name, at as u64, part) {
            Ok(0) | Err(_) => {
                wipe(&mut bytes);
                return Ok(None);
            }
            Ok(n) => at += n,
        }
    }
    let text = core::str::from_utf8(&bytes).map_or(Ok(None), |text| copy(text).map(Some));
    wipe(&mut bytes);
    text
}

/// This device's id: the one kept in `device.ini`, else a new one made from
/// `seed` and kept (a file that cannot be written leaves the id for this
/// start only).
///
/// # Errors
/// `NoMemory`.
pub fn device_id(host: &Host<'_>, seed: &[u8]) -> Result<String, Error> {
    if let Some(text) = read_file(host, DEVICE)?
        && let Some(id) = value_of(&text, "id")?.filter(|id| !id.is_empty())
    {
        return Ok(id);
    }
    let id = id_from(seed)?;
    let text = device_text(&id)?;
    // Not kept: another id at the next start, and the server sees another device.
    storage::write(host, Root::Data, DEVICE, text.as_bytes()).ok();
    Ok(id)
}

/// The key an earlier version kept in `account.ini`: `None` where there is
/// no such file or it has no key. The file is not touched; [`forget_old`]
/// removes it once the key is among the secrets.
///
/// # Errors
/// `NoMemory`.
pub fn old_key(host: &Host<'_>) -> Result<Option<Secret>, Error> {
    let Some(text) = read_file(host, OLD_ACCOUNT)?.map(Secret::new) else {
        return Ok(None);
    };
    Ok(value_of(text.as_str(), "key")?
        .filter(|key| !key.is_empty())
        .map(Secret::new))
}

/// Removes `account.ini`.
pub fn forget_old(host: &Host<'_>) {
    storage::remove(host, Root::Data, OLD_ACCOUNT).ok();
}

/// The account's key among the secrets; `None` where there is none (or an
/// empty one).
///
/// # Errors
/// What the service says; `NoMemory`.
pub fn read_key(secrets: &Secrets, host: &Host<'_>) -> Result<Option<Secret>, Error> {
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let found = match secrets.get(host, KEY, &mut input, &mut reply) {
        Ok(value) if value.value().is_empty() => Ok(None),
        Ok(value) => Secret::copy(value.value()).map(Some),
        Err(Error::NotFound) => Ok(None),
        Err(error) => Err(error),
    };
    wipe(&mut input);
    wipe(&mut reply);
    found
}

/// Keeps the account's key under `label` (not changeable: it is no password
/// anyone can type).
///
/// # Errors
/// What the service says (`Unsupported` where the device keeps no secrets);
/// `NoMemory`.
pub fn keep_key(secrets: &Secrets, host: &Host<'_>, label: &str, key: &str) -> Result<(), Error> {
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let result = SecretName::new(KEY)
        .and_then(|name| SecretWrite::new(name, key))
        .and_then(|write| write.label(label))
        .and_then(|write| secrets.keep(host, write, &mut input, &mut reply));
    wipe(&mut input);
    wipe(&mut reply);
    result
}

/// Removes the account's key; none is no error.
///
/// # Errors
/// What the service says; `NoMemory`.
pub fn remove_key(secrets: &Secrets, host: &Host<'_>) -> Result<(), Error> {
    let mut input = buffer(INPUT)?;
    let mut reply = buffer(REPLY)?;
    let result =
        SecretName::new(KEY).and_then(|name| secrets.remove(host, name, &mut input, &mut reply));
    wipe(&mut input);
    wipe(&mut reply);
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn values_are_read_as_interface_6_reads_them() {
        let old = include_str!("../testdata/account.ini.v1");
        assert!(value_of(old, "key").unwrap().is_some());
        assert_eq!(value_of("", "key").unwrap(), None);
        assert_eq!(
            value_of("[a]\nkey = 1\nkey = 2\n", "key")
                .unwrap()
                .as_deref(),
            Some("2")
        );
        assert_eq!(value_of("junk\n=\nkey\n", "key").unwrap(), None);
        assert_eq!(
            value_of("[d]\nid = \"A B\"\n", "id").unwrap().as_deref(),
            Some("A B")
        );
    }

    #[test]
    fn the_device_text_and_id() {
        let id = id_from(b"seed").unwrap();
        assert_eq!(id.len(), 32);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
        );
        assert_eq!(
            device_text(&id).unwrap(),
            alloc::format!("[device]\nid = {id}\n")
        );
        assert_eq!(
            value_of(&device_text(&id).unwrap(), "id")
                .unwrap()
                .as_deref(),
            Some(id.as_str())
        );
        assert_ne!(id, id_from(b"other").unwrap());
    }
}
