//! The script hint (#257 item 16, Beta 3's `screen/script_hint.rs`): a book in
//! a script the reading font has no letters of (the X4 family's firmware
//! carries Latin alone) asks for the packs of letters with
//! `command.request_plugins`, instead of leaving the reader with question
//! marks: the store shows what they are and what they take, and downloads
//! them if the reader says so (`docs/abi/registry.md` 5).
//!
//! Which packs a book needs is seen in the language it declares and in its
//! text (the first blocks: a book may mix scripts, or declare none). Each pack
//! is asked for once for a book; what was asked is kept in the book's folder
//! (`books/<hash>/packs-asked.txt` in the reader's data, the file Beta 3 kept),
//! so that closing and opening the book asks no more, and so that a pack that
//! could not be had (`event.plugin_answered`, `failed`) is asked for again at
//! the next opening.
//!
//! The decisions ([`lacking`], [`needed`], [`with_asked`], [`without`],
//! [`effect`]) hold no host and are tested without one; [`ask`] and
//! [`answered`] are the host's side. Nothing is held after a decision.
use crate::native_document::HostDocument;
use crate::native_view::View;
use alloc::string::String;
use core::fmt::Write as _;
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, PluginOutcome},
    document::{DocId, Documents},
    files::{self, Files},
    fonts::{Font, FontRole},
    registry::Registry,
    storage::{DataStorage, Root},
};
use paperpaw_sdk::{Storage, StorageError, read_file};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// A script (ISO 15924), a letter every font of it has, and the pack that
/// brings its letters (Beta 3's table).
pub const SCRIPTS: [(&str, char, &str); 5] = [
    ("Grek", 'α', "script-greek"),
    ("Cyrl", 'ж', "script-cyrillic"),
    ("Arab", 'ع', "script-arabic"),
    ("Hebr", 'א', "script-hebrew"),
    ("Kore", '한', KOREAN),
];

/// The Korean pack (the syllables of everyday text) and the one that adds
/// the rare ones, which a book needs only if it uses them.
pub const KOREAN: &str = "script-korean";
pub const KOREAN_EXTRA: &str = "script-korean-extra";
/// Where Korean is in [`SCRIPTS`].
const KOREAN_AT: usize = 4;

/// The text looked at: its first sections, and in them as many blocks and
/// letters as a few pages hold, so that opening a book costs little.
pub const SECTIONS: u32 = 3;
pub const BLOCKS: usize = 80;
pub const LETTERS: usize = 12_000;
/// The packs asked for at most, for a book at once, and kept for it.
pub const MOST: usize = 4;

/// The file of the packs asked for, in the book's folder.
const FILE: &str = "packs-asked.txt";
/// The most of it that is read (Beta 3's).
const KEPT_MAX: usize = 1024;
/// Bytes of letters asked about at once (`text.covers` takes up to 1,024).
const ASK: usize = 512;
/// The scratch of the request: four ids of at most 64 bytes, and the frame.
const COMMAND_BYTES: usize = 512;
/// The input of the registry's `plugin` call (one id).
const REGISTRY_INPUT: usize = 256;

/// The script `c` is written in, as [`SCRIPTS`] names it, where it is one of
/// those.
#[must_use]
pub fn script_of(c: char) -> Option<&'static str> {
    Some(match u32::from(c) {
        0x0370..=0x03FF | 0x1F00..=0x1FFF => "Grek",
        0x0400..=0x052F | 0x1C80..=0x1C8F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => "Cyrl",
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => "Hebr",
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
            "Arab"
        }
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7A3 => "Kore",
        _ => return None,
    })
}

/// Where the script `name` is in [`SCRIPTS`].
fn index_of(name: &str) -> Option<usize> {
    SCRIPTS.iter().position(|(script, ..)| *script == name)
}

/// Where the script of `c` is in [`SCRIPTS`].
fn script_index(c: char) -> Option<usize> {
    script_of(c).and_then(index_of)
}

/// A Hangul syllable (the letters of everyday Korean text).
fn is_syllable(c: char) -> bool {
    ('\u{ac00}'..='\u{d7a3}').contains(&c)
}

