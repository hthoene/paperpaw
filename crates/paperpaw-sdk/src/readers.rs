//! Opening books, and the reading-app choices kept for books and folders.
//!
//! - Whatever opens a book (the library, the file manager, the browser, a
//!   catalog) opens it with [`open`] ([`Command::OpenBook`]): the kernel
//!   opens the screen bound to [`slots::READER`](crate::builtin::slots::READER) with what [`opened`]
//!   passes on. A reading app that cannot open a book after all says so
//!   with `cannot_open`.
//! - The choices for a book and for a folder are kept ([`remember`]): the
//!   book's moved and removed with it, the folders' in one small file
//!   (moved with [`folder_moved`] by whatever moves a folder). [`ask`] lets
//!   the reader choose (*Open with*, a folder's reading app).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::params;
use crate::{Command, Id, Params, Storage, StorageError};


















#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    #[test]
    fn choices_that_cannot_be_read_are_not_overwritten() {
        let big = "x\tfolder\n".repeat(LIMIT / 8);
        let storage = MemoryStorage::new().with(FOLDERS, big.as_str());
        let comics = Id::new_static("comics.reader");
        assert_eq!(
            remember(&storage, Choice::Folder("manga"), Some(&comics)),
            Err(StorageError::TooLarge)
        );
        let kept = crate::read_file(&storage, FOLDERS, 2 * LIMIT).unwrap();
        assert_eq!(kept.len(), big.len());
    }
}
