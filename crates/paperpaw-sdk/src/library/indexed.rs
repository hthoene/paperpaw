//! Whether a book's details say it was read from the book, without
//! reading the details: the file is scanned a piece at a time through a
//! line buffer of 128 bytes (the keys that matter are short, and a long
//! line, a field's value, is skipped), so nothing here is allocated
//! beyond the two file names and nothing of the file is held (the details
//! can be 16 KiB, and [`Book`](super::Book) as much again).

use super::fallible::{try_book_file, try_old_book_file};
use super::{DETAILS, MAX_DETAILS, config};
use crate::data::Error;
use crate::{Storage, StorageError};

/// Bytes of a line looked at; the rest of a longer line is not.
const LINE: usize = 128;

/// What the details say of the three keys.
struct Facts {
    size: u64,
    indexed: bool,
    version: u32,
    section_field: bool,
    first: bool,
    /// The file is of a format this build does not read.
    unreadable: bool,
}

impl Facts {
    fn line(&mut self, bytes: &[u8]) {
        // A line cut inside a character (it was longer than the buffer) is
        // looked at up to the cut.
        let line = match core::str::from_utf8(bytes) {
            Ok(line) => line,
            Err(e) => core::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default(),
        };
        let line = if self.first {
            self.first = false;
            let line = line.strip_prefix('\u{feff}').unwrap_or(line);
            if let Some(n) = line.trim().strip_prefix(crate::data::MARK) {
                // A file with a version mark: the details are of format 1.
                self.unreadable |= n.trim().parse::<u32>().unwrap_or(1) != 1;
            }
            line
        } else {
            line
        };
        let trimmed = line.trim();
        if let Some(name) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            self.section_field = name.trim() == "field";
            return;
        }
        if self.section_field {
            return;
        }
        for entry in config::parse(trimmed).filter_map(Result::ok) {
            match entry.key {
                "size" => self.size = entry.value.raw().parse().unwrap_or_default(),
                "indexed" => self.indexed = entry.value.as_bool().unwrap_or_default(),
                "version" => self.version = entry.value.as_u32().unwrap_or(1),
                _ => {}
            }
        }
    }
}

/// The facts of the details file `file`: `None` where there is none.
fn scan(storage: &dyn Storage, file: &str) -> Result<Option<Facts>, Error> {
    match storage.size(file) {
        Ok(size) if size > MAX_DETAILS as u64 => {
            return Ok(Some(Facts {
                unreadable: true,
                ..Facts::new()
            }));
        }
        Ok(_) => {}
        Err(StorageError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let mut facts = Facts::new();
    let (mut at, mut chunk) = (0_u64, [0_u8; 256]);
    let (mut line, mut len) = ([0_u8; LINE], 0);
    loop {
        let n = match storage.read_at(file, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            break;
        }
        at += n as u64;
        for &byte in &chunk[..n] {
            if byte == b'\n' {
                facts.line(&line[..len]);
                len = 0;
            } else if len < LINE {
                line[len] = byte;
                len += 1;
            }
        }
    }
    if len > 0 {
        facts.line(&line[..len]);
    }
    Ok(Some(facts))
}

impl Facts {
    fn new() -> Self {
        Self {
            size: 0,
            indexed: false,
            version: 1,
            section_field: false,
            first: true,
            unreadable: false,
        }
    }
}

/// Whether the library read the book at `path` (a file of `size` bytes)
/// for its title, author and cover: what [`try_book_sized`](super::try_book_sized)
/// answers in `Book::indexed`, without the book. `false` for details that
/// are not there, belong to a file of another size, are of another kind or
/// cannot be read.
///
/// # Errors
/// [`Error::OutOfMemory`] where the two file names cannot be had; the
/// storage's error where the details cannot be read.
pub fn try_indexed(storage: &dyn Storage, path: &str, size: u64) -> Result<bool, Error> {
    let file = try_book_file(path).map_err(|_| Error::OutOfMemory)?;
    let mut facts = scan(storage, &file)?;
    if facts.is_none() {
        drop(file);
        let old = try_old_book_file(path).map_err(|_| Error::OutOfMemory)?;
        facts = scan(storage, &old)?;
    }
    Ok(facts.is_some_and(|f| !f.unreadable && f.size == size && f.indexed && f.version == DETAILS))
}
