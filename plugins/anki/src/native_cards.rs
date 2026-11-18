//! The cards on the reader and the answers waiting to go to Anki, in the
//! plugin's data folder (`docs/data-files.md`):
//!
//! - `cards.txt`: the cards of the last sync, a line each (id, front, back),
//!   at most [`MAX_CARDS`](crate::native_anki::MAX_CARDS). Only one card is
//!   ever read into memory, from its place in the file.
//! - `answers.txt`: a line `id<TAB>ease` for each card answered since the
//!   last sync: the queue that goes to Anki with the next sync, offline or not.
//!   A card with an answer is done.
//! - `cards.new`: the cards of a sync under way, until they replace `cards.txt`.
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::files::{Volume, for_each_line_on};
use paperpaw_ui_kit::mem::push;

use crate::native_anki::Card;

pub const CARDS: &str = "cards.txt";
pub const NEW: &str = "cards.new";
pub const ANSWERS: &str = "answers.txt";
/// The answers waiting are at most this many bytes (about 1,500 answers): a
/// reader that was never synced says so, rather than grow without end.
pub const ANSWERS_MAX: u64 = 32 * 1024;
/// The most a card's line may be (both sides at most, with their escapes).
const LINE_MAX: usize = 2 * (crate::native_anki::FRONT_MAX + crate::native_anki::BACK_MAX) + 32;

/// Where a card's line is in `cards.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    at: u64,
    len: usize,
}

/// The places of the lines of `cards.txt` (none where there is no file).
///
/// # Errors
/// `NoMemory`; what the card says, except that there is no file.
pub fn places(volume: &dyn Volume) -> Result<Vec<Place>, Error> {
    let size = match volume.size(CARDS) {
        Ok(size) => size,
        Err(Error::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut places = Vec::new();
    let (mut at, mut start, mut chunk) = (0u64, 0u64, [0u8; 512]);
    while at < size {
        let n = volume.read_at(CARDS, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        for (i, b) in chunk.iter().take(n).enumerate() {
            if *b == b'\n' {
                let end = at + i as u64;
                let len = usize::try_from(end - start).unwrap_or(usize::MAX);
                if len > 0 && len <= LINE_MAX {
                    push(&mut places, Place { at: start, len })?;
                }
                start = end + 1;
            }
        }
        at += n as u64;
    }
    Ok(places)
}

/// The card at `place`; `None` for a line that is no card.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn read(volume: &dyn Volume, place: Place) -> Result<Option<Card>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(place.len)
        .map_err(|_| Error::NoMemory)?;
    bytes.resize(place.len, 0);
    let mut done = 0;
    while done < place.len {
        let end = (done + MAX_DATA).min(place.len);
        let n = volume.read_at(CARDS, place.at + done as u64, &mut bytes[done..end])?;
        if n == 0 {
            return Err(Error::Io);
        }
        done += n;
    }
    match core::str::from_utf8(&bytes) {
        Ok(line) => Card::parse_line(line),
        Err(_) => Ok(None),
    }
}

/// The ids of the cards answered and not yet sent, sorted.
///
/// # Errors
/// `NoMemory`; what the card says, except that there is no file.
pub fn answered(volume: &dyn Volume) -> Result<Vec<u64>, Error> {
    let mut ids = Vec::new();
    let read = for_each_line_on(volume, ANSWERS, &mut |line| {
        if let Some((id, _)) = answer_of(line) {
            push(&mut ids, id)?;
        }
        Ok(ControlFlow::Continue(()))
    });
    match read {
        Ok(()) | Err(Error::NotFound) => {}
        Err(error) => return Err(error),
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// The id and ease of a line of `answers.txt`.
#[must_use]
pub fn answer_of(line: &str) -> Option<(u64, u8)> {
    let (id, ease) = line.split_once('\t')?;
    let ease = ease
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|e| (1..=4).contains(e))?;
    Some((id.parse().ok()?, ease))
}

/// How many answers wait.
///
/// # Errors
/// `NoMemory`; what the card says, except that there is no file.
pub fn waiting(volume: &dyn Volume) -> Result<usize, Error> {
    let mut count = 0;
    let read = for_each_line_on(volume, ANSWERS, &mut |line| {
        count += usize::from(answer_of(line).is_some());
        Ok(ControlFlow::Continue(()))
    });
    match read {
        Ok(()) | Err(Error::NotFound) => Ok(count),
        Err(error) => Err(error),
    }
}

/// Adds the answer `ease` of card `id` to the queue.
///
/// # Errors
/// `NoSpace` where the queue is full (sync first); what the card says.
pub fn record(volume: &dyn Volume, id: u64, ease: u8) -> Result<(), Error> {
    match volume.size(ANSWERS) {
        Ok(size) if size >= ANSWERS_MAX => return Err(Error::NoSpace),
        Ok(_) | Err(Error::NotFound) => {}
        Err(error) => return Err(error),
    }
    let mut digits = [0u8; 20];
    let id = paperpaw_ui_kit::text::decimal_u64(id, &mut digits);
    let mut line = [0u8; 32];
    let n = id.len();
    line[..n].copy_from_slice(id.as_bytes());
    line[n] = b'\t';
    line[n + 1] = b'0' + ease;
    line[n + 2] = b'\n';
    volume.append(ANSWERS, &line[..n + 3])?;
    volume.sync()
}

/// The queue without its first `skip` answers (those Anki took); gone where
/// none is left.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn drop_first(volume: &dyn Volume, skip: usize) -> Result<(), Error> {
    if skip == 0 {
        return Ok(());
    }
    const TEMP: &str = "answers.new";
    volume.write(TEMP, &[])?;
    let (mut seen, mut kept) = (0usize, 0usize);
    let copied = for_each_line_on(volume, ANSWERS, &mut |line| {
        if answer_of(line).is_none() {
            return Ok(ControlFlow::Continue(()));
        }
        seen += 1;
        if seen > skip {
            kept += 1;
            volume.append(TEMP, line.as_bytes())?;
            volume.append(TEMP, b"\n")?;
        }
        Ok(ControlFlow::Continue(()))
    });
    match copied {
        Ok(()) | Err(Error::NotFound) => {}
        Err(error) => {
            discard(volume, TEMP);
            return Err(error);
        }
    }
    if kept == 0 {
        discard(volume, TEMP);
        return match volume.remove(ANSWERS) {
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(error),
        };
    }
    volume.rename(TEMP, ANSWERS)
}

/// Removes `name` where that can be done: a cleanup that cannot fail the
/// caller (a file left behind is replaced or removed by the next sync).
pub fn discard(volume: &dyn Volume, name: &str) {
    match volume.remove(name) {
        Ok(()) | Err(_) => {}
    }
}

/// Removes `name`; no file is no error.
///
/// # Errors
/// What the card says.
pub fn remove(volume: &dyn Volume, name: &str) -> Result<(), Error> {
    match volume.remove(name) {
        Ok(()) | Err(Error::NotFound) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Appends the line of `card` to `cards.new`.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn append_new(volume: &dyn Volume, card: &Card) -> Result<(), Error> {
    let mut line = Vec::new();
    card.push_line(&mut line)?;
    for part in line.chunks(MAX_DATA) {
        volume.append(NEW, part)?;
    }
    Ok(())
}
