//! Which files are pictures, and in what order they are read.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::{Storage, StorageError};

use paperpaw_sdk::document::DocumentError;

use crate::provider::NO_MEMORY;
use crate::{MAX_NAME, MAX_PAGES, pictures};

/// Names that wait while a folder is listed, at most: a list of this many
/// is the largest the vector holds without growing again.
const HOLD: usize = 512;

/// Whether `name` is a picture PaperPaw shows (hidden files, such as the
/// `._` copies macOS leaves on cards, are not).
pub(crate) fn is_picture(name: &str) -> bool {
    !name.starts_with('.')
        && [".jpg", ".jpeg", ".png", ".gif"].iter().any(|extension| {
            name.len()
                .checked_sub(extension.len())
                .and_then(|at| name.get(at..))
                .is_some_and(|tail| tail.eq_ignore_ascii_case(extension))
        })
}

/// Names in reading order: numbers within them by value, so `2.jpg` comes
/// before `10.jpg`; letters without regard to case.
pub(crate) fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let digits = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
                let (n, m) = (digits(a), digits(b));
                let zeros = |s: &[u8]| s.iter().take_while(|&&c| c == b'0').count();
                let (x0, y0) = (zeros(&a[..n]).min(n - 1), zeros(&b[..m]).min(m - 1));
                let (x, y) = (&a[x0..n], &b[y0..m]);
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[n..];
                b = &b[m..];
            }
            (Some(x), Some(y)) => {
                let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

/// The order of the pages: by [`natural`], names that read alike by their
/// letters, so it is total.
fn order(a: &str, b: &str) -> Ordering {
    natural(a, b).then_with(|| a.cmp(b))
}

/// A page: the name of its file, and its size.
pub(crate) type Page = (String, u64);

/// A copy of `text`; an error instead of an abort when memory is short.
pub(crate) fn copy(text: &str) -> Result<String, DocumentError> {
    pictures::text(&[text])
}

/// The pictures of the folder `dir` that come from the picture `first` on
/// (it first), in reading order: at most [`MAX_PAGES`], the first of them,
/// whatever the folder holds. Names wait in no more than [`HOLD`] pieces of
/// memory. A folder that is not there to list (not found, not offered) has
/// `first` alone; memory that runs out and any other failure of the listing
/// are errors, never a book of one page without a word.
pub(crate) fn pages_from(
    storage: &dyn Storage,
    dir: &str,
    first: &Page,
) -> Result<Vec<Page>, DocumentError> {
    let mut pages: Vec<Page> = Vec::new();
    let mut short = false;
    let keep = |pages: &mut Vec<Page>| {
        // Unstable: the order is total, and the stable sort takes scratch room
        // (about 19 KB above 128 names) that a refusal would abort.
        paperpaw_sdk::sort::sort_by(pages, |a, b| order(&a.0, &b.0));
        pages.truncate(MAX_PAGES);
    };
    let listed = storage.list(dir, &mut |entry| {
        if short
            || entry.is_dir
            || entry.name.len() > MAX_NAME
            || !is_picture(entry.name)
            || order(entry.name, &first.0) == Ordering::Less
            || entry.name == first.0
        {
            return;
        }
        // Room for this one name, as the list grows: a folder of three
        // pictures takes three entries, not the most there can be.
        match (pages.try_reserve(1), copy(entry.name)) {
            (Ok(()), Ok(name)) => pages.push((name, entry.size)),
            _ => {
                short = true;
                return;
            }
        }
        if pages.len() >= HOLD {
            keep(&mut pages);
        }
    });
    if short {
        return Err(DocumentError::OutOfMemory);
    }
    match listed {
        Ok(()) => {}
        // No folder to list here (a book that is one file): the picture alone.
        Err(StorageError::NotFound | StorageError::Unsupported) => pages.clear(),
        Err(StorageError::Other(NO_MEMORY)) => return Err(DocumentError::OutOfMemory),
        Err(error) => return Err(DocumentError::Storage(error)),
    }
    pages
        .try_reserve(1)
        .map_err(|_| DocumentError::OutOfMemory)?;
    pages.push((copy(&first.0)?, first.1));
    keep(&mut pages);
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::{Page, pages_from};
    use crate::provider::NO_MEMORY;
    use alloc::string::ToString;
    use paperpaw_sdk::document::DocumentError;
    use paperpaw_sdk::{DirEntry, Storage, StorageError};

    /// A folder whose listing fails with `error`.
    struct Refusing(StorageError);

    impl Storage for Refusing {
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
            Err(self.0)
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
    }

    fn first() -> Page {
        ("01.png".to_string(), 10)
    }

    #[test]
    fn a_folder_that_is_not_there_to_list_leaves_the_picture_alone() {
        for error in [StorageError::NotFound, StorageError::Unsupported] {
            let pages = pages_from(&Refusing(error), "", &first()).unwrap();
            assert_eq!(pages, [first()], "{error:?}");
        }
    }

    #[test]
    fn a_listing_that_fails_otherwise_is_an_error_not_a_book_of_one_page() {
        assert_eq!(
            pages_from(&Refusing(StorageError::Other(NO_MEMORY)), "", &first()),
            Err(DocumentError::OutOfMemory)
        );
        for error in [StorageError::Io, StorageError::Full] {
            assert_eq!(
                pages_from(&Refusing(error), "", &first()),
                Err(DocumentError::Storage(error))
            );
        }
    }
}
