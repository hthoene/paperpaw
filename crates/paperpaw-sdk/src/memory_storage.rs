use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use paperpaw_platform::storage::is_valid_path;
use paperpaw_platform::{DirEntry, Storage, StorageError};

/// A storage kept in memory, for tests of plugins and for devices or
/// demos without a file system.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    /// Each entry's identity (`Storage::entry_identity`): a file's from
    /// when it was made, a folder's from when it was first asked for; kept
    /// by renames, dropped with the entry.
    ids: RefCell<BTreeMap<String, u64>>,
    /// The last identity given.
    last: Cell<u64>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file (builder style).
    pub fn with(self, path: &str, data: impl Into<Vec<u8>>) -> Self {
        self.files
            .borrow_mut()
            .insert(String::from(path), data.into());
        self.made(path);
        self
    }

    /// The content of a file, for assertions.
    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(path).cloned()
    }

    /// Gives `path` an identity unless it has one.
    fn made(&self, path: &str) -> u64 {
        *self
            .ids
            .borrow_mut()
            .entry(String::from(path))
            .or_insert_with(|| {
                let id = self.last.get().wrapping_add(1);
                self.last.set(id);
                id
            })
    }

    /// Drops the identities of the folders above `path` that no longer
    /// hold a file (folders exist through their files here).
    fn forget_empty_above(&self, path: &str) {
        let files = self.files.borrow();
        let mut ids = self.ids.borrow_mut();
        let mut at = path;
        while let Some((parent, _)) = at.rsplit_once('/') {
            let prefix = alloc::format!("{parent}/");
            if files.keys().any(|k| k.starts_with(&prefix)) {
                break;
            }
            ids.remove(parent);
            at = parent;
        }
    }

    fn check(path: &str) -> Result<(), StorageError> {
        if is_valid_path(path) {
            Ok(())
        } else {
            Err(StorageError::InvalidPath)
        }
    }
}

