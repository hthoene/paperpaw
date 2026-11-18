//! The words, one a line in `words.txt`, oldest first:
//! `word<TAB>language<TAB>section<TAB>offset<TAB>book path<TAB>book title<TAB>sentence`.
//!
//! The file is only ever streamed, a line at a time, so a long list needs
//! no more memory than a page of it. It is written anew beside itself
//! (`words.tmp`) and put in its place only once complete, so a failed
//! read or write, or a power cut, never loses words.
//!
//! Every allocation is fallible: where the heap refuses one, the call ends
//! with [`StorageError::TooLarge`], nothing half written, never an abort.
//! The layer works on any [`Storage`]: on the host's data folder (root 0,
//! through [`crate::files::DataFolder`]) as on a card.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Storage, StorageError};

use crate::lines::{self, copy, folded, push, push_number};

/// Words kept at most; the oldest go.
pub(crate) const MAX_WORDS: usize = 2000;
/// Longest sentence kept, in characters.
const MAX_SENTENCE: usize = 300;
/// Bytes written at a time.
const CHUNK: usize = 1024;

/// The list: the plugin's folder under the card (interface 6's names; the
/// host's root 0 is that folder, see [`crate::files::DataFolder`]).
const FILE: &str = ".paperpaw/plugins/vocabulary/words.txt";
/// The list being written anew.
const TEMPORARY: &str = ".paperpaw/plugins/vocabulary/words.tmp";

pub(crate) const fn file() -> &'static str {
    FILE
}

#[cfg(test)]
fn temporary() -> &'static str {
    TEMPORARY
}

/// A word looked up, where and in which sentence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Word {
    pub word: String,
    pub language: String,
    pub section: u32,
    pub offset: u32,
    pub path: String,
    pub title: String,
    pub sentence: String,
}

/// Appends `text` as a field: tabs and line breaks become spaces; at most
/// `limit` characters.
fn push_field(out: &mut String, text: &str, limit: usize) -> Result<(), StorageError> {
    let text = match text.char_indices().nth(limit) {
        Some((end, _)) => text.get(..end).unwrap_or(text),
        None => text,
    };
    out.try_reserve(text.len())
        .map_err(|_| StorageError::TooLarge)?;
    out.extend(text.chars().map(|c| {
        if matches!(c, '\t' | '\n' | '\r') {
            ' '
        } else {
            c
        }
    }));
    Ok(())
}

impl Word {
    /// What [`Word::push_line`] may add at most, in bytes.
    fn line_size(&self) -> usize {
        [&self.word, &self.language, &self.path, &self.title]
            .iter()
            .map(|f| f.len())
            .fold(0usize, usize::saturating_add)
            .saturating_add(MAX_SENTENCE * 4 + 32)
    }

    /// Appends the word's line, with its line break, to `out`.
    pub(crate) fn push_line(&self, out: &mut String) -> Result<(), StorageError> {
        out.try_reserve(self.line_size())
            .map_err(|_| StorageError::TooLarge)?;
        push_field(out, &self.word, usize::MAX)?;
        push(out, "\t")?;
        push_field(out, &self.language, usize::MAX)?;
        push(out, "\t")?;
        push_number(out, self.section)?;
        push(out, "\t")?;
        push_number(out, self.offset)?;
        push(out, "\t")?;
        push_field(out, &self.path, usize::MAX)?;
        push(out, "\t")?;
        push_field(out, &self.title, usize::MAX)?;
        push(out, "\t")?;
        push_field(out, &self.sentence, MAX_SENTENCE)?;
        push(out, "\n")
    }

    /// The word's line, with its line break.
    #[cfg(test)]
    pub(crate) fn line(&self) -> Result<String, StorageError> {
        let mut out = String::new();
        self.push_line(&mut out)?;
        Ok(out)
    }