/// The script of [`SCRIPTS`] the language `tag` (BCP 47) is written in, if it
/// is one of those.
#[must_use]
pub fn declared(tag: &str) -> Option<&'static str> {
    let script = paperpaw_sdk::lang::likely_script_of(tag)?;
    SCRIPTS
        .iter()
        .find(|(name, ..)| *name == script)
        .map(|(name, ..)| *name)
}

/// The scripts of [`SCRIPTS`] the font lacks letters of, in the order they
/// were found, and whether a Hangul syllable was among the letters it lacks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lacking {
    order: [usize; SCRIPTS.len()],
    len: usize,
    syllable: bool,
}

impl Lacking {
    /// The scripts, by name, in the order found.
    pub fn scripts(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.indexes()
            .filter_map(|at| SCRIPTS.get(at).map(|(name, ..)| *name))
    }

    /// Whether a Hangul syllable was among the letters the font lacks.
    #[must_use]
    pub const fn syllable(&self) -> bool {
        self.syllable
    }

    fn indexes(&self) -> impl Iterator<Item = usize> + '_ {
        self.order.iter().take(self.len).copied()
    }

    fn has(&self, script: usize) -> bool {
        self.indexes().any(|at| at == script)
    }

    fn add(&mut self, script: usize) {
        if self.has(script) {
            return;
        }
        if let Some(slot) = self.order.get_mut(self.len) {
            *slot = script;
            self.len += 1;
        }
    }

    /// The font lacks `c`.
    fn note(&mut self, c: char) {
        if let Some(script) = script_index(c) {
            self.add(script);
            self.syllable |= script == KOREAN_AT && is_syllable(c);
        }
    }

    /// Whether asking about `c` (of the script at `script`) could tell
    /// nothing new: its script is known to lack letters (and, for Hangul, a
    /// syllable is known to be lacking, or `c` is none).
    fn settled(&self, script: usize, c: char) -> bool {
        self.has(script) && (script != KOREAN_AT || self.syllable || !is_syllable(c))
    }

    fn settled_char(&self, c: char) -> bool {
        script_index(c).is_some_and(|script| self.settled(script, c))
    }
}

/// What the first blocks of a book of `sections` sections hold of the scripts
/// of [`SCRIPTS`] that the font lacks letters of (Beta 3's `lacking`).
/// `read(section, offset)` gives the text of the block there and its end, none
/// at a section's end or where it cannot be read (the section ends there);
/// `covers` is the font's `text.covers` (the bytes of the longest start of a
/// text it draws). The letters are asked about in runs of up to 512 bytes
/// and only those that could tell something new, so a book of one script asks
/// the host a few dozen times, not once a letter.
///
/// # Errors
/// What `covers` refuses: the font cannot tell, and nothing is asked for.
pub fn lacking(
    sections: u32,
    mut read: impl FnMut(u32, u32) -> Option<(String, u32)>,
    covers: &mut dyn FnMut(&str) -> Result<usize, Error>,
) -> Result<Lacking, Error> {
    let mut found = Lacking::default();
    let (mut blocks, mut letters) = (0usize, 0usize);
    'sections: for section in 0..sections.min(SECTIONS) {
        let mut offset = 0;
        while let Some((text, end)) = read(section, offset) {
            blocks += 1;
            letters += text.chars().count();
            ask_text(&text, &mut found, covers)?;
            if blocks >= BLOCKS || letters >= LETTERS {
                break 'sections;
            }
            // Past the end of what was read: the next block is further on.
            if end <= offset {
                break;
            }
            offset = end;
        }
    }
    Ok(found)
}

/// Asks about the letters of `text` of the scripts of [`SCRIPTS`] not yet
/// settled, a run of them at a time.
fn ask_text(
    text: &str,
    found: &mut Lacking,
    covers: &mut dyn FnMut(&str) -> Result<usize, Error>,
) -> Result<(), Error> {
    let mut run = [0u8; ASK];
    let mut len = 0;
    for c in text.chars() {
        let Some(script) = script_index(c) else {
            continue;
        };
        if found.settled(script, c) {
            continue;
        }
        if len + c.len_utf8() > ASK {
            ask_run(run.get(..len).unwrap_or(&[]), found, covers)?;
            len = 0;
            // What the run told may have settled this letter too.
            if found.settled(script, c) {
                continue;
            }
        }
        if let Some(room) = run.get_mut(len..) {
            len += c.encode_utf8(room).len();
        }
    }
    ask_run(run.get(..len).unwrap_or(&[]), found, covers)
}

