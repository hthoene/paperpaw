//! Flash cards: decks of cards, each with a front, a back and an example,
//! and what FSRS knows of it (see [`fsrs`](crate::fsrs)).
//!
//! Kept in the plugin's folder, `cards/`, as text, a line each with tabs
//! between the fields (a tab, a line break or a backslash in a text
//! written `\t`, `\n`, `\\`), the version on the first line:
//!
//! - `decks.txt`: `paperpaw-decks 1`, then `next<TAB>id`, the id the
//!   next deck gets, then `id<TAB>name` for each deck;
//! - `deck-<id>.txt`: `paperpaw-cards 1`, then a card a line,
//!   `id<TAB>due<TAB>last<TAB>stability<TAB>difficulty<TAB>reps<TAB>lapses<TAB>front<TAB>back<TAB>example`:
//!   `due` and `last` in seconds since 1970 (0 for a new card, never
//!   seen), stability (in days) and difficulty in hundredths; and a line
//!   `next<TAB>id`, the id the next card gets (written after the cards
//!   when the deck is written anew);
//! - `answers.txt`: the answers given since the decks were last written,
//!   `deck<TAB>` a card's fields up to `lapses`, then the due time it had
//!   before. Appended at each answer, so a review writes little; folded
//!   into the decks ([`fold`]) when a review ends, at a start, and before
//!   a deck is changed otherwise;
//! - `due.txt`: `paperpaw-due 1`, then `deck<TAB>new cards<TAB>quarter=count …`
//!   for each deck, the quarter hours (since 1970) its other cards are due
//!   in: what the widget counts without reading the decks (every time
//!   zone's day begins on a quarter hour, so the count is exact);
//! - `looked-up.txt`: words looked up, `word<TAB>sentence`, waiting to
//!   become cards of the deck *Looked up* when the flash cards open next
//!   (a lookup only appends a line);
//! - `adding.txt`: the deck cards are being added to, while they are: an
//!   addition cut off is counted into the index again at the next start;
//! - `state.txt`: `clock <seconds>`, the time last known (for a device
//!   that does not know it now), and `new <day> <count>`, the new cards
//!   shown that day.
//!
//! Ids are never given twice, so that a change meant for a card or a deck
//! deleted never reaches a newer one: the next is the counter or the one
//! after the highest there is, whichever is more. A file without the
//! counter (written before it was kept) goes on from the highest; older
//! versions skip the line, and keep it as it is in a deck.
//!
//! A deck is only ever streamed, a line at a time. One written anew goes
//! beside itself (`deck-<id>.tmp`) and takes its place once whole, as do
//! the lists, so a failed write or a power cut never loses cards; lines a
//! deck holds that are no card are kept as they are. A review holds the
//! due cards' state and where their lines are, never their texts but the
//! one shown.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Env, Storage, StorageError, files};

use crate::PLUGIN;
use crate::fsrs::{self, Fx, Grade, Memory};

/// Decks kept at most.
pub(crate) const MAX_DECKS: usize = 50;
/// Cards in a deck at most.
pub(crate) const MAX_CARDS: usize = 20_000;
/// Characters of a front, a back or an example at most.
pub(crate) const MAX_TEXT: usize = 500;
/// Characters of a deck's name at most.
pub(crate) const MAX_NAME: usize = 60;
/// The longest line read: a card's three texts at their longest.
const MAX_LINE: usize = 8 * 1024;
const DAY: i64 = 24 * 3600;
/// The index counts cards by quarter hour.
const QUARTER: i64 = 900;
const CARDS_HEADER: &str = "paperpaw-cards 1";
const DECKS_HEADER: &str = "paperpaw-decks 1";
const DUE_HEADER: &str = "paperpaw-due 1";
/// The counter line of `decks.txt` and of a deck begins so.
const NEXT: &str = "next\t";
/// Bytes written at a time.
const CHUNK: usize = 2048;

fn dir() -> String {
    format!("{}/cards", Env::plugin_dir(PLUGIN.as_str()))
}

pub(crate) fn path(name: &str) -> String {
    format!("{}/{name}", dir())
}

fn deck_path(deck: u32) -> String {
    path(&format!("deck-{deck}.txt"))
}

fn deck_temporary(deck: u32) -> String {
    path(&format!("deck-{deck}.tmp"))
}

/// Writes `bytes` as the whole of `file`: beside it first, then in its
/// place, so a cut never leaves half of it.
fn write_whole(storage: &dyn Storage, file: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let part = files::part_path(file);
    storage.write(&part, bytes)?;
    files::put_in_place(storage, &part, file)
}

/// After a cut while `file` was written whole: the new one where the old
/// one is gone already, else the unfinished one goes.
fn recover_whole(storage: &dyn Storage, file: &str, part: &str) {
    if !storage.exists(part) {
        return;
    }
    if storage.exists(file) {
        let _ = storage.remove(part);
    } else {
        let _ = storage.rename(part, file);
    }
}

/// The id a counter line (see [`NEXT`]) says comes next.
fn counter(line: &str) -> Option<u32> {
    line.strip_prefix(NEXT)?.parse().ok()
}

/// What went wrong with a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Problem {
    /// No such deck or card.
    Missing,
    /// No more decks, or no more cards in the deck.
    Full,
    /// A name or a front is needed.
    Empty,
    /// The card changed since it was seen (see [`Card::version`]).
    Changed,
    Storage(StorageError),
}

impl From<StorageError> for Problem {
    fn from(e: StorageError) -> Self {
        Problem::Storage(e)
    }
}

