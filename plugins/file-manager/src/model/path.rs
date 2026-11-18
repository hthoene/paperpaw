//! Paths as the file manager names them: segments joined by `/`, no leading
//! slash, `""` for the top of the storage.

use alloc::string::String;
use paperpaw_sdk::Storage;

use super::Error;

/// The last segment.
#[must_use]
pub fn name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The folder above, `""` at the top.
#[must_use]
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `text` appended to `out`, with fallible allocation.
pub(crate) fn push(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

/// An owned copy of `text`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

/// `dir` and `name` joined into `out`, which is emptied first.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn join_into(out: &mut String, dir: &str, name: &str) -> Result<(), Error> {
    out.clear();
    if !dir.is_empty() {
        push(out, dir)?;
        push(out, "/")?;
    }
    push(out, name)
}

/// `dir` and `name` joined.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn join(dir: &str, name: &str) -> Result<String, Error> {
    let mut out = String::new();
    join_into(&mut out, dir, name)?;
    Ok(out)
}

/// Whether two names are one on this storage: equal, or alike but for case
/// where the storage ignores it (the card's file system does).
#[must_use]
pub fn same_name(storage: &dyn Storage, a: &str, b: &str) -> bool {
    a == b
        || storage.ignores_case()
            && a.chars()
                .flat_map(char::to_lowercase)
                .eq(b.chars().flat_map(char::to_lowercase))
}

/// Whether two paths are one (slashes around them do not count).
#[must_use]
pub fn same_path(storage: &dyn Storage, a: &str, b: &str) -> bool {
    same_name(storage, a.trim_matches('/'), b.trim_matches('/'))
}

/// Whether `path` is `dir` or below it.
#[must_use]
pub fn is_within(storage: &dyn Storage, path: &str, dir: &str) -> bool {
    let (path, dir) = (path.trim_matches('/'), dir.trim_matches('/'));
    let mut parts = path.split('/');
    dir.is_empty()
        || dir
            .split('/')
            .all(|d| parts.next().is_some_and(|p| same_name(storage, p, d)))
}

/// Whether `path` is a folder: there is no file by that name, and it lists.
#[must_use]
pub fn is_dir(storage: &dyn Storage, path: &str) -> bool {
    !storage.exists(path) && storage.list(path, &mut |_| {}).is_ok()
}

/// The most names tried for a free one before the one after the last is taken.
const FREE_NAMES: u32 = 1 << 16;

/// A path in `dir` that nothing has: `name`, else `name (2)`, `name (3)`, …
/// (before the extension); where the storage ignores case, `Name` is not free
/// beside `name`. One buffer is reused for every name tried.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn free_path(storage: &dyn Storage, dir: &str, name: &str) -> Result<String, Error> {
    let mut path = join(dir, name)?;
    let taken = |path: &str| storage.exists(path) || is_dir(storage, path);
    if !taken(&path) {
        return Ok(path);
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => (stem, name.get(stem.len()..).unwrap_or("")),
        _ => (name, ""),
    };
    let mut candidate = String::new();
    for n in 2..=FREE_NAMES + 1 {
        candidate.clear();
        push(&mut candidate, stem)?;
        push(&mut candidate, " (")?;
        push_number(&mut candidate, n)?;
        push(&mut candidate, ")")?;
        push(&mut candidate, ext)?;
        join_into(&mut path, dir, &candidate)?;
        // Past them, only a storage that says everything is there: the name
        // after the last.
        if n == FREE_NAMES + 1 || !taken(&path) {
            return Ok(path);
        }
    }
    Ok(path)
}

/// `number` in decimal appended to `out`.
fn push_number(out: &mut String, number: u32) -> Result<(), Error> {
    let mut digits = [0u8; 10];
    let (mut at, mut rest) = (digits.len(), number);
    loop {
        at -= 1;
        if let Some(slot) = digits.get_mut(at) {
            *slot = b'0' + (rest % 10) as u8;
        }
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    push(
        out,
        digits
            .get(at..)
            .and_then(|d| core::str::from_utf8(d).ok())
            .unwrap_or("0"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn names_and_parents() {
        assert_eq!((name("a/b/c.txt"), parent("a/b/c.txt")), ("c.txt", "a/b"));
        assert_eq!((name("top"), parent("top")), ("top", ""));
        assert_eq!(join("", "x").unwrap(), "x");
        assert_eq!(join("a/b", "x").unwrap(), "a/b/x");
    }

    #[test]
    fn within_and_same() {
        let card = MemoryStorage::new();
        assert!(is_within(&card, "a/b/c", "a/b"));
        assert!(is_within(&card, "a/b", "a/b"));
        assert!(is_within(&card, "a", ""));
        assert!(!is_within(&card, "a/bc", "a/b"));
        assert!(!is_within(&card, "a", "a/b"));
        assert!(same_path(&card, "/a/b/", "a/b"));
        assert!(!same_path(&card, "a/B", "a/b"));
    }

    #[test]
    fn a_free_path_is_numbered_before_the_extension() {
        let card = MemoryStorage::new()
            .with("d/a.txt", "")
            .with("d/a (2).txt", "")
            .with("d/dir/x", "")
            .with("d/noext", "");
        assert_eq!(free_path(&card, "d", "b.txt").unwrap(), "d/b.txt");
        assert_eq!(free_path(&card, "d", "a.txt").unwrap(), "d/a (3).txt");
        assert_eq!(free_path(&card, "d", "dir").unwrap(), "d/dir (2)");
        assert_eq!(free_path(&card, "d", "noext").unwrap(), "d/noext (2)");
        assert_eq!(free_path(&card, "", "d").unwrap(), "d (2)");
    }
}