    /// The word `line` holds; `None` where it is none (a line cut off by
    /// a power cut, say); an error where the heap refuses its strings.
    pub(crate) fn parse(line: &str) -> Result<Option<Word>, StorageError> {
        if Word::kinds(line).is_none() {
            return Ok(None);
        }
        let mut f = line.splitn(7, '\t');
        let mut next = || copy(f.next().unwrap_or_default());
        let word = next()?;
        let language = next()?;
        let section = f.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let offset = f.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let mut next = || copy(f.next().unwrap_or_default());
        Ok(Some(Word {
            word,
            language,
            section,
            offset,
            path: next()?,
            title: next()?,
            sentence: next()?,
        }))
    }

    /// The language and the book's path of `line` where it is a word
    /// ([`Word::parse`] reads it), told without making one.
    fn kinds(line: &str) -> Option<(&str, &str)> {
        let mut f = line.splitn(7, '\t');
        let number = |f: Option<&str>| f?.parse::<u32>().ok();
        f.next()?;
        let language = f.next()?;
        number(f.next())?;
        number(f.next())?;
        let path = f.next()?;
        f.next()?;
        Some((language, path))
    }

    /// Whether `line` (a word's line) is the word `word` in `language`,
    /// whatever its case, told without making a word.
    fn is_line_of(line: &str, word: &str, language: &str) -> bool {
        Word::kinds(line).is_some() && {
            let mut f = line.splitn(3, '\t');
            matches!((f.next(), f.next()), (Some(w), Some(l))
                if l == language && folded(w).eq(folded(word)))
        }
    }
}

/// Calls `line` with each line of `path`, in order (a missing file has
/// none); `Err` where the file could not be read to its end.
pub(crate) fn each_line(
    storage: &dyn Storage,
    path: &str,
    mut line: impl FnMut(&str),
) -> Result<(), StorageError> {
    lines::each_line(storage, path, |_, l| {
        line(l);
        ControlFlow::Continue(())
    })
}

/// Calls `line` with each line of the list that is a word, oldest first;
/// `Err` where the list could not be read to its end. Lines that are no
/// word (cut off by a power cut) are skipped, and the words are numbered
/// without them.
fn each_word_line(storage: &dyn Storage, mut line: impl FnMut(&str)) -> Result<(), StorageError> {
    each_line(storage, FILE, |l| {
        if Word::kinds(l).is_some() {
            line(l);
        }
    })
}

/// After a power cut while the words were written anew: the new list where
/// the old one is gone already (file systems that replace a file by
/// removing it first), else the unfinished one goes. Called at start.
pub(crate) fn recover(storage: &dyn Storage) {
    if !storage.exists(TEMPORARY) {
        return;
    }
    if storage.size(FILE) == Err(StorageError::NotFound) {
        let _ = storage.rename(TEMPORARY, FILE);
    } else {
        let _ = storage.remove(TEMPORARY);
    }
}

/// The size of the file of words (0 where there is none), which tells that the
/// words changed.
pub(crate) fn size(storage: &dyn Storage) -> u64 {
    storage.size(FILE).unwrap_or(0)
}

/// How many words there are.
pub(crate) fn count(storage: &dyn Storage) -> Result<usize, StorageError> {
    let mut n = 0;
    each_word_line(storage, |_| n += 1)?;
    Ok(n)
}

/// Books and languages told apart at most in a [`Summary`].
const MOST_KINDS: usize = 32;

/// The words at a glance: how many, the newest, from how many books and
/// in how many languages (up to [`MOST_KINDS`] of each).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub count: usize,
    pub newest: Option<Word>,
    pub books: usize,
    pub languages: usize,
}

/// Remembers `kind` in `seen` where it is new and there is room.
fn note(seen: &mut Vec<String>, kind: &str) -> Result<(), StorageError> {
    if !kind.is_empty() && seen.len() < MOST_KINDS && !seen.iter().any(|s| s == kind) {
        seen.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
        seen.push(copy(kind)?);
    }
    Ok(())
}

