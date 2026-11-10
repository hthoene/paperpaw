//! Where the book read last is and how long its pages take, for the reading
//! widget.
//!
//! No plugin may read the page a reader is at, nor the pages it counted, from
//! what the library keeps; the reading events carry them. So the widget records
//! them itself: the page and the chapter of the book read last, and how long a
//! page took (the time between pages turned, pauses left out), kept small in
//! `reading.ini` of the plugin's folder when the book is closed or the device
//! sleeps or stops.
//!
//! The pages left in the chapter, which the events of interface 6 carried,
//! are not in a reading position of interface 7; the widget does not show them.
use alloc::string::String;
use paperpaw_sdk::{
    config,
    native::{Error, Host, events::ReadingView, storage::Root},
};
use paperpaw_ui_kit::{files, text::copy};

/// The name of the file, in the plugin's data folder.
const FILE: &str = "reading.ini";
/// The largest file read.
const READ_LIMIT: usize = 4096;
/// The time between two pages turned that counts as reading, in milliseconds:
/// shorter is leafing through, longer a pause.
const READ_PAGE: core::ops::RangeInclusive<u64> = 2_000..=600_000;
/// Pages the speed is worked out from; beyond them the older count half.
const SPEED_PAGES: u32 = 400;
/// Pages read before there is a speed to go by.
const MIN_PAGES: u32 = 10;

/// Where the book read last is, and how long its pages take.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Position {
    pub path: String,
    /// The page (from 1) and the pages, once the reader counted them.
    pub page: Option<u32>,
    pub pages: Option<u32>,
    /// The chapter's title (empty where not known).
    pub chapter: String,
    /// Seconds read and the pages they took, for the speed.
    pub seconds: u32,
    pub turned: u32,
}

impl Position {
    /// A page turned after `millis` on the last one.
    fn turn(&mut self, millis: u64) {
        if !READ_PAGE.contains(&millis) {
            return;
        }
        self.seconds = self
            .seconds
            .saturating_add(u32::try_from(millis / 1000).unwrap_or(u32::MAX));
        self.turned = self.turned.saturating_add(1);
        if self.turned > SPEED_PAGES {
            self.seconds /= 2;
            self.turned /= 2;
        }
    }

    /// Seconds left to read in the book, if its pages are counted and enough
    /// was read for a speed.
    #[must_use]
    pub fn left(&self) -> Option<u64> {
        let pages = self.pages?.saturating_sub(self.page?);
        (self.turned >= MIN_PAGES)
            .then(|| u64::from(pages) * u64::from(self.seconds) / u64::from(self.turned))
    }

    /// The position written as the file holds it.
    fn format(&self) -> Result<String, Error> {
        let mut out = copy("[reading]\n")?;
        put(&mut out, "path", &self.path)?;
        if let Some(page) = self.page {
            put(&mut out, "page", &Number::new(page))?;
        }
        if let Some(pages) = self.pages {
            put(&mut out, "pages", &Number::new(pages))?;
        }
        if !self.chapter.is_empty() {
            put(&mut out, "chapter", &self.chapter)?;
        }
        put(&mut out, "seconds", &Number::new(self.seconds))?;
        put(&mut out, "turned", &Number::new(self.turned))?;
        Ok(out)
    }

    /// The position in a file's text; what is not understood is left out.
    fn parse(source: &str) -> Result<Self, Error> {
        let mut position = Self::default();
        for entry in config::parse(source).filter_map(Result::ok) {
            let value = &entry.value;
            match entry.key {
                "path" => {
                    position.path = copy(&value.try_as_str().map_err(|_| Error::NoMemory)?)?;
                }
                "chapter" => {
                    position.chapter = copy(&value.try_as_str().map_err(|_| Error::NoMemory)?)?;
                }
                "page" => position.page = value.as_u32().filter(|n| *n > 0),
                "pages" => position.pages = value.as_u32().filter(|n| *n > 0),
                "seconds" => position.seconds = value.as_u32().unwrap_or(0),
                "turned" => position.turned = value.as_u32().unwrap_or(0),
                _ => {}
            }
        }
        Ok(position)
    }
}

/// A number written out in a small buffer, so that a line costs no
/// allocation.
struct Number {
    bytes: [u8; 10],
    start: usize,
}

impl Number {
    fn new(mut n: u32) -> Self {
        let mut bytes = [b'0'; 10];
        let mut start = bytes.len();
        loop {
            start -= 1;
            bytes[start] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        Self { bytes, start }
    }
}

impl core::ops::Deref for Number {
    type Target = str;

    fn deref(&self) -> &str {
        // Only ASCII digits are ever written.
        core::str::from_utf8(&self.bytes[self.start..]).unwrap_or("0")
    }
}

/// The line `key = value` (the value quoted where it needs it).
fn put(out: &mut String, key: &str, value: &str) -> Result<(), Error> {
    let value = config::quote(value);
    out.try_reserve(key.len() + value.len() + 4)
        .map_err(|_| Error::NoMemory)?;
    out.push_str(key);
    out.push_str(" = ");
    out.push_str(&value);
    out.push('\n');
    Ok(())
}

/// The recorded position, kept in memory while reading and written when the
/// book is closed or the device sleeps or stops.
#[derive(Debug, Default)]
pub struct Recorder {
    /// The position, read from its file the first time.
    position: Option<Position>,
    /// When the last page was turned (milliseconds since start).
    last: Option<u64>,
    unsaved: bool,
    /// Moves with every change, for the tick to tell the widget changed.
    revision: u32,
}

impl Recorder {
    /// What the widget shows moved with this number.
    #[must_use]
    pub fn revision(&self) -> u32 {
        self.revision
    }