/// `text` as a field: a tab, a line break, a backslash written with a
/// backslash.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// A field as it was written by [`escape`].
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `text` as kept: trimmed, at most `most` characters.
pub(crate) fn clean(text: &str, most: usize) -> String {
    text.trim().chars().take(most).collect()
}

/// What FSRS knows of a card, and when it is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct State {
    pub id: u32,
    /// Seconds since 1970; 0 for a new card.
    pub due: i64,
    /// When it was last answered; 0 for a new card.
    pub last: i64,
    /// In hundredths of a day.
    pub stability: u32,
    /// In hundredths.
    pub difficulty: u16,
    pub reps: u32,
    pub lapses: u32,
}

impl State {
    pub(crate) fn is_new(&self) -> bool {
        self.reps == 0
    }

    fn memory(&self) -> Option<Memory> {
        (!self.is_new()).then(|| Memory {
            stability: Fx::hundredths(i64::from(self.stability)),
            difficulty: Fx::hundredths(i64::from(self.difficulty)),
        })
    }

    /// Answered `grade` at `now`, `retention` wanted; `same_day` where it
    /// was last answered on the same local day.
    pub(crate) fn answered(&self, grade: Grade, now: i64, same_day: bool, retention: Fx) -> State {
        let elapsed = now.saturating_sub(self.last);
        let (memory, wait) = fsrs::answer(self.memory(), elapsed, same_day, grade, retention);
        State {
            id: self.id,
            due: now.saturating_add(wait),
            last: now,
            stability: memory
                .stability
                .to_hundredths()
                .clamp(1, i64::from(u32::MAX)) as u32,
            difficulty: memory.difficulty.to_hundredths().clamp(100, 1000) as u16,
            reps: self.reps.saturating_add(1),
            lapses: self
                .lapses
                .saturating_add(u32::from(grade == Grade::Again && !self.is_new())),
        }
    }

    fn fields(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.id, self.due, self.last, self.stability, self.difficulty, self.reps, self.lapses
        )
    }

    fn parse<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Option<State> {
        Some(State {
            id: fields.next()?.parse().ok()?,
            due: fields.next()?.parse().ok()?,
            last: fields.next()?.parse().ok()?,
            stability: fields.next()?.parse().ok()?,
            difficulty: fields.next()?.parse().ok()?,
            reps: fields.next()?.parse().ok()?,
            lapses: fields.next()?.parse().ok()?,
        })
    }
}

/// A card.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Card {
    pub state: State,
    pub front: String,
    pub back: String,
    /// A sentence it is used in, or empty.
    pub example: String,
}

impl Card {
    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\n",
            self.state.fields(),
            escape(&self.front),
            escape(&self.back),
            escape(&self.example)
        )
    }

    /// A number that changes with the card's texts (FNV-1a over them):
    /// sent with a change from the phone, so that one made from texts seen
    /// before the card changed is refused.
    pub(crate) fn version(&self) -> u32 {
        let mut hash: u32 = 0x811c_9dc5;
        for text in [&self.front, &self.back, &self.example] {
            for byte in text.bytes().chain([0]) {
                hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
            }
        }
        hash
    }

    fn parse(line: &str) -> Option<Card> {
        let mut fields = line.split('\t');
        let state = State::parse(&mut fields)?;
        Some(Card {
            state,
            front: unescape(fields.next()?),
            back: unescape(fields.next().unwrap_or_default()),
            example: unescape(fields.next().unwrap_or_default()),
        })
    }
}

/// Texts of a new card, or of a card changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Texts {
    pub front: String,
    pub back: String,
    pub example: String,
}

impl Texts {
    /// Trimmed and cut to what is kept; `None` without a front.
    pub(crate) fn cleaned(&self) -> Option<Texts> {
        let texts = Texts {
            front: clean(&self.front, MAX_TEXT),
            back: clean(&self.back, MAX_TEXT),
            example: clean(&self.example, MAX_TEXT),
        };
        (!texts.front.is_empty()).then_some(texts)
    }
}

/// Calls `visit` with each line of `path` and where it begins, in order
/// (a missing file has none); a line longer than [`MAX_LINE`] is cut.
fn each_line(
    storage: &dyn Storage,
    path: &str,
    visit: impl FnMut(u64, &str) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    each_line_from(storage, path, 0, visit)
}

/// [`each_line`] from the line that begins at `from`.
fn each_line_from(
    storage: &dyn Storage,
    path: &str,
    from: u64,
    mut visit: impl FnMut(u64, &str) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    let mut chunk = [0u8; 512];
    let mut line: Vec<u8> = Vec::new();
    let (mut at, mut start) = (from, from);
    loop {
        let n = match storage.read_at(path, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) if at == from => return Ok(()),
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        for (i, &byte) in chunk[..n].iter().enumerate() {
            if byte == b'\n' {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if visit(start, &String::from_utf8_lossy(&line)).is_break() {
                    return Ok(());
                }
                line.clear();
                start = at + i as u64 + 1;
            } else if line.len() < MAX_LINE {
                line.push(byte);
            }
        }
        at += n as u64;
    }
    if !line.is_empty() {
        let _ = visit(start, &String::from_utf8_lossy(&line));
    }
    Ok(())
}

