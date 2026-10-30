//! Versioned formats of the files plugins keep their data in.
//!
//! What PaperPaw keeps on the card stays readable by every later version:
//! a file's format has a version, and a plugin that changes it in a way
//! older readers would misread raises the version and adds the step from
//! the one before ([`Format::upgrades`]). Files are upgraded as they are
//! read, in memory, and written in the new format when they are saved
//! next. Adding a key to an ini file needs no new version: readers skip
//! keys they do not know.
//!
//! A text file says its version in its first line, `# format 2`; a file
//! without that line is version 1, so files from before versions came
//! need no change. A firmware or app older than a file (after going back
//! to an older update) does not read it and does not write over it: it
//! stays as the newer version left it ([`Error::Newer`]).
//!
//! Binary files change their name for a new version instead (e.g.
//! `pages.bin` to `pages2.bin`), caches their stamp: they are made again.
//! Each format's versions have files in the repository that the tests
//! read (see `docs/data-files.md`).

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;

use paperpaw_platform::{Storage, StorageError};

/// PaperPaw's own folder, beside the books: settings, the library, every
/// plugin's data ([`Env::plugin_dir`](crate::Env::plugin_dir)). A path in
/// the shell's storage, not on a disk: a shell may keep it elsewhere than
/// the books (e.g. in an app's own storage), as its storage maps paths.
/// [`data_path!`](crate::data_path) names a file in it where a constant is
/// needed.
pub const DIR: &str = ".paperpaw";

/// The name PaperPaw had up to 0.3.
const OLD_NAME: &str = "quireink"; // now paperpaw

/// Moves what PaperPaw kept on the card under its old name (its folder and
/// configuration file) to where it is kept now, once: the kernel does it as
/// it starts. Where the folder is there under both names (a shell may
/// write to it before the kernel starts), it moves each entry the new one
/// does not have, however deep; a file the new one has stays as it is, and
/// the old one is left under the old name.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn take_over_old_name(storage: &dyn Storage) {
    let names = [
        (format!(".{OLD_NAME}"), DIR),
        (format!("{OLD_NAME}.ini"), crate::builtin::CONFIG_FILE),
    ];
    for (old, new) in names {
        take_over(storage, &old, new, TAKE_OVER_DEPTH);
    }
}

/// How many folders deep the old folder is merged into the new one: deeper
/// than PaperPaw keeps anything, and a bound on the work at each start.
const TAKE_OVER_DEPTH: u8 = 8;

/// Moves `old` to `new`. Where both are folders, it moves what `old` holds
/// entry by entry, down to `depth` folders deep.
fn take_over(storage: &dyn Storage, old: &str, new: &str, depth: u8) {
    let folder = |path: &str| storage.list(path, &mut |_| {}).is_ok();
    // A file or a folder.
    let there = |path: &str| storage.exists(path) || folder(path);
    if !there(old) {
        return;
    }
    if !there(new) {
        // Not moved: tried again at the next start.
        let _ = storage.rename(old, new);
        return;
    }
    if depth == 0 || storage.exists(old) || storage.exists(new) {
        return;
    }
    let mut names = alloc::vec::Vec::new();
    if storage
        .list(old, &mut |entry| names.push(String::from(entry.name)))
        .is_err()
    {
        return;
    }
    for name in names {
        let (old, new) = (format!("{old}/{name}"), format!("{new}/{name}"));
        take_over(storage, &old, &new, depth - 1);
    }
}

/// The first line of a file from version 2 on, before the number.
pub const MARK: &str = "# format ";

/// One step: a file's text in version `n`, made into version `n + 1`.
pub type Upgrade = fn(&str) -> String;

/// A format of a text file and how its older versions become this one.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Format {
    /// The version written, from 1.
    pub version: u32,
    /// From version `n` to `n + 1` for `n` from 1: one less than
    /// [`Format::version`].
    pub upgrades: &'static [Upgrade],
}

/// Why a file is not read or not written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Written by a newer PaperPaw (format version this): left as it is.
    Newer(u32),
    /// Not text, or not as the format says.
    Unreadable,
    Storage(StorageError),
}

impl From<StorageError> for Error {
    fn from(e: StorageError) -> Self {
        Error::Storage(e)
    }
}