    fn loaded(&mut self, host: &Host<'_>) -> Result<&mut Position, Error> {
        if self.position.is_none() {
            let read = match files::read_all(host, Root::Data, FILE, READ_LIMIT) {
                Ok(Some(bytes)) => Some(bytes),
                Ok(None) => None,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // A file that cannot be read is no position.
                Err(_) => None,
            };
            let position = match read.as_deref().map(core::str::from_utf8) {
                Some(Ok(text)) => Position::parse(text)?,
                _ => Position::default(),
            };
            self.position = Some(position);
        }
        self.position.as_mut().ok_or(Error::NoMemory)
    }

    /// The position of the book at `path`: what was recorded of it, else
    /// none.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn of(&mut self, host: &Host<'_>, path: &str) -> Result<Position, Error> {
        let position = self.loaded(host)?;
        if position.path != path {
            return Ok(Position::default());
        }
        Ok(Position {
            path: copy(&position.path)?,
            page: position.page,
            pages: position.pages,
            chapter: copy(&position.chapter)?,
            seconds: position.seconds,
            turned: position.turned,
        })
    }

    /// The book is open at `view`; a page was turned (`turned`) or it was
    /// just opened.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn reading(
        &mut self,
        host: &Host<'_>,
        view: &ReadingView<'_>,
        turned: bool,
    ) -> Result<(), Error> {
        let now = view.now_ms();
        let last = self.last.replace(now);
        let position = self.loaded(host)?;
        if position.path != view.path() {
            position.path = copy(view.path())?;
        } else if turned && let Some(last) = last {
            position.turn(now.saturating_sub(last));
        }
        position.page = view.page();
        position.pages = view.pages();
        if position.chapter != view.chapter() {
            position.chapter = copy(view.chapter())?;
        }
        self.unsaved = true;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// The book was closed: the time between pages stops there, and the
    /// position is written.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn closed(&mut self, host: &Host<'_>, view: &ReadingView<'_>) -> Result<(), Error> {
        self.reading(host, view, false)?;
        self.last = None;
        self.save(host);
        Ok(())
    }

    /// Writes the position if it changed; a refusal keeps it for the next
    /// time.
    pub fn save(&mut self, host: &Host<'_>) {
        let (true, Some(position)) = (self.unsaved, &self.position) else {
            return;
        };
        let written = position
            .format()
            .and_then(|text| files::replace(host, Root::Data, FILE, text.as_bytes()));
        self.unsaved = written.is_err();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position() -> Position {
        Position {
            path: String::from("/Books/a b.epub"),
            page: Some(34),
            pages: Some(210),
            chapter: String::from(" Chapter \"3\" "),
            seconds: 900,
            turned: 30,
        }
    }

    #[test]
    fn a_position_is_written_and_read_back() {
        let text = position().format().unwrap();
        assert_eq!(Position::parse(&text).unwrap(), position());
    }

    #[test]
    fn a_file_of_interface_six_is_understood() {
        let text = "[reading]\npath = /a.epub\npage = 5\npages = 100\nchapter-left = 4\nseconds = 60\nturned = 12\n";
        let read = Position::parse(text).unwrap();
        assert_eq!(read.page, Some(5));
        assert_eq!(read.pages, Some(100));
        assert_eq!(read.turned, 12);
    }

    #[test]
    fn a_page_count_of_nought_is_not_a_count() {
        let read = Position::parse("page = 0\npages = 0\n").unwrap();
        assert_eq!((read.page, read.pages), (None, None));
    }

    #[test]
    fn a_page_turn_counts_between_two_seconds_and_ten_minutes() {
        let mut p = Position::default();
        p.turn(1_999);
        p.turn(600_001);
        assert_eq!(p.turned, 0);
        p.turn(2_000);
        p.turn(600_000);
        assert_eq!((p.turned, p.seconds), (2, 602));
    }

    #[test]
    fn the_older_pages_count_half_past_four_hundred() {
        let mut p = Position {
            seconds: 4_000,
            turned: 400,
            ..Position::default()
        };
        p.turn(10_000);
        assert_eq!((p.turned, p.seconds), (200, 2_005));
    }

    #[test]
    fn the_time_left_needs_ten_pages_read_and_the_pages_counted() {
        let mut p = position();
        assert_eq!(p.left(), Some(176 * 30));
        p.turned = 9;
        assert_eq!(p.left(), None);
        p.turned = 30;
        p.pages = None;
        assert_eq!(p.left(), None);
    }

    #[test]
    fn digits_are_written_without_allocation() {
        assert_eq!(&*Number::new(0), "0");
        assert_eq!(&*Number::new(4_294_967_295), "4294967295");
    }
}
