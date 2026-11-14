//! A feed's answer, saved in the data folder, made into the page of its book,
//! in the data folder too (the caller then puts it in the library). The
//! page and the result are those of the interface-6 conversion
//! (`native_convert_tests.rs`); every step is fallible and the feed is read in
//! small pieces over the storage calls.
use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::{files::Volume, mem};

use crate::native_convert::{Converted, Converter, DateText};
use crate::native_news::MAX_FEED_FILE;

/// How much of the feed is read at a time.
const PIECE: usize = 2048;

/// Why a feed was not made into a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Failure {
    /// The answer was no feed, held no article (a Wi-Fi's sign-in page does
    /// not), was too large or could not be read: the book there is stays.
    Failed,
    /// The heap had no room to read it; the rest would fail as well.
    Memory,
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        if error == Error::NoMemory {
            Failure::Memory
        } else {
            Failure::Failed
        }
    }
}

/// Writes what the converter has taken to `page`, in calls the storage takes.
fn write_out(
    volume: &(impl Volume + ?Sized),
    page: &str,
    converter: &mut Converter<'_>,
) -> Result<(), Error> {
    for part in converter.taken().chunks(MAX_DATA) {
        volume.append(page, part)?;
    }
    converter.clear();
    Ok(())
}

/// Makes the feed in file `from` the page `page`: at most `max_articles`
/// articles with at most `max_text` bytes of text each. A page is made only
/// where the feed held an article; else (and on any failure) it is not left
/// behind.
///
/// # Errors
/// [`Failure::Failed`] for no feed, a feed of more than 4 MiB or a card that
/// does not read or write; [`Failure::Memory`] for a refused allocation.
pub fn make_page(
    volume: &(impl Volume + ?Sized),
    (from, page): (&str, &str),
    (max_articles, max_text): (usize, usize),
    date_text: DateText<'_>,
) -> Result<Converted, Failure> {
    let made = convert(volume, from, page, (max_articles, max_text), date_text);
    if made.is_err() {
        let _ = volume.remove(page);
    }
    made
}

fn convert(
    volume: &(impl Volume + ?Sized),
    from: &str,
    page: &str,
    (max_articles, max_text): (usize, usize),
    date_text: DateText<'_>,
) -> Result<Converted, Failure> {
    if volume.size(from)? > MAX_FEED_FILE {
        return Err(Failure::Failed);
    }
    // A page left by an earlier try is not a start.
    match volume.remove(page) {
        Ok(()) | Err(Error::NotFound) => {}
        Err(error) => return Err(error.into()),
    }
    let mut converter = Converter::new(max_articles, max_text, date_text);
    // On the heap: a 2 KiB array here would make the frame too large for the X4's stack guard.
    let mut piece = mem::buffer(PIECE)?;
    let mut at = 0u64;
    while !converter.is_done() {
        let n = volume.read_at(from, at, &mut piece)?;
        if n == 0 {
            break;
        }
        at += n as u64;
        converter.feed(piece.get(..n).unwrap_or(&[]))?;
        write_out(volume, page, &mut converter)?;
    }
    let converted = converter.finish()?;
    write_out(volume, page, &mut converter)?;
    volume.sync()?;
    if converted.articles == 0 {
        return Err(Failure::Failed);
    }
    Ok(converted)
}