/// The words at a glance, read in one pass.
pub(crate) fn summary(storage: &dyn Storage) -> Result<Summary, StorageError> {
    let mut summary = Summary::default();
    let mut books: Vec<String> = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    // The newest word's line, made a word once all are read.
    let mut newest = String::new();
    let mut failed = Ok(());
    each_word_line(storage, |l| {
        if failed.is_err() {
            return;
        }
        let Some((language, path)) = Word::kinds(l) else {
            return;
        };
        summary.count += 1;
        failed = note(&mut books, path)
            .and_then(|()| note(&mut languages, language))
            .and_then(|()| {
                newest.clear();
                push(&mut newest, l)
            });
    })?;
    failed?;
    if summary.count > 0 {
        summary.newest = Word::parse(&newest)?;
    }
    summary.books = books.len();
    summary.languages = languages.len();
    Ok(summary)
}

/// The words with numbers in `from..to` (0 the oldest).
pub(crate) fn range(
    storage: &dyn Storage,
    from: usize,
    to: usize,
) -> Result<Vec<(usize, Word)>, StorageError> {
    let mut words = Vec::new();
    let mut n = 0;
    let mut failed = Ok(());
    // Only the words asked for are made; the others only counted, and
    // those after them not read.
    lines::each_line(storage, FILE, |_, l| {
        if n >= to {
            return ControlFlow::Break(());
        }
        if Word::kinds(l).is_none() {
            return ControlFlow::Continue(());
        }
        if n >= from {
            let made = words
                .try_reserve(1)
                .map_err(|_| StorageError::TooLarge)
                .and_then(|()| Word::parse(l));
            match made {
                Ok(Some(word)) => words.push((n, word)),
                Ok(None) => {}
                Err(e) => {
                    failed = Err(e);
                    return ControlFlow::Break(());
                }
            }
        }
        n += 1;
        ControlFlow::Continue(())
    })?;
    failed?;
    Ok(words)
}

