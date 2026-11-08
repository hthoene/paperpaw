//! The vault's index: a file of its notes (when each changed, and where
//! it is), made by walking the vault a piece at a time, and a file of the
//! tags of its notes, made from that. Both are written as they are made
//! and read a line at a time, so the vault's size is a file's, never
//! memory's; the lists made from them (see `page`) hold a window.
//!
//! The files are in the plugin's data folder, under these names:
//!
//! ```text
//! index.txt   <modified>\t<path>             a line for each note
//! tags.txt    <path>\t<tag> <tag> ...        a line for each note with tags
//! state.txt   notes-vault 1, the vault's folder, when it was made (unix
//!             seconds, 0 if unknown), its notes, their stamp, tags made
//! ```
//!
//! A walk or a tagging that ends in an error is over: the caller starts anew.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::mem::stable_sort_by;
use paperpaw_ui_kit::text::cmp_lowercase;

use super::lines;
use super::note;
use super::tags;
use super::text::{buffer, copy, join, lossy, push, push_char, push_i64, push_u64};

/// The index of the notes.
pub const INDEX: &str = "index.txt";
/// The tags of the notes.
pub const TAGS: &str = "tags.txt";
/// What is known of the index.
pub const STATE: &str = "state.txt";
const INDEX_PART: &str = "index.part";
const TAGS_PART: &str = "tags.part";

/// Entries read from one folder in one step.
const PAGE: u64 = 64;
/// Notes of the index at most.
pub const MAX_NOTES: u32 = 50_000;
/// Folders below one another at most (the vault's folder is the first).
pub const MAX_DEPTH: u8 = 24;
/// Notes whose tags are read in one step.
pub const TAGGED_PER_STEP: usize = 3;
/// Distinct tags listed at most.
pub const MAX_TAGS: usize = 400;
/// The most the state file holds, in bytes.
const MAX_STATE: u64 = 2048;
/// A buffer of lines is written when it holds this much.
const FLUSH_AT: usize = 2048;

/// What is known of the index made last.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The vault's folder it was made for.
    pub folder: String,
    /// When it was made, in unix seconds; 0 where the time is not known.
    pub built: i64,
    pub notes: u32,
    /// Changes when a note comes, goes or changes: tags are made anew then.
    pub stamp: u64,
    /// The tags are made.
    pub tagged: bool,
}

impl State {
    /// The state kept in `data`, if it is there and whole.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn read(data: &dyn Storage) -> Result<Option<State>, Error> {
        let Ok(size) = data.size(STATE) else {
            return Ok(None);
        };
        if size > MAX_STATE {
            return Ok(None);
        }
        let mut bytes = buffer(usize::try_from(size).map_err(|_| Error::NoSpace)?)?;
        match data.read_at(STATE, 0, &mut bytes) {
            Ok(n) => bytes.truncate(n),
            Err(_) => return Ok(None),
        }
        // Not text: not this plugin's file.
        let Ok(text) = core::str::from_utf8(&bytes) else {
            return Ok(None);
        };
        let mut lines = text.lines();
        let (
            Some("notes-vault 1"),
            Some(folder),
            Some(built),
            Some(notes),
            Some(stamp),
            Some(tagged),
        ) = (
            lines.next(),
            lines.next(),
            lines.next(),
            lines.next(),
            lines.next(),
            lines.next(),
        )
        else {
            return Ok(None);
        };
        let (Ok(built), Ok(notes), Ok(stamp)) = (built.parse(), notes.parse(), stamp.parse())
        else {
            return Ok(None);
        };
        Ok(Some(State {
            folder: copy(folder)?,
            built,
            notes,
            stamp,
            tagged: tagged == "1",
        }))
    }

    /// Keeps the state in `data`. A refusal of the storage is not told: the
    /// index is made anew next time.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn write(&self, data: &dyn Storage) -> Result<(), Error> {
        let mut text = String::new();
        push(&mut text, "notes-vault 1\n")?;
        for c in self.folder.chars() {
            push_char(&mut text, if c == '\n' { ' ' } else { c })?;
        }
        push_char(&mut text, '\n')?;
        push_i64(&mut text, self.built)?;
        push_char(&mut text, '\n')?;
        push_u64(&mut text, u64::from(self.notes))?;
        push_char(&mut text, '\n')?;
        push_u64(&mut text, self.stamp)?;
        push_char(&mut text, '\n')?;
        push(&mut text, if self.tagged { "1\n" } else { "0\n" })?;
        data.write(STATE, text.as_bytes()).ok();
        Ok(())
    }
}