/// Asks about a run of letters: each one the font lacks is noted, and the
/// question goes on after it.
fn ask_run(
    bytes: &[u8],
    found: &mut Lacking,
    covers: &mut dyn FnMut(&str) -> Result<usize, Error>,
) -> Result<(), Error> {
    let Ok(mut rest) = core::str::from_utf8(bytes) else {
        return Ok(());
    };
    while !rest.is_empty() {
        let drawn = covers(rest)?;
        let mut tail = rest.get(drawn..).ok_or(Error::Invalid)?.chars();
        let Some(c) = tail.next() else {
            break;
        };
        found.note(c);
        let after = tail.as_str();
        rest = after.trim_start_matches(|c| found.settled_char(c));
    }
    Ok(())
}

/// The packs asked for at once (at most [`MOST`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    ids: [&'static str; MOST],
    len: usize,
}

impl Wanted {
    /// The packs, in the order they are asked for.
    #[must_use]
    pub fn ids(&self) -> &[&'static str] {
        self.ids.get(..self.len).unwrap_or(&[])
    }

    /// Whether none is.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn push(&mut self, id: &'static str) {
        if self.ids().contains(&id) {
            return;
        }
        if let Some(slot) = self.ids.get_mut(self.len) {
            *slot = id;
            self.len += 1;
        }
    }
}

/// Whether `kept` (the file of the packs asked for) names `id`.
#[must_use]
pub fn was_asked(kept: &str, id: &str) -> bool {
    kept.lines().any(|line| line == id)
}

/// The packs the book needs for the letters the font lacks (`found` in its
/// text; `script`, the one its language is written in, if the font lacks its
/// letter) that `kept` does not name (Beta 3's `needed`). `covers` is the
/// font's `text.covers`; `korean_installed` says whether the Korean pack is on
/// the device, asked only when Korean is lacking: once it is there, a book
/// that still has syllables the font lacks needs the rare ones as well; it is
/// not asked for them before, not to push their size on a book that does not.
///
/// # Errors
/// What `covers` refuses.
pub fn needed(
    mut found: Lacking,
    script: Option<&'static str>,
    covers: &mut dyn FnMut(&str) -> Result<usize, Error>,
    korean_installed: &mut dyn FnMut() -> bool,
    kept: &str,
) -> Result<Wanted, Error> {
    if let Some(at) = script.and_then(index_of)
        && !found.has(at)
        && let Some((_, letter, _)) = SCRIPTS.get(at)
    {
        let mut bytes = [0u8; 4];
        let letter = letter.encode_utf8(&mut bytes);
        if covers(letter)? < letter.len() {
            found.add(at);
        }
    }
    let mut wanted = Wanted::default();
    for at in found.indexes() {
        let Some(&(_, _, pack)) = SCRIPTS.get(at) else {
            continue;
        };
        let pack = match pack == KOREAN && korean_installed() {
            true if found.syllable => KOREAN_EXTRA,
            true => continue,
            false => pack,
        };
        if !was_asked(kept, pack) {
            wanted.push(pack);
        }
    }
    Ok(wanted)
}

/// The file `kept` with `ids` added as asked for: the ids not in it are put
/// last, and the last [`MOST`] lines are kept (it stays a few lines long).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn with_asked(kept: &str, ids: &[&str]) -> Result<String, Error> {
    let lines = kept.lines().filter(|line| !line.is_empty());
    let new = ids.iter().copied().filter(|id| !was_asked(kept, id));
    let all = lines.chain(new);
    let skip = all.clone().count().saturating_sub(MOST);
    joined(all.skip(skip))
}

