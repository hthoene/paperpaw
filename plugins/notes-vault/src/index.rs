//! The vault's index: a file of its notes (when each changed, and where
//! it is), made by walking the vault a piece at a time, and a file of the
//! tags of its notes, made from that. Both are written as they are made
//! and read a line at a time, so the vault's size is a file's, never
//! memory's; the lists made from them (see `page`) hold a window.
//!
//! ```text
//! index.txt   <modified>\t<path>             a line for each note
//! tags.txt    <path>\t<tag> <tag> ...        a line for each note with tags
//! state.txt   notes-vault 1, the vault's folder, when it was made (unix
//!             seconds, 0 if unknown), its notes, their stamp, tags made
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::{Env, Storage};

use crate::lines;
use crate::note::{self, join};
use crate::tags;

/// Entries read from one folder in one step.
const PAGE: u64 = 64;
/// Notes of the index at most.
pub(crate) const MAX_NOTES: u32 = 50_000;
/// Folders below one another at most (the vault's folder is the first).
const MAX_DEPTH: u8 = 24;
/// Notes whose tags are read in one step.
pub(crate) const TAGGED_PER_STEP: usize = 3;
/// Distinct tags listed at most.
pub(crate) const MAX_TAGS: usize = 400;

fn file(name: &str) -> String {
    alloc::format!("{}/{name}", Env::plugin_dir(crate::PLUGIN.as_str()))
}

pub(crate) fn index_path() -> String {
    file("index.txt")
}

pub(crate) fn tags_path() -> String {
    file("tags.txt")
}

fn state_path() -> String {
    file("state.txt")
}

/// What is known of the index made last.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct State {
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
    pub(crate) fn read(storage: &dyn Storage) -> Option<State> {
        let bytes = paperpaw_sdk::read_file(storage, &state_path(), 2048).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines = text.lines();
        if lines.next()? != "notes-vault 1" {
            return None;
        }
        Some(State {
            folder: lines.next()?.to_string(),
            built: lines.next()?.parse().ok()?,
            notes: lines.next()?.parse().ok()?,
            stamp: lines.next()?.parse().ok()?,
            tagged: lines.next()? == "1",
        })
    }

    pub(crate) fn write(&self, storage: &dyn Storage) {
        let text = alloc::format!(
            "notes-vault 1\n{}\n{}\n{}\n{}\n{}\n",
            self.folder.replace('\n', " "),
            self.built,
            self.notes,
            self.stamp,
            u8::from(self.tagged)
        );
        // Not written: the index is made anew next time.
        storage.write(&state_path(), text.as_bytes()).ok();
    }
}

/// The index kept for `folder`, if there is one.
pub(crate) fn kept(storage: &dyn Storage, folder: &str) -> Option<State> {
    State::read(storage).filter(|s| s.folder == folder && storage.exists(&index_path()))
}

/// How the walk went.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
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
pub(crate) struct Indexer {
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
    pub(crate) fn new(root: &str) -> Self {
        let root = root.trim_matches('/').to_string();
        Self {
            stack: alloc::vec![Frame::new(root.clone(), 0)],
            root,
            buffer: String::new(),
            notes: 0,
            stamp: 0,
            started: false,
        }
    }

    fn part() -> String {
        file("index.part")
    }

    /// Reads one piece of the vault, writes what it found.
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> Step {
        if !self.started {
            self.started = true;
            let _ = storage.create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
            if storage.write(&Self::part(), b"").is_err() {
                return Step::Failed;
            }
        }
        let Some(frame) = self.stack.last() else {
            return self.finish(storage);
        };
        let (dir, from, depth, look) = (frame.dir.clone(), frame.from, frame.depth, frame.look);
        let mut child: Option<String> = None;
        let mut seen = 0u64;
        let (buffer, notes, stamp) = (&mut self.buffer, &mut self.notes, &mut self.stamp);
        let listed = storage.list_from(&dir, from, &mut |entry| {
            // A page of entries, or, looking for folders, one found.
            if seen >= PAGE || child.is_some() {
                return false;
            }
            seen += 1;
            // A name with a tab or a line break would break a line of the
            // index; the card's file systems do not allow one, others may.
            if note::is_hidden(entry.name) || entry.name.chars().any(char::is_control) {
                return true;
            }
            match look {
                Look::Folders if entry.is_dir => child = Some(join(&dir, entry.name)),
                Look::Notes if !entry.is_dir && note::is_note(entry.name) && *notes < MAX_NOTES => {
                    let modified = entry.modified.unwrap_or(0);
                    buffer.push_str(&alloc::format!("{modified}\t{}\n", join(&dir, entry.name)));
                    *notes += 1;
                    *stamp = stamp
                        .wrapping_mul(1_000_003)
                        .wrapping_add(modified as u64 ^ entry.size);
                }
                _ => {}
            }
            true
        });
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
            self.stack.push(Frame::new(sub, depth + 1));
        }
        if self.buffer.len() > 2048 && !self.flush(storage) {
            return Step::Failed;
        }
        if self.notes >= MAX_NOTES {
            self.stack.clear();
        }
        Step::More
    }

    fn flush(&mut self, storage: &dyn Storage) -> bool {
        let done = storage
            .append(&Self::part(), self.buffer.as_bytes())
            .is_ok();
        self.buffer.clear();
        done
    }

    fn finish(&mut self, storage: &dyn Storage) -> Step {
        if !self.flush(storage) {
            return Step::Failed;
        }
        // The new index in place of the old; without a time, none is told.
        let _ = storage.remove(&index_path());
        if storage.rename(&Self::part(), &index_path()).is_err() {
            return Step::Failed;
        }
        Step::Done(State {
            folder: self.root.clone(),
            built: 0,
            notes: self.notes,
            stamp: self.stamp,
            tagged: false,
        })
    }
}

