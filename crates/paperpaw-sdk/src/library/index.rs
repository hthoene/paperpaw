//! An index of the library for views of every book (a list, a grid): each
//! book of the catalog with what it is sorted and filtered by, in 24
//! bytes, made in one pass over the catalog ([`Index::build`]). Its
//! books' lines are read again by where they start ([`Index::record`])
//! for the page a view shows, never while drawing.
//!
//! On devices of little memory it holds [`cap`] books at most and counts
//! the others ([`Index::total`]). A view covered by a book can keep it in
//! a file (`Index::save`) and let it go, and read it back when it shows
//! again ([`Index::load`]).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::catalog::{self, Books, Record, Show};
use crate::{MemoryClass, Storage};

/// Not among the books read last ([`Item::recent`]).
pub const NEVER: u8 = u8::MAX;
/// Never opened ([`Item::progress`]).
pub const NONE: u8 = u8::MAX;
/// No series ([`Item::series`]).
pub const NO_SERIES: u16 = u16::MAX;
/// Letters of a title kept to sort by ([`sort_key`]).
pub const KEY: usize = 8;
/// Books indexed at most on devices of little memory (the X4: 48 KB),
/// and on others.
const MAX_BOOKS_SMALL: usize = 2000;
const MAX_BOOKS: usize = 20_000;
/// Series told apart at most; books of others count as of none.
const MAX_SERIES: usize = 500;
/// The kept index's first bytes: what it is, and its version.
const MAGIC: &[u8; 4] = b"PPI3";
/// Bytes of an item kept.
const ITEM_BYTES: usize = 23;
/// Bytes of the kept index's head: what it is, the catalog's state, the
/// archive (hashed), how many books are in it, in the catalog and
/// indexed, how many series.
const HEAD: usize = 30;

/// The most books an index holds on a device of `memory`.
pub fn cap(memory: MemoryClass) -> usize {
    if memory == MemoryClass::Small {
        MAX_BOOKS_SMALL
    } else {
        MAX_BOOKS
    }
}

/// A book of the library, as the index keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Item {
    /// Its key in the library: where its line starts in the catalog
    /// ([`catalog::read_page`]), or what a library of one's own names it
    /// by ([`super::Library::each`]).
    pub offset: u32,
    /// A hash of its path, to find it again ([`hash`]).
    pub hash: u32,
    /// Its title's first letters, to sort by ([`sort_key`]).
    pub key: [u8; KEY],
    /// Its series, as an index of [`Index::series`], or [`NO_SERIES`].
    pub series: u16,
    /// Its number in the series in tenths ([`number`]), to order by.
    pub number: u16,
    /// How far it is read in percent, or [`NONE`] never opened.
    pub progress: u8,
    /// Its place among the books read last (0 the last), or [`NEVER`].
    pub recent: u8,
    /// [`FAVOURITE`], [`OPENED`], [`REMOVED`].
    pub flags: u8,
}

/// A favourite ([`Item::flags`]).
pub const FAVOURITE: u8 = 1;
/// Opened once ([`Item::flags`]).
pub const OPENED: u8 = 2;
/// Removed since the index was made ([`Item::flags`]): shown nowhere.
pub const REMOVED: u8 = 4;