impl Storage for MemoryStorage {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        Self::check(path)?;
        let files = self.files.borrow();
        let data = files.get(path).ok_or(StorageError::NotFound)?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        Self::check(path)?;
        self.files
            .borrow()
            .get(path)
            .map(|data| data.len() as u64)
            .ok_or(StorageError::NotFound)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        Self::check(path)?;
        self.files
            .borrow_mut()
            .insert(String::from(path), data.to_vec());
        self.made(path);
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        Self::check(path)?;
        self.files
            .borrow_mut()
            .entry(String::from(path))
            .or_default()
            .extend_from_slice(data);
        self.made(path);
        Ok(())
    }

    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        Self::check(path)?;
        let mut files = self.files.borrow_mut();
        let file = files.get_mut(path).ok_or(StorageError::NotFound)?;
        let at = usize::try_from(offset).map_err(|_| StorageError::TooLarge)?;
        let end = at.checked_add(data.len()).ok_or(StorageError::TooLarge)?;
        if file.len() < end {
            file.resize(end, 0);
        }
        file[at..end].copy_from_slice(data);
        Ok(())
    }

    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        Self::check(path)?;
        let mut files = self.files.borrow_mut();
        let file = files.get_mut(path).ok_or(StorageError::NotFound)?;
        let len = usize::try_from(len).map_err(|_| StorageError::TooLarge)?;
        file.resize(len, 0);
        Ok(())
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        if !dir.is_empty() {
            Self::check(dir)?;
        }
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            alloc::format!("{dir}/")
        };
        let files = self.files.borrow();
        let mut dirs = BTreeSet::new();
        let mut found = dir.is_empty();
        for (path, data) in files.iter() {
            let Some(rest) = path.strip_prefix(prefix.as_str()) else {
                continue;
            };
            found = true;
            match rest.split_once('/') {
                Some((sub, _)) => {
                    dirs.insert(String::from(sub));
                }
                None => visit(DirEntry::new(rest, false, data.len() as u64)),
            }
        }
        for name in &dirs {
            visit(DirEntry::new(name, true, 0));
        }
        if found {
            Ok(())
        } else {
            Err(StorageError::NotFound)
        }
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        Self::check(path)?;
        self.files.borrow_mut().remove(path);
        self.ids.borrow_mut().remove(path);
        self.forget_empty_above(path);
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        Self::check(from)?;
        Self::check(to)?;
        let mut files = self.files.borrow_mut();
        let mut ids = self.ids.borrow_mut();
        if let Some(data) = files.remove(from) {
            files.insert(String::from(to), data);
            match ids.remove(from) {
                Some(id) => ids.insert(String::from(to), id),
                None => ids.remove(to),
            };
            drop((files, ids));
            self.forget_empty_above(from);
            return Ok(());
        }
        // A directory: everything below it.
        let prefix = alloc::format!("{from}/");
        let moved: Vec<String> = files
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        if moved.is_empty() {
            return Err(StorageError::NotFound);
        }
        for key in moved {
            if let Some(data) = files.remove(&key) {
                files.insert(alloc::format!("{to}/{}", &key[prefix.len()..]), data);
            }
        }
        // The folder's identity and those of everything below it go along.
        let named: Vec<String> = ids
            .keys()
            .filter(|k| k.as_str() == from || k.starts_with(&prefix))
            .cloned()
            .collect();
        for key in named {
            if let Some(id) = ids.remove(&key) {
                ids.insert(alloc::format!("{to}{}", &key[from.len()..]), id);
            }
        }
        drop((files, ids));
        self.forget_empty_above(from);
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        // Directories exist through their files here.
        Self::check(path)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        Self::check(path)?;
        let prefix = alloc::format!("{path}/");
        self.files
            .borrow_mut()
            .retain(|k, _| !k.starts_with(&prefix));
        self.ids
            .borrow_mut()
            .retain(|k, _| k.as_str() != path && !k.starts_with(&prefix));
        self.forget_empty_above(path);
        Ok(())
    }

    /// A file's identity is given when it is made, a folder's when it is
    /// first asked for; renames keep them. The root has none.
    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        if path.is_empty() {
            return Ok(None);
        }
        Self::check(path)?;
        let prefix = alloc::format!("{path}/");
        let exists = {
            let files = self.files.borrow();
            files.contains_key(path) || files.keys().any(|k| k.starts_with(&prefix))
        };
        if exists {
            Ok(Some(self.made(path)))
        } else {
            Err(StorageError::NotFound)
        }
    }
}

