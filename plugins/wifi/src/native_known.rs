//! The networks joined before, kept among the plugin's secrets
//! (`core.secrets`, `docs/abi/secrets.md`): one secret for each network,
//! named by the network's name, labelled `Wi-Fi <name>`, its value the
//! password (empty for an open network), changeable in the list of passwords
//! and keys. Interface 6 kept them as `wifi.<name>`; the plugin's own names
//! have no prefix (`secrets.md` §7), and `SecretName` takes the signs a
//! network's name may hold (tab and line breaks included).
//!
//! Host-free parts (`Remember`, `ssid_of`, `known_with`) and the calls that
//! use the facade with caller-owned buffers (`remember`, `forget`,
//! `password_of`, `names`); every buffer that held a password is wiped.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::Host;
use paperpaw_sdk::native::secrets::{SecretList, SecretName, SecretWrite, Secrets, wipe};
use paperpaw_ui_kit::{mem, text};

use crate::native_policy::{Known, Password};

/// The longest name of a network (`wifi_join.ssid`).
pub const MAX_SSID: usize = 32;
/// The longest password (`wifi_join.password`: WPA's 63 characters).
pub const MAX_PASSWORD: usize = 63;
/// What a secrets call needs of its buffers (`secrets.md` 2: the host refuses a
/// reply buffer smaller than the op's `max_reply`, an input longer than
/// `max_input`): `keep` takes 1,280 B in and gives 1,280 B; `get` and `remove`
/// take 256 B in and give 1,280 B; `list` takes 256 B in and gives 4,096 B.
pub const KEEP_INPUT: usize = 1280;
/// The input of `get`, `remove` and `list`.
pub const QUERY_INPUT: usize = 256;
/// The reply of `keep`, `get` and `remove`.
pub const SMALL_REPLY: usize = 1280;
/// The reply of `list`.
pub const LIST_REPLY: usize = 4096;
/// Networks kept at most: more are not remembered (the list of secrets).
pub const MAX_KNOWN: usize = 64;

/// What to keep for a network joined: its name, label and password, owned so
/// that the request can be built any time.
pub struct Remember {
    ssid: String,
    label: String,
    password: Password,
}

impl core::fmt::Debug for Remember {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Remember")
            .field("ssid", &self.ssid)
            .finish_non_exhaustive()
    }
}

impl Remember {
    /// The request for network `ssid` with `password`.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a name or password that cannot be a network's
    /// (empty or over [`MAX_SSID`] bytes, a control sign other than tab and
    /// line breaks, a password over [`MAX_PASSWORD`] bytes); [`Error::NoMemory`].
    pub fn new(ssid: &str, password: &str) -> Result<Self, Error> {
        if ssid.len() > MAX_SSID || password.len() > MAX_PASSWORD {
            return Err(Error::Invalid);
        }
        SecretName::new(ssid)?;
        let label = text::concat(&["Wi-Fi ", ssid])?;
        SecretWrite::new(SecretName::new(ssid)?, "")?.label(&label)?;
        Ok(Self {
            ssid: text::copy(ssid)?,
            label,
            password: Password::new(password)?,
        })
    }

    /// The write to hand to the secrets service.
    ///
    /// # Errors
    /// What the service's checks say.
    pub fn write(&self) -> Result<SecretWrite<'_>, Error> {
        SecretWrite::new(SecretName::new(&self.ssid)?, self.password.as_str())?
            .label(&self.label)
            .map(SecretWrite::changeable)
    }
}

/// The names of the networks among the plugin's own secrets `names`, in the
/// order given, at most [`MAX_KNOWN`] (the rest are not read).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn ssids_of<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Vec<String>, Error> {
    let mut list = Vec::new();
    for name in names.into_iter().take(MAX_KNOWN) {
        mem::push(&mut list, text::copy(name)?)?;
    }
    Ok(list)
}

/// The policy's list of known networks for `names`, each password asked of
/// `password_of` (`None` for a secret that is gone since the list: skipped).
///
/// # Errors
/// [`Error::NoMemory`]; what `password_of` says.
pub fn known_with(
    names: &[String],
    mut password_of: impl FnMut(&str) -> Result<Option<Password>, Error>,
) -> Result<Vec<Known>, Error> {
    let mut known = Vec::new();
    for name in names {
        if let Some(password) = password_of(name)? {
            mem::push(
                &mut known,
                Known {
                    ssid: text::copy(name)?,
                    password,
                },
            )?;
        }
    }
    Ok(known)
}