/// The line of `path` that begins at `offset`.
fn line_at(storage: &dyn Storage, path: &str, offset: u64) -> Option<String> {
    let mut out: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 512];
    let mut at = offset;
    while out.len() < MAX_LINE {
        let n = storage.read_at(path, at, &mut chunk).ok()?;
        if n == 0 {
            break;
        }
        match chunk[..n].iter().position(|b| *b == b'\n') {
            Some(end) => {
                out.extend_from_slice(&chunk[..end]);
                break;
            }
            None => out.extend_from_slice(&chunk[..n]),
        }
        at += n as u64;
    }
    out.truncate(MAX_LINE);
    Some(String::from_utf8_lossy(&out).into_owned())
}

// ---- Decks -------------------------------------------------------------

/// A deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Deck {
    pub id: u32,
    pub name: String,
}

/// The decks, in the order they were made; an error where the list could
/// not be read to its end: then it must not be written anew from them.
pub(crate) fn decks(storage: &dyn Storage) -> Result<Vec<Deck>, StorageError> {
    Ok(decks_and_next(storage)?.0)
}

/// The decks, and the id the next one gets.
fn decks_and_next(storage: &dyn Storage) -> Result<(Vec<Deck>, u32), StorageError> {
    let mut decks: Vec<Deck> = Vec::new();
    let mut next = 1u32;
    each_line(storage, &path("decks.txt"), |_, line| {
        if let Some(n) = counter(line) {
            next = next.max(n);
            return ControlFlow::Continue(());
        }
        let (id, name) = line.split_once('\t').unwrap_or((line, ""));
        if let Ok(id) = id.parse() {
            decks.push(Deck {
                id,
                name: unescape(name),
            });
        }
        if decks.len() >= MAX_DECKS {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    })?;
    let after_highest = decks.iter().map(|d| d.id.saturating_add(1)).max();
    Ok((decks, next.max(after_highest.unwrap_or(1))))
}

/// Writes the list of decks, `next` the id the next one gets.
fn write_decks(storage: &dyn Storage, decks: &[Deck], next: u32) -> Result<(), StorageError> {
    let mut text = format!("{DECKS_HEADER}\n{NEXT}{next}\n");
    for deck in decks {
        text.push_str(&format!("{}\t{}\n", deck.id, escape(&deck.name)));
    }
    write_whole(storage, &path("decks.txt"), text.as_bytes())
}

/// A new deck named `name`: its id.
pub(crate) fn add_deck(storage: &dyn Storage, name: &str) -> Result<u32, Problem> {
    let name = clean(name, MAX_NAME);
    if name.is_empty() {
        return Err(Problem::Empty);
    }
    let (mut decks, mut id) = decks_and_next(storage)?;
    if decks.len() >= MAX_DECKS {
        return Err(Problem::Full);
    }
    // Past a deck's file left by a cut (its list not written): never over
    // its cards.
    while storage.exists(&deck_path(id)) {
        id += 1;
    }
    write_whole(
        storage,
        &deck_path(id),
        format!("{CARDS_HEADER}\n").as_bytes(),
    )?;
    decks.push(Deck { id, name });
    write_decks(storage, &decks, id.saturating_add(1))?;
    Ok(id)
}

/// The deck named `name` (in any case), made if there is none.
pub(crate) fn deck_named(storage: &dyn Storage, name: &str) -> Result<u32, Problem> {
    let lower = name.to_lowercase();
    match decks(storage)?
        .into_iter()
        .find(|d| d.name.to_lowercase() == lower)
    {
        Some(deck) => Ok(deck.id),
        None => add_deck(storage, name),
    }
}

pub(crate) fn rename_deck(storage: &dyn Storage, deck: u32, name: &str) -> Result<(), Problem> {
    let name = clean(name, MAX_NAME);
    if name.is_empty() {
        return Err(Problem::Empty);
    }
    let (mut decks, next) = decks_and_next(storage)?;
    let found = decks
        .iter_mut()
        .find(|d| d.id == deck)
        .ok_or(Problem::Missing)?;
    found.name = name;
    write_decks(storage, &decks, next)?;
    Ok(())
}

/// Deletes a deck and its cards.
pub(crate) fn delete_deck(storage: &dyn Storage, deck: u32) -> Result<(), Problem> {
    fold(storage)?;
    // The counter is read with the deck still there: its id is past it.
    let (mut decks, next) = decks_and_next(storage)?;
    let before = decks.len();
    decks.retain(|d| d.id != deck);
    if decks.len() == before {
        return Err(Problem::Missing);
    }
    write_decks(storage, &decks, next)?;
    storage.remove(&deck_path(deck))?;
    set_index(storage, deck, None)?;
    Ok(())
}

// ---- Cards -------------------------------------------------------------

/// Calls `card` with each card of `deck` and where its line begins.
pub(crate) fn each_card(
    storage: &dyn Storage,
    deck: u32,
    card: impl FnMut(u64, Card) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    each_card_from(storage, deck, 0, card)
}

/// [`each_card`] from the card whose line begins at `from` (a position a
/// visit was given, or 0), to read a deck a piece at a time.
pub(crate) fn each_card_from(
    storage: &dyn Storage,
    deck: u32,
    from: u64,
    mut card: impl FnMut(u64, Card) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    each_line_from(
        storage,
        &deck_path(deck),
        from,
        |at, line| match Card::parse(line) {
            Some(c) => card(at, c),
            None => ControlFlow::Continue(()),
        },
    )
}

/// Calls `card` with each card of `deck` from number `from` on, one at a
/// time, until it says `false`; how many cards the deck has.
pub(crate) fn visit_page(
    storage: &dyn Storage,
    deck: u32,
    from: usize,
    mut card: impl FnMut(&Card) -> bool,
) -> usize {
    let (mut n, mut wanted) = (0, true);
    let _ = each_card(storage, deck, |_, c| {
        if wanted && n >= from {
            wanted = card(&c);
        }
        n += 1;
        ControlFlow::Continue(())
    });
    n
}

/// The cards of `deck` numbered `from..from + count`, and how many there
/// are.
#[cfg(test)]
pub(crate) fn page(
    storage: &dyn Storage,
    deck: u32,
    from: usize,
    count: usize,
) -> (Vec<Card>, usize) {
    let mut cards = Vec::new();
    let total = visit_page(storage, deck, from, |card| {
        cards.push(card.clone());
        cards.len() < count
    });
    (cards, total)
}

/// The card whose line begins at `offset` in `deck` (as [`due`] says).
pub(crate) fn card_at(storage: &dyn Storage, deck: u32, offset: u64) -> Option<Card> {
    Card::parse(&line_at(storage, &deck_path(deck), offset)?)
}

/// Adds `texts` to `deck` as new cards, as many as it has room for; how
/// many it added. Texts without a front are left out.
pub(crate) fn add(
    storage: &dyn Storage,
    deck: u32,
    texts: impl IntoIterator<Item = Texts>,
) -> Result<usize, Problem> {
    let mut adder = Adder::new(storage, deck)?;
    for texts in texts {
        if !adder.push(texts)? {
            break;
        }
    }
    adder.finish()
}

/// Adds the cards of `file` to `deck`, a line each as `front<TAB>back<TAB>example`
/// (the texts written as in a deck: `\t`, `\n`, `\\`), as many as it has
/// room for, read a line at a time; how many it added.
pub(crate) fn add_from(storage: &dyn Storage, deck: u32, file: &str) -> Result<usize, Problem> {
    let mut adder = Adder::new(storage, deck)?;
    let mut failed = Ok(());
    each_line(storage, file, |_, line| {
        let mut fields = line.split('\t').map(unescape);
        let texts = Texts {
            front: fields.next().unwrap_or_default(),
            back: fields.next().unwrap_or_default(),
            example: fields.next().unwrap_or_default(),
        };
        match adder.push(texts) {
            Ok(true) => ControlFlow::Continue(()),
            Ok(false) => ControlFlow::Break(()),
            Err(e) => {
                failed = Err(e);
                ControlFlow::Break(())
            }
        }
    })?;
    failed?;
    adder.finish()
}

/// New cards added to a deck, written a part at a time.
struct Adder<'a> {
    storage: &'a dyn Storage,
    deck: u32,
    /// Cards in the deck before.
    count: usize,
    next: u32,
    added: usize,
    out: String,
}