impl Item {
    /// The item of `record`, whose key is `offset`, of the series
    /// `series` ([`Index::series`], or [`NO_SERIES`]), at the place
    /// `recent` among the books read last (or [`NEVER`]).
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
pub fn number(text: &str) -> u16 {
    match catalog::series_order(text) {
        u32::MAX => u16::MAX,
        thousandths => (thousandths / 100).min(u32::from(u16::MAX) - 1) as u16,
    }
}

/// The number of `name` in `series`, added where it is not there yet and
/// there is room (else [`NO_SERIES`]). `by_hash` holds the hash and the
/// number of each, sorted by hash; names of the same hash are compared.
fn series_of(series: &mut Vec<String>, by_hash: &mut Vec<(u32, u16)>, name: &str) -> u16 {
    let key = hash(name);
    let mut at = by_hash.partition_point(|&(other, _)| other < key);
    while let Some(&(other, number)) = by_hash.get(at) {
        if other != key {
            break;
        }
        if series.get(usize::from(number)).is_some_and(|s| s == name) {
            return number;
        }
        at += 1;
    }
    if series.len() >= MAX_SERIES {
        return NO_SERIES;
    }
    by_hash.insert(at, (key, series.len() as u16));
    series.push(name.to_string());
    (series.len() - 1) as u16
}

/// A hash of `path`, to find a book of the index by its path: the low
/// bits of the one the catalog's log keys changes by
/// ([`catalog::changes`]).
pub fn hash(path: &str) -> u32 {
    paperpaw_digest::fnv1a64(path) as u32
}

/// `title` without a leading article, as libraries sort it ("The Hound
/// of the Baskervilles" under H).
pub fn without_article(title: &str) -> &str {
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
pub fn sort_key(title: &str) -> [u8; KEY] {
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

/// The surname of the first of `authors`: before the comma of "Doyle,
/// Arthur Conan", else the last word.
pub fn surname(authors: &str) -> &str {
    let first = authors.split([';', '&']).next().unwrap_or("").trim();
    match first.split_once(',') {
        Some((surname, _)) => surname.trim(),
        None => first.rsplit(' ').next().unwrap_or(first),
    }
}

/// The index; see the [module documentation](self).
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Index {
    pub items: Vec<Item>,
    /// The names of the series, as [`Item::series`] counts them.
    pub series: Vec<String>,
    /// Books in the catalog (but the archive's): more than
    /// [`Index::items`] where some were left out.
    pub total: usize,
    /// Books in the archive folder, left out.
    pub archived: usize,
    /// Changed since it was kept in a file (`Index::save`).
    pub dirty: bool,
    /// Of which version of the library's books it is
    /// ([`super::Library::version`]): where its keys hold.
    pub version: Option<u64>,
}

impl Index {
    /// Whether there are books at all (in the archive too).
    pub fn has_books(&self) -> bool {
        !self.items.is_empty() || self.archived > 0
    }

    /// Makes the index from the catalog: at most `cap` books, those in the
    /// folder `archive` left out (counted).
    pub fn build(storage: &dyn Storage, cap: usize, archive: &str) -> Index {
        Index::of_catalog(
            storage,
            cap,
            archive,
            &opened(storage),
            catalog_version(storage),
        )
    }

    /// [`Index::collect`] of the catalog's books, read once: there are no
    /// more books than the catalog has lines, which are counted without
    /// reading the books.
    pub(crate) fn of_catalog(
        storage: &dyn Storage,
        cap: usize,
        archive: &str,
        opened: &[u32],
        version: Option<u64>,
    ) -> Index {
        let lines = catalog::count_lines(storage, &catalog::catalog_path()).ok();
        Index::collect_at_most(cap, archive, opened, version, lines, &mut |f| {
            catalog::each_at(storage, f);
        })
    }

    /// Makes the index from `books` (any library's, read twice: counted,
    /// then taken): at most `cap`, those in the folder `archive` left out
    /// (counted), `opened` the books read last (their paths [`hash`]ed,
    /// the last first), of the library's `version`.
    pub fn collect(
        cap: usize,
        archive: &str,
        opened: &[u32],
        version: Option<u64>,
        books: Books<'_>,
    ) -> Index {
        Index::collect_at_most(cap, archive, opened, version, None, books)
    }

    /// [`Index::collect`], `books` read once where it is known that there
    /// are at most `most` (where not, they are counted first).
    fn collect_at_most(
        cap: usize,
        archive: &str,
        opened: &[u32],
        version: Option<u64>,
        most: Option<usize>,
        books: Books<'_>,
    ) -> Index {
        let archive = if archive.is_empty() {
            String::new()
        } else {
            format!("{archive}/")
        };
        let in_archive = |path: &str| !archive.is_empty() && path.starts_with(&archive);
        // As many as there are books: a list grown by doubling would hold
        // twice that for a moment.
        let count = most.unwrap_or_else(|| {
            let mut count = 0;
            books(&mut |_, record| count += usize::from(!in_archive(&record.path)));
            count
        });
        let mut items = Vec::with_capacity(count.min(cap));
        let mut series: Vec<String> = Vec::new();
        // The series' hashes and numbers, sorted by hash: found without
        // comparing a book's series with every one before it.
        let mut by_hash: Vec<(u32, u16)> = Vec::new();
        let (mut archived, mut total) = (0, 0);
        books(&mut |key, record| {
            if in_archive(&record.path) {
                archived += 1;
                return;
            }
            total += 1;
            let Ok(key) = u32::try_from(key) else {
                return;
            };
            if items.len() >= cap {
                return;
            }
            let of = if record.series.is_empty() {
                NO_SERIES
            } else {
                series_of(&mut series, &mut by_hash, &record.series)
            };
            let recent = rank(opened, hash(&record.path));
            items.push(Item::new(key, &record, of, recent));
        });
        Index {
            total: total.max(items.len()),
            archived,
            items,
            series,
            dirty: true,
            version,
        }
    }

    /// The book of item `i`, read from the catalog.
    pub fn record(&self, storage: &dyn Storage, i: usize) -> Option<Record> {
        catalog::read_at(storage, u64::from(self.items.get(i)?.offset))
    }

    /// Takes the order of the books read last, and what was noted beside
    /// the catalog (how far read, favourite, removed), anew: after
    /// reading, a few short files read, not the catalog.
    pub fn refresh(&mut self, storage: &dyn Storage) {
        let opened = opened(storage);
        let changes = catalog::changes(storage);
        for item in &mut self.items {
            let recent = rank(&opened, item.hash);
            let (mut progress, mut flags) = (item.progress, item.flags);
            if let Some(change) = changes.get_short(item.hash) {
                match change {
                    Some(status) => {
                        progress = status.progress.unwrap_or(NONE);
                        flags = (u8::from(status.favourite) * FAVOURITE)
                            | (u8::from(status.opened) * OPENED);
                    }
                    None => flags |= REMOVED,
                }
            }
            self.dirty |= (item.recent, item.progress, item.flags) != (recent, progress, flags);
            (item.recent, item.progress, item.flags) = (recent, progress, flags);
        }
    }

    /// The books `show` shows, as indices of [`Index::items`]: the books
    /// read last first (`by_recent`), the others by title.
    pub fn order(&self, show: Show, by_recent: bool) -> Vec<u16> {
        let mut order: Vec<u16> = (0..self.items.len() as u16)
            .filter(|&i| self.items[usize::from(i)].shown(show))
            .collect();
        paperpaw_sort::sort_unstable_by_key(&mut order, |&i| {
            let item = &self.items[usize::from(i)];
            let recent = if by_recent { item.recent } else { NEVER };
            (recent, item.key, item.offset)
        });
        order
    }

    /// For each series begun (a book of it opened or read), its unread
    /// book of the lowest number never opened: the series read last
    /// first (`by_recent`), else by title.
    pub fn next_in_series(&self, by_recent: bool) -> Vec<u16> {
        let count = self.series.len();
        // Per series: when a book of it was read last, and its next book.
        let mut last = alloc::vec![None::<u8>; count];
        let mut next: Vec<Option<u16>> = alloc::vec![None; count];
        for (i, item) in self.items.iter().enumerate() {
            let at = usize::from(item.series);
            if at >= count {
                continue;
            }
            if !item.shown(Show::Unread) {
                last[at] = Some(last[at].map_or(item.recent, |l| l.min(item.recent)));
            } else {
                let better = next[at].is_none_or(|n| {
                    let other = &self.items[usize::from(n)];
                    (item.number, item.key) < (other.number, other.key)
                });
                if better {
                    next[at] = Some(i as u16);
                }
            }
        }
        let mut order: Vec<u16> = (0..count)
            .filter(|&s| last[s].is_some())
            .filter_map(|s| next[s])
            .collect();
        paperpaw_sort::sort_unstable_by_key(&mut order, |&i| {
            let item = &self.items[usize::from(i)];
            let recent = match last.get(usize::from(item.series)) {
                Some(Some(recent)) if by_recent => *recent,
                _ => NEVER,
            };
            (recent, item.key, item.offset)
        });
        order
    }

    /// `order` with the books of each series as one entry (a stack) at
    /// the place of its first book there: the entries, and how many books
    /// each stands for. Books being read stay on their own.
    pub fn stacked(&self, order: &[u16]) -> (Vec<u16>, Vec<u16>) {
        let mut entries: Vec<u16> = Vec::new();
        let mut counts: Vec<u16> = Vec::new();
        // Where each series' stack is among the entries.
        let mut stacks: Vec<(u16, usize)> = Vec::new();
        for &i in order {
            let item = &self.items[usize::from(i)];
            let series = if (1..100).contains(&item.progress) {
                NO_SERIES
            } else {
                item.series
            };
            if series != NO_SERIES
                && let Some(&(_, at)) = stacks.iter().find(|(s, _)| *s == series)
            {
                counts[at] += 1;
                continue;
            }
            if series != NO_SERIES {
                stacks.push((series, entries.len()));
            }
            entries.push(i);
            counts.push(1);
        }
        (entries, counts)
    }

    /// Notes the progress of the book whose path hashes to `hash`, e.g.
    /// as a page of a view read it.
    pub fn set_progress(&mut self, hash: u32, progress: Option<u8>) {
        let progress = progress.unwrap_or(NONE);
        if let Some(item) = self.items.iter_mut().find(|i| i.hash == hash)
            && item.progress != progress
        {
            item.progress = progress;
            self.dirty = true;
        }
    }

    /// Keeps the index in the file `file`, to be read again
    /// ([`Index::load`]) instead of made anew, so a view holds no memory
    /// for it meanwhile; `archive` is the folder it was made without.
    /// Written a piece at a time; an error where it could not be (an
    /// index not made yet: [`StorageError::InvalidData`](crate::StorageError::InvalidData)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn save(
        &self,
        storage: &dyn Storage,
        file: &str,
        archive: &str,
    ) -> Result<(), crate::data::Error> {
        self.write(storage, file, archive)
    }

    /// [`Index::save`], for the library's service.
    pub(crate) fn write(
        &self,
        storage: &dyn Storage,
        file: &str,
        archive: &str,
    ) -> Result<(), crate::data::Error> {
        let Some(stamp) = self.version else {
            return Err(crate::StorageError::InvalidData.into());
        };
        let mut head = Vec::with_capacity(HEAD);
        head.extend_from_slice(MAGIC);
        head.extend_from_slice(&stamp.to_le_bytes());
        head.extend_from_slice(&hash(archive).to_le_bytes());
        head.extend_from_slice(&(self.archived as u32).to_le_bytes());
        head.extend_from_slice(&(self.total as u32).to_le_bytes());
        head.extend_from_slice(&(self.items.len() as u32).to_le_bytes());
        head.extend_from_slice(&(self.series.len() as u16).to_le_bytes());
        storage.write(file, &head)?;
        let mut piece: Vec<u8> = Vec::with_capacity(64 * ITEM_BYTES);
        for chunk in self.items.chunks(64) {
            piece.clear();
            for item in chunk {
                piece.extend_from_slice(&item.offset.to_le_bytes());
                piece.extend_from_slice(&item.hash.to_le_bytes());
                piece.extend_from_slice(&item.key);
                piece.extend_from_slice(&item.series.to_le_bytes());
                piece.extend_from_slice(&item.number.to_le_bytes());
                piece.push(item.progress);
                piece.push(item.recent);
                piece.push(item.flags);
            }
            storage.append(file, &piece)?;
        }
        for name in &self.series {
            piece.clear();
            let name = &name.as_bytes()[..name.len().min(usize::from(u16::MAX))];
            piece.extend_from_slice(&(name.len() as u16).to_le_bytes());
            piece.extend_from_slice(name);
            storage.append(file, &piece)?;
        }
        Ok(())
    }

    /// The index kept by `Index::save` in `file`, if it is of the
    /// library's `version` there is now ([`super::Library::version`];
    /// [`catalog_version`] for the catalog) and made without the folder
    /// `archive`.
    pub fn load(storage: &dyn Storage, file: &str, archive: &str, version: u64) -> Option<Index> {
        let stamp = version;
        let mut head = [0u8; HEAD];
        if storage.read_at(file, 0, &mut head).ok()? != head.len() || &head[..4] != MAGIC {
            return None;
        }
        let u32_at =
            |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        let mut kept = [0u8; 8];
        kept.copy_from_slice(&head[4..12]);
        if u64::from_le_bytes(kept) != stamp || u32_at(12) != hash(archive) {
            return None;
        }
        let archived = u32_at(16) as usize;
        let total = u32_at(20) as usize;
        let count = u32_at(24) as usize;
        let series = usize::from(u16::from_le_bytes([head[28], head[29]]));
        let mut items = Vec::new();
        items.try_reserve_exact(count).ok()?;
        let mut at = head.len() as u64;
        let mut buf = [0u8; 64 * ITEM_BYTES];
        while items.len() < count {
            let n = (count - items.len()).min(64);
            let want = &mut buf[..n * ITEM_BYTES];
            if storage.read_at(file, at, want).ok()? != want.len() {
                return None;
            }
            at += want.len() as u64;
            for b in want.as_chunks::<ITEM_BYTES>().0 {
                let word = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
                let mut key = [0u8; KEY];
                key.copy_from_slice(&b[8..16]);
                items.push(Item {
                    offset: word(0),
                    hash: word(4),
                    key,
                    series: u16::from_le_bytes([b[16], b[17]]),
                    number: u16::from_le_bytes([b[18], b[19]]),
                    progress: b[20],
                    recent: b[21],
                    flags: b[22],
                });
            }
        }
        let mut names = Vec::with_capacity(series);
        for _ in 0..series {
            let mut len = [0u8; 2];
            if storage.read_at(file, at, &mut len).ok()? != 2 {
                return None;
            }
            let len = usize::from(u16::from_le_bytes(len));
            let mut name = alloc::vec![0u8; len];
            if storage.read_at(file, at + 2, &mut name).ok()? != len {
                return None;
            }
            at += 2 + len as u64;
            names.push(String::from_utf8(name).ok()?);
        }
        Some(Index {
            items,
            series: names,
            total,
            archived,
            dirty: false,
            version: Some(version),
        })
    }
}

/// The books opened last, their paths [`hash`]ed, the last first.
pub fn opened(storage: &dyn Storage) -> Vec<u32> {
    super::opened_books(storage)
        .iter()
        .map(|p| hash(p))
        .collect()
}

/// The place of the book hashing to `hash` among the books read last.
fn rank(opened: &[u32], hash: u32) -> u8 {
    opened
        .iter()
        .position(|o| *o == hash)
        .map_or(NEVER, |r| r.min(usize::from(NEVER) - 1) as u8)
}

/// Which catalog there is, as an index of it keeps it
/// ([`Index::version`]): its generation and length (not its log's, which
/// [`Index::refresh`] reads).
pub fn catalog_version(storage: &dyn Storage) -> Option<u64> {
    Some((catalog::generation(storage)? << 40) ^ catalog::size(storage)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::library::Book;
    use crate::library::catalog::Status;

    #[test]
    fn titles_sort_without_articles() {
        assert_eq!(&sort_key("The Hound of the Baskervilles"), b"hound of");
        assert_eq!(&sort_key("Émile Zola"), b"emile zo");
        assert!(sort_key("Book 000") < sort_key("Book 019"));
        assert_eq!(letter(without_article("Die Verwandlung")), b'V');
        assert_eq!(letter("1984"), b'#');
        assert_eq!(surname("Doyle, Arthur Conan"), "Doyle");
        assert_eq!(surname("Ann Smith & Bob Jones"), "Smith");
        assert_eq!(number("2.5"), 25);
        assert_eq!(number("x"), u16::MAX);
    }

    /// A catalog of `count` books, `books/Book 0001.txt` and on, every
    /// third of a series, the last two in the archive folder.
    fn catalog(count: usize) -> MemoryStorage {
        let storage = MemoryStorage::new();
        let records: Vec<Record> = (1..=count)
            .map(|n| {
                let folder = if n + 2 > count {
                    "books/Archive"
                } else {
                    "books"
                };
                let book = Book::default()
                    .title(Some(format!("Book {n:04}")))
                    .author(Some(String::from("Ann")))
                    .series((n % 3 == 0).then(|| String::from("Tales")))
                    .series_index((n % 3 == 0).then(|| format!("{n}")));
                let status = Status::new((n % 4 == 1).then_some(40), n == 2, n % 4 == 1);
                Record::of(&format!("{folder}/Book {n:04}.txt"), &book).with_status(status)
            })
            .collect();
        catalog::write(&storage, &records).unwrap();
        storage
    }

    #[test]
    fn an_index_holds_at_most_its_cap_and_counts_the_others() {
        // A book costs 24 bytes; the X4's 2000 at most 48 KB.
        assert!(core::mem::size_of::<Item>() <= 24);
        assert!(cap(MemoryClass::Small) * core::mem::size_of::<Item>() <= 48 * 1024);
        let storage = catalog(12);
        let index = Index::build(&storage, 5, "books/Archive");
        assert_eq!(index.items.len(), 5);
        assert!(index.items.capacity() <= 5, "made no larger than it holds");
        assert_eq!(index.archived, 2);
        assert_eq!(index.total, 10);
    }

    #[test]
    fn filters_come_from_the_catalog_at_once() {
        let storage = catalog(10);
        let index = Index::build(&storage, 100, "books/Archive");
        // Books 1 and 5 begun, book 2 a favourite.
        assert_eq!(index.order(Show::Reading, false).len(), 2);
        assert_eq!(index.order(Show::Favourites, false).len(), 1);
        assert_eq!(index.order(Show::Unread, false).len(), 6);
        assert_eq!(index.order(Show::All, false).len(), 8);
        let record = index.record(&storage, 0).unwrap();
        assert_eq!(record.title, "Book 0001");
        // Changed after: taken anew.
        catalog::note(
            &storage,
            "books/Book 0002.txt",
            Status::new(Some(100), true, true),
        )
        .unwrap();
        let mut index = index;
        index.refresh(&storage);
        assert_eq!(index.order(Show::Finished, false).len(), 1);
    }

    #[test]
    fn the_next_book_of_each_series_begun() {
        let storage = MemoryStorage::new();
        let book = |n: u32| {
            Book::default()
                .title(Some(format!("Tale {n}")))
                .series(Some(String::from("Tales")))
                .series_index(Some(format!("{n}")))
        };
        let status = |p: Option<u8>| Status::new(p, false, p.is_some());
        catalog::write(
            &storage,
            &[
                Record::of("b/4.txt", &book(4)).with_status(status(None)),
                Record::of("b/1.txt", &book(1)).with_status(status(Some(100))),
                Record::of("b/3.txt", &book(3)).with_status(status(None)),
                Record::of("b/2.txt", &book(2)).with_status(status(Some(42))),
                Record::of("b/x.txt", &Book::default().series(Some("Other".into())))
                    .with_status(status(None)),
            ],
        )
        .unwrap();
        let index = Index::build(&storage, 100, "");
        let next = index.next_in_series(false);
        assert_eq!(next.len(), 1, "the other series is not begun");
        assert_eq!(
            index.record(&storage, usize::from(next[0])).unwrap().path,
            "b/3.txt"
        );
        // As covers: the series a stack, the book being read apart.
        let all = index.order(Show::All, false);
        let (entries, counts) = index.stacked(&all);
        assert_eq!(entries.len(), 3);
        assert!(counts.contains(&3), "{counts:?}");
    }

    #[test]
    fn a_kept_index_is_read_back_while_its_catalog_is_the_same() {
        let storage = catalog(9);
        let mut index = Index::build(&storage, 100, "books/Archive");
        let version = catalog_version(&storage).unwrap();
        assert_eq!(index.version, Some(version));
        index.set_progress(index.items[2].hash, Some(42));
        assert!(index.save(&storage, "kept.bin", "books/Archive").is_ok());
        let kept = Index::load(&storage, "kept.bin", "books/Archive", version).unwrap();
        assert_eq!(kept.items, index.items);
        assert_eq!(kept.series, index.series);
        assert_eq!((kept.total, kept.archived), (index.total, index.archived));
        // Another archive folder: made anew.
        assert!(Index::load(&storage, "kept.bin", "books/Old", version).is_none());
        // A change noted beside the catalog: still of it, the change
        // taken by refreshing; a book removed is shown nowhere.
        catalog::note(
            &storage,
            "books/Book 0001.txt",
            Status::new(Some(100), false, true),
        )
        .unwrap();
        catalog::forget(&storage, "books/Book 0003.txt").unwrap();
        assert_eq!(catalog_version(&storage), Some(version));
        let mut kept = Index::load(&storage, "kept.bin", "books/Archive", version).unwrap();
        kept.refresh(&storage);
        assert_eq!(kept.order(Show::Finished, false).len(), 1);
        assert_eq!(kept.order(Show::All, false).len(), index.items.len() - 1);
        // The catalog written anew, even as long as it was: another
        // version, where its lines start may have moved.
        let records: Vec<Record> = {
            let mut all = Vec::new();
            catalog::each(&storage, &mut |r| all.push(r));
            all
        };
        catalog::write(&storage, &records).unwrap();
        assert_ne!(catalog_version(&storage), Some(version));
    }

    /// A library of one's own makes its index of its own books.
    #[test]
    fn any_library_makes_an_index_of_its_books() {
        let books: Vec<(u64, Record)> = (0..4u64)
            .map(|n| {
                let book = Book::default().title(Some(format!("Book {n}")));
                (n * 10, Record::of(&format!("remote/{n}"), &book))
            })
            .collect();
        let index = Index::collect(3, "", &[], Some(7), &mut |f| {
            for (key, record) in &books {
                f(*key, record.clone());
            }
        });
        assert_eq!(index.items.len(), 3, "at most its cap");
        assert_eq!(index.total, 4);
        assert_eq!(index.version, Some(7));
        assert_eq!(index.items[1].offset, 10);
        let item = Item::new(5, &books[0].1, NO_SERIES, NEVER);
        assert_eq!(item.hash, hash("remote/0"));
    }
}
