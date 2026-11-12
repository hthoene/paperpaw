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

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Env, Storage, StorageError};

use crate::files::{part_of, put_in_place};
use crate::fsrs::{self, Fx, Grade, Memory};
use crate::lines::{self, copy, push, push_number, push_signed, same_folded};

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

/// The folder of the cards: the plugin's folder under the card (interface
/// 6's names; the host's root 0 is that folder, see
/// [`crate::files::DataFolder`]).
pub(crate) const DIR: &str = ".paperpaw/plugins/vocabulary/cards";
const DECKS: &str = ".paperpaw/plugins/vocabulary/cards/decks.txt";
const STATE: &str = ".paperpaw/plugins/vocabulary/cards/state.txt";
const INDEX: &str = ".paperpaw/plugins/vocabulary/cards/due.txt";
const INDEX_TEMPORARY: &str = ".paperpaw/plugins/vocabulary/cards/due.tmp";
const ANSWERS: &str = ".paperpaw/plugins/vocabulary/cards/answers.txt";
const LOOKUPS: &str = ".paperpaw/plugins/vocabulary/cards/looked-up.txt";
const ADDING: &str = ".paperpaw/plugins/vocabulary/cards/adding.txt";

/// The file `name` of the cards' folder, for the tests; the plugin uses the
/// constants and [`deck_path`].
#[cfg(test)]
pub(crate) fn path(name: &str) -> String {
    alloc::format!("{DIR}/{name}")
}

/// `deck-<id><suffix>` in the cards' folder.
fn deck_file(deck: u32, suffix: &str) -> Result<String, StorageError> {
    let mut out = String::new();
    push(&mut out, DIR)?;
    push(&mut out, "/deck-")?;
    push_number(&mut out, deck)?;
    push(&mut out, suffix)?;
    Ok(out)
}

fn deck_path(deck: u32) -> Result<String, StorageError> {
    deck_file(deck, ".txt")
}

fn deck_temporary(deck: u32) -> Result<String, StorageError> {
    deck_file(deck, ".tmp")
}

/// Writes `bytes` as the whole of `file`: beside it first, then in its
/// place, so a cut never leaves half of it.
fn write_whole(storage: &dyn Storage, file: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let part = part_of(file)?;
    storage.write(&part, bytes)?;
    put_in_place(storage, &part, file)
}

/// After a cut while `file` was written whole: the new one where the old
/// one is gone already, else the unfinished one goes.
fn recover_whole(storage: &dyn Storage, file: &str, part: &str) {
    if !storage.exists(part) {
        return;
    }
    put_right(storage, file, part);
}

