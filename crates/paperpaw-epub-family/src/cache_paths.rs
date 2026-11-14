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
    /// Begins a conversion: not complete from here on, and nothing an
    /// earlier try left stays. No file is made: each is made by the first
    /// bytes appended to it, and one that gets none is not there, which
    /// reads as empty. (Made empty first, each was a whole write: eight
    /// writes to a FAT card, 1.6 s for a book's first chapters, #250.)
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
            // Each by its name: where the folder could not be removed (the
            // host's cache is no tree the format may remove), what an
            // aborted try left must not be appended to, nor stay.
            match storage.remove(path) {
                Ok(()) | Err(StorageError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Marks the conversion complete, last: `end` says the text's length,
    /// and the text is as long. A chapter without text gets its text and
    /// index made empty first: they are asked for their sizes.
    pub fn complete(&self, storage: &dyn Storage, length: u32) -> Result<(), StorageError> {
        if length == 0 {
            storage.write(&self.text, &[])?;
            storage.write(&self.index, &[])?;
        }
        storage.write(&self.end, &length.to_le_bytes())
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
    fn a_retry_starts_from_nothing_even_where_no_tree_can_be_removed() {
        let storage = NoTrees(MemoryStorage::new());
        let paths = CachePaths::new("cache", "3").unwrap();
        // What an aborted conversion left: text, an index and links, no
        // end mark.
        storage.write(&paths.text, b"half a chapter").unwrap();
        storage.write(&paths.index, b"half an index").unwrap();
        storage.write(&paths.links, b"a link\n").unwrap();
        paths.begin(&storage).unwrap();
        for path in [&paths.text, &paths.index, &paths.links, &paths.pictures] {
            assert!(!storage.exists(path), "{path}");
        }
        assert!(!storage.exists(&paths.end));
        // What is appended then is all there is.
        storage.append(&paths.text, b"whole").unwrap();
        assert_eq!(storage.0.get(&paths.text).unwrap(), b"whole");
    }

    #[test]
    fn a_chapter_without_text_is_complete_with_its_text_and_index_there() {
        let storage = MemoryStorage::new();
        let paths = CachePaths::new("cache", "3").unwrap();
        paths.begin(&storage).unwrap();
        paths.complete(&storage, 0).unwrap();
        assert_eq!(storage.size(&paths.text), Ok(0));
        assert_eq!(storage.size(&paths.index), Ok(0));
        assert_eq!(storage.get(&paths.end).unwrap(), 0u32.to_le_bytes());
    }
}
