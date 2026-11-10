//! Where a chapter's conversion is kept on the card.

use alloc::string::String;

use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::{Storage, StorageError};

use crate::fallible;

/// Paths of a chapter's cache files, in a folder of the chapter's own:
/// made beside those of hundreds of other chapters, each file would be
/// looked for among all of theirs (on a FAT card, every name of the
/// folder read again for each file made: seconds for a chapter).
#[derive(Debug, Clone)]
pub(crate) struct CachePaths {
    /// The book's cache directory.
    pub dir: String,
    /// The chapter's folder in it.
    pub folder: String,
    pub text: String,
    pub index: String,
    pub anchors: String,
    pub links: String,
    pub positions: String,
    pub xpointers: String,
    pub pictures: String,
    pub end: String,
}

impl CachePaths {
    /// The files of chapter `name` (`0`, `12`, …) in the cache `dir`.
    ///
    /// # Errors
    /// `OutOfMemory` where a path finds no room.
    pub fn new(dir: &str, name: &str) -> Result<Self, DocumentError> {
        let folder = fallible::try_format!("{dir}/sections/{name}")?;
        let file = |extension: &str| fallible::try_format!("{folder}/{extension}");
        Ok(CachePaths {
            dir: fallible::string(dir)?,
            text: file("txt")?,
            index: file("idx")?,
            anchors: file("anc")?,
            links: file("lnk")?,
            positions: file("pos")?,
            xpointers: file("xpt")?,
            pictures: file("pic")?,
            end: file("end")?,
            folder,
        })
    }
}

impl CachePaths {
    /// Makes the chapter's files anew, empty, for a conversion: not
    /// complete from here on, and what an earlier try left goes whole (not
    /// each file written whole, which takes a FAT card several looks
    /// through the folder).
    pub fn begin(&self, storage: &dyn Storage) -> Result<(), StorageError> {
        let _ = storage.remove(&self.end);
        let _ = storage.remove_dir_all(&self.folder);
        for path in [
            &self.text,
            &self.index,
            &self.anchors,
            &self.links,
            &self.positions,
            &self.xpointers,
            &self.pictures,
        ] {
            storage.append(path, &[])?;
        }
        Ok(())
    }
}
