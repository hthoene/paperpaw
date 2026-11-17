//! An index of the library for views of every book (a list, a grid): each
//! book of the catalog with what it is sorted and filtered by, in 24
//! bytes, made in one pass over the catalog (`Index::build`). Its
//! books' lines are read again by where they start (`Index::record`)
//! for the page a view shows, never while drawing.
//!
//! On devices of little memory it holds `cap` books at most and counts
//! the others (`Index::total`). A view covered by a book can keep it in
//! a file (`Index::save`) and let it go, and read it back when it shows
//! again (`Index::load`).

use alloc::vec::Vec;

use super::catalog::{self, Record, Show};
use crate::Storage;

/// Never opened ([`Item::progress`]).
pub const NONE: u8 = u8::MAX;
/// Letters of a title kept to sort by ([`sort_key`]).
pub(crate) const KEY: usize = 8;

/// A book of the library, as the index keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Item {
    /// Its key in the library: where its line starts in the catalog
    /// (`catalog::read_page`), or what a library of one's own names it
    /// by (`super::Library::each`).
    pub offset: u32,
    /// A hash of its path, to find it again ([`hash`]).
    pub hash: u32,
    /// Its title's first letters, to sort by (`sort_key`).
    pub key: [u8; KEY],
    /// Its series, as an index of `Index::series`, or `NO_SERIES`.
    pub series: u16,
    /// Its number in the series in tenths (`number`), to order by.
    pub number: u16,
    /// How far it is read in percent, or [`NONE`] never opened.
    pub progress: u8,
    /// Its place among the books read last (0 the last), or `NEVER`.
    pub recent: u8,
    /// `FAVOURITE`, `OPENED`, `REMOVED`.
    pub flags: u8,
}

/// A favourite ([`Item::flags`]).
pub(crate) const FAVOURITE: u8 = 1;
/// Opened once ([`Item::flags`]).
pub(crate) const OPENED: u8 = 2;
/// Removed since the index was made ([`Item::flags`]): shown nowhere.
pub(crate) const REMOVED: u8 = 4;

impl Item {
    /// The item of `record`, whose key is `offset`, of the series
    /// `series` (`Index::series`, or `NO_SERIES`), at the place
    /// `recent` among the books read last (or `NEVER`).
    pub fn new(offset: u32, record: &Record, series: u16, recent: u8) -> Item {
        let mut flags = 0;
        if record.status.favourite {
            flags |= FAVOURITE;
        }
        if record.status.opened {
            flags |= OPENED;
        }
        Item {
            offset,
            hash: hash(&record.path),
            key: sort_key(&record.title),
            series,
            number: number(&record.number),
            progress: record.status.progress.unwrap_or(NONE),
            recent,
            flags,
        }
    }

    /// Whether `show` shows it.
    pub fn shown(&self, show: Show) -> bool {
        if self.flags & REMOVED != 0 {
            return false;
        }
        let opened = self.flags & OPENED != 0 || self.progress != NONE;
        match show {
            Show::Unread => !opened,
            Show::Reading => opened && self.progress != 100,
            Show::Finished => self.progress == 100,
            Show::Favourites => self.flags & FAVOURITE != 0,
            _ => true,
        }
    }
}

/// A book's number in its series (`"2"`, `"2.5"`) in tenths, to order
/// by; those without one last.
pub(crate) fn number(text: &str) -> u16 {
    match catalog::series_order(text) {
        u32::MAX => u16::MAX,
        thousandths => (thousandths / 100).min(u32::from(u16::MAX) - 1) as u16,
    }
}

/// A hash of `path`, to find a book of the index by its path: the low
/// bits of the one the catalog's log keys changes by
/// ([`catalog::changes`]).
pub fn hash(path: &str) -> u32 {
    paperpaw_digest::fnv1a64(path) as u32
}

/// `title` without a leading article, as libraries sort it ("The Hound
/// of the Baskervilles" under H).
pub(crate) fn without_article(title: &str) -> &str {
    let title = title.trim_start();
    for article in ["the ", "a ", "an ", "der ", "die ", "das "] {
        if title.len() > article.len()
            && title.is_char_boundary(article.len())
            && title[..article.len()].eq_ignore_ascii_case(article)
        {
            return title[article.len()..].trim_start();
        }
    }
    title
}

/// `c` as a small ASCII letter or digit to sort by: accents dropped,
/// other letters after `z`; `None` for what is not sorted by (marks).
fn fold(c: char) -> Option<u8> {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'a'..='z' | '0'..='9' => Some(c as u8),
        ' ' => Some(b' '),
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => Some(b'a'),
        'ç' => Some(b'c'),
        'è' | 'é' | 'ê' | 'ë' => Some(b'e'),
        'ì' | 'í' | 'î' | 'ï' => Some(b'i'),
        'ñ' => Some(b'n'),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => Some(b'o'),
        'ù' | 'ú' | 'û' | 'ü' => Some(b'u'),
        'ß' => Some(b's'),
        c if c.is_alphabetic() => Some(b'~'),
        _ => None,
    }
}

/// The first letters of `title` to sort by ([`KEY`]), without a leading
/// article; titles with the same first letters are kept in the
/// catalog's order.
pub(crate) fn sort_key(title: &str) -> [u8; KEY] {
    let mut key = [0u8; KEY];
    for (slot, byte) in key
        .iter_mut()
        .zip(without_article(title).chars().filter_map(fold))
    {
        *slot = byte;
    }
    key
}

/// The capital `text` is listed under: its first letter without accents,
/// `#` for a digit or anything else.
pub fn letter(text: &str) -> u8 {
    match text.chars().find_map(fold) {
        Some(b @ b'a'..=b'z') => b.to_ascii_uppercase(),
        _ => b'#',
    }
}

/// The books opened last, their paths [`hash`]ed, the last first.
pub fn opened(storage: &dyn Storage) -> Vec<u32> {
    super::opened_books(storage)
        .iter()
        .map(|p| hash(p))
        .collect()
}

/// Which catalog there is, as an index of it keeps it
/// ([`Index::version`]): its generation and length (not its log's, which
/// [`Index::refresh`] reads).
pub(crate) fn catalog_version(storage: &dyn Storage) -> Option<u64> {
    Some((catalog::generation(storage)? << 40) ^ catalog::size(storage)?)
}