impl<'a> Adder<'a> {
    fn new(storage: &'a dyn Storage, deck: u32) -> Result<Self, Problem> {
        Self::seeing(storage, deck, |_| {})
    }

    /// [`Adder::new`], `seen` called with each card the deck has (in the
    /// one pass over it that counts them).
    fn seeing(
        storage: &'a dyn Storage,
        deck: u32,
        mut seen: impl FnMut(&Card),
    ) -> Result<Self, Problem> {
        if !decks(storage)?.iter().any(|d| d.id == deck) {
            return Err(Problem::Missing);
        }
        let (mut count, mut next) = (0usize, 1u32);
        each_line(storage, &deck_path(deck), |_, line| {
            if let Some(card) = Card::parse(line) {
                count += 1;
                next = next.max(card.state.id.saturating_add(1));
                seen(&card);
            } else if let Some(n) = counter(line) {
                next = next.max(n);
            }
            ControlFlow::Continue(())
        })?;
        // A last line cut off is ended first, so no card joins it.
        let file = deck_path(deck);
        let size = storage.size(&file).unwrap_or(0);
        let mut last = *b"\n";
        if size > 0 {
            let _ = storage.read_at(&file, size - 1, &mut last);
        }
        let out = if last[0] == b'\n' {
            String::new()
        } else {
            String::from("\n")
        };
        Ok(Self {
            storage,
            deck,
            count,
            next,
            added: 0,
            out,
        })
    }

    /// Writes what is waiting; the first time, says which deck cards are
    /// added to, until they are counted (see [`recover`]).
    fn flush(&mut self) -> Result<(), StorageError> {
        if self.out.is_empty() {
            return Ok(());
        }
        if self.added > 0 && !self.storage.exists(&adding()) {
            self.storage
                .write(&adding(), format!("{}\n", self.deck).as_bytes())?;
        }
        self.storage
            .append(&deck_path(self.deck), self.out.as_bytes())?;
        self.out.clear();
        Ok(())
    }

    /// Adds `texts` (left out without a front); `false` once the deck is
    /// full.
    fn push(&mut self, texts: Texts) -> Result<bool, Problem> {
        if self.count + self.added >= MAX_CARDS {
            return Ok(false);
        }
        let Some(texts) = texts.cleaned() else {
            return Ok(true);
        };
        let card = Card {
            state: State {
                id: self.next,
                ..State::default()
            },
            front: texts.front,
            back: texts.back,
            example: texts.example,
        };
        self.next += 1;
        self.added += 1;
        self.out.push_str(&card.line());
        if self.out.len() >= CHUNK {
            self.flush()?;
        }
        Ok(true)
    }

    fn finish(mut self) -> Result<usize, Problem> {
        if self.added > 0 {
            self.flush()?;
            add_to_index(self.storage, self.deck, self.added as u32)?;
            self.storage.remove(&adding())?;
        } else if self.count >= MAX_CARDS {
            return Err(Problem::Full);
        }
        Ok(self.added)
    }
}