/// The unfinished `part` goes where `file` is gone (not found), and goes
/// where `file` is whole. Where the file cannot be told (a read error
/// that may pass) the part is kept: it never replaces a deck that is
/// there.
fn put_right(storage: &dyn Storage, file: &str, part: &str) {
    match storage.size(file) {
        Err(StorageError::NotFound) => {
            let _ = storage.rename(part, file);
        }
        Ok(_) => {
            let _ = storage.remove(part);
        }
        Err(_) => {}
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

/// Appends `text` as a field: a tab, a line break, a backslash written
/// with a backslash.
fn push_escaped(out: &mut String, text: &str) -> Result<(), StorageError> {
    // No character takes more than twice its bytes.
    out.try_reserve(text.len().saturating_mul(2))
        .map_err(|_| StorageError::TooLarge)?;
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    Ok(())
}

/// A field as it was written by [`push_escaped`].
fn unescape(field: &str) -> Result<String, StorageError> {
    let mut out = String::new();
    out.try_reserve_exact(field.len())
        .map_err(|_| StorageError::TooLarge)?;
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
    Ok(out)
}

/// `text` as kept: trimmed, at most `most` characters.
pub(crate) fn clean(text: &str, most: usize) -> Result<String, StorageError> {
    copy(lines::trimmed_cut(text, most))
}

/// [`clean`] for the modules of the plugin not ported yet: an empty text
/// where the heap refuses (a name or a front is then missing, not wrong).
pub(crate) fn clean_or_empty(text: &str, most: usize) -> String {
    clean(text, most).unwrap_or_default()
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

    /// Appends the fields, separated by tabs.
    fn push_fields(&self, out: &mut String) -> Result<(), StorageError> {
        push_number(out, self.id)?;
        push(out, "\t")?;
        push_signed(out, self.due)?;
        push(out, "\t")?;
        push_signed(out, self.last)?;
        push(out, "\t")?;
        push_number(out, self.stability)?;
        push(out, "\t")?;
        push_number(out, self.difficulty)?;
        push(out, "\t")?;
        push_number(out, self.reps)?;
        push(out, "\t")?;
        push_number(out, self.lapses)
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
    /// Appends the card's line, with its line break.
    fn push_line(&self, out: &mut String) -> Result<(), StorageError> {
        self.state.push_fields(out)?;
        for text in [&self.front, &self.back, &self.example] {
            push(out, "\t")?;
            push_escaped(out, text)?;
        }
        push(out, "\n")
    }

    /// The card's line, for the tests.
    #[cfg(test)]
    fn line(&self) -> String {
        let mut out = String::new();
        self.push_line(&mut out).unwrap();
        out
    }

    /// [`Card::parse`] for the tests: `None` for no card.
    #[cfg(test)]
    fn parsed(line: &str) -> Option<Card> {
        Card::parse(line).unwrap()
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

    /// The card `line` holds, `None` where it holds none; an error where
    /// the heap refuses its texts.
    fn parse(line: &str) -> Result<Option<Card>, StorageError> {
        let mut fields = line.split('\t');
        let Some(state) = State::parse(&mut fields) else {
            return Ok(None);
        };
        let Some(front) = fields.next() else {
            return Ok(None);
        };
        Ok(Some(Card {
            state,
            front: unescape(front)?,
            back: unescape(fields.next().unwrap_or_default())?,
            example: unescape(fields.next().unwrap_or_default())?,
        }))
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
    pub(crate) fn cleaned(&self) -> Result<Option<Texts>, StorageError> {
        let front = clean(&self.front, MAX_TEXT)?;
        if front.is_empty() {
            return Ok(None);
        }
        Ok(Some(Texts {
            front,
            back: clean(&self.back, MAX_TEXT)?,
            example: clean(&self.example, MAX_TEXT)?,
        }))
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
    visit: impl FnMut(u64, &str) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    lines::each_line_limited(storage, path, from, MAX_LINE, visit)
}

/// The line of `path` that begins at `offset`.
fn line_at(storage: &dyn Storage, path: &str, offset: u64) -> Option<String> {
    let mut found = None;
    each_line_from(storage, path, offset, |_, line| {
        found = copy(line).ok();
        ControlFlow::Break(())
    })
    .ok()?;
    found
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
    let mut failed = Ok(());
    each_line(storage, DECKS, |_, line| {
        if let Some(n) = counter(line) {
            next = next.max(n);
            return ControlFlow::Continue(());
        }
        let (id, name) = line.split_once('\t').unwrap_or((line, ""));
        if let Ok(id) = id.parse() {
            let made = decks
                .try_reserve(1)
                .map_err(|_| StorageError::TooLarge)
                .and_then(|()| unescape(name));
            match made {
                Ok(name) => decks.push(Deck { id, name }),
                Err(e) => {
                    failed = Err(e);
                    return ControlFlow::Break(());
                }
            }
        }
        if decks.len() >= MAX_DECKS {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    })?;
    failed?;
    let after_highest = decks.iter().map(|d| d.id.saturating_add(1)).max();
    Ok((decks, next.max(after_highest.unwrap_or(1))))
}

/// Writes the list of decks, `next` the id the next one gets.
fn write_decks(storage: &dyn Storage, decks: &[Deck], next: u32) -> Result<(), StorageError> {
    let mut text = String::new();
    push(&mut text, DECKS_HEADER)?;
    push(&mut text, "\n")?;
    push(&mut text, NEXT)?;
    push_number(&mut text, next)?;
    push(&mut text, "\n")?;
    for deck in decks {
        push_number(&mut text, deck.id)?;
        push(&mut text, "\t")?;
        push_escaped(&mut text, &deck.name)?;
        push(&mut text, "\n")?;
    }
    write_whole(storage, DECKS, text.as_bytes())
}

/// The first line of a deck's file.
fn cards_header() -> Result<String, StorageError> {
    let mut out = String::new();
    push(&mut out, CARDS_HEADER)?;
    push(&mut out, "\n")?;
    Ok(out)
}

/// A new deck named `name`: its id.
pub(crate) fn add_deck(storage: &dyn Storage, name: &str) -> Result<u32, Problem> {
    let name = clean(name, MAX_NAME)?;
    if name.is_empty() {
        return Err(Problem::Empty);
    }
    let (mut decks, mut id) = decks_and_next(storage)?;
    if decks.len() >= MAX_DECKS {
        return Err(Problem::Full);
    }
    // Past a deck's file left by a cut (its list not written): never over
    // its cards.
    while storage.exists(&deck_path(id)?) {
        id += 1;
    }
    write_whole(storage, &deck_path(id)?, cards_header()?.as_bytes())?;
    decks.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
    decks.push(Deck { id, name });
    write_decks(storage, &decks, id.saturating_add(1))?;
    Ok(id)
}

/// The deck named `name` (in any case), made if there is none.
pub(crate) fn deck_named(storage: &dyn Storage, name: &str) -> Result<u32, Problem> {
    match decks(storage)?
        .into_iter()
        .find(|d| same_folded(&d.name, name))
    {
        Some(deck) => Ok(deck.id),
        None => add_deck(storage, name),
    }
}

pub(crate) fn rename_deck(storage: &dyn Storage, deck: u32, name: &str) -> Result<(), Problem> {
    let name = clean(name, MAX_NAME)?;
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
    // Every allocation before anything is changed.
    let file = deck_path(deck)?;
    fold(storage)?;
    // The counter is read with the deck still there: its id is past it.
    let (mut decks, next) = decks_and_next(storage)?;
    let before = decks.len();
    decks.retain(|d| d.id != deck);
    if decks.len() == before {
        return Err(Problem::Missing);
    }
    write_decks(storage, &decks, next)?;
    // The deck is out of the list: what follows is not to be left half done.
    let removed = storage.remove(&file);
    keep_index(storage, set_index(storage, deck, None))?;
    removed?;
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
    let mut failed = Ok(());
    each_line_from(
        storage,
        &deck_path(deck)?,
        from,
        |at, line| match Card::parse(line) {
            Ok(Some(c)) => card(at, c),
            Ok(None) => ControlFlow::Continue(()),
            Err(e) => {
                failed = Err(e);
                ControlFlow::Break(())
            }
        },
    )?;
    failed
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
    // Only the cards asked for are made: the others are counted from
    // their lines (a card is a line that parses).
    let _ = deck_path(deck).and_then(|file| {
        each_line(storage, &file, |_, line| {
            if wanted && n >= from {
                match Card::parse(line) {
                    Ok(Some(c)) => wanted = card(&c),
                    Ok(None) => return ControlFlow::Continue(()),
                    Err(_) => wanted = false,
                }
            } else if !looks_like_card(line) {
                return ControlFlow::Continue(());
            }
            n += 1;
            ControlFlow::Continue(())
        })
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

/// Whether `line` is a card's (what [`Card::parse`] reads), told without
/// making one.
fn looks_like_card(line: &str) -> bool {
    let mut fields = line.split('\t');
    State::parse(&mut fields).is_some() && fields.next().is_some()
}

/// The card whose line begins at `offset` in `deck` (as [`due`] says).
pub(crate) fn card_at(storage: &dyn Storage, deck: u32, offset: u64) -> Option<Card> {
    Card::parse(&line_at(storage, &deck_path(deck).ok()?, offset)?)
        .ok()
        .flatten()
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
        let mut fields = line.split('\t');
        let made = (|| {
            Ok::<_, StorageError>(Texts {
                front: unescape(fields.next().unwrap_or_default())?,
                back: unescape(fields.next().unwrap_or_default())?,
                example: unescape(fields.next().unwrap_or_default())?,
            })
        })();
        match made.map_err(Problem::from).and_then(|t| adder.push(t)) {
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
        let file = deck_path(deck)?;
        let mut failed = Ok(());
        each_line(storage, &file, |_, line| {
            match Card::parse(line) {
                Ok(Some(card)) => {
                    count += 1;
                    next = next.max(card.state.id.saturating_add(1));
                    seen(&card);
                }
                Ok(None) => {
                    if let Some(n) = counter(line) {
                        next = next.max(n);
                    }
                }
                Err(e) => {
                    failed = Err(e);
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        })?;
        failed?;
        // A last line cut off is ended first, so no card joins it.
        let size = storage.size(&file).unwrap_or(0);
        // A byte not read (the read failed, or gave less) counts as the end of a line.
        let mut last = [0u8; 1];
        if size == 0 || !matches!(storage.read_at(&file, size - 1, &mut last), Ok(1)) {
            last = *b"\n";
        }
        let mut out = String::new();
        if last[0] != b'\n' {
            push(&mut out, "\n")?;
        }
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
        if self.added > 0 && !self.storage.exists(ADDING) {
            let mut which = String::new();
            push_number(&mut which, self.deck)?;
            push(&mut which, "\n")?;
            self.storage.write(ADDING, which.as_bytes())?;
        }
        self.storage
            .append(&deck_path(self.deck)?, self.out.as_bytes())?;
        self.out.clear();
        Ok(())
    }

    /// Adds `texts` (left out without a front); `false` once the deck is
    /// full.
    fn push(&mut self, texts: Texts) -> Result<bool, Problem> {
        if self.count + self.added >= MAX_CARDS {
            return Ok(false);
        }
        let Some(texts) = texts.cleaned()? else {
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
        card.push_line(&mut self.out)?;
        self.next += 1;
        self.added += 1;
        if self.out.len() >= CHUNK {
            self.flush()?;
        }
        Ok(true)
    }

    fn finish(mut self) -> Result<usize, Problem> {
        if self.added > 0 {
            self.flush()?;
            keep_index(
                self.storage,
                add_to_index(self.storage, self.deck, self.added as u32),
            )?;
            self.storage.remove(ADDING)?;
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

/// Changes the texts of card `id` of `deck`; refused where `seen` is
/// given and the card's [`Card::version`] is no longer it.
pub(crate) fn edit(
    storage: &dyn Storage,
    deck: u32,
    id: u32,
    seen: Option<u32>,
    texts: &Texts,
) -> Result<(), Problem> {
    let texts = texts.cleaned()?.ok_or(Problem::Empty)?;
    fold(storage)?;
    let (mut found, mut changed) = (false, false);
    let mut refused = Ok(());
    rewrite(storage, deck, |card| {
        if card.state.id == id {
            found = true;
            changed = seen.is_some_and(|v| v != card.version());
            if !changed {
                // Where the heap refuses, the card is left as it was.
                match (copy(&texts.front), copy(&texts.back), copy(&texts.example)) {
                    (Ok(front), Ok(back), Ok(example)) => {
                        card.front = front;
                        card.back = back;
                        card.example = example;
                    }
                    (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => refused = Err(e),
                }
            }
        }
        true
    })?;
    refused?;
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
    let (file, temporary) = (deck_path(deck)?, deck_temporary(deck)?);
    if !storage.exists(&file) {
        return Err(Problem::Missing);
    }
    storage.write(&temporary, cards_header()?.as_bytes())?;
    let mut index = Index {
        deck,
        ..Index::default()
    };
    let mut out = String::new();
    let mut written = Ok(());
    let (mut next, mut count, mut added) = (1u32, 0usize, 0usize);
    let mut work = || -> Result<(), StorageError> {
        each_line(storage, &file, |_, line| {
            let mut step = || -> Result<(), StorageError> {
                // The counter goes after the cards, the highest found.
                if let Some(n) = counter(line) {
                    next = next.max(n);
                    return Ok(());
                }
                match Card::parse(line)? {
                    Some(mut card) => {
                        next = next.max(card.state.id.saturating_add(1));
                        if !change(&mut card) {
                            return Ok(());
                        }
                        index.count(&card.state)?;
                        count += 1;
                        card.push_line(&mut out)?;
                    }
                    // The version line is written above; empty lines go; what is
                    // no card (written by a later version, damaged) stays.
                    None if line == CARDS_HEADER || line.trim().is_empty() => {}
                    None => {
                        push(&mut out, line)?;
                        push(&mut out, "\n")?;
                    }
                }
                if out.len() >= CHUNK {
                    storage.append(&temporary, out.as_bytes())?;
                    out.clear();
                }
                Ok(())
            };
            match step() {
                Ok(()) => ControlFlow::Continue(()),
                Err(e) => {
                    written = Err(e);
                    ControlFlow::Break(())
                }
            }
        })?;
        written?;
        while count < MAX_CARDS {
            let Some(texts) = adds() else { break };
            let Some(texts) = texts.cleaned()? else {
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
            index.count(&card.state)?;
            card.push_line(&mut out)?;
            if out.len() >= CHUNK {
                storage.append(&temporary, out.as_bytes())?;
                out.clear();
            }
        }
        push(&mut out, NEXT)?;
        push_number(&mut out, next)?;
        push(&mut out, "\n")?;
        storage.append(&temporary, out.as_bytes())
    };
    work().inspect_err(|_| {
        let _ = storage.remove(&temporary);
    })?;
    put_in_place(storage, &temporary, &file)?;
    keep_index(storage, set_index(storage, deck, Some(index)))?;
    Ok(added)
}

/// After a power cut while a deck was written anew: the new deck where
/// the old one is gone already, else the unfinished one goes; and the
/// answers given are folded in. Called at start.
pub(crate) fn recover(storage: &dyn Storage) {
    for file in [DECKS, STATE, INDEX] {
        if let Ok(part) = part_of(file) {
            recover_whole(storage, file, &part);
        }
    }
    recover_whole(storage, INDEX, INDEX_TEMPORARY);
    for deck in decks(storage).unwrap_or_default() {
        let (Ok(file), Ok(temporary)) = (deck_path(deck.id), deck_temporary(deck.id)) else {
            continue;
        };
        if !storage.exists(&temporary) {
            continue;
        }
        put_right(storage, &file, &temporary);
    }
    // Cards added while the power went: counted into the index anew.
    let mut which = [0u8; 32];
    let cut_off = storage.read_at(ADDING, 0, &mut which).ok().and_then(|n| {
        core::str::from_utf8(which.get(..n)?)
            .ok()?
            .trim()
            .parse::<u32>()
            .ok()
    });
    if let Some(deck) = cut_off {
        let _ = rewrite(storage, deck, |_| true);
    }
    let _ = storage.remove(ADDING);
    let _ = fold(storage);
    let _ = rebuild_index(storage);
}

mod answers;
mod clock;
mod index;
mod lookups;
mod schedule;
pub(crate) use self::answers::*;
pub(crate) use self::clock::*;
pub(crate) use self::index::*;
pub(crate) use self::lookups::*;
pub(crate) use self::schedule::*;

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
