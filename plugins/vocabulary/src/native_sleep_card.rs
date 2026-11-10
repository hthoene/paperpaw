//! *A flash card due* as the sleep screen (interface 6: `sleep.rs`; the choice
//! of `sleep.show` is `vocabulary.sleep-card`): the front of the card that
//! comes next, large in the middle, and below it how many are due. The host
//! shows the screen as the slot `core.sleep` and replaces it on waking; the
//! plugin then opens the review with that card first (`woke`, in the shell),
//! its back shown on *Select*.
//!
//! The card shown is kept in `cards/asleep.txt` (`<deck> <id>`) for the review
//! that opens on waking. Reading the next card also tells the review, as in
//! interface 6.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose};
use paperpaw_sdk::native::fonts::{Font, FontRole};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Rect, Storage};
use paperpaw_ui_kit::widget::Align;
use paperpaw_ui_kit::widget::text::Wrapped;
use paperpaw_ui_kit::{Messages, Paint, Style, geometry, paint, text};

use crate::cards::{self, Day};
use crate::files::DataFolder;
use crate::native_review::{Time, due_now, storage_error};
use crate::native_review_screen::{HostTime, settings};

/// Where the card shown asleep is kept: `<deck> <id>`.
const ASLEEP: &str = ".paperpaw/plugins/vocabulary/cards/asleep.txt";
/// Lines of the front at most, and of the note.
const FRONT_LINES: u16 = 6;
const NOTE_LINES: u16 = 4;

/// What the screen shows: the front of the card due next, and how many are due.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Asleep {
    pub front: Option<String>,
    pub due: usize,
}

/// The card due next (kept for the review that opens on waking), and how many
/// are due; none where no card is.
///
/// # Errors
/// Host refusals, [`Error::NoMemory`].
pub(crate) fn read(host: &Host<'_>) -> Result<Asleep, Error> {
    let data = DataStorage::new(host);
    let folder = DataFolder(&data);
    let time = HostTime::read(host, &folder)?;
    next(&folder, &time, settings(host)?.new_per_day)
}

/// The card due next at `time`, with `per_day` new ones a day: the front, and
/// how many are due. It is kept in `cards/asleep.txt` for the review that opens
/// on waking, the card of the last sleep forgotten.
///
/// # Errors
/// What reading the cards says, [`Error::NoMemory`].
pub(crate) fn next(storage: &dyn Storage, time: &impl Time, per_day: u32) -> Result<Asleep, Error> {
    let now = time.now();
    let day = Day::of(now, time.local(now));
    let new = cards::new_left(storage, day, per_day);
    let next = cards::due(storage, day.end, new, 1, None)
        .map_err(storage_error)?
        .into_iter()
        .next()
        .and_then(|due| Some((due, cards::card_at(storage, due.deck, due.offset)?)));
    // The card of the last sleep is forgotten, whatever comes now.
    let _ = storage.remove(ASLEEP);
    let Some((due, card)) = next else {
        return Ok(Asleep::default());
    };
    let (mut deck, mut id) = ([0u8; 20], [0u8; 20]);
    let kept = text::concat(&[
        text::decimal(due.deck as usize, &mut deck),
        " ",
        text::decimal(due.state.id as usize, &mut id),
        "\n",
    ])?;
    // The review opens at the card shown; without the file it opens as it
    // would on any day.
    let _ = storage.write(ASLEEP, kept.as_bytes());
    Ok(Asleep {
        front: Some(card.front),
        due: due_now(storage, time, per_day),
    })
}

/// The card shown asleep, if one was: its deck and id.
pub(crate) fn shown(storage: &dyn Storage) -> Option<(u32, u32)> {
    let mut buffer = [0u8; 32];
    let read = storage.read_at(ASLEEP, 0, &mut buffer).ok()?;
    let text = core::str::from_utf8(buffer.get(..read)?).ok()?;
    let (deck, id) = text.trim().split_once(' ')?;
    Some((deck.parse().ok()?, id.parse().ok()?))
}

/// Draws the screen into `frame`: the front large in the middle (six lines at
/// most), the note below it.
///
/// # Errors
/// Font, catalog and drawing refusals; [`Error::NoMemory`].
pub(crate) fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    asleep: &Asleep,
) -> Result<(), Error> {
    if frame.purpose != DrawPurpose::Screen {
        return Ok(());
    }
    let display = snapshot::capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let style = Style::read(host, &display, &font)?;
    let area = Rect::new(
        geometry::clamp_u16(i64::from(frame.rect.x)),
        geometry::clamp_u16(i64::from(frame.rect.y)),
        geometry::clamp_u16(i64::from(frame.rect.width)),
        geometry::clamp_u16(i64::from(frame.rect.height)),
    );
    let mut messages = Messages::new()?;
    let note = match asleep.front {
        Some(_) => messages.format(
            host,
            "vocabulary.sleep-card.due",
            &[Arg::Count(asleep.due as i64)],
        )?,
        None => messages.copy(host, "vocabulary.sleep-card.none")?,
    };
    let width = area.width.saturating_sub(style.margin.saturating_mul(2));
    let large = style.text * 1.8;
    let big = geometry::line_px(&font, large)?;
    let small = geometry::line_px(&font, style.small)?;
    let front = match &asleep.front {
        Some(front) => Some(Wrapped::new(
            &font,
            front,
            large,
            (width, big.saturating_mul(FRONT_LINES)),
            false,
        )?),
        None => None,
    };
    let note = Wrapped::new(
        &font,
        &note,
        style.small,
        (width, small.saturating_mul(NOTE_LINES)),
        false,
    )?;
    let front_h = match &front {
        Some(front) => front.height(&font)?,
        None => 0,
    };
    let gap = if front.is_some() {
        style.gap.saturating_mul(3)
    } else {
        0
    };
    let note_h = note.height(&font)?;
    let total = front_h.saturating_add(gap).saturating_add(note_h);
    let top = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    let x = area.x.saturating_add(style.margin);
    let mut out: Vec<Paint> = Vec::new();
    paint::push(&mut out, Paint::Fill(area, style.paper))?;
    if let Some(front) = &front {
        front.draw(
            &mut out,
            &font,
            Rect::new(x, top, width, front_h),
            Align::Center,
            style.ink,
        )?;
    }
    let below = top.saturating_add(front_h).saturating_add(gap);
    note.draw(
        &mut out,
        &font,
        Rect::new(x, below, width, note_h),
        Align::Center,
        style.note,
    )?;
    paint::replay(frame, &font, out.iter())
}

#[cfg(test)]
#[path = "native_sleep_card_tests.rs"]
mod tests;
