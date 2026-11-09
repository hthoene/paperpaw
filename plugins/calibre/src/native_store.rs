//! What the session asks of the device, over the host: the books Calibre sent
//! are remembered in the plugin's data folder (each one's metadata as Calibre
//! gave it, without the cover thumbnail, in a file of its own named after the
//! hash of its `lpath`), with what Calibre said about itself (`device.json`)
//! and the computer the reader chose to trust (`host.txt`, its address on a
//! line); the password is a secret (`core.secrets`).
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::{
    digest::md5::Md5,
    native::{
        Error, Host,
        files::Files,
        secrets::Secrets,
        storage::{self, Root},
    },
};
use paperpaw_ui_kit::{
    files as kit_files,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
};

use crate::native_keep::{self as keep, Secret};
use crate::native_model as model;
use crate::native_session::Backing;

/// The folder of the books' records.
const BOOKS: &str = "books";
const DEVICE: &str = "device.json";
const TRUSTED: &str = "host.txt";
/// Metadata kept at most per book (Calibre's descriptions can be long): what
/// fits in a message on the smallest devices.
pub const MAX_METADATA: usize = 64 * 1024;
/// Records listed at most (a library of tens of thousands is not sent to
/// Calibre): 2,000 on a device of the small class (an X4: 32 KiB of ids on a
/// 256 KiB heap), 20,000 on any other.
const fn most_books(small: bool) -> usize {
    if small { 2_000 } else { 20_000 }
}

/// `path` below the library folder `folder` as a path on the card.
///
/// # Errors
/// `NoMemory`.
pub fn library_path(folder: &str, path: &str) -> Result<String, Error> {
    if folder.is_empty() {
        copy(path)
    } else {
        concat(&[folder, "/", path])
    }
}

/// The record of the book at `lpath`: the hash of the path names its file.
fn record(lpath: &str) -> Result<String, Error> {
    let mut md5 = Md5::new();
    md5.update(lpath.as_bytes());
    concat(&[BOOKS, "/", &md5.hex(), ".json"])
}

/// The record file of the book with `id` (see [`Backing::books`]).
fn record_of(id: u128) -> Result<String, Error> {
    let mut digits = [0u8; 32];
    for (n, slot) in digits.iter_mut().enumerate() {
        let shift = 124 - 4 * n;
        let nibble = u8::try_from((id >> shift) & 15).unwrap_or(0);
        *slot = b"0123456789abcdef"[usize::from(nibble)];
    }
    let hex = core::str::from_utf8(&digits).map_err(|_| Error::Invalid)?;
    concat(&[BOOKS, "/", hex, ".json"])
}

/// The records, the secret and the device facts, over `host`.
pub struct Store<'a, 'h> {
    pub host: &'a Host<'h>,
    pub files: Option<&'a Files>,
    pub secrets: Option<&'a Secrets>,
    pub extensions: &'a [String],
    /// The library folder below the card's top, without slashes around it.
    pub folder: &'a str,
    /// The device is of the small class (an X4).
    pub small: bool,
    pub name: &'a str,
    pub version: &'a str,
}

impl core::fmt::Debug for Store<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store<'_, '_> {
    /// `path` (below the library folder) as a path on the card.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn library_path(&self, path: &str) -> Result<String, Error> {
        library_path(self.folder, path)
    }

    /// A text file of at most `max` bytes: `None` where there is none, or it
    /// is not text; a refused allocation is `NoMemory`.
    fn read(&self, name: &str, max: usize) -> Result<Option<String>, Error> {
        match kit_files::read_all(self.host, Root::Data, name, max) {
            Ok(Some(bytes)) => Ok(String::from_utf8(bytes).ok()),
            Ok(None) | Err(Error::NotFound | Error::NoSpace) => Ok(None),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // A card that cannot be read: nothing known.
            Err(_) => Ok(None),
        }
    }

    fn write(&self, name: &str, text: &str) -> Result<(), Error> {
        match kit_files::replace(self.host, Root::Data, name, text.as_bytes()) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // Best effort: what could not be kept is asked of Calibre again.
            Ok(()) | Err(_) => Ok(()),
        }
    }
}

impl Backing for Store<'_, '_> {
    fn sent(&self, lpath: &str) -> Result<bool, Error> {
        Ok(storage::size(self.host, Root::Data, record(lpath)?).is_ok())
    }

    fn remember(&mut self, lpath: &str, metadata: &str) -> Result<(), Error> {
        let metadata = model::without_member(metadata, "thumbnail")?;
        if metadata.len() > MAX_METADATA {
            return Ok(());
        }
        self.write(&record(lpath)?, &metadata)
    }

