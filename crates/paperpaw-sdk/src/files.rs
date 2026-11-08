//! Working with files and folders of a storage: listing, copying,
//! moving and deleting them, reading text files line by line, free
//! names, sizes as people read them. Used by file managers and transfers
//! alike.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_platform::{Storage, StorageError};

/// The longest line [`for_each_line`] hands on; the rest of a longer one
/// is left out.
pub const MAX_LINE: usize = 4096;

/// An entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

impl Entry {
    pub fn new(name: impl Into<String>, is_dir: bool, size: u64) -> Self {
        Self {
            name: name.into(),
            is_dir,
            size,
        }
    }
}

/// `dir/name`, or `name` at the top.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// The folder a path is in.
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The last part of a path.
pub fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Whether `name` ends in `.` and `extension` (given without the dot),
/// in any case: `Book.EPUB` is an EPUB on every storage.
pub fn has_extension(name: &str, extension: &str) -> bool {
    let (name, extension) = (name.as_bytes(), extension.as_bytes());
    name.len() > extension.len()
        && name[name.len() - extension.len() - 1] == b'.'
        && name[name.len() - extension.len()..].eq_ignore_ascii_case(extension)
}

/// Whether `a` and `b` are equal, in any case where the storage ignores
/// case ([`Storage::ignores_case`]).
fn same_name(storage: &dyn Storage, a: &str, b: &str) -> bool {
    a == b
        || storage.ignores_case()
            && a.chars()
                .flat_map(char::to_lowercase)
                .eq(b.chars().flat_map(char::to_lowercase))
}

/// Whether paths `a` and `b` name the same entry of `storage`: equal, or
/// equal but for case where the storage ignores case
/// ([`Storage::ignores_case`]), e.g. `Books/A.epub` and `books/a.epub` on
/// a memory card. Slashes at either end do not count.
pub fn same_path(storage: &dyn Storage, a: &str, b: &str) -> bool {
    same_name(storage, a.trim_matches('/'), b.trim_matches('/'))
}

/// Whether `path` is folder `dir` or lies in it, at any depth, as the
/// storage names entries ([`same_path`]); every path lies in `""`.
pub fn is_within(storage: &dyn Storage, path: &str, dir: &str) -> bool {
    let (path, dir) = (path.trim_matches('/'), dir.trim_matches('/'));
    let mut parts = path.split('/');
    dir.is_empty()
        || dir
            .split('/')
            .all(|d| parts.next().is_some_and(|p| same_name(storage, p, d)))
}

/// Whether a part of `path` starts with `.`: PaperPaw's own data, or
/// documents a plugin keeps for itself (e.g. web pages a browser shows),
/// which are no books.
pub fn is_hidden(path: &str) -> bool {
    path.split('/').any(|part| part.starts_with('.'))
}

/// Whether `name`, a part of a path that came from outside (a server,
/// another device, a browser), is a plain name of a file or a folder: not
/// hidden (PaperPaw's own data, [`data::DIR`](crate::data::DIR), and what
/// plugins keep for themselves start with `.`), not `..`, and nothing a
/// file system takes for another name or for more than one (a space at
/// either end, a dot at the end, which Windows drops; `/`, `\`, `:`, NUL,
/// control characters).
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['.', ' '])
        && !name.ends_with(['.', ' '])
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(char::is_control)
}

/// `path` from outside as a path of plain names ([`is_plain_name`]),
/// without `/` at either end; `None` where a part is not plain, or there
/// is none. The short names FAT and NTFS keep beside long ones
/// (`PAPERP~1` for `.paperpaw`) reach nothing on PaperPaw's storages.
pub fn plain_path(path: &str) -> Option<String> {
    let path = path.trim_matches('/');
    path.split('/').all(is_plain_name).then(|| path.to_string())
}

