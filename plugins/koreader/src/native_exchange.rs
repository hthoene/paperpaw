//! The exchange of bookmarks and highlights with KOReader, free of host
//! calls: what both sides had at the last exchange, KOReader's file read
//! as a list of annotations, and the merge that says what to change here
//! (edits of the library's annotations), what to leave for KOReader (its
//! `.annotations.lua`) and what both then have.
//!
//! The rules are those of interface 6 (`lib.rs` `exchange`): what is new on
//! one side goes to the other; what was removed on one side goes too (the
//! reader is asked when KOReader's file has none of what both had); a note
//! changed on one side only goes over, one changed on both keeps the later
//! change; a file that cannot be read or understood, a side without any of
//! what both had (data moved halfway, a sync not complete), stops the
//! exchange before anything changes. Every allocation is fallible.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{annotations::Kind, document::Locator, native::Error};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, format},
};

use crate::native_model::{self as model, Known, Last, Table, Value, fingerprint};

/// Why an exchange stopped before it changed anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// A file that is there but cannot be read or understood, or a side
    /// that lost what both had: nothing changes on either side.
    Unreadable,
    /// The host's refusal or memory.
    Fail(Error),
}

impl From<Error> for Stop {
    fn from(error: Error) -> Self {
        Self::Fail(error)
    }
}

/// Who wrote the annotations file, for KOReader (which takes over only
/// those of other devices).
pub const DEVICE: &str = "PaperPaw";
/// Most bytes of a note kept (a line the annotations file reads whole).
const MAX_NOTE: usize = 3 * 1024;
/// Most bytes of the snippet kept with an annotation.
const MAX_SNIPPET: usize = 240;

/// What an exchange does where KOReader's file, seen with annotations
/// before, has none any more: removed there, or a fresh file synced in
/// from another device, which cannot be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    /// Nothing changes on either side (the reader is asked).
    Ask,
    /// Removed here too, a copy kept aside.
    Remove,
    /// Kept here, and offered to KOReader again.
    Keep,
}

/// What is to be done to an annotation of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// As the library has it.
    None,
    /// New (no id yet).
    Add,
    /// Replaced by this one.
    Replace,
    /// Removed.
    Remove,
}

/// One annotation of the library, as the exchange needs it, at its place
/// in the conversion the reader shows the book in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mine {
    /// The library's id; 0 for one not added yet.
    pub id: u32,
    pub kind: Kind,
    pub start: Locator,
    pub end: Locator,
    /// How far into the book, 0 to 100.
    pub percent: u8,
    /// Seconds since 1970 (0 if not known).
    pub created: i64,
    pub updated: i64,
    /// The text of a highlight, on one line (cut at 240 bytes).
    pub text: String,
    pub note: String,
    /// KOReader's name for it (its time and place), empty for one made here.
    pub client_key: String,
    /// The conversion the offsets were made in, and the XPointers kept with
    /// them (written back where they moved).
    pub conversion: Option<u64>,
    pub xpointers: Option<(String, String)>,
    pub edit: Edit,
}

impl Mine {
    /// What it is known by: its kind and place.
    #[must_use]
    pub fn key(&self) -> model::Key {
        key_of(self.kind, self.start, self.end)
    }
}

fn key_of(kind: Kind, start: Locator, end: Locator) -> model::Key {
    match kind {
        Kind::Bookmark => (Kind::Bookmark, start, start),
        _ => (kind, start, end),
    }
}

/// The book as KOReader's positions see it.
pub trait Placer {
    /// KOReader's XPointer of `at`; none where the format has none there.
    ///
    /// # Errors
    /// The host's refusal or `NoMemory`.
    fn xpointer(&mut self, at: Locator) -> Result<Option<String>, Error>;

    /// The place of an XPointer; none where it is not in the book.
    ///
    /// # Errors
    /// The host's refusal or `NoMemory`.
    fn locate(&mut self, xpointer: &str) -> Result<Option<Locator>, Error>;
}

/// What an exchange works with besides the book and the annotations.
#[derive(Debug, Clone, Copy)]
pub struct Setup<'a> {
    /// What to do where KOReader's file has none any more of what both had.
    pub removal: Removal,
    /// Seconds since 1970 (UTC) now, 0 if not known.
    pub now: i64,
    /// The zone's offset from UTC now, in minutes.
    pub offset_minutes: i64,
    /// How new highlights look in KOReader: `lighten` or `underscore`.
    pub drawer: &'a str,
}