    fn forget(&mut self, lpath: &str) -> Result<Option<String>, Error> {
        let name = record(lpath)?;
        let uuid = match self.read(&name, MAX_METADATA)? {
            Some(metadata) => model::text(&metadata, "uuid")?,
            None => None,
        };
        // An absent file is removed all the same: nothing to do.
        storage::remove(self.host, Root::Data, &name).ok();
        Ok(uuid)
    }

    fn books(&self) -> Result<Vec<u128>, Error> {
        let mut ids: Vec<u128> = Vec::new();
        let Some(files) = self.files else {
            return Ok(ids);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = 0;
        loop {
            let page = match files.list(self.host, Root::Data, BOOKS, cursor, &mut input, &mut out)
            {
                Ok(page) => page,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // No folder yet, or a card that cannot be read: no books.
                Err(_) => return Ok(Vec::new()),
            };
            for entry in page.entries() {
                let id = entry
                    .name()
                    .strip_suffix(".json")
                    .filter(|hex| hex.len() == 32)
                    .and_then(|hex| u128::from_str_radix(hex, 16).ok());
                if let (false, Some(id)) = (entry.is_folder(), id)
                    && ids.len() < most_books(self.small)
                {
                    push(&mut ids, id)?;
                }
            }
            match page.next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        paperpaw_sdk::sort::sort(&mut ids);
        // Only the books still in the library: one deleted on the device is
        // not Calibre's to list.
        let mut kept: Vec<u128> = Vec::new();
        for id in ids {
            let Some(metadata) = self.metadata(id)? else {
                continue;
            };
            let Some(lpath) = model::text(&metadata, "lpath")? else {
                continue;
            };
            let Some(path) = model::safe_path(&lpath)? else {
                continue;
            };
            let full = self.library_path(&path)?;
            if storage::size(self.host, Root::Files, &full).is_ok() {
                push(&mut kept, id)?;
            }
        }
        Ok(kept)
    }

    fn metadata(&self, id: u128) -> Result<Option<String>, Error> {
        // A record that cannot be read is sent as nothing (`{}`) by the
        // session; Calibre then asks for the book again.
        Ok(self.read(&record_of(id)?, MAX_METADATA).unwrap_or(None))
    }

    fn device_info(&self) -> Result<Option<String>, Error> {
        self.read(DEVICE, MAX_METADATA)
    }

    fn set_device_info(&mut self, info: &str) -> Result<(), Error> {
        if info.len() > MAX_METADATA {
            return Ok(());
        }
        self.write(DEVICE, info)
    }

    fn trusted(&self) -> Result<Option<String>, Error> {
        let Some(text) = self.read(TRUSTED, 256)? else {
            return Ok(None);
        };
        match text.lines().next().map(str::trim) {
            Some(host) if !host.is_empty() => copy(host).map(Some),
            _ => Ok(None),
        }
    }

    fn trust(&mut self, host: &str) -> Result<(), Error> {
        let line = concat(&[host, "\n"])?;
        self.write(TRUSTED, &line)
    }

    fn extensions(&self) -> Result<Vec<String>, Error> {
        let mut out = Vec::new();
        for extension in self.extensions {
            push(&mut out, copy(extension)?)?;
        }
        Ok(out)
    }

    fn password(&self) -> Result<Option<Secret>, Error> {
        match self.secrets {
            Some(secrets) => keep::read(secrets, self.host, keep::PASSWORD),
            None => Ok(None),
        }
    }

    fn device_name(&self) -> Result<String, Error> {
        copy(self.name)
    }

    fn version(&self) -> &str {
        self.version
    }

    fn space(&self) -> (Option<u64>, Option<u64>) {
        let Some(files) = self.files else {
            return (None, None);
        };
        let (Ok(mut input), Ok(mut out)) = (buffer(256), buffer(256)) else {
            return (None, None);
        };
        match files.space(self.host, Root::Files, &mut input, &mut out) {
            Ok(space) => (space.free(), space.total()),
            Err(_) => (None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_named_after_the_hash_of_its_path() {
        let name = record("Ann/A.epub").unwrap_or_default();
        assert_eq!(
            name,
            alloc::format!("books/{}.json", paperpaw_sdk::digest::md5::of("Ann/A.epub"))
        );
        // The id of a record is its name's 32 hex digits.
        let hex = name
            .strip_prefix("books/")
            .and_then(|n| n.strip_suffix(".json"))
            .unwrap_or_default();
        let id = u128::from_str_radix(hex, 16).unwrap_or(0);
        assert_eq!(record_of(id).unwrap_or_default(), name);
        assert_eq!(
            record_of(0xAB).unwrap_or_default(),
            "books/000000000000000000000000000000ab.json"
        );
    }
}