/// The index kept for `folder`, if there is one.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn kept(data: &dyn Storage, folder: &str) -> Result<Option<State>, Error> {
    Ok(State::read(data)?.filter(|s| s.folder == folder && data.exists(INDEX)))
}

/// How the walk went.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Not done: call again.
    More,
    /// Done: the index is made.
    Done(State),
    /// Could not be written.
    Failed,
}

/// Which of a folder's entries are looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// Its notes, a page at a time.
    Notes,
    /// Its folders, one after the other: each is walked before the next is
    /// found, so a folder of thousands of folders costs a frame, not
    /// thousands.
    Folders,
}

/// A folder being walked, and where its listing goes on.
#[derive(Debug)]
struct Frame {
    dir: String,
    from: u64,
    /// Folders above it; the vault's folder has none.
    depth: u8,
    look: Look,
}

impl Frame {
    fn new(dir: String, depth: u8) -> Self {
        Self {
            dir,
            from: 0,
            depth,
            look: Look::Notes,
        }
    }
}

/// Walks the vault a piece at a time and writes the index.
#[derive(Debug)]
pub struct Indexer {
    root: String,
    /// The folders from the vault's down to the one being walked: at most
    /// [`MAX_DEPTH`].
    stack: Vec<Frame>,
    buffer: String,
    pub notes: u32,
    stamp: u64,
    started: bool,
}

impl Indexer {
    /// A walk of the vault at `root` (slashes at its ends are left out).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(root: &str) -> Result<Self, Error> {
        let root = copy(root.trim_matches('/'))?;
        let mut stack = Vec::new();
        stack.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
        stack.push(Frame::new(copy(&root)?, 0));
        Ok(Self {
            root,
            stack,
            buffer: String::new(),
            notes: 0,
            stamp: 0,
            started: false,
        })
    }

    /// Reads one piece of the vault, writes what it found.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the walk is over then.
    pub fn step(&mut self, vault: &dyn Storage, data: &dyn Storage) -> Result<Step, Error> {
        if !self.started {
            self.started = true;
            if data.write(INDEX_PART, b"").is_err() {
                return Ok(Step::Failed);
            }
        }
        let Some(frame) = self.stack.last() else {
            return self.finish(data);
        };
        let (from, depth, look) = (frame.from, frame.depth, frame.look);
        let dir = frame.dir.as_str();
        let mut child: Option<String> = None;
        let mut seen = 0u64;
        let mut failed: Option<Error> = None;
        let (buffer, notes, stamp) = (&mut self.buffer, &mut self.notes, &mut self.stamp);
        let listed = vault.list_from(dir, from, &mut |entry| {
            // A page of entries, or, looking for folders, one found.
            if seen >= PAGE || child.is_some() || failed.is_some() {
                return false;
            }
            seen += 1;
            // A name with a tab or a line break would break a line of the
            // index; the card's file systems do not allow one, others may.
            if note::is_hidden(entry.name) || entry.name.chars().any(char::is_control) {
                return true;
            }
            let made = match look {
                Look::Folders if entry.is_dir => join(dir, entry.name).map(|sub| child = Some(sub)),
                Look::Notes if !entry.is_dir && note::is_note(entry.name) && *notes < MAX_NOTES => {
                    let modified = entry.modified.unwrap_or(0);
                    *notes += 1;
                    *stamp = stamp
                        .wrapping_mul(1_000_003)
                        .wrapping_add(modified as u64 ^ entry.size);
                    record(buffer, modified, dir, entry.name)
                }
                _ => Ok(()),
            };
            if let Err(error) = made {
                failed = Some(error);
                return false;
            }
            true
        });
        if let Some(error) = failed {
            return Err(error);
        }
        match listed {
            Ok(Some(next)) => {
                if let Some(frame) = self.stack.last_mut() {
                    frame.from = next;
                }
            }
            // The notes are done: its folders next, from the start.
            Ok(None) | Err(_) if look == Look::Notes => {
                if let Some(frame) = self.stack.last_mut() {
                    (frame.from, frame.look) = (0, Look::Folders);
                }
            }
            Ok(None) | Err(_) => {
                self.stack.pop();
            }
        }
        if let Some(sub) = child
            && depth + 1 < MAX_DEPTH
        {
            self.stack.try_reserve(1).map_err(|_| Error::NoMemory)?;
            self.stack.push(Frame::new(sub, depth + 1));
        }
        if self.buffer.len() > FLUSH_AT && !self.flush(data) {
            return Ok(Step::Failed);
        }
        if self.notes >= MAX_NOTES {
            self.stack.clear();
        }
        Ok(Step::More)
    }

    fn flush(&mut self, data: &dyn Storage) -> bool {
        let done = data.append(INDEX_PART, self.buffer.as_bytes()).is_ok();
        self.buffer.clear();
        done
    }

    fn finish(&mut self, data: &dyn Storage) -> Result<Step, Error> {
        if !self.flush(data) {
            return Ok(Step::Failed);
        }
        // The new index in place of the old; without a time, none is told.
        data.remove(INDEX).ok();
        if data.rename(INDEX_PART, INDEX).is_err() {
            return Ok(Step::Failed);
        }
        Ok(Step::Done(State {
            folder: copy(&self.root)?,
            built: 0,
            notes: self.notes,
            stamp: self.stamp,
            tagged: false,
        }))
    }
}