/// A folder's entries: folders first, then by name (case ignored);
/// names starting with `.` only with `hidden`.
pub fn list(storage: &dyn Storage, dir: &str, hidden: bool) -> Result<Vec<Entry>, StorageError> {
    let mut entries = Vec::new();
    storage.list(dir, &mut |e| {
        if hidden || !e.name.starts_with('.') {
            entries.push(Entry {
                name: e.name.to_string(),
                is_dir: e.is_dir,
                size: e.size,
            });
        }
    })?;
    // Compared letter by letter, without a lower-case copy of each name
    // for every comparison; names alike but for case by their letters.
    // Unstable: a stable sort takes 4 KiB of stack (too much on the X4).
    paperpaw_sort::sort_by(&mut entries, |a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| {
                a.name
                    .chars()
                    .flat_map(char::to_lowercase)
                    .cmp(b.name.chars().flat_map(char::to_lowercase))
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

/// Whether `path` is a folder.
pub fn is_dir(storage: &dyn Storage, path: &str) -> bool {
    !storage.exists(path) && storage.list(path, &mut |_| {}).is_ok()
}

/// Names tried by [`free_path`]: more than a folder on a memory card
/// holds (65,536 entries).
const FREE_NAMES: u32 = 1 << 16;

/// A free path for `name` in `dir`: `name`, else `name (2)`, `name (3)`, …
/// (before the extension). Where the storage ignores case, `Name` is not
/// free beside `name`.
pub fn free_path(storage: &dyn Storage, dir: &str, name: &str) -> String {
    let taken = |path: &str| storage.exists(path) || is_dir(storage, path);
    let first = join(dir, name);
    if !taken(&first) {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let numbered = |n: u32| join(dir, &format!("{stem} ({n}){ext}"));
    // Past them, only a storage that says everything is there: the name
    // after the last.
    (2..=FREE_NAMES)
        .map(numbered)
        .find(|p| !taken(p))
        .unwrap_or_else(|| numbered(FREE_NAMES + 1))
}

/// Where a file being made at `path` (a download, a copy) is written
/// until it is whole: `path.part` beside it. [`put_in_place`] moves it
/// to `path` then.
pub fn part_path(path: &str) -> String {
    format!("{path}.part")
}

/// Puts the file made at `part` (see [`part_path`]) at `path`, replacing
/// what is there: what was at `path` stays until the new file is whole.
/// Where that fails, the part is removed, unless the failed move took
/// the old file away (FAT removes it first): then the part is all there is.
pub fn put_in_place(storage: &dyn Storage, part: &str, path: &str) -> Result<(), StorageError> {
    let had_old = storage.exists(path);
    let moved = move_to(storage, part, path);
    if moved.is_err() && (!had_old || storage.exists(path)) {
        let _ = storage.remove(part);
    }
    moved
}

/// Puts the file made at `part` (e.g. a download) into `dir` as `name`,
/// or under a free name beside it ([`free_path`]); the path it is at
/// now. Where that fails, the part is removed.
pub fn put_in_free_place(
    storage: &dyn Storage,
    part: &str,
    dir: &str,
    name: &str,
) -> Result<String, StorageError> {
    let path = free_path(storage, dir, name);
    put_in_place(storage, part, &path).map(|()| path)
}

/// Copies a file or a folder with everything in it. Each file is copied
/// to a part first ([`part_path`]) and put in place when whole: a copy
/// cut short (a full card) leaves no half file that looks whole.
pub fn copy(storage: &dyn Storage, from: &str, to: &str) -> Result<(), StorageError> {
    if is_dir(storage, from) {
        if is_within(storage, to, from) {
            // Into itself: it would never end.
            return Err(StorageError::InvalidPath);
        }
        storage.create_dir(to)?;
        for entry in list(storage, from, true)? {
            copy(storage, &join(from, &entry.name), &join(to, &entry.name))?;
        }
        return Ok(());
    }
    let part = part_path(to);
    let copied = copy_file(storage, from, &part);
    let had_old = storage.exists(to);
    match copied {
        // Renamed directly, not by `move_to`: where the storage cannot
        // rename, that would copy the part again through its own part.
        Ok(()) => match storage.rename(&part, to) {
            Err(StorageError::Unsupported) => {
                let _ = storage.remove(&part);
                copy_file(storage, from, to)
            }
            Err(e) => {
                // As in `put_in_place`: kept where it is the only copy.
                if !had_old || storage.exists(to) {
                    let _ = storage.remove(&part);
                }
                Err(e)
            }
            Ok(()) => Ok(()),
        },
        Err(e) => {
            let _ = storage.remove(&part);
            Err(e)
        }
    }
}

fn copy_file(storage: &dyn Storage, from: &str, to: &str) -> Result<(), StorageError> {
    storage.write(to, &[])?;
    let mut chunk = vec![0u8; 16 * 1024];
    let mut at = 0u64;
    loop {
        let n = storage.read_at(from, at, &mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        storage.append(to, &chunk[..n])?;
        at += n as u64;
    }
}

/// Moves a file or folder; between storages that cannot rename across
/// (e.g. a memory card and the device), by copying and deleting. Not
/// into itself (a folder into one in it), in any case where the storage
/// ignores case.
pub fn move_to(storage: &dyn Storage, from: &str, to: &str) -> Result<(), StorageError> {
    if is_within(storage, to, from) && !same_path(storage, to, from) {
        return Err(StorageError::InvalidPath);
    }
    match storage.rename(from, to) {
        Err(StorageError::Unsupported) => {
            copy(storage, from, to)?;
            delete(storage, from)
        }
        result => result,
    }
}

/// Deletes a file or a folder with everything in it.
pub fn delete(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    if is_dir(storage, path) {
        storage.remove_dir_all(path)
    } else {
        storage.remove(path)
    }
}

/// The byte order mark some editors (e.g. Windows Notepad) put at the
/// start of UTF-8 text.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// `data` without a UTF-8 byte order mark at its start.
pub fn without_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(BOM).unwrap_or(data)
}

/// The text file at `path` (at most `cap` bytes), without a byte order
/// mark: [`StorageError::NotFound`] where there is none,
/// [`StorageError::TooLarge`] over `cap`, [`StorageError::InvalidData`]
/// where it is not UTF-8, any other error where it could not be read.
pub fn read_text(storage: &dyn Storage, path: &str, cap: usize) -> Result<String, StorageError> {
    text_of(crate::read_file(storage, path, cap)?)
}

fn text_of(mut data: Vec<u8>) -> Result<String, StorageError> {
    if data.starts_with(BOM) {
        data.drain(..BOM.len());
    }
    String::from_utf8(data).map_err(|_| StorageError::InvalidData)
}

/// What the file at `path` (at most `cap` bytes) holds before it is
/// changed and written back, without a UTF-8 byte order mark: `None`
/// where there is no such file (start from nothing), an error where there
/// is one that could not be read (too large, a failing card). Write the
/// file only after `Ok`: one written from an empty start after a failed
/// read loses what it held.
pub fn load_for_update(
    storage: &dyn Storage,
    path: &str,
    cap: usize,
) -> Result<Option<Vec<u8>>, StorageError> {
    match crate::read_file(storage, path, cap) {
        Ok(mut data) => {
            if data.starts_with(BOM) {
                data.drain(..BOM.len());
            }
            Ok(Some(data))
        }
        Err(StorageError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

/// [`load_for_update`] for a text file:
/// [`StorageError::InvalidData`] where it is not UTF-8 (e.g. saved in
/// another encoding), so it is not written over.
pub fn load_text_for_update(
    storage: &dyn Storage,
    path: &str,
    cap: usize,
) -> Result<Option<String>, StorageError> {
    // The mark is left out already: a second one belongs to the text.
    load_for_update(storage, path, cap)?
        .map(|data| String::from_utf8(data).map_err(|_| StorageError::InvalidData))
        .transpose()
}

/// Reads the text file at `path` line by line as it streams from the
/// storage, so lists of any length are read in little memory: `visit`
/// gets each line without its line break (`\n` or `\r\n`, invalid UTF-8
/// replaced, a byte order mark at the start left out) and says whether
/// to go on. Lines are cut at [`MAX_LINE`] bytes.
pub fn for_each_line(
    storage: &dyn Storage,
    path: &str,
    visit: &mut dyn FnMut(&str) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    let mut chunk = [0u8; 512];
    let mut line: Vec<u8> = Vec::new();
    let mut at = 0u64;
    let mut first = true;
    let mut emit = |line: &mut Vec<u8>| {
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let text = if first { without_bom(line) } else { &line[..] };
        first = false;
        let flow = visit(&String::from_utf8_lossy(text));
        line.clear();
        flow
    };
    loop {
        let n = storage.read_at(path, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        at += n as u64;
        for piece in chunk[..n].split_inclusive(|b| *b == b'\n') {
            let (text, ends) = match piece.split_last() {
                Some((b'\n', text)) => (text, true),
                _ => (piece, false),
            };
            let room = MAX_LINE.saturating_sub(line.len());
            line.extend_from_slice(&text[..text.len().min(room)]);
            if ends && emit(&mut line).is_break() {
                return Ok(());
            }
        }
    }
    if !line.is_empty() {
        let _ = emit(&mut line);
    }
    Ok(())
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB` (with the
/// decimal mark given, in Latin digits). To show one, see [`shown_size`].
pub fn size(bytes: u64, decimal: &str) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 10.0 {
        format!("{} {}", (value + 0.5) as u64, UNITS[unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        format!("{}{decimal}{} {}", tenths / 10, tenths % 10, UNITS[unit])
    }
}

/// A size as the language shown writes it: [`size`] with its decimal mark
/// ([`decimal_mark`](crate::numbers::decimal_mark)) and its digits.
pub fn shown_size(env: &crate::env::Env<'_>, bytes: u64) -> String {
    let text = size(bytes, crate::numbers::decimal_mark(env));
    env.i18n().localize_digits(&text).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::memory_storage::Unreadable;

    fn storage() -> MemoryStorage {
        MemoryStorage::new()
            .with("books/b.epub", b"bb".to_vec())
            .with("books/A.epub", b"aaa".to_vec())
            .with("books/sub/c.txt", b"c".to_vec())
            .with("books/.hidden", b"h".to_vec())
    }

    #[test]
    fn paths_from_outside_are_plain() {
        assert_eq!(
            plain_path("/Ann Author/A Book.epub/").as_deref(),
            Some("Ann Author/A Book.epub")
        );
        assert_eq!(plain_path("Vol~1.txt").as_deref(), Some("Vol~1.txt"));
        for bad in [
            "",
            "/",
            "a//b",
            ".",
            "..",
            "../x",
            "a/../b",
            ".paperpaw/secrets.txt",
            ".PaperPaw/x",
            "Books/.hidden",
            ".paperpaw./x",
            ".paperpaw /x",
            "paperpaw./x",
            "x.epub.",
            "x.epub ",
            " x",
            "a\\b",
            "C:/x",
            "a:stream",
            "a\0b",
            "a\nb",
            "a\tb",
        ] {
            assert_eq!(plain_path(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn lists_folders_first() {
        let s = storage();
        let names: Vec<String> = list(&s, "books", false)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["sub", "A.epub", "b.epub"]);
        assert_eq!(list(&s, "books", true).unwrap().len(), 4);
        assert!(is_dir(&s, "books/sub"));
        assert!(!is_dir(&s, "books/A.epub"));
        assert_eq!(
            (parent("books/sub/c.txt"), name("books/sub/c.txt")),
            ("books/sub", "c.txt")
        );
        assert_eq!(parent("top"), "");
    }

    /// A storage that says it ignores case, as a memory card does, and
    /// takes `room` bytes appended at most.
    struct Card {
        files: MemoryStorage,
        room: core::cell::Cell<usize>,
    }

    impl Card {
        fn new(files: MemoryStorage) -> Self {
            Self {
                files,
                room: core::cell::Cell::new(usize::MAX),
            }
        }
    }

    impl Storage for Card {
        fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.files.read_at(path, at, buf)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.files.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.files.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            let room = self.room.get();
            if data.len() > room {
                return Err(StorageError::Full);
            }
            self.room.set(room - data.len());
            self.files.append(path, data)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), StorageError> {
            self.files.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.files.remove(path)
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
            self.files.rename(from, to)
        }
        fn create_dir(&self, path: &str) -> Result<(), StorageError> {
            self.files.create_dir(path)
        }
        fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
            self.files.remove_dir_all(path)
        }
        fn ignores_case(&self) -> bool {
            true
        }
    }

    #[test]
    fn paths_compared_as_the_storage_names_them() {
        let exact = MemoryStorage::new();
        let card = Card::new(MemoryStorage::new());
        assert!(same_path(&exact, "/books/a.epub", "books/a.epub"));
        assert!(!same_path(&exact, "Books/A.epub", "books/a.epub"));
        assert!(same_path(&card, "Books/\u{c4}.epub", "books/\u{e4}.EPUB"));
        assert!(!same_path(&card, "books/a.epub", "books/b.epub"));
        assert!(is_within(&card, "Books/Sub/x.txt", "books/sub"));
        assert!(is_within(&card, "books/sub", "Books/Sub"));
        assert!(!is_within(&card, "books/subway", "books/sub"));
        assert!(!is_within(&exact, "Books/sub/x", "books"));
        assert!(is_within(&exact, "any/path", ""));
        assert!(has_extension("Book.EPUB", "epub"));
        assert!(has_extension("a.tar.gz", "gz"));
        assert!(!has_extension("epub", "epub"));
        assert!(!has_extension("bookepub", "epub"));
        assert!(!has_extension("\u{4e66}.epu", "epub"));
    }

    #[test]
    fn not_moved_into_itself_in_another_case() {
        let card = Card::new(MemoryStorage::new().with("Books/a.epub", "a"));
        assert_eq!(
            move_to(&card, "Books", "books/inner"),
            Err(StorageError::InvalidPath)
        );
        assert_eq!(
            copy(&card, "Books", "BOOKS/copy"),
            Err(StorageError::InvalidPath)
        );
        assert!(card.files.get("Books/a.epub").is_some());
    }

    #[test]
    fn copies_cut_short_leave_nothing_half() {
        let card = Card::new(
            MemoryStorage::new()
                .with("a.epub", vec![1u8; 40_000])
                .with("b.epub", "old"),
        );
        card.room.set(20_000);
        assert_eq!(copy(&card, "a.epub", "b.epub"), Err(StorageError::Full));
        assert_eq!(card.files.get("b.epub").as_deref(), Some(&b"old"[..]));
        assert_eq!(card.files.get("b.epub.part"), None);
        card.room.set(usize::MAX);
        copy(&card, "a.epub", "b.epub").unwrap();
        assert_eq!(card.files.get("b.epub").map(|d| d.len()), Some(40_000));
        assert_eq!(card.files.get("b.epub.part"), None);
    }

    #[test]
    fn parts_put_in_place() {
        let s = MemoryStorage::new()
            .with("books/a.epub", "old")
            .with("books/a.epub.part", "new");
        assert_eq!(part_path("books/a.epub"), "books/a.epub.part");
        put_in_place(&s, "books/a.epub.part", "books/a.epub").unwrap();
        assert_eq!(s.get("books/a.epub").as_deref(), Some(&b"new"[..]));
        let s = s.with("dl.part", "more");
        assert_eq!(
            put_in_free_place(&s, "dl.part", "books", "a.epub").as_deref(),
            Ok("books/a (2).epub")
        );
        // Where it cannot be put, the part goes.
        let s = s.with("x.part", "x");
        assert!(put_in_place(&s, "x.part", "../x").is_err());
        assert!(s.get("x.part").is_none());
    }

    /// A storage whose renames fail: `Unsupported` every time, or, as FAT
    /// may, after removing the file at the target. Counts them.
    struct BadRename {
        files: MemoryStorage,
        renames: core::cell::Cell<u32>,
        fat: bool,
    }

    impl BadRename {
        fn new(files: MemoryStorage, fat: bool) -> Self {
            Self {
                files,
                renames: core::cell::Cell::new(0),
                fat,
            }
        }
    }

    impl Storage for BadRename {
        fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            self.files.read_at(path, at, buf)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.files.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.files.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.files.append(path, data)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), StorageError> {
            self.files.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.files.remove(path)
        }
        fn rename(&self, _: &str, to: &str) -> Result<(), StorageError> {
            self.renames.set(self.renames.get() + 1);
            if self.renames.get() > 10 {
                // Far past what a move needs: stop the recursion.
                return Err(StorageError::Io);
            }
            if self.fat {
                self.files.remove(to)?;
                return Err(StorageError::Io);
            }
            Err(StorageError::Unsupported)
        }
    }

    #[test]
    fn moves_where_nothing_renames() {
        let s = BadRename::new(MemoryStorage::new().with("dl.tmp", "BOOK"), false);
        assert_eq!(move_to(&s, "dl.tmp", "Books/a.epub"), Ok(()));
        assert!(s.renames.get() <= 2, "{} renames", s.renames.get());
        assert_eq!(s.files.get("Books/a.epub").as_deref(), Some(&b"BOOK"[..]));
        assert_eq!(s.files.get("Books/a.epub.part"), None);
        assert_eq!(s.files.get("dl.tmp"), None);
    }

    #[test]
    fn part_kept_where_a_failed_move_took_the_old_file() {
        let s = BadRename::new(
            MemoryStorage::new()
                .with("a.epub", "old")
                .with("a.epub.part", "new"),
            true,
        );
        assert!(put_in_place(&s, "a.epub.part", "a.epub").is_err());
        // The old file is gone: the part is the only copy left.
        assert_eq!(s.files.get("a.epub.part").as_deref(), Some(&b"new"[..]));
    }

    #[test]
    fn copies_moves_and_deletes() {
        let s = storage();
        copy(&s, "books", "copy").unwrap();
        assert_eq!(s.get("copy/sub/c.txt").as_deref(), Some(&b"c"[..]));
        assert_eq!(
            copy(&s, "books", "books/sub/x"),
            Err(StorageError::InvalidPath)
        );
        move_to(&s, "copy/A.epub", "A.epub").unwrap();
        assert!(s.get("copy/A.epub").is_none() && s.get("A.epub").is_some());
        assert_eq!(free_path(&s, "", "A.epub"), "A (2).epub");
        assert_eq!(free_path(&s, "", "new"), "new");
        delete(&s, "copy").unwrap();
        assert!(s.get("copy/sub/c.txt").is_none());
        assert_eq!(size(820, ","), "820 B");
        assert_eq!(size(2_400_000, ","), "2,3 MB");
        assert_eq!(size(14_000, "."), "14 KB");
    }

    #[test]
    fn files_to_update_tell_missing_from_unreadable() {
        let s = MemoryStorage::new()
            .with("a.ini", b"\xEF\xBB\xBFkey = \xC3\xA4".to_vec())
            .with("ansi.ini", b"key = \xE4".to_vec());
        assert_eq!(load_for_update(&s, "none.ini", 64), Ok(None));
        assert_eq!(
            load_for_update(&s, "a.ini", 64),
            Ok(Some(b"key = \xC3\xA4".to_vec()))
        );
        assert_eq!(
            load_text_for_update(&s, "a.ini", 64),
            Ok(Some(String::from("key = \u{e4}")))
        );
        assert_eq!(read_text(&s, "a.ini", 64).as_deref(), Ok("key = \u{e4}"));
        assert_eq!(load_for_update(&s, "a.ini", 4), Err(StorageError::TooLarge));
        assert_eq!(
            load_text_for_update(&s, "ansi.ini", 64),
            Err(StorageError::InvalidData)
        );
        assert_eq!(read_text(&s, "none.ini", 64), Err(StorageError::NotFound));
        assert_eq!(
            load_for_update(&Unreadable(s), "a.ini", 64),
            Err(StorageError::Io)
        );
        assert_eq!(without_bom(b"\xEF\xBB\xBFx"), b"x");
        assert_eq!(without_bom(b"x"), b"x");
    }

    #[test]
    fn reads_lines_as_they_stream() {
        let long = "x".repeat(MAX_LINE + 600);
        // A byte order mark first, as Notepad writes.
        let text = format!("\u{feff}one\r\n\ntwo {long}\nthree");
        let s = MemoryStorage::new().with("list.txt", text.into_bytes());
        let mut lines = Vec::new();
        for_each_line(&s, "list.txt", &mut |line| {
            lines.push(line.to_string());
            ControlFlow::Continue(())
        })
        .unwrap();
        assert_eq!(lines.len(), 4);
        assert_eq!((lines[0].as_str(), lines[1].as_str()), ("one", ""));
        assert_eq!(lines[2].len(), MAX_LINE);
        assert_eq!(lines[3], "three");
        // Stopped early.
        let mut first = String::new();
        for_each_line(&s, "list.txt", &mut |line| {
            first = line.to_string();
            ControlFlow::Break(())
        })
        .unwrap();
        assert_eq!(first, "one");
        assert_eq!(
            for_each_line(&s, "none.txt", &mut |_| ControlFlow::Continue(())),
            Err(StorageError::NotFound)
        );
    }
}
