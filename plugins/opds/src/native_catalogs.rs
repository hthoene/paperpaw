//! The catalogs the user added: a list in the plugin's data folder
//! (`catalogs.txt`), one catalog a line with its fields separated by tabs, as
//! interface 6 kept it (version 1, `docs/data-files.md`). Until the user
//! changes it, the list holds a free catalog to start with. Their passwords
//! are secrets (`native_keep`), named by the catalog's address; the list's own
//! password column stays empty except for a password an older version put
//! there that no secret holds yet.
//!
//! Nothing here panics or aborts on a bad list: lines without an address are
//! passed over, a list that is too large or not text is an error (and so never
//! written over, which would lose the catalogs and their passwords), every
//! allocation is fallible.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{native::Error, web};
use paperpaw_ui_kit::{
    mem::{push, wipe_text},
    text::{concat, copy},
};

use crate::native_files::{self as files, CATALOGS, Files};
use crate::native_keep::{self as keep, Secret};

/// The most the list may take; a list that has outgrown it is not read.
pub const MAX_FILE: usize = 64 * 1024;
/// How many catalogs the list holds, by the device's memory class.
#[must_use]
pub const fn max_catalogs(small: bool) -> usize {
    if small { 100 } else { 500 }
}

/// A catalog the user added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Catalog {
    pub name: String,
    pub url: String,
    pub user: String,
    /// Where syncing puts its books, below the library folder; empty for a
    /// folder named after the catalog.
    pub folder: String,
    /// Synced with the others by *Sync catalogs*.
    pub sync: bool,
    /// A WebDAV folder (e.g. in Nextcloud) rather than an OPDS catalog.
    pub webdav: bool,
    /// A password an older version kept in the list: used until a secret
    /// holds it, then let go of (see [`move_passwords`]).
    pub legacy: Secret,
}

impl Catalog {
    /// A new catalog at `url`, named after its host.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(url: &str) -> Result<Self, Error> {
        let host = web::host(url);
        Ok(Self {
            name: copy(host.strip_prefix("www.").unwrap_or(host))?,
            url: copy(url)?,
            // Nextcloud's and ownCloud's addresses say so.
            webdav: url.contains("/remote.php/") || url.contains("/webdav"),
            ..Self::default()
        })
    }

    /// The folder its books go to, below the library folder: the one set
    /// (a leading `library` folder's name is left out, as the card's own
    /// path of it would be written), else one named after the catalog.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn sync_folder(&self, library: &str) -> Result<String, Error> {
        let set = self.folder.trim().trim_matches('/');
        if !set.is_empty() {
            return copy(strip_library(set, library));
        }
        let name = web::try_clean_name(&self.name, 60).map_err(|_| Error::NoMemory)?;
        copy(name.as_deref().unwrap_or("OPDS"))
    }

    /// The folder as the reader sees it: from the card's root.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn shown_folder(&self, library: &str) -> Result<String, Error> {
        let inner = self.sync_folder(library)?;
        let library = library.trim_matches('/');
        if library.is_empty() {
            Ok(inner)
        } else {
            concat(&[library, "/", &inner])
        }
    }
}

/// `folder` without the library folder at its start, which interface 6 had
/// the folder name from the card's root with.
fn strip_library<'a>(folder: &'a str, library: &str) -> &'a str {
    let library = library.trim_matches('/');
    if library.is_empty() {
        return folder;
    }
    match folder.strip_prefix(library) {
        Some("") => "",
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/'),
        _ => folder,
    }
}

/// The list: until the user changes it, one catalog to start with.
///
/// # Errors
/// `NoSpace` (too large or too many), `Utf8` (not text), `NoMemory`, what the
/// card says: the list is there but cannot be read, so nothing may be saved
/// over it.
pub fn load(files: &dyn Files, max: usize) -> Result<Vec<Catalog>, Error> {
    match files::read_text(files, CATALOGS, MAX_FILE)? {
        Some(text) => parse(&text, max),
        None => {
            let mut list = Vec::new();
            push(
                &mut list,
                Catalog {
                    name: copy("Project Gutenberg")?,
                    url: copy("https://www.gutenberg.org/ebooks.opds/")?,
                    ..Catalog::default()
                },
            )?;
            Ok(list)
        }
    }
}

/// The list as the text has it.
///
/// # Errors
/// `NoSpace` for more than `max` catalogs, `NoMemory`.
pub fn parse(text: &str, max: usize) -> Result<Vec<Catalog>, Error> {
    let mut list = Vec::new();
    for line in text.lines() {
        let mut fields = line.split('\t');
        let name = fields.next().unwrap_or_default();
        let Some(url) = fields.next().filter(|url| !url.is_empty()) else {
            continue;
        };
        if list.len() >= max {
            return Err(Error::NoSpace);
        }
        let user = fields.next().unwrap_or_default();
        let password = fields.next().unwrap_or_default();
        let folder = fields.next().unwrap_or_default();
        let sync = fields.next() == Some("sync");
        let webdav = fields.next() == Some("webdav");
        push(
            &mut list,
            Catalog {
                name: copy(name)?,
                url: copy(url)?,
                user: copy(user)?,
                folder: copy(folder)?,
                sync,
                webdav,
                legacy: Secret::copy(password)?,
            },
        )?;
    }
    Ok(list)
}

/// A field as the list keeps it: no tab and no line break in it.
fn put(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    out.extend(text.chars().map(|c| {
        if matches!(c, '\t' | '\n' | '\r') {
            ' '
        } else {
            c
        }
    }));
    Ok(())
}

