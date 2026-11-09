//! The status items "Time left in chapter" and "Time left in book": the pages
//! still to read at one's reading speed, as recorded by the statistics. The
//! reader's status stage (`core.status`) asks for their values with the
//! reader's context (the pages counted, the progress, where the chapters
//! begin).
//!
//! The speed is worked out when a book is opened and now and then while
//! reading, never as a status line is drawn: the stage only does the sum.
use crate::files::DataFolder;
use crate::native_screens::utc_day;
use crate::store;
use crate::texts::{HostWords, Words, duration};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::reply::Reply;
use paperpaw_sdk::native::snapshot::capture_snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::ui::Param;
use paperpaw_sdk::native::ui::stages::{StatusAnswer, StatusCall};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::text;

/// The items' ids (`stats.` and the names of the declarations).
pub(crate) const CHAPTER: &str = "stats.status.left-chapter";
pub(crate) const BOOK: &str = "stats.status.left-book";
/// Days the reading speed is taken from.
const DAYS: i32 = 30;
/// Pages read before there is a speed to go by.
const MIN_PAGES: u32 = 10;
/// Pages turned after which the speed is worked out again.
const RECOUNT: u32 = 50;

/// What the reader says about the page shown, as far as the items need it.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Context<'a> {
    /// The pages counted.
    pages: Option<f32>,
    /// The progress in percent.
    progress: Option<f32>,
    /// Where the chapters begin, as thousandths of the book separated by
    /// commas.
    chapters: Option<&'a str>,
}

impl<'a> Context<'a> {
    /// What the context of a call says (the first of a key that comes twice).
    fn of(params: impl Iterator<Item = Result<Param<'a>, Error>>) -> Self {
        let mut context = Self::default();
        for param in params.flatten() {
            match param.key() {
                "pages" if context.pages.is_none() => context.pages = param.value().parse().ok(),
                "progress" if context.progress.is_none() => {
                    context.progress = param.value().parse().ok();
                }
                "chapters" if context.chapters.is_none() => context.chapters = Some(param.value()),
                _ => {}
            }
        }
        context
    }

    /// Pages left in the book (`chapter` false) or in the chapter: the pages
    /// counted, the progress and where the chapters begin.
    fn pages_left(self, chapter: bool) -> Option<f32> {
        let (pages, progress) = (self.pages?, self.progress?);
        let at = progress * 10.0;
        let end = if chapter {
            self.chapters
                .unwrap_or_default()
                .split(',')
                .filter_map(|mark| mark.trim().parse::<f32>().ok())
                .find(|&mark| mark > at)
                .unwrap_or(1000.0)
        } else {
            1000.0
        };
        Some(((end - at) / 1000.0 * pages).max(0.0))
    }
}

/// Seconds a page, from the reading of the last [`DAYS`] days, if enough was
/// read.
fn speed(host: &Host<'_>) -> Option<f32> {
    let today = capture_snapshot(host)
        .ok()?
        .unix_seconds()
        .and_then(|unix| u32::try_from(unix).ok())
        .map(utc_day)?;
    let storage = DataStorage::new(host);
    // A log that cannot be read shows as no days (as interface 6 did).
    let days = store::days(&DataFolder(&storage), utc_day).unwrap_or_default();
    let (mut seconds, mut pages) = (0u64, 0u32);
    for day in days
        .iter()
        .filter(|day| day.day > today.saturating_sub(DAYS))
    {
        seconds = seconds.saturating_add(u64::from(day.seconds));
        pages = pages.saturating_add(day.stays);
    }
    (pages >= MIN_PAGES).then(|| seconds as f32 / pages as f32)
}

/// The reading speed, kept between events.
#[derive(Debug, Default)]
pub(crate) struct Left {
    /// Seconds a page, once worked out (`Some(None)`: too little read).
    known: Option<Option<f32>>,
    /// The book it was worked out for.
    book: String,
    /// Pages turned since.
    turns: u32,
}

impl Left {
    /// Whether the speed is to be worked out anew as the book at `path` is
    /// shown: not when the same book shows again (the reader says so after
    /// every menu).
    ///
    /// # Errors
    /// [`Error::NoMemory`]: nothing changes.
    fn opened(&mut self, path: &str) -> Result<bool, Error> {
        if self.known.is_some() && self.book == path {
            return Ok(false);
        }
        self.book = text::copy(path)?;
        self.turns = 0;
        Ok(true)
    }

    /// Whether the speed is to be worked out anew after a page was turned:
    /// every [`RECOUNT`] pages, every [`MIN_PAGES`] while there is none.
    fn turned(&mut self) -> bool {
        self.turns = self.turns.wrapping_add(1);
        let every = match self.known {
            Some(Some(_)) => RECOUNT,
            _ => MIN_PAGES,
        };
        self.turns.is_multiple_of(every)
    }

    /// A book was opened (or shown again).
    pub(crate) fn book_opened(&mut self, host: &Host<'_>, path: &str) {
        if self.opened(path) == Ok(true) {
            self.known = Some(speed(host));
        }
    }

    /// A page was turned.
    pub(crate) fn page_turned(&mut self, host: &Host<'_>) {
        if self.turned() {
            self.known = Some(speed(host));
        }
    }

    /// The text of an item: "Chapter: 12 min", where there is a speed and the
    /// reader said what it takes to tell.
    fn text(
        &self,
        words: &mut HostWords<'_, '_>,
        context: Context<'_>,
        chapter: bool,
    ) -> Result<Option<String>, Error> {
        let Some(speed) = self.known.flatten() else {
            return Ok(None);
        };
        let Some(left) = context.pages_left(chapter) else {
            return Ok(None);
        };
        let seconds = (left * speed) as u64;
        let time = duration(words, seconds.max(60))?;
        let key = if chapter {
            "stats.left.chapter"
        } else {
            "stats.left.book"
        };
        words
            .format(
                key,
                &[Arg::Text {
                    name: "time",
                    value: &time,
                }],
            )
            .map(Some)
    }

    /// Answers a call of the stage `core.status`: the text of each of the
    /// items asked for (none: an empty text, which hides the item).
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub(crate) fn status(
        &self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = StatusCall::decode(payload)?;
        let context = Context::of(call.context());
        let mut words = HostWords::new(host)?;
        let mut shown: Vec<(&str, String)> = Vec::new();
        shown.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
        for item in call.items() {
            let chapter = match item {
                CHAPTER => true,
                BOOK => false,
                _ => continue,
            };
            if shown.iter().any(|(asked, _)| *asked == item) {
                continue;
            }
            let text = self.text(&mut words, context, chapter)?.unwrap_or_default();
            shown.push((item, text));
        }
        let mut answers: Vec<StatusAnswer<'_>> = Vec::new();
        answers
            .try_reserve_exact(shown.len())
            .map_err(|_| Error::NoMemory)?;
        for (item, text) in &shown {
            answers.push(StatusAnswer::text(item, text)?);
        }
        let mut out = [0u8; 512];
        let length = call.reply(&answers, &mut out)?;
        reply.write(out.get(..length).ok_or(Error::NoSpace)?)
    }
}

#[cfg(test)]
mod tests;