/// Adds the cards `source` pushes into `deck`, in order, until it is done
/// or the deck is full (`push` then says `false`); how many were added.
/// Texts without a front are left out.
pub(crate) fn add_each(
    storage: &dyn Storage,
    deck: u32,
    source: impl FnOnce(&mut dyn FnMut(Texts) -> bool) -> Result<(), StorageError>,
) -> Result<usize, Problem> {
    let mut adder = Adder::new(storage, deck)?;
    let mut failed = Ok(());
    source(&mut |texts| match adder.push(texts) {
        Ok(more) => more,
        Err(e) => {
            failed = Err(e);
            false
        }
    })?;
    failed?;
    adder.finish()
}

/// Which deck cards are being added to (see [`Adder::flush`]).
fn adding() -> String {
    path("adding.txt")
}

/// Changes the texts of card `id` of `deck`; refused where `seen` is
/// given and the card's [`Card::version`] is no longer it.
pub(crate) fn edit(
    storage: &dyn Storage,
    deck: u32,
    id: u32,
    seen: Option<u32>,
    texts: &Texts,
) -> Result<(), Problem> {
    let texts = texts.cleaned().ok_or(Problem::Empty)?;
    fold(storage)?;
    let (mut found, mut changed) = (false, false);
    rewrite(storage, deck, |card| {
        if card.state.id == id {
            found = true;
            changed = seen.is_some_and(|v| v != card.version());
            if !changed {
                card.front.clone_from(&texts.front);
                card.back.clone_from(&texts.back);
                card.example.clone_from(&texts.example);
            }
        }
        true
    })?;
    match (found, changed) {
        (false, _) => Err(Problem::Missing),
        (true, true) => Err(Problem::Changed),
        (true, false) => Ok(()),
    }
}

/// Deletes card `id` of `deck`; refused where `seen` is given and the
/// card's [`Card::version`] is no longer it.
pub(crate) fn delete(
    storage: &dyn Storage,
    deck: u32,
    id: u32,
    seen: Option<u32>,
) -> Result<(), Problem> {
    fold(storage)?;
    let (mut found, mut changed) = (false, false);
    rewrite(storage, deck, |card| {
        if card.state.id != id {
            return true;
        }
        found = true;
        changed = seen.is_some_and(|v| v != card.version());
        changed
    })?;
    match (found, changed) {
        (false, _) => Err(Problem::Missing),
        (true, true) => Err(Problem::Changed),
        (true, false) => Ok(()),
    }
}

/// Writes `deck` anew, each card as `change` leaves it (or without it,
/// where it says `false`), and its index with it; the counter after the
/// cards, past every card there was.
pub(crate) fn rewrite(
    storage: &dyn Storage,
    deck: u32,
    change: impl FnMut(&mut Card) -> bool,
) -> Result<(), Problem> {
    rewrite_with(storage, deck, change, &mut || None).map(|_| ())
}

