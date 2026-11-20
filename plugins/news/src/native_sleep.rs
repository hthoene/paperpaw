//! The sleep screen: the newest headlines of the feeds fetched last, each with
//! the name of its feed, under a masthead with the date, and when they were
//! fetched at the foot. Only the plugin's two small files are read (the
//! headlines and the time); nothing is fetched. One bit is enough: ink on paper.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::storage::{self, Root};
use paperpaw_sdk::native::{Error, Host, callbacks::DrawFrame};
use paperpaw_ui_kit::page::{Face, Look, Words};
use paperpaw_ui_kit::widget::Align;
use paperpaw_ui_kit::{files::read_all_on, text};

use crate::native_news::{FETCHED_FILE, HEADLINES_CAP, HEADLINES_FILE, without_mark};
use crate::native_plugin::DataVolume;

/// How many headlines the screen shows at most, and where they start and end
/// (design pixels).
const MOST: usize = 6;
const FIRST_AT: u32 = 140;
const END: u32 = 740;

/// A headline and the name of its feed.
#[derive(Debug, PartialEq, Eq)]
pub struct Headline {
    pub title: String,
    pub feed: String,
}

/// The feed a book is of: `News/Tagesschau (3fa9c2).html` is "Tagesschau".
fn feed_of(book: &str) -> &str {
    let file = book.rsplit('/').next().unwrap_or(book);
    without_mark(file.rsplit_once('.').map_or(file, |(stem, _)| stem))
}

/// The headlines of a `headlines.txt` (`title<TAB>book` lines, the newest
/// first) whose book `exists`, at most `most`.
///
/// # Errors
/// `NoMemory`.
pub fn read_lines(
    file: &str,
    most: usize,
    exists: &dyn Fn(&str) -> bool,
) -> Result<Vec<Headline>, Error> {
    let mut shown = Vec::new();
    for line in file.lines() {
        if shown.len() >= most {
            break;
        }
        let Some((title, book)) = line.split_once('\t') else {
            continue;
        };
        if title.trim().is_empty() || !exists(book) {
            continue;
        }
        shown.try_reserve(1).map_err(|_| Error::NoMemory)?;
        shown.push(Headline {
            title: text::copy(title)?,
            feed: text::copy(feed_of(book))?,
        });
    }
    Ok(shown)
}

/// When the headlines were fetched (seconds since 1970), 0 if not known.
fn fetched(host: &Host<'_>) -> i64 {
    match read_all_on(&DataVolume(host), FETCHED_FILE, 32) {
        Ok(Some(bytes)) => core::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0),
        _ => 0,
    }
}

/// Draws the sleep screen.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
    let bytes = match read_all_on(&DataVolume(host), HEADLINES_FILE, HEADLINES_CAP) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => Vec::new(),
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => Vec::new(),
    };
    let file = String::from_utf8_lossy(&bytes);
    let headlines = read_lines(&file, MOST, &|book| {
        storage::size(host, Root::Files, book).is_ok()
    })?;
    let look = Look::open(host)?;
    let mut page = look.page(frame)?;
    let mut words = Words::new(host)?;
    let title = words.copy("news.title")?;
    page.put(
        page.rect(0, 24, 0, 58),
        Face::bold(42),
        Align::Center,
        &title,
    )?;
    page.rule(86, 3)?;
    page.rule(92, 1)?;
    if let Some((day, _)) = words.now()? {
        let date = words.date(day, true)?;
        page.put(
            page.rect(0, 98, 0, 26),
            Face::quiet(17),
            Align::Center,
            &date,
        )?;
    }
    page.rule(128, 1)?;
    let mut y = FIRST_AT;
    if headlines.is_empty() {
        let none = words.copy("news.sleep.empty")?;
        page.wrap(
            page.rect(0, 280, 0, 300),
            Face::quiet(26),
            Align::Center,
            (&none, 6),
        )?;
    }
    for headline in &headlines {
        if y + 17 + 29 > END {
            break;
        }
        let feed = headline.feed.to_uppercase();
        page.put(page.rect(0, y, 0, 17), Face::bold(14), Align::Left, &feed)?;
        let used = page.wrap(
            page.rect(0, y + 17, 0, 60),
            Face::new(23),
            Align::Left,
            (&headline.title, 2),
        )?;
        let used = u32::from(used) * 480 / u32::from(page.area.width.max(1));
        y += 17 + used + 14;
        page.rule(y - 6, 1)?;
    }
    let foot = words.stand(("news.sleep.stand", "news.sleep.never"), fetched(host), &[])?;
    page.footer(&foot)?;
    page.show(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headlines_come_with_their_feed_and_only_where_the_book_is_there() {
        let file = "A\tNews/Tagesschau (3fa9c2).html\nB\tNews/Gone.html\n\tNews/Empty.html\nC\tNews/Heise.html\n";
        let shown = read_lines(file, 5, &|book| !book.contains("Gone")).unwrap();
        assert_eq!(
            shown,
            [
                Headline {
                    title: "A".into(),
                    feed: "Tagesschau".into()
                },
                Headline {
                    title: "C".into(),
                    feed: "Heise".into()
                },
            ]
        );
        assert_eq!(read_lines(file, 1, &|_| true).unwrap().len(), 1);
    }
}