/// Appends the line of a note of the index.
fn record(buffer: &mut String, modified: i64, dir: &str, name: &str) -> Result<(), Error> {
    push_i64(buffer, modified)?;
    push_char(buffer, '\t')?;
    push(buffer, dir)?;
    if !dir.is_empty() {
        push_char(buffer, '/')?;
    }
    push(buffer, name)?;
    push_char(buffer, '\n')
}

/// Calls `each` with when each note changed and where it is, until it
/// answers `false`. No index: no notes.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn notes(data: &dyn Storage, each: &mut dyn FnMut(i64, &str) -> bool) -> Result<(), Error> {
    storage_ok(lines::each(
        data,
        INDEX,
        &mut |line| match line.split_once('\t') {
            Some((modified, path)) => each(modified.parse().unwrap_or(0), path),
            None => true,
        },
    ))
}

/// What a read of a file of the index says: a file that cannot be read is no
/// lines; only memory is an error.
fn storage_ok(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        _ => Ok(()),
    }
}

/// Writes the tags of the notes of the index, a few at a time.
#[derive(Debug, Default)]
pub struct Tagger {
    offset: u64,
    buffer: String,
    pub done: u32,
    started: bool,
}

impl Tagger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the tags of the next notes; `Some(true)` once all are, and
    /// the tags are in place; `None` where a file could not be written.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the tagging is over then.
    pub fn step(&mut self, vault: &dyn Storage, data: &dyn Storage) -> Result<Option<bool>, Error> {
        if !self.started {
            self.started = true;
            if data.write(TAGS_PART, b"").is_err() {
                return Ok(None);
            }
        }
        for _ in 0..TAGGED_PER_STEP {
            let Some((path, next)) = next_path(data, self.offset)? else {
                return self.finish(data);
            };
            self.offset = next;
            self.done += 1;
            let found = tags::of_file(vault, &path)?;
            if !found.is_empty() {
                push(&mut self.buffer, &path)?;
                push_char(&mut self.buffer, '\t')?;
                for (n, tag) in found.iter().enumerate() {
                    if n > 0 {
                        push_char(&mut self.buffer, ' ')?;
                    }
                    push(&mut self.buffer, tag)?;
                }
                push_char(&mut self.buffer, '\n')?;
            }
        }
        if self.buffer.len() > FLUSH_AT {
            if data.append(TAGS_PART, self.buffer.as_bytes()).is_err() {
                return Ok(None);
            }
            self.buffer.clear();
        }
        Ok(Some(false))
    }

    fn finish(&mut self, data: &dyn Storage) -> Result<Option<bool>, Error> {
        if data.append(TAGS_PART, self.buffer.as_bytes()).is_err() {
            return Ok(None);
        }
        self.buffer.clear();
        data.remove(TAGS).ok();
        if data.rename(TAGS_PART, TAGS).is_err() {
            return Ok(None);
        }
        Ok(Some(true))
    }
}

