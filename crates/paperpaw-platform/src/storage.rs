//! Access to the device's storage (SD card, flash file system, disk).
//!
//! Paths are virtual, `/`-separated and relative to the storage root, e.g.
//! `books/alice.txt` or `.paperpaw/settings.ini`; `""` denotes the root in
//! [`Storage::list`]. Shells map paths to the real file system and must
//! reject paths that escape the root (see [`is_valid_path`]).
//!
//! Local storage is accessed synchronously: reads from an SD card or flash
//! are fast enough on every target, and it keeps the kernel simple. Slow,
//! remote I/O (network) is modelled as effects instead.

use core::fmt;

/// Why a storage operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StorageError {
    NotFound,
    /// The path is malformed or escapes the storage root.
    InvalidPath,
    /// The storage is read-only or the operation is not supported.
    Unsupported,
    /// The medium is full.
    Full,
    /// Any other I/O failure.
    Io,
    /// The file is larger than the reader takes (e.g. a whole file read
    /// into memory).
    TooLarge,
    /// The file is there but does not hold what was asked for, e.g. text
    /// that is not UTF-8.
    InvalidData,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StorageError::NotFound => "not found",
            StorageError::InvalidPath => "invalid path",
            StorageError::Unsupported => "not supported",
            StorageError::Full => "storage full",
            StorageError::Io => "input/output error",
            StorageError::TooLarge => "file too large",
            StorageError::InvalidData => "invalid data",
            StorageError::Other(code) => return write!(f, "storage error {code}"),
        })
    }
}

impl core::error::Error for StorageError {}

/// One entry of a directory listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirEntry<'a> {
    pub name: &'a str,
    pub is_dir: bool,
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// When it was last changed, in seconds since 1970-01-01 as the
    /// storage keeps it (a memory card's FAT: the device's local time,
    /// read as UTC), where the storage tells.
    pub modified: Option<i64>,
}

impl<'a> DirEntry<'a> {
    pub const fn new(name: &'a str, is_dir: bool, size: u64) -> Self {
        Self {
            name,
            is_dir,
            size,
            modified: None,
        }
    }

    /// With the time it was last changed ([`DirEntry::modified`]).
    pub const fn with_modified(mut self, modified: Option<i64>) -> Self {
        self.modified = modified;
        self
    }
}

/// Storage provided by a shell.
///
/// All methods take `&self` so the storage can be shared with plugins;
/// implementations use interior mutability where needed (e.g. to keep the
/// last file open).
pub trait Storage {
    /// Reads up to `buf.len()` bytes at `offset`; returns the number read
    /// (0 at the end of the file).
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError>;

    /// Size of a file in bytes.
    fn size(&self, path: &str) -> Result<u64, StorageError>;

    /// Replaces the file's content, creating it and missing parent
    /// directories. Implementations write atomically where the medium
    /// allows it, so power loss never leaves a half-written file.
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError>;

    /// Appends to a file, creating it and missing parent directories.
    /// Storages may keep what is appended back until [`Storage::sync`] or
    /// until the file is wanted otherwise; reading it sees it at once.
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError>;

    /// Writes `data` at `offset` of the file at `path`, past its end if
    /// need be (an emulator's saved memory, a database's pages); the file
    /// must be there. Storages that cannot say
    /// [`StorageError::Unsupported`].
    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        let _ = (path, offset, data);
        Err(StorageError::Unsupported)
    }

    /// Cuts the file at `path` to `len` bytes. Storages that cannot say
    /// [`StorageError::Unsupported`].
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        let _ = (path, len);
        Err(StorageError::Unsupported)
    }

    /// Writes back everything kept back, e.g. before the device sleeps.
    fn sync(&self) -> Result<(), StorageError> {
        Ok(())
    }

    /// Calls `visit` for each entry of a directory, in no particular order.
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError>;

    /// Deletes a file; deleting a missing file is not an error.
    fn remove(&self, path: &str) -> Result<(), StorageError>;

    /// When the file or directory at `path` was last changed, in seconds
    /// since 1970-01-01 as the storage keeps it ([`DirEntry::modified`]);
    /// `None` where it is not there, or the storage does not tell.
    fn modified(&self, _path: &str) -> Option<i64> {
        None
    }

    /// Whether a file exists.
    fn exists(&self, path: &str) -> bool {
        self.size(path).is_ok()
    }

    /// Moves a file or a directory to `to`, replacing a file there and
    /// creating missing parent directories. Storages that cannot say
    /// [`StorageError::Unsupported`].
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let _ = (from, to);
        Err(StorageError::Unsupported)
    }

    /// Creates a directory and its missing parents; an existing one is
    /// fine.
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        let _ = path;
        Err(StorageError::Unsupported)
    }

    /// Deletes a directory with everything in it; a missing one is fine.
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        let _ = path;
        Err(StorageError::Unsupported)
    }

    /// Whether names that differ only in case name the same entry, as on
    /// FAT (memory cards), NTFS and APFS: `Books/A.epub` is
    /// `books/a.epub` there. Storages that wrap another say what it says.
    fn ignores_case(&self) -> bool {
        false
    }

    /// Bytes free for new files, where the storage can tell without
    /// reading much at once; `None` where it cannot (yet). A memory card
    /// may say its own count first and count exactly a part at each ask.
    /// Storages that wrap another say what it says.
    fn free_space(&self) -> Option<u64> {
        None
    }

    /// Bytes the storage holds in all, used and free, where it can tell;
    /// `None` where it cannot. Storages that wrap another say what it
    /// says.
    fn total_space(&self) -> Option<u64> {
        None
    }

    /// Calls `with` with the file's path in the device's own file system,
    /// for platform code that opens files only by name (e.g. a system PDF
    /// renderer); does nothing where files have none.
    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        let _ = (path, with);
    }
}