/// The file `kept` without `id`: the store could not get it, so it is asked
/// for again at the next opening. Empty where nothing is left (the file is
/// removed then).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn without(kept: &str, id: &str) -> Result<String, Error> {
    let rest = kept.lines().filter(|line| !line.is_empty() && *line != id);
    let skip = rest.clone().count().saturating_sub(MOST);
    joined(rest.skip(skip))
}

/// `lines` joined by line breaks, allocated fallibly.
fn joined<'a>(lines: impl Iterator<Item = &'a str> + Clone) -> Result<String, Error> {
    let bytes = lines.clone().map(|line| line.len() + 1).sum::<usize>();
    let mut text = String::new();
    text.try_reserve_exact(bytes).map_err(|_| Error::NoMemory)?;
    for (n, line) in lines.enumerate() {
        if n > 0 {
            text.push('\n');
        }
        text.push_str(line);
    }
    Ok(text)
}

/// The file of the packs asked for the book at `path`: in the book's folder
/// as Beta 3 named it (`books/<FNV-1a 64 of the path, 16 hex digits>`), in the
/// reader's data.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn kept_path(path: &str) -> Result<String, Error> {
    let mut name = String::new();
    name.try_reserve_exact(23 + FILE.len())
        .map_err(|_| Error::NoMemory)?;
    write!(name, "books/{:016x}/{FILE}", fnv1a64(path)).map_err(|_| Error::NoMemory)?;
    Ok(name)
}

/// What an answer to a pack asked for does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Installed: the books on show are laid out again (their letters came),
    /// and asked about once more (the rare Hangul syllables).
    Relayout,
    /// It could not be had: taken off the file, asked for at the next opening.
    Forget,
    /// Declined, or the store has no such pack: it stays asked for.
    Keep,
}

/// What the answer `outcome` does (an outcome the reader does not know is
/// `failed`, as the SDK reads it).
#[must_use]
pub const fn effect(outcome: PluginOutcome) -> Effect {
    match outcome {
        PluginOutcome::Installed => Effect::Relayout,
        PluginOutcome::Declined | PluginOutcome::NotFound => Effect::Keep,
        _ => Effect::Forget,
    }
}

/// Whether `id` is one of the packs the reader asks for.
#[must_use]
pub fn is_pack(id: &str) -> bool {
    id == KOREAN_EXTRA || SCRIPTS.iter().any(|(.., pack)| *pack == id)
}

/// The open book as the hint reads it.
#[derive(Clone, Copy, Debug)]
pub struct Book<'a> {
    /// The open document.
    pub id: DocId,
    pub sections: u32,
    pub whole_pages: bool,
    /// The book's path, which names its folder.
    pub path: &'a str,
    /// The script of [`SCRIPTS`] its language is written in ([`declared`]).
    pub script: Option<&'static str>,
}

/// A book opened (or its letters came): the packs it needs and that were not
/// asked for are asked for, and kept as asked. A refusal on the way asks
/// nothing (the next opening asks again); it never ends the caller's callback.
/// Called from events only (the screen opened or shown, the document ready,
/// an answer), never from a draw.
#[inline(never)]
pub fn ask(host: &Host<'_>, docs: &Documents, book: &Book<'_>) {
    let Ok((wanted, kept)) = decide(host, docs, book) else {
        return;
    };
    if wanted.is_empty() {
        return;
    }
    let Ok(mut scratch) = buffer(COMMAND_BYTES) else {
        return;
    };
    // Kept only once the request was taken: a refused one (two of the
    // reader's own still open, `ERR_FULL`) is asked for at the next opening.
    if CommandSender::new(host, &mut scratch)
        .request_plugins(wanted.ids())
        .is_ok()
        && let Ok(text) = with_asked(&kept, wanted.ids())
    {
        keep(host, book.path, &text);
    }
}