/// Keeps the network `remember` names, with its password.
///
/// # Errors
/// What the service says; [`Error::NoMemory`].
pub fn remember(secrets: &Secrets, host: &Host<'_>, remember: &Remember) -> Result<(), Error> {
    let mut input = mem::buffer(KEEP_INPUT)?;
    let mut reply = mem::buffer(SMALL_REPLY)?;
    let result = remember
        .write()
        .and_then(|write| secrets.keep(host, write, &mut input, &mut reply));
    // `keep` clears its input; the reply holds no value.
    wipe(&mut input);
    wipe(&mut reply);
    result
}

/// Forgets network `ssid`; one that is not kept is no error.
///
/// # Errors
/// What the service says; [`Error::NoMemory`].
pub fn forget(secrets: &Secrets, host: &Host<'_>, ssid: &str) -> Result<(), Error> {
    let mut input = mem::buffer(QUERY_INPUT)?;
    let mut reply = mem::buffer(SMALL_REPLY)?;
    let result =
        SecretName::new(ssid).and_then(|name| secrets.remove(host, name, &mut input, &mut reply));
    wipe(&mut input);
    wipe(&mut reply);
    result
}

/// The password kept for network `ssid`: `None` for a network not kept.
///
/// # Errors
/// What the service says; [`Error::NoMemory`].
pub fn password_of(
    secrets: &Secrets,
    host: &Host<'_>,
    ssid: &str,
) -> Result<Option<Password>, Error> {
    let mut input = mem::buffer(QUERY_INPUT)?;
    let mut reply = mem::buffer(SMALL_REPLY)?;
    let found = match secrets.get(host, ssid, &mut input, &mut reply) {
        Ok(value) => Password::new(value.value()).map(Some),
        Err(Error::NotFound) => Ok(None),
        Err(error) => Err(error),
    };
    // The view of the reply is gone; the value is wiped where it was read.
    wipe(&mut input);
    wipe(&mut reply);
    found
}

/// Whether any network is kept: the list read until its first entry, no name
/// copied and no password read (Beta 3 asked the list's length).
///
/// # Errors
/// What the service says; [`Error::NoMemory`].
pub fn any(secrets: &Secrets, host: &Host<'_>) -> Result<bool, Error> {
    let mut input = mem::buffer(QUERY_INPUT)?;
    let mut reply = mem::buffer(LIST_REPLY)?;
    let result = (|| {
        let mut query = SecretList::own();
        loop {
            let page = secrets.list(host, query, &mut input, &mut reply)?;
            let more = page.end().next();
            if let Some(entry) = page.into_iter().next() {
                entry?;
                return Ok(true);
            }
            match more {
                Some(cursor) => query = SecretList::own().from(cursor),
                None => return Ok(false),
            }
        }
    })();
    wipe(&mut input);
    wipe(&mut reply);
    result
}

/// The names of the networks kept, page after page of the list, at most
/// [`MAX_KNOWN`].
///
/// # Errors
/// What the service says; [`Error::NoMemory`].
pub fn names(secrets: &Secrets, host: &Host<'_>) -> Result<Vec<String>, Error> {
    let mut input = mem::buffer(QUERY_INPUT)?;
    let mut reply = mem::buffer(LIST_REPLY)?;
    let mut names: Vec<String> = Vec::new();
    let mut query = SecretList::own();
    let result = (|| {
        loop {
            let page = secrets.list(host, query, &mut input, &mut reply)?;
            let more = page.end().next();
            for entry in page {
                if names.len() == MAX_KNOWN {
                    return Ok(());
                }
                mem::push(&mut names, text::copy(entry?.name())?)?;
            }
            match more {
                Some(cursor) => query = SecretList::own().from(cursor),
                None => return Ok(()),
            }
        }
    })();
    wipe(&mut input);
    wipe(&mut reply);
    result.map(|()| names)
}