/// Storage for devices without any: everything is empty and read-only.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStorage;

impl Storage for NoStorage {
    fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
        Err(StorageError::NotFound)
    }

    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Err(StorageError::NotFound)
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }

    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

/// Two storages in one: paths below the mount point `at` go to `mounted`
/// (with `at/` removed), all others to `base`. Shells use it to put the
/// user's book folder next to PaperPaw's private files, e.g. `books` on
/// Android's shared `Books` folder. Nest it for several mount points.
#[derive(Debug, Clone)]
pub struct Mounted<B, M> {
    base: B,
    at: &'static str,
    mounted: M,
}

impl<B: Storage, M: Storage> Mounted<B, M> {
    /// Mounts `mounted` at `at`, a single valid path segment.
    pub fn new(base: B, at: &'static str, mounted: M) -> Self {
        Self { base, at, mounted }
    }

    /// The path inside the mounted storage for `path`, where it is there.
    fn inside<'p>(&self, path: &'p str) -> Option<&'p str> {
        if path == self.at {
            return Some("");
        }
        path.strip_prefix(self.at)
            .and_then(|rest| rest.strip_prefix('/'))
    }

    /// The storage and path inside it for `path`.
    fn route<'p>(&self, path: &'p str) -> (&dyn Storage, &'p str) {
        match self.inside(path) {
            Some(rest) => (&self.mounted, rest),
            None => (&self.base, path),
        }
    }
}

impl<B: Storage, M: Storage> Storage for Mounted<B, M> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let (storage, path) = self.route(path);
        storage.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let (storage, path) = self.route(path);
        storage.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (storage, path) = self.route(path);
        storage.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (storage, path) = self.route(path);
        storage.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        if dir.is_empty() {
            // The root lists the mount point next to the base's entries;
            // a base that cannot be listed is told, not taken for empty.
            let base = self.base.list(dir, visit);
            visit(DirEntry::new(self.at, true, 0));
            return base;
        }
        let (storage, dir) = self.route(dir);
        storage.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let (storage, path) = self.route(path);
        storage.remove(path)
    }

    fn exists(&self, path: &str) -> bool {
        let (storage, path) = self.route(path);
        storage.exists(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        // Within one storage only; across a mount point it is a copy.
        // (Told by the paths: two storages of no size share an address.)
        if self.inside(from).is_some() != self.inside(to).is_some() {
            return Err(StorageError::Unsupported);
        }
        let (storage, from_path) = self.route(from);
        let (_, to_path) = self.route(to);
        storage.rename(from_path, to_path)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        let (storage, path) = self.route(path);
        storage.create_dir(path)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        if path == self.at {
            // Never the mount point itself.
            return Err(StorageError::Unsupported);
        }
        let (storage, path) = self.route(path);
        storage.remove_dir_all(path)
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        let (storage, path) = self.route(path);
        storage.local_path(path, with);
    }

    fn sync(&self) -> Result<(), StorageError> {
        let base = self.base.sync();
        self.mounted.sync().and(base)
    }

    /// Where either ignores case: a path is taken for another more often
    /// then, never less.
    fn ignores_case(&self) -> bool {
        self.base.ignores_case() || self.mounted.ignores_case()
    }

    /// The base's, where data and plugins are.
    fn free_space(&self) -> Option<u64> {
        self.base.free_space()
    }

    /// The base's, as its free space.
    fn total_space(&self) -> Option<u64> {
        self.base.total_space()
    }
}