/// Writes the words for which `keep` holds anew, beside the list, and only
/// then puts them in its place: where anything cannot be read or written,
/// the list stays as it was. The lines are written as they are.
fn rewrite(storage: &dyn Storage, mut keep: impl FnMut(&str) -> bool) -> Result<(), StorageError> {
    let mut written = || {
        storage.write(TEMPORARY, &[])?;
        let mut out = String::new();
        // A line is at most `MAX_LINE` bytes and a piece is `CHUNK * 4` and
        // one line: nothing grows past this.
        out.try_reserve_exact(CHUNK * 4 + lines::MAX_LINE + 1)
            .map_err(|_| StorageError::TooLarge)?;
        let mut failed = Ok(());
        lines::each_line(storage, FILE, |_, l| {
            if Word::kinds(l).is_some() && keep(l) {
                failed = push(&mut out, l).and_then(|()| push(&mut out, "\n"));
                if failed.is_ok() && out.len() >= CHUNK * 4 {
                    failed = storage.append(TEMPORARY, out.as_bytes());
                    out.clear();
                }
            }
            if failed.is_ok() {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        })?;
        failed?;
        storage.append(TEMPORARY, out.as_bytes())
    };
    if let Err(e) = written() {
        let _ = storage.remove(TEMPORARY);
        return Err(e);
    }
    let renamed = storage.rename(TEMPORARY, FILE);
    // A failed rename may have taken the list away (FAT removes it
    // first): then the new one is all there is, taken at start.
    if renamed.is_err() && storage.exists(FILE) {
        let _ = storage.remove(TEMPORARY);
    }
    renamed
}

/// Whether the list ends with a line break (or is empty): else a line cut
/// off by a power cut would run into the word appended next.
fn ends_whole(storage: &dyn Storage) -> Result<bool, StorageError> {
    let size = match storage.size(FILE) {
        Ok(size) => size,
        Err(StorageError::NotFound) => return Ok(true),
        Err(e) => return Err(e),
    };
    if size == 0 {
        return Ok(true);
    }
    let mut last = [0u8; 1];
    storage.read_at(FILE, size - 1, &mut last)?;
    Ok(last[0] == b'\n')
}

/// Adds `word` as the newest; an older entry of it goes, and the oldest
/// words beyond [`MAX_WORDS`]. Looked up again while it is the newest, it
/// stays as it is, with the sentence it was kept with first.
pub(crate) fn add(storage: &dyn Storage, word: &Word) -> Result<(), StorageError> {
    // The line first: where the heap refuses it, nothing is touched.
    let mut line = String::new();
    if !ends_whole(storage).unwrap_or(false) {
        // Where that cannot be told, an empty line (skipped) is safer.
        push(&mut line, "\n")?;
    }
    word.push_line(&mut line)?;
    let mut total = 0;
    let mut known = false;
    let mut newest = false;
    let read = each_word_line(storage, |l| {
        total += 1;
        newest = Word::is_line_of(l, &word.word, &word.language);
        known |= newest;
    });
    // A list that cannot be read is only added to, never written anew from
    // the part of it that was read.
    if read.is_ok() {
        if newest {
            return Ok(());
        }
        if known || total >= MAX_WORDS {
            let left = total - usize::from(known);
            let drop = (left + 1).saturating_sub(MAX_WORDS);
            let mut kept = 0;
            rewrite(storage, |l| {
                if Word::is_line_of(l, &word.word, &word.language) {
                    return false;
                }
                kept += 1;
                kept > drop
            })?;
        }
    }
    storage.append(FILE, line.as_bytes())
}

/// Deletes `word` (the same word in the same language, whatever its case),
/// wherever it is in the list by now.
pub(crate) fn remove(storage: &dyn Storage, word: &Word) -> Result<(), StorageError> {
    rewrite(storage, |l| {
        !Word::is_line_of(l, &word.word, &word.language)
    })
}

/// Appends `text` as a field of the export: in quotes where it has one
/// (the quote doubled), as flash card programs read text separated by tabs.
fn push_export_field(out: &mut String, text: &str) -> Result<(), StorageError> {
    if !text.contains('"') {
        return push(out, text);
    }
    out.try_reserve(text.len().saturating_mul(2).saturating_add(2))
        .map_err(|_| StorageError::TooLarge)?;
    out.push('"');
    for c in text.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    Ok(())
}

/// Writes all words to `path` for flash card programs (Anki and others):
/// a line each with the word, its sentence and the book, separated by
/// tabs, oldest first, after the lines that tell Anki how to read them.
pub(crate) fn export(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    // The buffer first: where the heap refuses it, nothing is written (an
    // export already there is not cut to its header).
    let mut out = String::new();
    out.try_reserve_exact(CHUNK * 4 + lines::MAX_LINE * 2)
        .map_err(|_| StorageError::TooLarge)?;
    storage.write(path, b"#separator:tab\n#html:false\n")?;
    let mut failed = Ok(());
    each_word_line(storage, |l| {
        if failed.is_err() {
            return;
        }
        // The fields of a line the list holds: as long as the line.
        let mut f = l.splitn(7, '\t');
        let word = f.next().unwrap_or_default();
        let title = f.nth(4).unwrap_or_default();
        let sentence = f.next().unwrap_or_default();
        failed = push_export_field(&mut out, word)
            .and_then(|()| push(&mut out, "\t"))
            .and_then(|()| push_export_field(&mut out, sentence))
            .and_then(|()| push(&mut out, "\t"))
            .and_then(|()| push_export_field(&mut out, title))
            .and_then(|()| push(&mut out, "\n"));
        if failed.is_ok() && out.len() >= CHUNK * 4 {
            failed = storage.append(path, out.as_bytes());
            out.clear();
        }
    })?;
    failed?;
    storage.append(path, out.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;
    use core::cell::Cell;
    use paperpaw_sdk::{DirEntry, MemoryStorage};

    fn word(word: &str) -> Word {
        Word {
            word: word.to_string(),
            language: "en".to_string(),
            section: 2,
            offset: 40,
            path: "books/a.epub".to_string(),
            title: "A".to_string(),
            sentence: format!("The {word}\tis here."),
        }
    }

    fn words(storage: &dyn Storage) -> Vec<String> {
        range(storage, 0, usize::MAX)
            .unwrap()
            .into_iter()
            .map(|(_, w)| w.word)
            .collect()
    }

    /// A card that fails on purpose: reads of the words from byte
    /// `read_fails_at` on, appends to the new list after `appends` of
    /// them, and with `rename_drops_target` renames that remove the file
    /// they replace and then fail (as FAT may).
    struct Failing {
        inner: MemoryStorage,
        read_fails_at: Option<u64>,
        appends: Cell<Option<usize>>,
        rename_drops_target: bool,
    }

    impl Failing {
        fn new(inner: MemoryStorage) -> Self {
            Self {
                inner,
                read_fails_at: None,
                appends: Cell::new(None),
                rename_drops_target: false,
            }
        }
    }

    impl Storage for Failing {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            if path == file() && self.read_fails_at.is_some_and(|at| offset >= at) {
                return Err(StorageError::Io);
            }
            self.inner.read_at(path, offset, buf)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            self.inner.size(path)
        }
        fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            self.inner.write(path, data)
        }
        fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
            if path == temporary()
                && let Some(left) = self.appends.get()
            {
                if left == 0 {
                    return Err(StorageError::Full);
                }
                self.appends.set(Some(left - 1));
            }
            self.inner.append(path, data)
        }
        fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.inner.list(dir, visit)
        }
        fn remove(&self, path: &str) -> Result<(), StorageError> {
            self.inner.remove(path)
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
            if self.rename_drops_target {
                let _ = self.inner.remove(to);
                return Err(StorageError::Io);
            }
            self.inner.rename(from, to)
        }
    }

    /// Many words, so that writing them anew takes several appends.
    fn many(n: usize) -> String {
        (0..n)
            .map(|i| word(&format!("w{i}")).line().unwrap())
            .collect()
    }

    #[test]
    fn adds_once_newest_last_and_removes() {
        let storage = MemoryStorage::new();
        add(&storage, &word("quire")).unwrap();
        add(&storage, &word("ink")).unwrap();
        add(&storage, &word("Quire")).unwrap();
        assert_eq!(count(&storage).unwrap(), 2);
        let all = range(&storage, 0, 10).unwrap();
        assert_eq!(all[0].1.word, "ink");
        assert_eq!(all[1].1.word, "Quire");
        assert_eq!(all[1].1.sentence, "The Quire is here.");
        remove(&storage, &word("ink")).unwrap();
        assert_eq!(words(&storage), ["Quire"]);
        export(&storage, "out.tsv").unwrap();
        let data = paperpaw_sdk::read_file(&storage, "out.tsv", 1024).unwrap();
        assert_eq!(
            data,
            b"#separator:tab\n#html:false\nQuire\tThe Quire is here.\tA\n"
        );
    }

    #[test]
    fn the_words_at_a_glance() {
        let storage = MemoryStorage::new();
        assert_eq!(summary(&storage).unwrap(), Summary::default());
        add(&storage, &word("quire")).unwrap();
        add(
            &storage,
            &Word {
                language: "de".to_string(),
                path: "books/b.epub".to_string(),
                ..word("Tinte")
            },
        )
        .unwrap();
        add(&storage, &word("ink")).unwrap();
        let s = summary(&storage).unwrap();
        assert_eq!((s.count, s.books, s.languages), (3, 2, 2));
        assert_eq!(s.newest.map(|w| w.word), Some("ink".to_string()));
    }

    #[test]
    fn keeps_at_most_the_newest() {
        let storage = MemoryStorage::new();
        storage.write(file(), many(MAX_WORDS).as_bytes()).unwrap();
        add(&storage, &word("new")).unwrap();
        assert_eq!(count(&storage).unwrap(), MAX_WORDS);
        let first = range(&storage, 0, 1);
        assert_eq!(first.unwrap()[0].1.word, "w1");
        assert_eq!(
            range(&storage, MAX_WORDS - 1, MAX_WORDS).unwrap()[0].1.word,
            "new"
        );
    }

    /// Writing the list anew fails partway (the card is full): the list
    /// stays whole and the unfinished copy goes.
    #[test]
    fn a_rewrite_that_fails_partway_keeps_the_list() {
        let list = many(300);
        let storage = Failing::new(MemoryStorage::new().with(file(), list.clone()));
        storage.appends.set(Some(1));
        assert_eq!(remove(&storage, &word("w0")), Err(StorageError::Full));
        assert_eq!(storage.inner.get(file()), Some(list.into_bytes()));
        assert!(!storage.exists(temporary()));
    }

    /// The help says how many words the list keeps: beyond that the
    /// oldest go, and nobody should find that out by losing them.
    #[test]
    fn the_help_says_how_many_words_are_kept() {
        let en = format!("{},{:03}", MAX_WORDS / 1000, MAX_WORDS % 1000);
        let de = format!("{MAX_WORDS}");
        for (text, number) in [
            (include_str!("../how/en.txt"), &en),
            (include_str!("../how/de.txt"), &de),
            (include_str!("../i18n/en.ini"), &en),
            (include_str!("../i18n/de.ini"), &de),
        ] {
            assert!(text.contains(&format!("{number} ")), "{number}: {text}");
        }
    }

    /// A rename that takes the list away and then fails: the new list is
    /// kept, as it is all there is, and taken at the next start.
    #[test]
    fn a_failed_rename_keeps_the_new_list() {
        let mut storage = Failing::new(MemoryStorage::new().with(file(), many(3)));
        storage.rename_drops_target = true;
        assert_eq!(remove(&storage, &word("w0")), Err(StorageError::Io));
        assert!(storage.exists(temporary()));
        recover(&storage.inner);
        assert_eq!(words(&storage.inner), ["w1", "w2"]);
    }

    /// The list cannot be read to its end: nothing is written anew from
    /// the part that was read, and a word looked up is still kept.
    #[test]
    fn a_list_read_only_in_part_is_not_written_anew() {
        let list = many(300);
        let mut storage = Failing::new(MemoryStorage::new().with(file(), list.clone()));
        storage.read_fails_at = Some(2048);
        assert_eq!(remove(&storage, &word("w299")), Err(StorageError::Io));
        assert_eq!(storage.inner.get(file()), Some(list.clone().into_bytes()));
        // Looked up again: only appended, the old entry stays.
        add(&storage, &word("w0")).unwrap();
        let mut expected = list.into_bytes();
        expected.push(b'\n');
        expected.extend_from_slice(word("w0").line().unwrap().as_bytes());
        assert_eq!(storage.inner.get(file()), Some(expected));
    }

    /// A power cut between removing the old list and renaming the new one
    /// (on FAT) leaves only the new one: it is taken at start. An
    /// unfinished new one beside the old list goes.
    #[test]
    fn the_new_list_is_taken_after_a_power_cut() {
        let storage = MemoryStorage::new().with(temporary(), word("kept").line().unwrap());
        recover(&storage);
        assert_eq!(words(&storage), ["kept"]);
        assert!(!storage.exists(temporary()));

        let storage = MemoryStorage::new()
            .with(file(), word("old").line().unwrap())
            .with(temporary(), "half a li");
        recover(&storage);
        assert_eq!(words(&storage), ["old"]);
        assert!(!storage.exists(temporary()));
    }

    /// A line cut off by a power cut is no word: it shifts no numbers,
    /// deleting deletes the word chosen, and the next word appended starts
    /// a line of its own.
    #[test]
    fn a_cut_line_shifts_nothing() {
        let list = format!("{}cut\ten\t1", word("first").line().unwrap());
        let storage = MemoryStorage::new().with(file(), list);
        add(&storage, &word("second")).unwrap();
        assert_eq!(count(&storage).unwrap(), 2);
        assert_eq!(words(&storage), ["first", "second"]);
        remove(&storage, &word("second")).unwrap();
        assert_eq!(words(&storage), ["first"]);
    }

    /// Counting and paging tell words from other lines as reading them
    /// does: a page numbers its words without the lines that are none.
    #[test]
    fn words_are_told_without_reading_them() {
        let lines = [
            "w\ten\t1\t2\tbooks/a.txt\tA\tsentence",
            "w\ten\t1\t2\tbooks/a.txt\tA",
            "w\ten\t1\t2\tbooks/a.txt",
            "w\ten\tx\t2\tbooks/a.txt\tA",
            "w\ten\t1\t-2\tbooks/a.txt\tA",
            "w\ten\t+1\t2\t\t\t",
            "",
            "cut\ten\t1",
        ];
        for line in lines {
            assert_eq!(
                Word::kinds(line).is_some(),
                Word::parse(line).unwrap().is_some(),
                "{line}"
            );
        }
        let list = format!("{}cut\ten\t1\n{}", many(3), many(2));
        let storage = MemoryStorage::new().with(file(), list);
        assert_eq!(count(&storage).unwrap(), 5);
        let page = range(&storage, 2, 4).unwrap();
        let numbered: Vec<(usize, &str)> =
            page.iter().map(|(n, w)| (*n, w.word.as_str())).collect();
        assert_eq!(numbered, [(2, "w2"), (3, "w0")]);
        assert_eq!(
            summary(&storage).unwrap().newest.map(|w| w.word),
            Some("w1".to_string())
        );
    }

    /// Deleting goes by the word, not by its number: the list may have
    /// changed since it was shown.
    #[test]
    fn deleting_finds_the_word_wherever_it_is() {
        let storage = MemoryStorage::new();
        for w in ["a", "b", "c"] {
            add(&storage, &word(w)).unwrap();
        }
        // "a" looked up again elsewhere: now the newest.
        add(&storage, &word("a")).unwrap();
        remove(&storage, &word("b")).unwrap();
        assert_eq!(words(&storage), ["c", "a"]);
    }

    /// The same word looked up again right after is not written anew.
    #[test]
    fn a_repeated_lookup_writes_nothing() {
        let storage = Failing::new(MemoryStorage::new());
        add(&storage, &word("quire")).unwrap();
        // Any rewrite would fail now.
        storage.appends.set(Some(0));
        let before = storage.inner.get(file());
        assert_eq!(add(&storage, &word("Quire")), Ok(()));
        assert_eq!(storage.inner.get(file()), before);
    }

    /// Quotes would make Anki read past the end of a field.
    #[test]
    fn the_export_quotes_quotes() {
        let storage = MemoryStorage::new();
        add(
            &storage,
            &Word {
                sentence: String::from("He said \"quire\"."),
                ..word("quire")
            },
        )
        .unwrap();
        export(&storage, "out.tsv").unwrap();
        let data = paperpaw_sdk::read_file(&storage, "out.tsv", 1024).unwrap();
        assert_eq!(
            data,
            b"#separator:tab\n#html:false\nquire\t\"He said \"\"quire\"\".\"\tA\n"
        );
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn words_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with(super::file(), include_str!("../testdata/words.txt.v1"));
        let mut words = alloc::vec::Vec::new();
        super::each_line(&storage, super::file(), |line| {
            words.extend(super::Word::parse(line).unwrap());
        })
        .unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "Gregor");
        assert_eq!(words[0].language, "de");
        assert_eq!((words[0].section, words[0].offset), (2, 1234));
        assert_eq!(words[0].title, "Die Verwandlung");
        assert_eq!(
            words[0].sentence,
            "Als Gregor Samsa eines Morgens erwachte."
        );
        assert_eq!(words[1].path, "books/alice.txt");
        assert_eq!(words[1].sentence, "");
    }
}

#[cfg(test)]
mod format_tests;