/// The file's text: the password's place holds what an older version kept
/// there that is not moved yet, else it stays empty.
///
/// # Errors
/// `NoSpace` where the text would pass [`MAX_FILE`], `NoMemory`.
pub fn encode(catalogs: &[Catalog]) -> Result<String, Error> {
    let mut text = String::new();
    for c in catalogs {
        put(&mut text, &c.name)?;
        text.push('\t');
        put(&mut text, &c.url)?;
        text.push('\t');
        put(&mut text, &c.user)?;
        text.push('\t');
        put(&mut text, c.legacy.as_str())?;
        text.push('\t');
        put(&mut text, &c.folder)?;
        text.push('\t');
        if c.sync {
            text.push_str("sync");
        }
        text.push('\t');
        if c.webdav {
            text.push_str("webdav");
        }
        text.push('\n');
        if text.len() > MAX_FILE {
            wipe_text(text);
            return Err(Error::NoSpace);
        }
    }
    Ok(text)
}

/// Keeps the list. `catalogs` must come from [`load`]: what is not in it is
/// gone.
///
/// # Errors
/// As [`encode`], and what the card says.
pub fn save(files: &dyn Files, catalogs: &[Catalog]) -> Result<(), Error> {
    let text = encode(catalogs)?;
    let written = files.write(CATALOGS, text.as_bytes());
    // The text may hold a password an older version kept there.
    wipe_text(text);
    written
}

/// Where the passwords are kept: the device's secrets, or nowhere where it
/// keeps none (`Unsupported`).
pub trait Vault {
    /// The secret `name`, `None` where there is none.
    ///
    /// # Errors
    /// What the service says.
    fn read(&self, name: &str) -> Result<Option<Secret>, Error>;
    /// Keeps `value` as the secret `name`, called `label`.
    ///
    /// # Errors
    /// What the service says; `Unsupported` where none are kept.
    fn keep(&self, name: &str, label: &str, value: &str) -> Result<(), Error>;
    /// Takes the secret `name` away.
    ///
    /// # Errors
    /// What the service says.
    fn remove(&self, name: &str) -> Result<(), Error>;
    /// The names of the secrets that are not in `wanted`.
    ///
    /// # Errors
    /// What the service says.
    fn others(&self, wanted: &[String]) -> Result<Vec<String>, Error>;
}

/// The password of `catalog`: the secret's, else one an older version kept in
/// the list; `None` where there is none.
///
/// # Errors
/// `NoMemory` and what the service says but `NotFound`.
pub fn password(vault: &dyn Vault, catalog: &Catalog) -> Result<Option<Secret>, Error> {
    match vault.read(&keep::name_of(&catalog.url)?) {
        Ok(Some(secret)) => Ok(Some(secret)),
        Ok(None) | Err(Error::Unsupported) => {
            Ok((!catalog.legacy.is_empty()).then(|| catalog.legacy.clone()))
        }
        Err(error) => Err(error),
    }
}

/// Moves the passwords an older version kept in the list to the secrets: a
/// list lets go of one only once a secret holds it. Returns whether any
/// changed (the list is to be saved). Where the device keeps none, or one
/// cannot be kept, the list keeps it as before.
///
/// # Errors
/// `NoMemory`.
pub fn move_passwords(
    vault: &dyn Vault,
    catalogs: &mut [Catalog],
    label: &mut dyn FnMut(&str) -> Result<String, Error>,
) -> Result<bool, Error> {
    let mut changed = false;
    for catalog in catalogs.iter_mut().filter(|c| !c.legacy.is_empty()) {
        let name = keep::name_of(&catalog.url)?;
        // A secret held already is the user's later choice: it stays, and this
        // older password goes.
        let held = match vault.read(&name) {
            Ok(Some(_)) => true,
            Ok(None) => {
                let label = label(&catalog.name)?;
                match vault.keep(&name, &label, catalog.legacy.as_str()) {
                    Ok(()) => true,
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => false,
                }
            }
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => false,
        };
        if held {
            catalog.legacy = Secret::default();
            changed = true;
        }
    }
    Ok(changed)
}

/// Takes the secrets of catalogs that are no longer in the list (removed, or
/// moved to another address) away. A failure is let go of but a refused
/// allocation: the next save tries again.
///
/// # Errors
/// `NoMemory`.
pub fn prune(vault: &dyn Vault, catalogs: &[Catalog]) -> Result<(), Error> {
    let mut wanted = Vec::new();
    for catalog in catalogs {
        push(&mut wanted, keep::name_of(&catalog.url)?)?;
    }
    let others = match vault.others(&wanted) {
        Ok(others) => others,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(()),
    };
    for name in others {
        // A secret that cannot be taken away now goes with the next save.
        if let Err(Error::NoMemory) = vault.remove(&name) {
            return Err(Error::NoMemory);
        }
    }
    Ok(())
}

/// What a change of a catalog's address does to its password: the secret
/// moves with it (a catalog's secret is named by its address). `Ok(false)`
/// where it could not be kept at the new address.
///
/// # Errors
/// `NoMemory`.
pub fn move_secret(vault: &dyn Vault, from: &str, to: &str, label: &str) -> Result<bool, Error> {
    if from == to {
        return Ok(true);
    }
    let Some(secret) = (match vault.read(&keep::name_of(from)?) {
        Ok(secret) => secret,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => None,
    }) else {
        return Ok(true);
    };
    match vault.keep(&keep::name_of(to)?, label, secret.as_str()) {
        Ok(()) => Ok(true),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(false),
    }
}