/// The path of the note of the index at byte `offset`, and the offset of
/// the next; a line that is no note's (or cut) is an empty path.
fn next_path(data: &dyn Storage, offset: u64) -> Result<Option<(String, u64)>, Error> {
    let mut chunk = [0u8; 1024];
    let Ok(n) = data.read_at(INDEX, offset, &mut chunk) else {
        return Ok(None);
    };
    if n == 0 {
        return Ok(None);
    }
    let bytes = chunk.get(..n).unwrap_or(&[]);
    let end = bytes.iter().position(|&b| b == b'\n');
    let line_end = end.unwrap_or(n);
    let next = offset
        .saturating_add(line_end as u64)
        .saturating_add(u64::from(end.is_some()));
    let line = bytes.get(..line_end).unwrap_or(&[]);
    // A line that is no note's (or cut): skipped.
    match line.iter().position(|&b| b == b'\t') {
        Some(tab) => {
            let path = line.get(tab + 1..).unwrap_or(&[]);
            if path.is_empty() {
                Ok(end.is_some().then_some((String::new(), next)))
            } else {
                Ok(Some((lossy(path)?, next)))
            }
        }
        None => Ok(end.is_some().then_some((String::new(), next))),
    }
}

/// The tags of the vault and how many notes have each, as found in the file
/// of tags, and whether more tags were left out.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn tag_counts(data: &dyn Storage) -> Result<(Vec<(String, u32)>, bool), Error> {
    let mut counts: Vec<(String, u32)> = Vec::new();
    counts
        .try_reserve_exact(MAX_TAGS)
        .map_err(|_| Error::NoMemory)?;
    let mut left_out = false;
    let mut failed: Option<Error> = None;
    let result = lines::each(data, TAGS, &mut |line| {
        let Some((_, words)) = line.split_once('\t') else {
            return true;
        };
        for tag in words.split(' ').filter(|t| !t.is_empty()) {
            if let Some((_, count)) = counts.iter_mut().find(|(t, _)| t.eq_ignore_ascii_case(tag)) {
                *count += 1;
            } else if counts.len() < MAX_TAGS {
                match copy(tag) {
                    // The room for `MAX_TAGS` was reserved above.
                    Ok(tag) => counts.push((tag, 1)),
                    Err(error) => {
                        failed = Some(error);
                        return false;
                    }
                }
            } else {
                left_out = true;
            }
        }
        true
    });
    if let Some(error) = failed {
        return Err(error);
    }
    if result.is_err() {
        counts.clear();
    }
    storage_ok(result)?;
    stable_sort_by(&mut counts, |a, b| cmp_lowercase(&a.0, &b.0));
    Ok((counts, left_out))
}

/// Calls `each` with the path of each note that has `tag`, until it
/// answers `false`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn notes_with_tag(
    data: &dyn Storage,
    tag: &str,
    each: &mut dyn FnMut(&str) -> bool,
) -> Result<(), Error> {
    storage_ok(lines::each(
        data,
        TAGS,
        &mut |line| match line.split_once('\t') {
            Some((path, words)) if words.split(' ').any(|t| t.eq_ignore_ascii_case(tag)) => {
                each(path)
            }
            _ => true,
        },
    ))
}