impl Setup<'_> {
    /// KOReader's times: local, `2026-10-16 14:05:00`.
    fn datetime(&self, unix: i64) -> Result<String, Error> {
        let local = unix.saturating_add(self.offset_minutes.saturating_mul(60));
        let (days, rest) = (local.div_euclid(86_400), local.rem_euclid(86_400));
        // The date of a day since 1970 (proleptic Gregorian).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format(format_args!(
            "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
            rest / 3600,
            rest % 3600 / 60,
            rest % 60
        ))
    }

    /// Seconds since 1970 (UTC) of one of KOReader's times; 0 if none.
    fn unix_of(&self, text: &str) -> i64 {
        let number = |range: core::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
        let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10))
        else {
            return 0;
        };
        let (hour, minute, second) = (
            number(11..13).unwrap_or(0),
            number(14..16).unwrap_or(0),
            number(17..19).unwrap_or(0),
        );
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        let local = days * 86_400 + hour * 3600 + minute * 60 + second;
        local.saturating_sub(self.offset_minutes.saturating_mul(60))
    }
}

/// The first of `times` that is known (seconds since 1970 over 0).
fn known_time(times: impl IntoIterator<Item = i64>) -> Option<i64> {
    times.into_iter().find(|&t| t > 0)
}

/// `text` on one line (white space as a blank), in at most `max` bytes,
/// ending in `…` where it is cut.
///
/// # Errors
/// `NoMemory` when it cannot be kept.
pub fn one_line(text: &str, max: usize, trim: bool) -> Result<String, Error> {
    let text = if trim { text.trim() } else { text };
    let mut out = String::new();
    out.try_reserve(text.len().min(max))
        .map_err(|_| Error::NoMemory)?;
    let cut = text.len() > max;
    let room = if cut {
        max.saturating_sub('…'.len_utf8())
    } else {
        max
    };
    for c in text.chars() {
        let c = if c.is_whitespace() && !trim || matches!(c, '\t' | '\n' | '\r') {
            ' '
        } else {
            c
        };
        if out.len() + c.len_utf8() > room {
            break;
        }
        out.push(c);
    }
    if cut {
        out.try_reserve('…'.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        out.push('…');
    }
    Ok(out)
}

/// The note of KOReader's annotation `item` as it is kept here: on one
/// line, cut as a note is kept (ending in `…`). Compared so, a long note is
/// not taken for one changed here.
fn note_of(item: &Table) -> Result<String, Error> {
    one_line(item.str("note").unwrap_or_default(), MAX_NOTE, true)
}

/// KOReader's side of an exchange.
#[derive(Debug)]
pub struct Theirs {
    /// What KOReader has, or, while it has not taken over the last
    /// exchange, what it will have.
    pub items: Vec<Table>,
    /// Whether the last exchange still waits for KOReader.
    pub pending: bool,
    /// Whether KOReader's file has a list of annotations, also an empty
    /// one: none in it is then everything removed there.
    pub listed: bool,
}

/// The annotations of KOReader's file `text`, and whether the file has a list
/// of them (also an empty one).
///
/// # Errors
/// `Unreadable` for a file that is not KOReader's form or has a list that is
/// not understood; `Fail` for memory.
pub fn items_of(text: &str) -> Result<(Vec<Table>, bool), Stop> {
    let Some(Value::Table(mut root)) = model::parse(text)? else {
        return Err(Stop::Unreadable);
    };
    let at = root
        .fields
        .iter()
        .position(|(key, _)| matches!(key, Value::Str(s) if s == "annotations"));
    let Some(at) = at else {
        // Older KOReaders kept them apart, in a form not read here.
        return if root.get("bookmarks").is_some() || root.get("highlight").is_some() {
            Err(Stop::Unreadable)
        } else {
            Ok((Vec::new(), false))
        };
    };
    let Value::Table(items) = root.fields.swap_remove(at).1 else {
        return Err(Stop::Unreadable);
    };
    drop(root);
    // A list of tables, nothing else: what is not understood is not passed
    // on, so it must not be there.
    let mut keyed: Vec<(u64, usize, Table)> = Vec::new();
    for (index, field) in items.fields.into_iter().enumerate() {
        match field {
            (Value::Number(n), Value::Table(item)) => {
                let n = n.parse::<u64>().map_err(|_| Stop::Unreadable)?;
                push(&mut keyed, (n, index, item))?;
            }
            _ => return Err(Stop::Unreadable),
        }
    }
    keyed.sort_unstable_by_key(|(n, index, _)| (*n, *index));
    let mut list = Vec::new();
    list.try_reserve_exact(keyed.len())
        .map_err(|_| Error::NoMemory)?;
    list.extend(keyed.into_iter().map(|(_, _, item)| item));
    Ok((list, true))
}

/// Where KOReader's annotation `item` is here; `None` where it cannot be
/// placed (it then stays as it is).
fn locate(doc: &mut impl Placer, item: &Table) -> Result<Option<model::Key>, Error> {
    if item.get("drawer").is_some() {
        let (Some(a), Some(b)) = (item.str("pos0"), item.str("pos1")) else {
            return Ok(None);
        };
        let (Some(start), Some(end)) = (doc.locate(a)?, doc.locate(b)?) else {
            return Ok(None);
        };
        Ok((start < end).then_some((Kind::Highlight, start, end)))
    } else {
        let Some(page) = item.str("page") else {
            return Ok(None);
        };
        Ok(doc.locate(page)?.map(|at| (Kind::Bookmark, at, at)))
    }
}

/// KOReader's annotation `item` as one here, at `key`. The text of a
/// highlight is KOReader's; the percentage is not known without the
/// document's progress (`PORTING-7.md`).
fn annotation_of(x: &Setup<'_>, item: &Table, key: model::Key) -> Result<Mine, Error> {
    let (kind, start, end) = key;
    let text = match kind {
        Kind::Highlight => one_line(item.str("text").unwrap_or_default(), MAX_SNIPPET, false)?,
        _ => String::new(),
    };
    Ok(Mine {
        id: 0,
        kind,
        start,
        end,
        percent: 0,
        created: x.unix_of(item.str("datetime").unwrap_or_default()),
        updated: x.unix_of(item.str("datetime_updated").unwrap_or_default()),
        text,
        note: note_of(item)?,
        client_key: client_key(item)?,
        conversion: None,
        xpointers: None,
        edit: Edit::Add,
    })
}

/// Annotation `a` as KOReader keeps it; `None` where it cannot be placed
/// there, or while neither it nor the clock tells when it was made
/// (KOReader knows annotations by that time).
fn item_of(x: &Setup<'_>, doc: &mut impl Placer, a: &Mine) -> Result<Option<Value>, Error> {
    let Some(created) = known_time([a.created, x.now]) else {
        return Ok(None);
    };
    let Some(page) = doc.xpointer(a.start)? else {
        return Ok(None);
    };
    let optional = |v: Option<&str>| match v {
        Some(v) => model::string(v),
        None => Ok(Value::Nil),
    };
    let note = optional((!a.note.is_empty()).then_some(a.note.as_str()))?;
    let updated = if a.updated > 0 {
        model::string(&x.datetime(a.updated)?)?
    } else {
        Value::Nil
    };
    // The chapter's title is not known without the document's table of
    // contents (`PORTING-7.md`).
    let chapter = Value::Nil;
    let created = model::string(&x.datetime(created)?)?;
    Ok(Some(match a.kind {
        Kind::Bookmark => model::table([
            ("chapter", chapter),
            ("datetime", created),
            ("datetime_updated", updated),
            ("note", note),
            ("page", model::string(&page)?),
        ])?,
        Kind::Highlight => {
            let Some(end) = doc.xpointer(a.end)? else {
                return Ok(None);
            };
            model::table([
                ("chapter", chapter),
                ("datetime", created),
                ("datetime_updated", updated),
                ("drawer", model::string(x.drawer)?),
                ("note", note),
                ("page", model::string(&page)?),
                ("pos0", model::string(&page)?),
                ("pos1", model::string(&end)?),
                ("text", model::string(&a.text)?),
            ])?
        }
        _ => return Ok(None),
    }))
}

/// What an exchange came to, to be done by the caller in this order: keep
/// a copy of the annotations aside, edit the library's, write KOReader's
/// file, note what both have.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Whether the annotations here changed (their `edit` says how).
    pub ours_changed: bool,
    /// Everything removed in KOReader: a copy is kept first.
    pub keep_aside: bool,
    /// KOReader's file as it is to be written, where it changed.
    pub file: Option<String>,
    /// What both have after this exchange, and how many annotations
    /// KOReader's own file has; kept once the file is written (and, where
    /// they differ from the last, the last).
    pub known: Vec<Known>,
    pub seen: u32,
    /// Whether what is known is to be written.
    pub save: bool,
}