/// Reads a whole (small) file: [`StorageError::NotFound`] where there is
/// none, [`StorageError::TooLarge`] for files larger than `limit` bytes,
/// [`StorageError::OutOfMemory`] where the heap cannot give the room now
/// (try again), any other error where it could not be read. Before writing back a
/// changed file, read it with
/// [`files::load_for_update`](crate::files::load_for_update), which tells
/// a missing file from one that could not be read.
pub fn read_file(storage: &dyn Storage, path: &str, limit: usize) -> Result<Vec<u8>, StorageError> {
    let size = storage.size(path)?;
    let size = usize::try_from(size).unwrap_or(usize::MAX);
    if size > limit {
        return Err(StorageError::TooLarge);
    }
    // Had fallibly: a file the heap cannot take now is too large for it,
    // not the end of the program.
    let mut data = Vec::new();
    data.try_reserve_exact(size)
        .map_err(|_| StorageError::OutOfMemory)?;
    data.resize(size, 0);
    let mut filled = 0;
    while filled < size {
        let n = storage.read_at(path, filled as u64, &mut data[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    data.truncate(filled);
    Ok(data)
}

/// A storage whose files cannot be read, as on a failing card: what is
/// there says [`StorageError::Io`] where it is read or its size asked
/// for. For tests of what must not be written over.
#[cfg(test)]
pub(crate) struct Unreadable(pub MemoryStorage);

#[cfg(test)]
impl Storage for Unreadable {
    fn read_at(&self, path: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
        self.size(path).map(|_| 0)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path).and(Err(StorageError::Io))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn writes_in_place_and_cuts() {
        let storage = MemoryStorage::new().with("save.bin", "abcdef");
        storage.write_at("save.bin", 2, b"XY").unwrap();
        storage.write_at("save.bin", 8, b"Z").unwrap();
        assert_eq!(read_file(&storage, "save.bin", 64).unwrap(), b"abXYef\0\0Z");
        storage.truncate("save.bin", 3).unwrap();
        assert_eq!(read_file(&storage, "save.bin", 64).unwrap(), b"abX");
        assert_eq!(
            storage.write_at("none", 0, b"x"),
            Err(StorageError::NotFound)
        );
    }

    #[test]
    fn identities_are_kept_by_renames_and_differ_between_entries() {
        let storage = MemoryStorage::new()
            .with("books/a.epub", b"a".as_slice())
            .with("books/b.epub", b"b".as_slice());
        let a = storage.entry_identity("books/a.epub").unwrap().unwrap();
        let b = storage.entry_identity("books/b.epub").unwrap().unwrap();
        let books = storage.entry_identity("books").unwrap().unwrap();
        assert_ne!(a, b);
        assert_ne!(a, books);
        assert_ne!(b, books);
        assert_eq!(storage.entry_identity(""), Ok(None));
        assert_eq!(
            storage.entry_identity("books/c.epub"),
            Err(StorageError::NotFound)
        );

        // A file's rename, over another file too.
        storage.rename("books/a.epub", "books/c.epub").unwrap();
        assert_eq!(storage.entry_identity("books/c.epub"), Ok(Some(a)));
        storage.rename("books/c.epub", "books/b.epub").unwrap();
        assert_eq!(storage.entry_identity("books/b.epub"), Ok(Some(a)));
        assert_eq!(
            storage.entry_identity("books/a.epub"),
            Err(StorageError::NotFound)
        );

        // A folder's rename takes its own and everything's below.
        storage.rename("books", "shelf/books").unwrap();
        assert_eq!(storage.entry_identity("shelf/books"), Ok(Some(books)));
        assert_eq!(storage.entry_identity("shelf/books/b.epub"), Ok(Some(a)));
        assert_eq!(storage.entry_identity("books"), Err(StorageError::NotFound));

        // A removed entry's identity is not given to what comes after it.
        storage.remove("shelf/books/b.epub").unwrap();
        storage.write("shelf/books/b.epub", b"new").unwrap();
        let new = storage.entry_identity("shelf/books/b.epub").unwrap();
        assert_ne!(new, Some(a));
        storage.remove_dir_all("shelf").unwrap();
        storage.write("shelf/books/x", b"x").unwrap();
        assert_ne!(storage.entry_identity("shelf/books").unwrap(), Some(books));
    }

    #[test]
    fn reads_writes_and_lists() {
        let storage = MemoryStorage::new()
            .with("books/a.txt", "hello")
            .with("books/sub/b.txt", "x");
        let mut buf = [0u8; 3];
        assert_eq!(storage.read_at("books/a.txt", 2, &mut buf), Ok(3));
        assert_eq!(&buf, b"llo");
        assert_eq!(storage.read_at("books/a.txt", 10, &mut buf), Ok(0));
        storage.append("notes/n.txt", b"ab").unwrap();
        storage.append("notes/n.txt", b"c").unwrap();
        assert_eq!(read_file(&storage, "notes/n.txt", 10).unwrap(), b"abc");
        assert_eq!(
            read_file(&storage, "notes/n.txt", 2),
            Err(StorageError::TooLarge)
        );

        let mut entries = vec![];
        storage
            .list("books", &mut |e| {
                entries.push((String::from(e.name), e.is_dir))
            })
            .unwrap();
        entries.sort();
        assert_eq!(
            entries,
            vec![(String::from("a.txt"), false), (String::from("sub"), true)]
        );
        assert_eq!(
            storage.list("nope", &mut |_| {}),
            Err(StorageError::NotFound)
        );
        assert_eq!(storage.size("../x"), Err(StorageError::InvalidPath));
    }
}