impl From<Error> for StorageError {
    /// A file written by a newer PaperPaw, or not as its format says, is
    /// [`StorageError::InvalidData`]: not read, not written over.
    fn from(e: Error) -> Self {
        match e {
            Error::Storage(e) => e,
            Error::Newer(_) | Error::Unreadable => StorageError::InvalidData,
        }
    }
}

impl Format {
    /// The first version of a format: files as they were before versions
    /// came, without a first line. Reading through it refuses files a
    /// newer PaperPaw marked with a later version.
    pub const FIRST: Format = Format::new(1, &[]);

    /// Format `version` with the steps from version 1 to it.
    pub const fn new(version: u32, upgrades: &'static [Upgrade]) -> Self {
        assert!(
            version >= 1 && upgrades.len() as u32 == version - 1,
            "one upgrade per version after the first"
        );
        Self { version, upgrades }
    }

    /// The version of a file's `text` (see the [module documentation](self)):
    /// 0 where its mark says so (`# format 0`), which no format reads.
    pub fn version_of(text: &str) -> u32 {
        text.lines()
            .next()
            .and_then(|line| line.trim().strip_prefix(MARK))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(1)
    }

    /// A file's `text` in this version, without its first line of the
    /// version: as it is if it is this version, upgraded step by step if
    /// it is older.
    pub fn read<'a>(&self, text: &'a str) -> Result<Cow<'a, str>, Error> {
        let mut version = Self::version_of(text);
        if version > self.version {
            return Err(Error::Newer(version));
        }
        if version == 0 {
            return Err(Error::Unreadable);
        }
        let body = if version > 1 {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            text
        };
        let mut body = Cow::Borrowed(body);
        while version < self.version {
            let step = self.upgrades[version as usize - 1];
            body = Cow::Owned(step(&body));
            version += 1;
        }
        Ok(body)
    }

    /// The file at `path` in this version (see [`Format::read`]), a
    /// UTF-8 byte order mark left out; `Ok(None)` if there is none. Files
    /// larger than `limit` are refused. An error means the file is there
    /// but could not be read: it is not to be written over.
    pub fn load(
        &self,
        storage: &dyn Storage,
        path: &str,
        limit: usize,
    ) -> Result<Option<String>, Error> {
        let text = match crate::files::load_text_for_update(storage, path, limit) {
            Ok(Some(text)) => text,
            Ok(None) => return Ok(None),
            Err(StorageError::InvalidData) => return Err(Error::Unreadable),
            Err(e) => return Err(Error::Storage(e)),
        };
        Ok(Some(self.read(&text)?.into_owned()))
    }

    /// `body` as a file of this version: with its first line from version
    /// 2 on.
    pub fn text(&self, body: &str) -> String {
        if self.version > 1 {
            format!("{MARK}{}\n{body}", self.version)
        } else {
            String::from(body)
        }
    }

    /// Writes `body` to `path` in this version, unless a newer PaperPaw
    /// wrote the file there ([`Error::Newer`]).
    pub fn save(&self, storage: &dyn Storage, path: &str, body: &str) -> Result<(), Error> {
        let mut head = [0u8; 32];
        if let Ok(n) = storage.read_at(path, 0, &mut head) {
            let head = &head[..n];
            let head = head.strip_prefix("\u{feff}".as_bytes()).unwrap_or(head);
            // The first line may be cut: only a whole one counts. What
            // follows it may be a character cut in two, so it is left out.
            if let Some(end) = head.iter().position(|&b| b == b'\n') {
                let line = core::str::from_utf8(&head[..end]).unwrap_or_default();
                let version = Self::version_of(line);
                if version > self.version {
                    return Err(Error::Newer(version));
                }
            }
        }
        storage
            .write(path, self.text(body).as_bytes())
            .map_err(Error::Storage)
    }
}

