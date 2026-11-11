//! The staged edits of a transaction on the card (`library.md` 2.2): one
//! file per transaction, `.paperpaw/library/edits/<transaction>.stage`, made
//! of the framed `lib_edit` and `lib_edit_text` records in the order the
//! plugin sent them. Nothing of it stays in memory; a commit reads it again
//! record by record. A power cut leaves the files behind and the next start
//! removes them ([`Stage::sweep`]): the book's annotations were never
//! touched.
use crate::native_host::codec;
use crate::native_host::lib_service::StageItem;
use crate::native_host::registrations::{self as wire, RawRecord, Record, WireRecord};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::fmt::Write;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};
use paperpaw_platform::{Storage, StorageError};

/// The host's folder for what a transaction keeps (root 1 never reaches it).
pub(super) const DIR: &str = ".paperpaw/library/edits";
/// The most one framed record takes: a `lib_edit` with its inline texts and
/// key, or a part of 3,072 bytes, with room to spare.
pub(super) const RECORD_MAX: usize = 4 * 1024 + 1024;

/// The status of a card error.
pub(super) fn status(error: StorageError) -> i32 {
    match error {
        StorageError::NotFound => ERR_NOT_FOUND,
        StorageError::InvalidPath => ERR_INVALID,
        StorageError::Unsupported => ERR_UNSUPPORTED,
        StorageError::TooLarge | StorageError::Full => ERR_NO_SPACE,
        StorageError::OutOfMemory => ERR_NO_MEMORY,
        _ => ERR_IO,
    }
}

/// `<DIR>/<transaction>.<extension>`, built fallibly.
pub(super) fn path(transaction: u32, extension: &str) -> Result<String, i32> {
    let mut name = String::new();
    name.try_reserve_exact(DIR.len() + 1 + 8 + 1 + extension.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    let _ = write!(name, "{DIR}/{transaction:x}.{extension}");
    Ok(name)
}

/// Reads from `at` until `buf` is full or the file ends: the card may answer
/// a read short in the middle of a file.
pub(super) fn read_full(
    storage: &dyn Storage,
    file: &str,
    at: u64,
    buf: &mut [u8],
) -> Result<usize, i32> {
    let mut done = 0;
    while done < buf.len() {
        let n = storage
            .read_at(file, at + done as u64, &mut buf[done..])
            .map_err(status)?;
        if n == 0 {
            break;
        }
        done += n;
    }
    Ok(done)
}

/// A buffer of `len` zero bytes, reserved fallibly.
pub(super) fn buffer(len: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// The staged files of the transactions on one card.
pub(crate) struct Stage {
    storage: Rc<dyn Storage>,
}

impl Stage {
    pub(crate) fn new(storage: Rc<dyn Storage>) -> Self {
        Self { storage }
    }

    pub(crate) fn storage(&self) -> &dyn Storage {
        &*self.storage
    }

    /// Removes what a power cut or a crash left (every transaction ends with
    /// the start); best effort, as the files are never read without their
    /// transaction.
    pub(crate) fn sweep(&self) {
        let _ = self.storage.remove_dir_all(DIR);
    }

    /// Keeps one item at the end of the transaction's file.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`, `ERR_NO_SPACE`, `ERR_IO`: the card's; the caller ends
    /// the transaction.
    pub(crate) fn append(&self, transaction: u32, item: &StageItem<'_>) -> Result<(), i32> {
        let record = match *item {
            StageItem::Edit { edit, .. } => Record::ServiceLibEdit(*edit),
            StageItem::Part {
                edit,
                field,
                offset,
                data,
                last,
            } => Record::ServiceLibEditText(wire::ServiceLibEditText {
                transaction,
                edit,
                field,
                offset,
                flags: u32::from(last),
                data,
            }),
        };
        let length = codec::framed_len(&record)?;
        let mut bytes = buffer(length)?;
        let written = codec::encode_one(&record, &mut bytes)?;
        let file = path(transaction, "stage")?;
        self.storage
            .append(&file, &bytes[..written])
            .map_err(status)
    }

    /// Drops what the transaction staged.
    pub(crate) fn remove(&self, transaction: u32) {
        self.remove_staged(transaction);
    }

    /// Drops the staged file of the transaction.
    pub(crate) fn remove_staged(&self, transaction: u32) {
        if let Ok(file) = path(transaction, "stage") {
            let _ = self.storage.remove(&file);
        }
    }

    /// Reads the staged records in order: `visit` gets each with the offset it
    /// starts at, until it breaks.
    ///
    /// # Errors
    /// The card's, or `ERR_INVALID` for a file that does not parse.
    pub(crate) fn each(
        &self,
        transaction: u32,
        visit: &mut dyn FnMut(u64, Staged<'_>) -> core::ops::ControlFlow<i32>,
    ) -> Result<(), i32> {
        let file = path(transaction, "stage")?;
        let size = match self.storage.size(&file) {
            Ok(size) => size,
            // Nothing staged: a transaction of no edits.
            Err(StorageError::NotFound) => return Ok(()),
            Err(error) => return Err(status(error)),
        };
        let mut chunk = buffer(RECORD_MAX)?;
        let mut at = 0_u64;
        while at < size {
            let read = read_full(&*self.storage, &file, at, &mut chunk)?;
            let (raw, used) = RawRecord::prefix(&chunk[..read]).map_err(|_| ERR_INVALID)?;
            let item = if raw.tag() == wire::ServiceLibEdit::TAG {
                Staged::Edit(codec::decode(raw)?)
            } else if raw.tag() == wire::ServiceLibEditText::TAG {
                Staged::Part(codec::decode(raw)?)
            } else {
                return Err(ERR_INVALID);
            };
            if let core::ops::ControlFlow::Break(error) = visit(at, item) {
                return if error == 0 { Ok(()) } else { Err(error) };
            }
            at += used as u64;
        }
        Ok(())
    }
}

/// A staged record, read back.
pub(crate) enum Staged<'a> {
    Edit(wire::ServiceLibEdit<'a>),
    Part(wire::ServiceLibEditText<'a>),
}