/// The packs `book` needs, and the file of those asked for before.
fn decide(host: &Host<'_>, docs: &Documents, book: &Book<'_>) -> Result<(Wanted, String), Error> {
    let font = Font::acquire_role(host, FontRole::Reading)?;
    let mut covers = |text: &str| font.covers(text);
    let found = {
        let document = HostDocument::new(host, docs, book.id, book.sections, book.whole_pages)?;
        let read = |section, offset| {
            let block = document.read_at(Locator::new(section, offset)).ok()?;
            Some((block.text, block.end))
        };
        lacking(book.sections, read, &mut covers)?
    };
    let kept = read_kept(host, book.path)?;
    let mut korean = || installed(host, KOREAN);
    let wanted = needed(found, book.script, &mut covers, &mut korean, &kept)?;
    Ok((wanted, kept))
}

/// The file of the packs asked for the book at `path`; empty where there is
/// none or it cannot be read (Beta 3's).
///
/// # Errors
/// [`Error::NoMemory`].
fn read_kept(host: &Host<'_>, path: &str) -> Result<String, Error> {
    let name = kept_path(path)?;
    match read_file(&DataStorage::new(host), &name, KEPT_MAX) {
        Ok(bytes) => Ok(String::from_utf8(bytes).unwrap_or_default()),
        Err(StorageError::OutOfMemory) => Err(Error::NoMemory),
        Err(_) => Ok(String::new()),
    }
}

/// Writes `text` as the file of the packs asked for the book at `path` (its
/// folder made where it is missing); an empty one removes it. Best effort:
/// what was not kept is asked again at the next opening, nothing is lost.
fn keep(host: &Host<'_>, path: &str, text: &str) {
    let Ok(name) = kept_path(path) else {
        return;
    };
    let storage = DataStorage::new(host);
    if text.is_empty() {
        storage.remove(&name).ok();
        return;
    }
    if storage.write(&name, text.as_bytes()).is_ok() {
        return;
    }
    if let Some((dir, _)) = name.rsplit_once('/') {
        make_dir(host, dir);
    }
    storage.write(&name, text.as_bytes()).ok();
}

/// Makes the book's folder in the reader's data.
fn make_dir(host: &Host<'_>, dir: &str) {
    let (Ok(service), Ok(mut input), Ok(mut out)) = (
        Files::open(host),
        buffer(files::INPUT_BYTES),
        buffer(files::OUTPUT_BYTES),
    ) else {
        return;
    };
    service
        .make_dir(host, Root::Data, dir, &mut input, &mut out)
        .ok();
    service.release(host).ok();
}

/// Whether the plugin `id` is on the device (any state): the registry knows
/// it. Not known where the registry cannot tell (the host drops an id already
/// on the device from a request anyway).
fn installed(host: &Host<'_>, id: &str) -> bool {
    let Ok(registry) = Registry::open(host) else {
        return false;
    };
    let known = match (buffer(REGISTRY_INPUT), buffer(SERVICE_BUFFER)) {
        (Ok(mut input), Ok(mut out)) => registry.plugin(host, id, &mut input, &mut out).is_ok(),
        _ => false,
    };
    registry.release(host).ok();
    known
}

/// The store answered for the pack `plugin` (`event.plugin_answered`): the
/// reading screens' books take it off what was asked where it could not be
/// had, or are laid out again and asked about once more where it was
/// installed. Whether a book was laid out again (its pages are counted anew).
#[inline(never)]
pub fn answered<'v>(
    host: &Host<'_>,
    docs: &Documents,
    views: impl Iterator<Item = &'v mut View>,
    plugin: &str,
    outcome: PluginOutcome,
) -> bool {
    if !is_pack(plugin) {
        return false;
    }
    let mut laid = false;
    for view in views {
        match effect(outcome) {
            Effect::Keep => {}
            Effect::Forget => {
                let Some((path, _)) = view.book() else {
                    continue;
                };
                let Ok(kept) = read_kept(host, path) else {
                    continue;
                };
                if was_asked(&kept, plugin)
                    && let Ok(text) = without(&kept, plugin)
                {
                    keep(host, path, &text);
                }
            }
            Effect::Relayout => {
                if view.hint_book().is_none() {
                    continue;
                }
                // A refused redraw leaves the page as it was: drawn at the
                // next turn.
                view.reflow(host, docs).ok();
                laid = true;
                if let Some(book) = view.hint_book() {
                    ask(host, docs, &book);
                }
            }
        }
    }
    laid
}

#[cfg(test)]
mod tests;