/// Whether KOReader's side `theirs` has none any more of what both had,
/// while there are annotations here: the reader is asked before anything
/// changes ([`Removal`]).
#[must_use]
pub fn removed_there(last: &Last, theirs: &Theirs, ours: usize) -> bool {
    let empty_there = theirs.listed && theirs.items.is_empty() && !last.known.is_empty();
    empty_there && !theirs.pending && last.seen > 0 && ours > 0
}

/// Exchanges the annotations `ours` of a book with KOReader's side
/// `theirs`; `ours` says what to do about each (`edit`). Nothing is
/// changed on either side if this ends in a `Stop`.
///
/// # Errors
/// `Unreadable` where the exchange must wait, `Fail` for a refusal.
#[expect(
    clippy::too_many_lines,
    reason = "the interface-6 exchange as it was, one rule after the other"
)]
pub fn exchange(
    x: &Setup<'_>,
    doc: &mut impl Placer,
    last: &Last,
    ours: &mut Vec<Mine>,
    theirs: Theirs,
) -> Result<Outcome, Stop> {
    // Only where KOReader's positions are understood.
    if doc.xpointer(Locator::new(0, 0))?.is_none() {
        return Ok(Outcome::default());
    }
    let Theirs {
        items: theirs,
        pending,
        listed,
    } = theirs;
    let emptied = last.emptied;
    // KOReader writes an empty list for every book it opened: everything
    // removed there only where its own file had annotations at the last
    // exchange. Else (a fresh file of another device, say) what both had
    // is not taken as known there, and goes to KOReader again.
    let empty_there = listed && theirs.is_empty() && !last.known.is_empty();
    let removed_there = empty_there && !pending && last.seen > 0;
    // The reader decides, in the book: nothing changes before.
    if removed_there && !ours.is_empty() && x.removal == Removal::Ask {
        return Ok(Outcome::default());
    }
    let removed_there = removed_there && x.removal == Removal::Remove;
    let known: &[Known] = if empty_there && !removed_there {
        &[]
    } else {
        &last.known
    };
    // None at all on one side of what both had is rather a move of book
    // data halfway or a sync not complete than everything removed there,
    // unless that side tells so.
    if !known.is_empty() && ((ours.is_empty() && !emptied) || (theirs.is_empty() && !listed)) {
        return Err(Stop::Unreadable);
    }
    // What KOReader's own file has now, for the next exchange.
    let seen = if pending {
        last.seen
    } else {
        u32::try_from(theirs.len()).unwrap_or(u32::MAX)
    };
    let known_of = |key: model::Key| known.iter().find(|k| k.key == key);
    let mut matched: Vec<bool> = Vec::new();
    matched
        .try_reserve_exact(ours.len())
        .map_err(|_| Error::NoMemory)?;
    matched.resize(ours.len(), false);
    let (mut ours_changed, mut theirs_changed) = (false, false);
    let mut items: Vec<Value> = Vec::new();
    // What both have after this exchange.
    let mut shared: Vec<Known> = Vec::new();
    for mut item in theirs {
        let Some(key) = locate(doc, &item)? else {
            push(&mut items, Value::Table(item))?;
            continue;
        };
        if let Some(i) = ours.iter().position(|a| a.key() == key) {
            if let Some(flag) = matched.get_mut(i) {
                *flag = true;
            }
            let Some(a) = ours.get_mut(i) else {
                continue;
            };
            let note = note_of(&item)?;
            let mut same = true;
            if note != a.note {
                let changed = item
                    .str("datetime_updated")
                    .or(item.str("datetime"))
                    .unwrap_or_default();
                let their_time = x.unix_of(changed);
                let our_time = a.updated.max(a.created);
                // A note changed on one side since the last exchange goes
                // to the other; one changed on both, the later change,
                // where both times are known. Else both stay as they are.
                let before = known_of(key).and_then(|k| k.note);
                let theirs_wins = match before {
                    Some(n) if n == fingerprint(&a.note) => Some(true),
                    Some(n) if n == fingerprint(&note) => Some(false),
                    _ if their_time > 0 && our_time > 0 => Some(their_time > our_time),
                    _ => None,
                };
                match theirs_wins {
                    Some(true) => {
                        a.note = note;
                        if their_time > 0 {
                            a.updated = their_time;
                        }
                        if a.edit == Edit::None {
                            a.edit = Edit::Replace;
                        }
                        ours_changed = true;
                    }
                    Some(false) => {
                        let ours_note = if a.note.is_empty() {
                            Value::Nil
                        } else {
                            model::string(&a.note)?
                        };
                        item.set("note", ours_note)?;
                        if let Some(time) = known_time([a.updated, a.created, x.now]) {
                            item.set("datetime_updated", model::string(&x.datetime(time)?)?)?;
                        }
                        theirs_changed = true;
                    }
                    None => same = false,
                }
            }
            push(
                &mut shared,
                Known {
                    key,
                    note: same.then(|| fingerprint(&a.note)),
                },
            )?;
            push(&mut items, Value::Table(item))?;
        } else if known_of(key).is_some() {
            // Removed here: not passed on.
            theirs_changed = true;
        } else {
            let a = annotation_of(x, &item, key)?;
            push(
                &mut shared,
                Known {
                    key,
                    note: Some(fingerprint(&a.note)),
                },
            )?;
            push(ours, a)?;
            push(&mut matched, true)?;
            ours_changed = true;
            push(&mut items, Value::Table(item))?;
        }
    }
    // Ours that KOReader does not have: new here, or removed there.
    let mut removed = false;
    for (i, was_matched) in matched.iter().enumerate() {
        if *was_matched {
            continue;
        }
        let Some(a) = ours.get(i) else {
            continue;
        };
        if known_of(a.key()).is_some() && !pending {
            // Removed there: removed here too.
            removed = true;
            ours_changed = true;
            if let Some(a) = ours.get_mut(i) {
                a.edit = Edit::Remove;
            }
        } else if let Some(item) = item_of(x, doc, a)? {
            // Those not passed on now are not known to both, so they are
            // offered again next time.
            push(&mut items, item)?;
            theirs_changed = true;
            push(
                &mut shared,
                Known {
                    key: a.key(),
                    note: Some(fingerprint(&a.note)),
                },
            )?;
        }
    }
    let file = if theirs_changed {
        let at = match known_time([x.now]) {
            Some(now) => model::string(&x.datetime(now)?)?,
            None => Value::Nil,
        };
        let file = model::table([
            ("annotations", Value::Table(Table::from_list(items)?)),
            ("datetime", at),
            ("device_id", model::string(DEVICE)?),
        ])?;
        Some(model::write(&file)?)
    } else {
        None
    };
    let save = shared != last.known || emptied || seen != last.seen;
    Ok(Outcome {
        ours_changed,
        keep_aside: removed && removed_there,
        file,
        known: shared,
        seen,
        save,
    })
}

