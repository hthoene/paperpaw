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
            // Truncated, not appended to: where the folder could not be
            // removed (the host's cache is no tree the format may remove),
            // what an aborted try left must not stay.
            storage.write(path, &[])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{DirEntry, MemoryStorage};

    /// A storage that cannot remove a tree, as the native roots cannot.
    struct NoTrees(MemoryStorage);

    impl Storage for NoTrees {
        fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(p, o, b)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.append(p, d)
        }
        fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            self.0.remove(p)
        }
        fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
            self.0.rename(a, b)
        }
    }

    #[test]
    fn a_retry_starts_from_empty_files_even_where_no_tree_can_be_removed() {
        let storage = NoTrees(MemoryStorage::new());
        let paths = CachePaths::new("cache", "3").unwrap();
        // What an aborted conversion left: text and an index, no end mark.
        storage.write(&paths.text, b"half a chapter").unwrap();
        storage.write(&paths.index, b"half an index").unwrap();
        paths.begin(&storage).unwrap();
        for path in [&paths.text, &paths.index, &paths.pictures] {
            assert_eq!(storage.size(path), Ok(0), "{path}");
        }
        assert!(!storage.exists(&paths.end));
    }
}