/// Whether `path` is a valid virtual file path: relative, `/`-separated,
/// without empty, `.` or `..` segments, without backslashes, colons or
/// control characters, and without names FAT and Windows take for
/// another (a dot or space at the end, which they drop) or for a device
/// ([`is_reserved_name`]). Every storage refuses other paths, so a path
/// means the same entry on each.
pub fn is_valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.ends_with(['.', ' '])
                && !is_reserved_name(segment)
        })
}

/// Whether Windows takes file `name` for a device, whatever follows its
/// first dot: `CON`, `PRN`, `AUX`, `NUL`, `COM0` to `COM9`, `LPT0` to
/// `LPT9`, in any case (`nul.txt`, `Com1.tar.gz`).
pub fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let Some(word) = stem.get(..3) else {
        return false;
    };
    let is = |words: &[&str]| words.iter().any(|w| word.eq_ignore_ascii_case(w));
    let mut rest = stem[3..].chars();
    match (rest.next(), rest.next()) {
        (None, _) => is(&["CON", "PRN", "AUX", "NUL"]),
        // Windows counts superscript digits too: `COM¹`.
        (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None) => is(&["COM", "LPT"]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cause_of_a_newer_build_says_its_code() {
        struct Text([u8; 32], usize);
        impl fmt::Write for Text {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                let end = self.1 + s.len();
                self.0
                    .get_mut(self.1..end)
                    .ok_or(fmt::Error)?
                    .copy_from_slice(s.as_bytes());
                self.1 = end;
                Ok(())
            }
        }
        let mut text = Text([0; 32], 0);
        fmt::Write::write_fmt(&mut text, format_args!("{}", StorageError::Other(3))).unwrap();
        assert_eq!(&text.0[..text.1], b"storage error 3");
    }

    /// A storage holding one file, `name`, of `size` bytes.
    struct One(&'static str, u64);

    impl Storage for One {
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
            Ok(0)
        }

        fn size(&self, path: &str) -> Result<u64, StorageError> {
            if path == self.0 {
                Ok(self.1)
            } else {
                Err(StorageError::NotFound)
            }
        }

        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }

        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }

        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            if !dir.is_empty() {
                return Err(StorageError::NotFound);
            }
            visit(DirEntry::new(self.0, false, self.1));
            Ok(())
        }

        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[test]
    fn mounted_storage_routes_by_prefix() {
        let storage = Mounted::new(One("paperpaw.ini", 1), "books", One("alice.epub", 2));
        assert_eq!(storage.size("paperpaw.ini"), Ok(1));
        assert_eq!(storage.size("books/alice.epub"), Ok(2));
        assert!(!storage.exists("alice.epub"));
        assert!(!storage.exists("booksx/alice.epub"));
        let mut size = 0;
        storage
            .list("books", &mut |entry| size = entry.size)
            .unwrap();
        assert_eq!(size, 2, "lists the mounted folder");
        let mut root = (0, false);
        storage
            .list("", &mut |entry| {
                root.0 += 1;
                root.1 |= entry.is_dir && entry.name == "books";
            })
            .unwrap();
        assert_eq!(root, (2, true), "the root shows the mount point");
    }

    /// The base's top that cannot be listed is told, not taken for empty:
    /// a clean-up would take every book there for gone.
    #[test]
    fn mounted_storage_tells_a_base_it_cannot_list() {
        let storage = Mounted::new(NoStorage, "books", One("alice.epub", 2));
        let mut shown = (0, false);
        let listed = storage.list("", &mut |entry| {
            shown.0 += 1;
            shown.1 |= entry.name == "books";
        });
        assert_eq!(listed, Err(StorageError::NotFound));
        assert_eq!(shown, (1, true), "the mount point still shows");
    }

    #[test]
    fn validates_paths() {
        for ok in ["a", "books/alice.txt", ".paperpaw/settings.ini"] {
            assert!(is_valid_path(ok), "{ok}");
        }
        for bad in [
            "",
            "/abs",
            "a//b",
            "a/../b",
            "..",
            "a/./b",
            "c:\\x",
            "a\\b",
            "x/",
            "a\0b",
            "a\tb",
            "a\nb/c",
            "books.",
            "books /a",
            ".paperpaw./x",
            "CON",
            "books/nul.txt",
            "Com1.tar.gz",
            "lpt9",
            "aux .txt",
            "COM\u{b9}",
        ] {
            assert!(!is_valid_path(bad), "{bad:?}");
        }
        for ok in [
            "Console.txt",
            "con-tour.txt",
            "COM10",
            "Aux armes.epub",
            ".x",
        ] {
            assert!(is_valid_path(ok), "{ok}");
        }
    }
}