/// A path in PaperPaw's own folder ([`data::DIR`](crate::data::DIR)), as
/// a constant: `data_path!("secrets.txt")` is `.paperpaw/secrets.txt`.
#[macro_export]
macro_rules! data_path {
    ($path:literal) => {
        concat!(".paperpaw/", $path)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_storage::MemoryStorage;

    #[test]
    fn data_paths_are_in_the_folder() {
        assert_eq!(crate::data_path!("a/b.txt"), format!("{DIR}/a/b.txt"));
    }

    /// Version 2 renamed `colour` to `color`, version 3 added a section.
    const FORMAT: Format = Format::new(
        3,
        &[
            |v1| v1.replace("colour", "color"),
            |v2| format!("[look]\n{v2}"),
        ],
    );

    #[test]
    fn older_files_are_upgraded_step_by_step() {
        assert_eq!(
            FORMAT.read("colour = red\n").unwrap(),
            "[look]\ncolor = red\n"
        );
        assert_eq!(
            FORMAT.read("# format 2\ncolor = red\n").unwrap(),
            "[look]\ncolor = red\n"
        );
        let current = FORMAT.read("# format 3\n[look]\ncolor = red\n").unwrap();
        assert!(matches!(current, Cow::Borrowed("[look]\ncolor = red\n")));
        assert_eq!(FORMAT.read("# format 4\nx\n"), Err(Error::Newer(4)));
        // A comment that is no mark is version 1.
        assert_eq!(Format::version_of("# my colours\ncolour = red"), 1);
        // Versions start at 1.
        assert_eq!(FORMAT.read("# format 0\nx\n"), Err(Error::Unreadable));
    }

    #[test]
    fn saved_with_its_version_and_never_over_a_newer_file() {
        let storage = MemoryStorage::new().with("new.ini", "# format 9\nwhatever\n");
        FORMAT
            .save(&storage, "a.ini", "[look]\ncolor = blue\n")
            .unwrap();
        assert_eq!(
            FORMAT.load(&storage, "a.ini", 1024).unwrap().unwrap(),
            "[look]\ncolor = blue\n"
        );
        assert_eq!(
            crate::read_file(&storage, "a.ini", 1024).unwrap(),
            b"# format 3\n[look]\ncolor = blue\n"
        );
        assert_eq!(FORMAT.save(&storage, "new.ini", "x"), Err(Error::Newer(9)));
        assert_eq!(FORMAT.load(&storage, "new.ini", 1024), Err(Error::Newer(9)));
        assert_eq!(FORMAT.load(&storage, "none.ini", 1024), Ok(None));
        // There but not readable: not taken for missing.
        let failing = crate::memory_storage::Unreadable(MemoryStorage::new().with("a.ini", "x"));
        assert_eq!(
            FORMAT.load(&failing, "a.ini", 1024),
            Err(Error::Storage(StorageError::Io))
        );
        let ansi = MemoryStorage::new().with("c.ini", b"colour = r\xF6d\n".to_vec());
        assert_eq!(FORMAT.load(&ansi, "c.ini", 1024), Err(Error::Unreadable));
        assert_eq!(
            StorageError::from(Error::Newer(2)),
            StorageError::InvalidData
        );
        let bom = MemoryStorage::new().with("d.ini", "\u{feff}# format 3\n[look]\n");
        assert_eq!(
            FORMAT.load(&bom, "d.ini", 1024).unwrap().unwrap(),
            "[look]\n"
        );
        // Version 1 is written as before versions came.
        let v1 = Format::new(1, &[]);
        v1.save(&storage, "b.txt", "a\nb\n").unwrap();
        assert_eq!(
            crate::read_file(&storage, "b.txt", 1024).unwrap(),
            b"a\nb\n"
        );
    }

    #[test]
    fn the_old_name_is_taken_over_once() {
        let storage = MemoryStorage::new()
            .with(&format!(".{OLD_NAME}/secrets.txt"), "s")
            .with(&format!("{OLD_NAME}.ini"), "[core]\n");
        take_over_old_name(&storage);
        assert_eq!(
            crate::read_file(&storage, &format!("{DIR}/secrets.txt"), 8).unwrap(),
            b"s"
        );
        assert_eq!(
            crate::read_file(&storage, crate::builtin::CONFIG_FILE, 8).unwrap(),
            b"[core]\n"
        );
        // What is kept under the new name stays.
        let storage = MemoryStorage::new()
            .with(&format!(".{OLD_NAME}/a.txt"), "old")
            .with(&format!("{DIR}/a.txt"), "new");
        take_over_old_name(&storage);
        assert_eq!(
            crate::read_file(&storage, &format!("{DIR}/a.txt"), 8).unwrap(),
            b"new"
        );
    }
}
