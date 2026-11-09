//! The plugin's data folder (`core.storage`, root 0) as the catalogs need it:
//! text files of a cap, read line by line, written whole or not at all, and
//! files read in pieces for the readers. Every read allocates fallibly and has
//! a cap; a file that is too large or not text is never taken for an empty
//! one, so no caller writes the file over what it holds.
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{
    Storage, StorageError,
    native::{
        Error, Host,
        storage::{self, DataStorage, Root},
    },
};
use paperpaw_ui_kit::mem::buffer;

use crate::native_model::Source;

/// The files of a catalog list and the records, as the plugin keeps them.
pub const CATALOGS: &str = "catalogs.txt";
/// The folder of the records of the books a catalog's sync fetched.
pub const SYNCED: &str = "synced";
/// The largest a line of a record is read: longer ones are cut.
pub const MAX_LINE: usize = 4096;
/// Bytes read at a time.
const PIECE: usize = 512;

/// The data folder as the plugin's engines need it: files read in pieces,
/// written whole or not at all, appended to and removed. [`DataFiles`] is the
/// host's; a test gives its own.
pub trait Files {
    /// The size of the file `name`.
    ///
    /// # Errors
    /// `NotFound` where there is none; what the host says.
    fn size(&self, name: &str) -> Result<u64, Error>;
    /// Reads at `offset`; a short read is the end.
    ///
    /// # Errors
    /// What the host says.
    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error>;
    /// Writes `data` as the whole file `name`.
    ///
    /// # Errors
    /// What the host says (`Full` for a full card).
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error>;
    /// Appends `data` to the file `name`, which is made where there is none.
    ///
    /// # Errors
    /// What the host says.
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error>;
    /// Takes the file `name` away.
    ///
    /// # Errors
    /// What the host says; a file that is not there is `NotFound` or fine.
    fn remove(&self, name: &str) -> Result<(), Error>;
}

/// The data folder through the host.
#[derive(Clone, Copy)]
pub struct DataFiles<'h, 'cb>(&'h Host<'cb>);

impl core::fmt::Debug for DataFiles<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("DataFiles")
    }
}

impl<'h, 'cb> DataFiles<'h, 'cb> {
    /// The data folder through `host`.
    #[must_use]
    pub const fn new(host: &'h Host<'cb>) -> Self {
        Self(host)
    }
}

impl Files for DataFiles<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, Root::Data, name)
    }

    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        DataStorage::new(self.0)
            .read_at(name, offset, out)
            .map_err(from_storage)
    }

    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        DataStorage::new(self.0)
            .write(name, data)
            .map_err(from_storage)
    }

    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        let store = DataStorage::new(self.0);
        store.append(name, data).map_err(from_storage)?;
        store.sync().map_err(from_storage)
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.0, Root::Data, name)
    }
}

fn from_storage(error: StorageError) -> Error {
    match error {
        StorageError::NotFound => Error::NotFound,
        StorageError::Full => Error::Full,
        StorageError::OutOfMemory => Error::NoMemory,
        StorageError::InvalidPath => Error::Invalid,
        StorageError::Unsupported => Error::Unsupported,
        _ => Error::Io,
    }
}

/// Takes the file `name` away; one that is not there is as good.
pub fn remove(files: &dyn Files, name: &str) {
    match files.remove(name) {
        Ok(()) | Err(_) => {}
    }
}

/// The text of the file `name`: `None` where there is none.
///
/// # Errors
/// `NoSpace` over `cap` bytes; `Utf8` for text that is not; `NoMemory`; what
/// the host says.
pub fn read_text(files: &dyn Files, name: &str, cap: usize) -> Result<Option<String>, Error> {
    let size = match files.size(name) {
        Ok(size) => size,
        Err(Error::NotFound) => return Ok(None),
        Err(error) => return Err(error),
    };
    let len = usize::try_from(size).map_err(|_| Error::NoSpace)?;
    if len > cap {
        return Err(Error::NoSpace);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    let mut read = 0;
    while read < len {
        let part = bytes.get_mut(read..).ok_or(Error::Invalid)?;
        match files.read_at(name, read as u64, part)? {
            0 => break,
            n => read += n,
        }
    }
    bytes.truncate(read);
    String::from_utf8(bytes).map(Some).map_err(|_| Error::Utf8)
}

/// Reads the file `name` line by line as it streams, so a list of any length
/// is read in little memory: `visit` gets each line without its line break
/// (`\n` or `\r\n`, a byte order mark at the start left out) and says
/// whether to go on. A line is cut at [`MAX_LINE`] bytes; a line that is not
/// UTF-8 is passed over.
///
/// # Errors
/// `NotFound` where there is none; `NoMemory`; what the host says; what
/// `visit` says.
pub fn for_each_line(
    files: &dyn Files,
    name: &str,
    visit: &mut dyn FnMut(&str) -> Result<ControlFlow<()>, Error>,
) -> Result<(), Error> {
    let mut chunk = buffer(PIECE)?;
    let mut line: Vec<u8> = Vec::new();
    let mut at = 0u64;
    let mut first = true;
    loop {
        let n = files.read_at(name, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        at += n as u64;
        for piece in chunk
            .get(..n)
            .ok_or(Error::Invalid)?
            .split_inclusive(|b| *b == b'\n')
        {
            let (text, ends) = match piece.split_last() {
                Some((b'\n', text)) => (text, true),
                _ => (piece, false),
            };
            let room = MAX_LINE.saturating_sub(line.len());
            let part = text.get(..text.len().min(room)).ok_or(Error::Invalid)?;
            line.try_reserve(part.len()).map_err(|_| Error::NoMemory)?;
            line.extend_from_slice(part);
            if ends && emit(&mut line, &mut first, visit)?.is_break() {
                return Ok(());
            }
        }
    }
    if !line.is_empty() {
        let _last = emit(&mut line, &mut first, visit)?;
    }
    Ok(())
}

/// Gives `visit` the line collected, without its line break, and clears it.
fn emit(
    line: &mut Vec<u8>,
    first: &mut bool,
    visit: &mut dyn FnMut(&str) -> Result<ControlFlow<()>, Error>,
) -> Result<ControlFlow<()>, Error> {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    let mut text: &[u8] = line;
    if *first {
        text = text.strip_prefix(b"\xef\xbb\xbf").unwrap_or(text);
    }
    *first = false;
    let flow = match core::str::from_utf8(text) {
        Ok(text) => visit(text),
        Err(_) => Ok(ControlFlow::Continue(())),
    };
    line.clear();
    flow
}

/// A file of the data folder as the readers read it.
#[derive(Debug)]
pub struct FileSource<'a> {
    files: &'a dyn Files,
    name: &'a str,
}

impl<'a> FileSource<'a> {
    /// The file `name` of `files`.
    #[must_use]
    pub const fn new(files: &'a dyn Files, name: &'a str) -> Self {
        Self { files, name }
    }
}

impl Source for FileSource<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        self.files.read_at(self.name, offset, buf)
    }
}

impl core::fmt::Debug for dyn Files + '_ {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Files")
    }
}