/// [`rewrite`], and after the cards new ones made from the texts `adds`
/// gives, until it gives none or the deck is full; how many were added.
pub(crate) fn rewrite_with(
    storage: &dyn Storage,
    deck: u32,
    mut change: impl FnMut(&mut Card) -> bool,
    adds: &mut dyn FnMut() -> Option<Texts>,
) -> Result<usize, Problem> {
    let (file, temporary) = (deck_path(deck), deck_temporary(deck));
    if !storage.exists(&file) {
        return Err(Problem::Missing);
    }
    storage.write(&temporary, format!("{CARDS_HEADER}\n").as_bytes())?;
    let mut index = Index {
        deck,
        ..Index::default()
    };
    let mut out = String::new();
    let mut written = Ok(());
    let (mut next, mut count, mut added) = (1u32, 0usize, 0usize);
    each_line(storage, &file, |_, line| {
        // The counter goes after the cards, the highest found.
        if let Some(n) = counter(line) {
            next = next.max(n);
            return ControlFlow::Continue(());
        }
        match Card::parse(line) {
            Some(mut card) => {
                next = next.max(card.state.id.saturating_add(1));
                if !change(&mut card) {
                    return ControlFlow::Continue(());
                }
                index.count(&card.state);
                count += 1;
                out.push_str(&card.line());
            }
            // The version line is written above; empty lines go; what is
            // no card (written by a later version, damaged) stays.
            None if line == CARDS_HEADER || line.trim().is_empty() => {}
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
        if out.len() >= CHUNK {
            written = storage.append(&temporary, out.as_bytes());
            out.clear();
            if written.is_err() {
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    })
    .and(written)
    .and_then(|()| {
        while count < MAX_CARDS {
            let Some(texts) = adds() else { break };
            let Some(texts) = texts.cleaned() else {
                continue;
            };
            let card = Card {
                state: State {
                    id: next,
                    ..State::default()
                },
                front: texts.front,
                back: texts.back,
                example: texts.example,
            };
            next = next.saturating_add(1);
            count += 1;
            added += 1;
            index.count(&card.state);
            out.push_str(&card.line());
            if out.len() >= CHUNK {
                storage.append(&temporary, out.as_bytes())?;
                out.clear();
            }
        }
        out.push_str(&format!("{NEXT}{next}\n"));
        storage.append(&temporary, out.as_bytes())
    })
    .inspect_err(|_| {
        let _ = storage.remove(&temporary);
    })?;
    files::put_in_place(storage, &temporary, &file)?;
    set_index(storage, deck, Some(index))?;
    Ok(added)
}

/// After a power cut while a deck was written anew: the new deck where
/// the old one is gone already, else the unfinished one goes; and the
/// answers given are folded in. Called at start.
pub(crate) fn recover(storage: &dyn Storage) {
    for name in ["decks.txt", "state.txt", "due.txt"] {
        let file = path(name);
        recover_whole(storage, &file, &files::part_path(&file));
    }
    recover_whole(storage, &index_file(), &path("due.tmp"));
    for deck in decks(storage).unwrap_or_default() {
        let (file, temporary) = (deck_path(deck.id), deck_temporary(deck.id));
        if !storage.exists(&temporary) {
            continue;
        }
        if storage.exists(&file) {
            let _ = storage.remove(&temporary);
        } else {
            let _ = storage.rename(&temporary, &file);
        }
    }
    // Cards added while the power went: counted into the index anew.
    let cut_off = paperpaw_sdk::read_file(storage, &adding(), 32)
        .ok()
        .and_then(|d| core::str::from_utf8(&d).ok()?.trim().parse::<u32>().ok());
    if let Some(deck) = cut_off {
        let _ = rewrite(storage, deck, |_| true);
    }
    let _ = storage.remove(&adding());
    let _ = fold(storage);
}

// ---- Words looked up ---------------------------------------------------

fn lookups() -> String {
    path("looked-up.txt")
}

/// The longest the list of words waiting to become cards grows.
const MAX_LOOKUPS: u64 = 64 * 1024;
/// Words taken from that list into the deck at a time.
const LOOKUPS_AT_ONCE: usize = 64;

/// Keeps `word` looked up in `sentence`, to become a card when the flash
/// cards open next: a line appended, nothing read.
pub(crate) fn queue_lookup(
    storage: &dyn Storage,
    word: &str,
    sentence: &str,
) -> Result<(), StorageError> {
    if storage.size(&lookups()).unwrap_or(0) >= MAX_LOOKUPS {
        return Ok(());
    }
    let word = clean(word, MAX_TEXT);
    if word.is_empty() {
        return Ok(());
    }
    let line = format!(
        "{}\t{}\n",
        escape(&word),
        escape(&clean(sentence, MAX_TEXT))
    );
    storage.append(&lookups(), line.as_bytes())
}

/// How many words wait to become cards (some may be there already).
fn lookups_waiting(storage: &dyn Storage) -> usize {
    let mut n = 0;
    let _ = each_line(storage, &lookups(), |_, line| {
        n += usize::from(!line.is_empty());
        ControlFlow::Continue(())
    });
    n
}

/// The words looked up as cards of the deck `deck_name` (made where there
/// is none), each front once, its sentence the example; a part at a time,
/// one pass over the deck each. How many became cards.
pub(crate) fn take_lookups(storage: &dyn Storage, deck_name: &str) -> Result<usize, Problem> {
    let mut added = 0;
    while storage.exists(&lookups()) {
        // A part of the list, each word once.
        let mut part: Vec<Texts> = Vec::new();
        let mut rest = String::new();
        each_line(storage, &lookups(), |_, line| {
            if part.len() >= LOOKUPS_AT_ONCE {
                rest.push_str(line);
                rest.push('\n');
                return ControlFlow::Continue(());
            }
            let (word, sentence) = line.split_once('\t').unwrap_or((line, ""));
            let texts = Texts {
                front: unescape(word),
                back: String::new(),
                example: unescape(sentence),
            };
            let lower = texts.front.to_lowercase();
            if !texts.front.is_empty() && !part.iter().any(|t| t.front.to_lowercase() == lower) {
                part.push(texts);
            }
            ControlFlow::Continue(())
        })?;
        let deck = deck_named(storage, deck_name)?;
        // Those the deck has already are left out, seen in the pass that
        // counts its cards.
        let mut there = alloc::vec![false; part.len()];
        let mut adder = Adder::seeing(storage, deck, |card| {
            let lower = card.front.to_lowercase();
            if let Some(at) = part.iter().position(|t| t.front.to_lowercase() == lower) {
                there[at] = true;
            }
        })?;
        for (texts, there) in part.into_iter().zip(there) {
            if !there && !adder.push(texts)? {
                break;
            }
        }
        added += adder.finish()?;
        if rest.is_empty() {
            storage.remove(&lookups())?;
        } else {
            write_whole(storage, &lookups(), rest.as_bytes())?;
        }
    }
    Ok(added)
}

// ---- Answers -----------------------------------------------------------

fn answers() -> String {
    path("answers.txt")
}

/// Keeps the answer that made a card of `deck` `after`; it was due at
/// `before` (0 if new).
pub(crate) fn record(
    storage: &dyn Storage,
    deck: u32,
    before: i64,
    after: &State,
) -> Result<(), StorageError> {
    let line = format!("{deck}\t{}\t{before}\n", after.fields());
    storage.append(&answers(), line.as_bytes())?;
    remember_clock(storage, after.last);
    Ok(())
}

/// Each card's last state, and when it was due before, by deck and id.
type Answers = BTreeMap<(u32, u32), (State, i64)>;

/// The answers given since the decks were written: each card's last
/// state, and when it was due before the first of them, by deck and id.
/// An error where they could not be read to their end.
fn answered(storage: &dyn Storage) -> Result<Answers, StorageError> {
    let mut found = Answers::new();
    each_line(storage, &answers(), |_, line| {
        let mut fields = line.split('\t');
        let parsed = (|| {
            let deck: u32 = fields.next()?.parse().ok()?;
            let state = State::parse(&mut fields)?;
            let before: i64 = fields.next()?.parse().ok()?;
            Some((deck, state, before))
        })();
        if let Some((deck, state, before)) = parsed {
            found
                .entry((deck, state.id))
                .and_modify(|(last, _)| *last = state)
                .or_insert((state, before));
        }
        ControlFlow::Continue(())
    })?;
    Ok(found)
}

/// Writes the answers given into their decks. Answers that cannot be
/// read are kept for the next time.
pub(crate) fn fold(storage: &dyn Storage) -> Result<(), StorageError> {
    if !storage.exists(&answers()) {
        return Ok(());
    }
    let answered = answered(storage)?;
    let mut decks: Vec<u32> = answered.keys().map(|(deck, _)| *deck).collect();
    decks.dedup();
    for deck in decks {
        let result = rewrite(storage, deck, |card| {
            if let Some((state, _)) = answered.get(&(deck, card.state.id)) {
                card.state = *state;
            }
            true
        });
        match result {
            Ok(()) | Err(Problem::Missing) => {}
            Err(Problem::Storage(e)) => return Err(e),
            Err(_) => {}
        }
    }
    storage.remove(&answers())
}

// ---- What is due -------------------------------------------------------

/// A card due, as a review holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Due {
    pub deck: u32,
    /// Where its line begins in the deck.
    pub offset: u64,
    pub state: State,
}

/// The cards to review by `until` (of the deck `only`, or of all): at
/// most `most` due ones, those due longest first, then up to `new` new
/// ones, deck by deck. The answers given are folded in first.
pub(crate) fn due(
    storage: &dyn Storage,
    until: i64,
    new: usize,
    most: usize,
    only: Option<u32>,
) -> Vec<Due> {
    let _ = fold(storage);
    let (mut reviews, mut fresh) = (Vec::new(), Vec::new());
    let decks = decks(storage).unwrap_or_default();
    for deck in decks.iter().filter(|d| only.is_none_or(|o| o == d.id)) {
        let _ = each_card(storage, deck.id, |offset, card| {
            let state = card.state;
            let due = Due {
                deck: deck.id,
                offset,
                state,
            };
            if state.is_new() {
                if fresh.len() < new {
                    fresh.push(due);
                }
            } else if state.due < until {
                reviews.push(due);
                // Keep those due longest, without holding more than twice
                // as many at a time.
                if reviews.len() >= most * 2 {
                    reviews.sort_by_key(|d: &Due| d.state.due);
                    reviews.truncate(most);
                }
            }
            ControlFlow::Continue(())
        });
    }
    reviews.sort_by_key(|d| d.state.due);
    reviews.truncate(most);
    reviews.extend(fresh);
    reviews
}

/// The cards due by `until`, and the new ones to show today (at most
/// `new`), as the widget counts them: from the index and the answers
/// given since, without reading the decks.
pub(crate) fn due_count(storage: &dyn Storage, until: i64, new: usize) -> usize {
    let mut reviews: i64 = 0;
    let mut fresh: i64 = 0;
    each_index(storage, |part| {
        fresh += i64::from(part.new);
        // A quarter hour all before `until`: every day begins on one.
        reviews += part
            .hours
            .iter()
            .filter(|(quarter, _)| (quarter + 1) * QUARTER <= until)
            .map(|(_, n)| i64::from(*n))
            .sum::<i64>();
    });
    for (state, before) in answered(storage).unwrap_or_default().values() {
        match *before {
            0 => fresh -= 1,
            b if b < until => reviews -= 1,
            _ => {}
        }
        if state.due < until {
            reviews += 1;
        }
    }
    // Words looked up that become cards when the flash cards open.
    fresh += lookups_waiting(storage) as i64;
    reviews.max(0) as usize + (fresh.max(0) as usize).min(new)
}

/// When the next card that is not new comes due after `after`, if any
/// does (from the index: the answers given since are not looked at).
pub(crate) fn next_due(storage: &dyn Storage, after: i64) -> Option<i64> {
    let mut next: Option<i64> = None;
    each_index(storage, |part| {
        for (quarter, _) in &part.hours {
            let at = quarter * QUARTER;
            if at > after && next.is_none_or(|n| at < n) {
                next = Some(at);
            }
        }
    });
    next
}

/// How many cards each deck has, by deck.
pub(crate) fn sizes(storage: &dyn Storage) -> BTreeMap<u32, usize> {
    let mut sizes: BTreeMap<u32, usize> = BTreeMap::new();
    each_index(storage, |part| {
        let others: u32 = part.hours.iter().map(|(_, n)| n).sum();
        *sizes.entry(part.deck).or_default() += (part.new + others) as usize;
    });
    sizes
}

// ---- The index ---------------------------------------------------------

/// When a deck's cards are due (or a part of that, a line of `due.txt`):
/// its new ones, and the others by quarter hour. A deck's lines add up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Index {
    deck: u32,
    new: u32,
    hours: Vec<(i64, u32)>,
}

/// Hours on a line of the index at most: far below [`MAX_LINE`].
const HOURS_A_LINE: usize = 300;

impl Index {
    fn count(&mut self, state: &State) {
        if state.is_new() {
            self.new += 1;
            return;
        }
        let quarter = state.due.div_euclid(QUARTER);
        match self.hours.binary_search_by_key(&quarter, |(q, _)| *q) {
            Ok(at) => self.hours[at].1 += 1,
            Err(at) => self.hours.insert(at, (quarter, 1)),
        }
    }

    /// Its lines: the new cards on the first, [`HOURS_A_LINE`] hours a
    /// line.
    fn lines(&self) -> String {
        let mut text = String::new();
        let mut parts = self.hours.chunks(HOURS_A_LINE);
        let mut first = true;
        loop {
            let part = parts.next();
            if part.is_none() && !first {
                break;
            }
            let new = if first { self.new } else { 0 };
            text.push_str(&format!("{}\t{new}\t", self.deck));
            for (i, (hour, n)) in part.unwrap_or_default().iter().enumerate() {
                if i > 0 {
                    text.push(' ');
                }
                text.push_str(&format!("{hour}={n}"));
            }
            text.push('\n');
            first = false;
        }
        text
    }

    fn parse(line: &str) -> Option<Index> {
        let mut fields = line.split('\t');
        let deck = fields.next()?.parse().ok()?;
        let new = fields.next()?.parse().ok()?;
        let hours = fields
            .next()
            .unwrap_or_default()
            .split(' ')
            .filter_map(|pair| {
                let (hour, n) = pair.split_once('=')?;
                Some((hour.parse().ok()?, n.parse().ok()?))
            })
            .collect();
        Some(Index { deck, new, hours })
    }
}

fn index_file() -> String {
    path("due.txt")
}

/// Calls `part` with each line of the index, read one at a time.
fn each_index(storage: &dyn Storage, mut part: impl FnMut(Index)) {
    let _ = each_line(storage, &index_file(), |_, line| {
        if let Some(i) = Index::parse(line) {
            part(i);
        }
        ControlFlow::Continue(())
    });
}

/// `added` new cards more in `deck`: a line of the index more.
fn add_to_index(storage: &dyn Storage, deck: u32, added: u32) -> Result<(), StorageError> {
    if !storage.exists(&index_file()) {
        storage.write(&index_file(), format!("{DUE_HEADER}\n").as_bytes())?;
    }
    storage.append(&index_file(), format!("{deck}\t{added}\t\n").as_bytes())
}

/// Puts `index` in place of `deck`'s (or takes it out), the others'
/// lines copied a line at a time.
fn set_index(storage: &dyn Storage, deck: u32, index: Option<Index>) -> Result<(), StorageError> {
    let (file, temporary) = (index_file(), path("due.tmp"));
    storage.write(&temporary, format!("{DUE_HEADER}\n").as_bytes())?;
    let mut out = String::new();
    let mut written = Ok(());
    each_line(storage, &file, |_, line| {
        if Index::parse(line).is_some_and(|i| i.deck != deck) {
            out.push_str(line);
            out.push('\n');
        }
        if out.len() >= CHUNK {
            written = storage.append(&temporary, out.as_bytes());
            out.clear();
        }
        if written.is_err() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    written?;
    if let Some(index) = index {
        out.push_str(&index.lines());
    }
    storage.append(&temporary, out.as_bytes())?;
    files::put_in_place(storage, &temporary, &file)
}

// ---- The clock and the day ---------------------------------------------

fn state_file() -> String {
    path("state.txt")
}

/// What `state.txt` says: the time last known, and the new cards shown
/// on a day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Kept {
    clock: i64,
    day: i64,
    new: u32,
}

fn kept(storage: &dyn Storage) -> Kept {
    let mut kept = Kept::default();
    let _ = each_line(storage, &state_file(), |_, line| {
        let mut words = line.split(' ');
        match (words.next(), words.next(), words.next()) {
            (Some("clock"), Some(t), _) => kept.clock = t.parse().unwrap_or(0),
            (Some("new"), Some(day), Some(n)) => {
                kept.day = day.parse().unwrap_or(0);
                kept.new = n.parse().unwrap_or(0);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
    kept
}

fn write_kept(storage: &dyn Storage, kept: Kept) {
    let text = format!("clock {}\nnew {} {}\n", kept.clock, kept.day, kept.new);
    let _ = write_whole(storage, &state_file(), text.as_bytes());
}

fn remember_clock(storage: &dyn Storage, now: i64) {
    let mut k = kept(storage);
    if now > k.clock {
        k.clock = now;
        write_kept(storage, k);
    }
}

/// The time now, in seconds since 1970: the device's, else the last one
/// known (time stands still then: what was due is, new cards come).
pub(crate) fn now(env: &Env<'_>) -> i64 {
    paperpaw_sdk::time::unix_seconds(env).unwrap_or_else(|| kept(env.storage()).clock)
}

/// The local day a time is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Day {
    /// Days since 1970, by the local time.
    pub number: i64,
    /// When it ends, in seconds since 1970: what is due by then is due
    /// today.
    pub end: i64,
}

impl Day {
    /// The day `now` is in, where the local time is `local`.
    pub(crate) fn of(now: i64, local: i64) -> Day {
        Day {
            number: local.div_euclid(DAY),
            end: now + (DAY - local.rem_euclid(DAY)),
        }
    }

    /// The day `now` is in on this device.
    pub(crate) fn now(env: &Env<'_>, now: i64) -> Day {
        Day::of(now, paperpaw_sdk::time::local_seconds(env, now))
    }
}

/// How many new cards may still be shown on `day`, `per_day` a day.
pub(crate) fn new_left(storage: &dyn Storage, day: Day, per_day: u32) -> usize {
    let k = kept(storage);
    let shown = if k.day == day.number { k.new } else { 0 };
    per_day.saturating_sub(shown) as usize
}

/// A new card was shown (and answered) on `day`, at `now`.
pub(crate) fn new_shown(storage: &dyn Storage, day: Day, now: i64) {
    let mut k = kept(storage);
    if k.day != day.number {
        k.day = day.number;
        k.new = 0;
    }
    k.new += 1;
    k.clock = k.clock.max(now);
    write_kept(storage, k);
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