/// The text of a copy of the annotations kept aside before they are
/// removed: a line each, kind, place, times, then the text and the note
/// (tabs and line breaks become blanks).
///
/// # Errors
/// `NoMemory` when it cannot be made.
pub fn aside(ours: &[Mine]) -> Result<String, Error> {
    let mut out = String::new();
    for a in ours {
        let kind = if a.kind == Kind::Bookmark { 'b' } else { 'h' };
        let text = one_line(&a.text, MAX_SNIPPET, true)?;
        let note = one_line(&a.note, MAX_NOTE, true)?;
        let line = format(format_args!(
            "{kind}\t{}\t{}\t{}\t{}\t{}\t{}\t{text}\t{note}\n",
            a.start.section, a.start.offset, a.end.section, a.end.offset, a.created, a.updated
        ))?;
        out = concat(&[&out, &line])?;
    }
    Ok(out)
}

/// A name for what KOReader's annotation `item` is to the library: its
/// time and place (a repeated import adds nothing twice), 64 bytes at most.
///
/// # Errors
/// `NoMemory` when it cannot be made.
pub fn client_key(item: &Table) -> Result<String, Error> {
    let hash = [item.str("datetime"), item.str("pos0"), item.str("page")]
        .into_iter()
        .flatten()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, part| {
            part.bytes()
                .chain(core::iter::once(0))
                .fold(hash, |hash, b| {
                    (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
                })
        });
    format(format_args!("koreader-{hash:016x}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::{format as fmt, vec};
    use core::cell::RefCell;

    /// A book whose XPointers are `/body/<section>.<offset>`.
    #[derive(Default)]
    struct Book {
        asked: RefCell<Vec<String>>,
        /// No XPointers at all (a format without them).
        none: bool,
    }

    impl Placer for Book {
        fn xpointer(&mut self, at: Locator) -> Result<Option<String>, Error> {
            if self.none {
                return Ok(None);
            }
            Ok(Some(fmt!("/body/{}.{}", at.section, at.offset)))
        }
        fn locate(&mut self, xpointer: &str) -> Result<Option<Locator>, Error> {
            self.asked.borrow_mut().push(xpointer.into());
            let Some(rest) = xpointer.strip_prefix("/body/") else {
                return Ok(None);
            };
            let (section, offset) = rest.split_once('.').ok_or(Error::Invalid)?;
            Ok(Some(Locator::new(
                section.parse().map_err(|_| Error::Invalid)?,
                offset.parse().map_err(|_| Error::Invalid)?,
            )))
        }
    }

    fn setup(removal: Removal) -> Setup<'static> {
        Setup {
            removal,
            now: 1_700_000_000,
            offset_minutes: 60,
            drawer: "lighten",
        }
    }

    fn mine(kind: Kind, start: (u32, u32), end: (u32, u32), note: &str) -> Mine {
        Mine {
            id: 1,
            kind,
            start: Locator::new(start.0, start.1),
            end: Locator::new(end.0, end.1),
            percent: 10,
            created: 1_600_000_000,
            updated: 0,
            text: "text".into(),
            note: note.into(),
            client_key: String::new(),
            conversion: None,
            xpointers: None,
            edit: Edit::None,
        }
    }

    fn highlight(from: &str, to: &str, note: &str, time: &str) -> Table {
        let table = model::table([
            ("datetime", model::string(time).unwrap()),
            ("drawer", model::string("lighten").unwrap()),
            ("note", model::string(note).unwrap()),
            ("pos0", model::string(from).unwrap()),
            ("pos1", model::string(to).unwrap()),
            ("text", model::string("words").unwrap()),
        ])
        .unwrap();
        match table {
            Value::Table(table) => table,
            _ => unreachable!(),
        }
    }

    fn theirs(items: Vec<Table>) -> Theirs {
        Theirs {
            items,
            pending: false,
            listed: true,
        }
    }

    #[test]
    fn times_are_koreaders_local_text_and_back() {
        let x = setup(Removal::Ask);
        assert_eq!(x.datetime(0).unwrap(), "1970-01-01 01:00:00");
        let text = x.datetime(1_700_000_000).unwrap();
        assert_eq!(text, "2023-11-14 23:13:20");
        assert_eq!(x.unix_of(&text), 1_700_000_000);
        assert_eq!(x.unix_of("nonsense"), 0);
        // Leap day, and a year before 1970.
        assert_eq!(x.unix_of(&x.datetime(951_782_400).unwrap()), 951_782_400);
        assert_eq!(
            x.unix_of(&x.datetime(-86_400 * 400).unwrap()),
            -86_400 * 400
        );
    }

    #[test]
    fn a_note_is_one_line_cut_with_an_ellipsis() {
        assert_eq!(one_line(" a\tb\nc ", 100, true).unwrap(), "a b c");
        assert_eq!(one_line("abcdef", 4, true).unwrap(), "a…");
        assert_eq!(one_line("ab", 4, true).unwrap(), "ab");
        assert_eq!(one_line("ééé", 5, true).unwrap(), "é…");
    }

    #[test]
    fn koreaders_file_is_a_numbered_list_of_tables_and_nothing_else() {
        let (items, listed) = items_of(
            "return {\n [\"annotations\"] = {\n [2] = { [\"page\"] = \"/body/1.0\" },\n [1] = { [\"page\"] = \"/body/0.0\" },\n },\n}",
        )
        .unwrap();
        assert!(listed);
        let pages: Vec<&str> = items.iter().filter_map(|i| i.str("page")).collect();
        assert_eq!(pages, ["/body/0.0", "/body/1.0"]);
        // None kept: listed, empty. No list at all: not listed.
        assert_eq!(
            items_of("return { [\"annotations\"] = {} }").unwrap(),
            (Vec::new(), true)
        );
        assert_eq!(items_of("return {}").unwrap(), (Vec::new(), false));
        // Not understood: stops.
        for bad in [
            "garbage",
            "return { [\"annotations\"] = 3 }",
            "return { [\"annotations\"] = { [1] = 3 } }",
            "return { [\"bookmarks\"] = {} }",
        ] {
            assert_eq!(items_of(bad).err(), Some(Stop::Unreadable), "{bad}");
        }
    }

    #[test]
    fn a_new_highlight_of_koreader_is_taken_over_and_known_to_both() {
        let mut ours: Vec<Mine> = Vec::new();
        let item = highlight("/body/1.5", "/body/1.9", "n", "2023-11-14 23:13:20");
        let out = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &Last::default(),
            &mut ours,
            theirs(vec![item]),
        )
        .unwrap();
        assert!(out.ours_changed);
        assert_eq!(ours.len(), 1);
        let a = ours.first().unwrap();
        assert_eq!(
            (a.kind, a.start, a.end, a.note.as_str(), a.edit),
            (
                Kind::Highlight,
                Locator::new(1, 5),
                Locator::new(1, 9),
                "n",
                Edit::Add
            )
        );
        assert_eq!(a.created, 1_700_000_000);
        // Nothing for KOReader to take over; both know it now, with the note.
        assert!(out.file.is_none());
        assert_eq!(out.known.len(), 1);
        assert_eq!(out.known.first().unwrap().note, Some(fingerprint("n")));
        assert!(out.save);
    }

    #[test]
    fn what_is_new_here_is_left_for_koreader() {
        let mut ours = vec![mine(Kind::Highlight, (1, 5), (1, 9), "mine")];
        let out = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &Last::default(),
            &mut ours,
            theirs(Vec::new()),
        )
        .unwrap();
        assert!(!out.ours_changed);
        let file = out.file.unwrap();
        assert!(file.contains("\"pos0\"") && file.contains("/body/1.5"));
        assert!(file.contains("\"device_id\"") && file.contains(DEVICE));
        assert_eq!(out.known.len(), 1);
        // A book whose positions are not KOReader's is left alone.
        let none = exchange(
            &setup(Removal::Ask),
            &mut Book {
                none: true,
                ..Book::default()
            },
            &Last::default(),
            &mut ours,
            theirs(Vec::new()),
        )
        .unwrap();
        assert!(none.file.is_none() && !none.ours_changed && !none.save);
    }

    #[test]
    fn a_note_changed_on_one_side_goes_to_the_other() {
        let key = (Kind::Highlight, Locator::new(1, 5), Locator::new(1, 9));
        let last = Last {
            known: vec![Known {
                key,
                note: Some(fingerprint("old")),
            }],
            emptied: false,
            seen: 1,
        };
        // KOReader changed it.
        let mut ours = vec![mine(Kind::Highlight, (1, 5), (1, 9), "old")];
        let item = highlight("/body/1.5", "/body/1.9", "new", "2023-11-14 23:13:20");
        let out = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &last,
            &mut ours,
            theirs(vec![item]),
        )
        .unwrap();
        assert!(out.ours_changed);
        assert_eq!(ours.first().unwrap().note, "new");
        assert_eq!(ours.first().unwrap().edit, Edit::Replace);
        assert!(out.file.is_none());
        // Changed here.
        let mut ours = vec![mine(Kind::Highlight, (1, 5), (1, 9), "edited")];
        let item = highlight("/body/1.5", "/body/1.9", "old", "2023-11-14 23:13:20");
        let out = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &last,
            &mut ours,
            theirs(vec![item]),
        )
        .unwrap();
        assert!(!out.ours_changed);
        assert!(out.file.unwrap().contains("edited"));
    }

    #[test]
    fn what_was_removed_here_is_not_passed_on_and_not_taken_back() {
        let key = (Kind::Highlight, Locator::new(1, 5), Locator::new(1, 9));
        let last = Last {
            known: vec![Known { key, note: None }],
            emptied: true,
            seen: 1,
        };
        let mut ours: Vec<Mine> = Vec::new();
        let item = highlight("/body/1.5", "/body/1.9", "", "2023-11-14 23:13:20");
        let out = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &last,
            &mut ours,
            theirs(vec![item]),
        )
        .unwrap();
        assert!(ours.is_empty() && !out.ours_changed);
        // KOReader's file is written without it.
        assert!(out.file.is_some());
        assert!(out.known.is_empty());
    }

    #[test]
    fn what_koreader_removed_asks_the_reader_and_then_removes_with_a_copy() {
        let key = (Kind::Bookmark, Locator::new(1, 5), Locator::new(1, 5));
        let last = Last {
            known: vec![Known { key, note: None }],
            emptied: false,
            seen: 1,
        };
        let there = || theirs(Vec::new());
        let mut ours = vec![mine(Kind::Bookmark, (1, 5), (1, 5), "")];
        assert!(removed_there(&last, &there(), ours.len()));
        // Asking changes nothing.
        let asked = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &last,
            &mut ours,
            there(),
        )
        .unwrap();
        assert!(!asked.ours_changed && asked.file.is_none() && !asked.save);
        // Keep: offered to KOReader again.
        let kept = exchange(
            &setup(Removal::Keep),
            &mut Book::default(),
            &last,
            &mut ours,
            there(),
        )
        .unwrap();
        assert!(!kept.ours_changed && kept.file.is_some() && !kept.keep_aside);
        // Remove: gone here, a copy first.
        let removed = exchange(
            &setup(Removal::Remove),
            &mut Book::default(),
            &last,
            &mut ours,
            there(),
        )
        .unwrap();
        assert!(removed.ours_changed && removed.keep_aside);
        assert_eq!(ours.first().unwrap().edit, Edit::Remove);
        assert!(aside(&ours).unwrap().starts_with("b\t1\t5\t1\t5"));
    }

    #[test]
    fn a_side_that_lost_what_both_had_stops_the_exchange() {
        let key = (Kind::Bookmark, Locator::new(1, 5), Locator::new(1, 5));
        let last = Last {
            known: vec![Known { key, note: None }],
            emptied: false,
            seen: 1,
        };
        // None here, not said to be removed: data moved halfway.
        let mut ours: Vec<Mine> = Vec::new();
        let item = highlight("/body/1.5", "/body/1.9", "", "2023-11-14 23:13:20");
        let stop = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &last,
            &mut ours,
            theirs(vec![item]),
        );
        assert_eq!(stop.err(), Some(Stop::Unreadable));
        // None there and no list: a sync not complete.
        let mut ours = vec![mine(Kind::Bookmark, (1, 5), (1, 5), "")];
        let stop = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &last,
            &mut ours,
            Theirs {
                items: Vec::new(),
                pending: false,
                listed: false,
            },
        );
        assert_eq!(stop.err(), Some(Stop::Unreadable));
    }

    #[test]
    fn an_annotation_koreader_has_that_cannot_be_placed_stays_as_it_is() {
        let mut ours: Vec<Mine> = Vec::new();
        let item = highlight("elsewhere", "/body/1.9", "", "2023-11-14 23:13:20");
        let out = exchange(
            &setup(Removal::Ask),
            &mut Book::default(),
            &Last::default(),
            &mut ours,
            theirs(vec![item]),
        )
        .unwrap();
        assert!(ours.is_empty() && !out.ours_changed && out.file.is_none());
    }

    #[test]
    fn koreaders_names_for_an_annotation_are_stable_and_short() {
        let item = highlight("/body/1.5", "/body/1.9", "", "2023-11-14 23:13:20");
        let a = client_key(&item).unwrap();
        assert_eq!(a, client_key(&item).unwrap());
        assert!(a.len() <= 64 && a.starts_with("koreader-"));
        let other = highlight("/body/1.6", "/body/1.9", "", "2023-11-14 23:13:20");
        assert_ne!(a, client_key(&other).unwrap());
    }
}
