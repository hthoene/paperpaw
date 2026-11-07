//! Whole files of a plugin's data folder over the 4 KiB storage calls.
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, MAX_DATA, Root},
};

/// The ending of the name a file is written under before it replaces the old.
const TEMP: &str = ".new";

/// A file of at most `max` bytes: `None` where there is none.
///
/// # Errors
/// [`Error::NoSpace`] for a larger file, [`Error::NoMemory`] when it does not
/// fit in memory, or the host's refusal.
pub fn read_all(
    host: &Host<'_>,
    root: Root,
    name: &str,
    max: usize,
) -> Result<Option<Vec<u8>>, Error> {
    let size = match storage::size(host, root, name) {
        Ok(size) => usize::try_from(size).map_err(|_| Error::NoSpace)?,
        Err(Error::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    if size > max {
        return Err(Error::NoSpace);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    bytes.resize(size, 0);
    let mut at = 0;
    while at < size {
        let end = (at + MAX_DATA).min(size);
        let n = storage::read_at(host, root, name, at as u64, &mut bytes[at..end])?;
        if n == 0 {
            // The file shrank under the read.
            return Err(Error::Io);
        }
        at += n;
    }
    Ok(Some(bytes))
}

/// Replaces a file with `bytes`, whole or not at all: a file beyond one call is
/// written under another name, flushed and renamed over the old.
///
/// # Errors
/// The host's refusal; the old file is intact then.
pub fn replace(host: &Host<'_>, root: Root, name: &str, bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() <= MAX_DATA {
        return storage::write(host, root, name, bytes);
    }
    let mut temp = alloc::string::String::new();
    temp.try_reserve_exact(name.len() + TEMP.len())
        .map_err(|_| Error::NoMemory)?;
    temp.push_str(name);
    temp.push_str(TEMP);
    let mut parts = bytes.chunks(MAX_DATA);
    let done = (|| {
        storage::write(host, root, &temp, parts.next().unwrap_or(&[]))?;
        for part in parts {
            storage::append(host, root, &temp, part)?;
        }
        storage::sync(host)?;
        storage::rename(host, root, &temp, name)
    })();
    if done.is_err() {
        let _ = storage::remove(host, root, &temp);
    }
    done
}