/// Calls `each` with when each note changed and where it is, until it
/// answers `false`.
pub(crate) fn notes(storage: &dyn Storage, each: &mut dyn FnMut(i64, &str) -> bool) {
    let result = lines::each(
        storage,
        &index_path(),
        &mut |line| match line.split_once('\t') {
            Some((modified, path)) => each(modified.parse().unwrap_or(0), path),
            None => true,
        },
    );
    // No index: no notes.
    result.ok();
}

/// Writes the tags of the notes of the index, a few at a time.
#[derive(Debug)]
pub(crate) struct Tagger {
    offset: u64,
    buffer: String,
    pub done: u32,
    started: bool,
}

impl Tagger {
    pub(crate) fn new() -> Self {
        Self {
            offset: 0,
            buffer: String::new(),
            done: 0,
            started: false,
        }
    }

    fn part() -> String {
        file("tags.part")
    }

    /// Reads the tags of the next notes; `Some(true)` once all are, and
    /// the tags are in place.
    pub(crate) fn step(&mut self, storage: &dyn Storage) -> Option<bool> {
        if !self.started {
            self.started = true;
            storage.write(&Self::part(), b"").ok()?;
        }
        for _ in 0..TAGGED_PER_STEP {
            let Some((path, next)) = next_path(storage, self.offset) else {
                return self.finish(storage);
            };
            self.offset = next;
            self.done += 1;
            let found = tags::of_file(storage, &path);
            if !found.is_empty() {
                self.buffer.push_str(&path);
                self.buffer.push('\t');
                self.buffer.push_str(&found.join(" "));
                self.buffer.push('\n');
            }
        }
        if self.buffer.len() > 2048 {
            storage.append(&Self::part(), self.buffer.as_bytes()).ok()?;
            self.buffer.clear();
        }
        Some(false)
    }

    fn finish(&mut self, storage: &dyn Storage) -> Option<bool> {
        storage.append(&Self::part(), self.buffer.as_bytes()).ok()?;
        self.buffer.clear();
        let _ = storage.remove(&tags_path());
        storage.rename(&Self::part(), &tags_path()).ok()?;
        Some(true)
    }
}

/// The path of the note of the index at byte `offset`, and the offset of
/// the next.
fn next_path(storage: &dyn Storage, offset: u64) -> Option<(String, u64)> {
    let mut chunk = [0u8; 1024];
    let n = storage.read_at(&index_path(), offset, &mut chunk).ok()?;
    if n == 0 {
        return None;
    }
    let end = chunk[..n].iter().position(|&b| b == b'\n');
    let line_end = end.unwrap_or(n);
    let text = String::from_utf8_lossy(&chunk[..line_end]);
    let next = offset + line_end as u64 + u64::from(end.is_some());
    // A line that is no note's (or cut): skipped.
    match text.split_once('\t') {
        Some((_, path)) if !path.is_empty() => Some((path.to_string(), next)),
        _ => end.is_some().then_some((String::new(), next)),
    }
}

/// Calls `each` with the tags of the vault and how many notes have each,
/// as found in the file of tags, and whether more tags were left out.
pub(crate) fn tag_counts(storage: &dyn Storage) -> (Vec<(String, u32)>, bool) {
    let mut counts: Vec<(String, u32)> = Vec::new();
    let mut left_out = false;
    let result = lines::each(storage, &tags_path(), &mut |line| {
        let Some((_, words)) = line.split_once('\t') else {
            return true;
        };
        for tag in words.split(' ').filter(|t| !t.is_empty()) {
            if let Some((_, count)) = counts.iter_mut().find(|(t, _)| t.eq_ignore_ascii_case(tag)) {
                *count += 1;
            } else if counts.len() < MAX_TAGS {
                counts.push((tag.to_string(), 1));
            } else {
                left_out = true;
            }
        }
        true
    });
    if result.is_err() {
        counts.clear();
    }
    paperpaw_sdk::sort::sort_by_cached_key(&mut counts, |(t, _)| t.to_lowercase());
    (counts, left_out)
}

/// Calls `each` with the path of each note that has `tag`, until it
/// answers `false`.
pub(crate) fn notes_with_tag(storage: &dyn Storage, tag: &str, each: &mut dyn FnMut(&str) -> bool) {
    let result = lines::each(
        storage,
        &tags_path(),
        &mut |line| match line.split_once('\t') {
            Some((path, words)) if words.split(' ').any(|t| t.eq_ignore_ascii_case(tag)) => {
                each(path)
            }
            _ => true,
        },
    );
    result.ok();
}
